//! `numeric` in PostgreSQL's binary format, converted to and from a 128-bit scaled integer.
//!
//! The wire format is: `ndigits: u16`, `weight: i16`, `sign: u16`, `dscale: u16`, then `ndigits`
//! base-10000 digits (`u16`, most significant first). The value is
//! `sum(digit[i] * 10000^(weight - i))`, negated when `sign` is `0x4000`. `0xC000` is NaN,
//! `0xD000` is `Infinity` and `0xF000` is `-Infinity` (PostgreSQL 14 and later).

use crate::error::{ProtoError, Result};
use crate::types::oid;

const NUMERIC_NEG: u16 = 0x4000;
const NUMERIC_NAN: u16 = 0xC000;
const NUMERIC_PINF: u16 = 0xD000;
const NUMERIC_NINF: u16 = 0xF000;

fn bad(what: &'static str) -> ProtoError {
    ProtoError::BadValue {
        oid: oid::NUMERIC,
        what,
    }
}

/// Precision and scale from a `numeric` type modifier, if it names a column that fits a
/// `Decimal128`: precision 1 to 38 and scale 0 to precision. `None` for an unconstrained
/// `numeric` (modifier -1, where the scale varies by value) and for the PostgreSQL 15 forms with a
/// negative scale or a scale above the precision.
pub fn precision_scale(typmod: i32) -> Option<(u8, u8)> {
    if typmod < 4 {
        return None;
    }
    let t = typmod - 4;
    let precision = (t >> 16) & 0xFFFF;
    let mut scale = t & 0x7FF;
    if scale >= 1024 {
        scale -= 2048; // PostgreSQL 15 stores the scale as an 11-bit signed number
    }
    if (1..=38).contains(&precision) && (0..=precision).contains(&scale) {
        Some((precision as u8, scale as u8))
    } else {
        None
    }
}

fn pow10(n: u32) -> Option<u128> {
    10u128.checked_pow(n)
}

/// Decode a binary `numeric` into `value * 10^scale` as an integer. NaN and the infinities have
/// no integer form and are errors, as is a value that has more fractional digits than `scale`
/// (a nonzero digit below the scale) or that does not fit in 38 digits.
pub fn decode(v: &[u8], scale: u8) -> Result<i128> {
    if v.len() < 8 {
        return Err(bad("too short"));
    }
    let nd = usize::from(u16::from_be_bytes([v[0], v[1]]));
    let weight = i32::from(i16::from_be_bytes([v[2], v[3]]));
    let sign = u16::from_be_bytes([v[4], v[5]]);
    let dscale = u16::from_be_bytes([v[6], v[7]]);
    if v.len() != 8 + 2 * nd {
        return Err(bad("wrong length"));
    }
    match sign {
        0 | NUMERIC_NEG => {}
        NUMERIC_NAN | NUMERIC_PINF | NUMERIC_NINF => {
            return Err(bad(
                "NaN and infinity cannot be stored in a decimal column; select the column ::text",
            ))
        }
        _ => return Err(bad("unknown sign")),
    }
    if dscale > 0x3FFF {
        return Err(bad("scale out of range"));
    }
    let mut acc: u128 = 0;
    for (i, c) in v[8..].chunks_exact(2).enumerate() {
        let d = u128::from(u16::from_be_bytes([c[0], c[1]]));
        if d >= 10_000 {
            return Err(bad("digit out of range"));
        }
        if d == 0 {
            continue;
        }
        // this digit stands for d * 10^e once the value is multiplied by 10^scale
        let e = 4 * (weight - i as i32) + i32::from(scale);
        let part = if e >= 0 {
            let p = pow10(e as u32).ok_or_else(|| bad("too many digits"))?;
            d.checked_mul(p).ok_or_else(|| bad("too many digits"))?
        } else {
            let k = (-e) as u32;
            let p = pow10(k).ok_or_else(|| bad("more fractional digits than the column scale"))?;
            if d % p != 0 {
                return Err(bad("more fractional digits than the column scale"));
            }
            d / p
        };
        acc = acc
            .checked_add(part)
            .ok_or_else(|| bad("too many digits"))?;
    }
    let mag = i128::try_from(acc).map_err(|_| bad("too many digits"))?;
    Ok(if sign == NUMERIC_NEG { -mag } else { mag })
}

/// Encode `unscaled / 10^scale` as a binary `numeric` with display scale `scale`.
pub fn encode(unscaled: i128, scale: u8, out: &mut Vec<u8>) {
    let digits = unscaled.unsigned_abs().to_string();
    let scale = usize::from(scale);
    // integer and fractional decimal digits
    let padded = if digits.len() <= scale {
        format!("{}{digits}", "0".repeat(scale + 1 - digits.len()))
    } else {
        digits
    };
    let (int_part, frac_part) = padded.split_at(padded.len() - scale);
    let int_part = int_part.trim_start_matches('0');
    // pad the integer part on the left and the fraction on the right to whole base-10000 digits
    let int_pad = (4 - int_part.len() % 4) % 4;
    let frac_pad = (4 - frac_part.len() % 4) % 4;
    let all: Vec<u8> = "0"
        .repeat(int_pad)
        .bytes()
        .chain(int_part.bytes())
        .chain(frac_part.bytes())
        .chain("0".repeat(frac_pad).bytes())
        .collect();
    let int_groups = (int_part.len() + int_pad) / 4;
    let mut groups: Vec<u16> = all
        .chunks_exact(4)
        .map(|c| c.iter().fold(0u16, |a, d| a * 10 + u16::from(d - b'0')))
        .collect();
    let mut weight = int_groups as i32 - 1;
    // leading zero groups move the weight, trailing ones are dropped
    let lead = groups.iter().take_while(|g| **g == 0).count();
    groups.drain(..lead);
    weight -= lead as i32;
    while groups.last() == Some(&0) {
        groups.pop();
    }
    let zero = groups.is_empty();
    if zero {
        weight = 0;
    }
    out.extend_from_slice(&(groups.len() as u16).to_be_bytes());
    out.extend_from_slice(&(weight as i16).to_be_bytes());
    let sign = if unscaled < 0 && !zero {
        NUMERIC_NEG
    } else {
        0
    };
    out.extend_from_slice(&sign.to_be_bytes());
    out.extend_from_slice(&(scale as u16).to_be_bytes());
    for g in groups {
        out.extend_from_slice(&g.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karez_chunk::rng::SplitMix64;

    fn wire(nd: &[u16], weight: i16, sign: u16, dscale: u16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(nd.len() as u16).to_be_bytes());
        b.extend_from_slice(&weight.to_be_bytes());
        b.extend_from_slice(&sign.to_be_bytes());
        b.extend_from_slice(&dscale.to_be_bytes());
        for d in nd {
            b.extend_from_slice(&d.to_be_bytes());
        }
        b
    }

    #[test]
    fn type_modifiers() {
        // numeric(12,2): ((12 << 16) | 2) + 4
        assert_eq!(precision_scale(((12 << 16) | 2) + 4), Some((12, 2)));
        assert_eq!(precision_scale(((38 << 16) | 38) + 4), Some((38, 38)));
        assert_eq!(precision_scale((38 << 16) + 4), Some((38, 0)));
        assert_eq!(precision_scale(-1), None, "unconstrained numeric");
        assert_eq!(precision_scale(((39 << 16) | 2) + 4), None, "wider than 38");
        assert_eq!(
            precision_scale(((5 << 16) | 7) + 4),
            None,
            "scale above precision"
        );
        // numeric(5,-2) on PostgreSQL 15: the scale is 2046 as 11 bits
        assert_eq!(precision_scale(((5 << 16) | (2048 - 2)) + 4), None);
        assert_eq!(precision_scale(0), None);
    }

    #[test]
    fn decodes_known_values() {
        // 12345.67 at scale 2: digits 1 2345 6700, weight 1
        assert_eq!(
            decode(&wire(&[1, 2345, 6700], 1, 0, 2), 2).unwrap(),
            1_234_567
        );
        // -0.05: digit 500 at weight -1
        assert_eq!(decode(&wire(&[500], -1, NUMERIC_NEG, 2), 2).unwrap(), -5);
        // zero has no digits
        assert_eq!(decode(&wire(&[], 0, 0, 3), 3).unwrap(), 0);
        // 10000 at scale 0: one digit 1 at weight 1
        assert_eq!(decode(&wire(&[1], 1, 0, 0), 0).unwrap(), 10_000);
        // a value with a smaller display scale than the column: 1.5 into scale 4
        assert_eq!(decode(&wire(&[1, 5000], 0, 0, 1), 4).unwrap(), 15_000);
        // 38 nines
        let nines = wire(
            &[99, 9999, 9999, 9999, 9999, 9999, 9999, 9999, 9999, 9999],
            9,
            0,
            0,
        );
        assert_eq!(decode(&nines, 0).unwrap(), 10i128.pow(38) - 1);
    }

    #[test]
    fn refuses_what_a_decimal_cannot_hold() {
        for sign in [NUMERIC_NAN, NUMERIC_PINF, NUMERIC_NINF] {
            let e = decode(&wire(&[], 0, sign, 0), 2).unwrap_err();
            assert!(e.to_string().contains("::text"), "{e}");
        }
        // 0.123 does not fit scale 2
        assert!(decode(&wire(&[1230], -1, 0, 3), 2).is_err());
        // but 0.120 does (trailing zero digit)
        assert_eq!(decode(&wire(&[1200], -1, 0, 3), 2).unwrap(), 12);
        // 39 digits
        assert!(decode(&wire(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0], 10, 0, 0), 0).is_err());
        // a huge weight with a nonzero digit
        assert!(decode(&wire(&[1], 30_000, 0, 0), 0).is_err());
        // malformed
        assert!(decode(&[0; 4], 0).is_err());
        assert!(decode(&wire(&[1], 0, 0, 0)[..9], 0).is_err());
        assert!(decode(&wire(&[10_000], 0, 0, 0), 0).is_err());
        assert!(decode(&wire(&[1], 0, 0x1234, 0), 0).is_err());
        assert!(decode(&wire(&[1], 0, 0, 0x4000), 0).is_err());
    }

    #[test]
    fn encode_matches_what_the_server_sends() {
        let enc = |u: i128, s: u8| {
            let mut b = Vec::new();
            encode(u, s, &mut b);
            b
        };
        assert_eq!(enc(1_234_567, 2), wire(&[1, 2345, 6700], 1, 0, 2));
        assert_eq!(enc(-5, 2), wire(&[500], -1, NUMERIC_NEG, 2));
        assert_eq!(
            enc(0, 3),
            wire(&[], 0, 0, 3),
            "zero has no digits, whatever the scale"
        );
        assert_eq!(enc(10_000, 0), wire(&[1], 1, 0, 0));
        assert_eq!(enc(100, 0), wire(&[100], 0, 0, 0));
        assert_eq!(enc(1, 6), wire(&[100], -2, 0, 6), "0.000001");
        assert_eq!(enc(1, 8), wire(&[1], -2, 0, 8), "0.00000001");
        assert_eq!(enc(15_000, 4), wire(&[1, 5000], 0, 0, 4));
    }

    #[test]
    fn encode_then_decode_is_the_identity() {
        let mut rng = SplitMix64::new(77);
        for _ in 0..20_000 {
            let digits = rng.range(1, 38) as u32;
            let mag = (u128::from(rng.next_u64()) << 64 | u128::from(rng.next_u64()))
                % 10u128.pow(digits);
            let mut u = mag as i128;
            if rng.chance(1, 2) {
                u = -u;
            }
            let scale = rng.range(0, digits as usize) as u8;
            let mut b = Vec::new();
            encode(u, scale, &mut b);
            assert_eq!(decode(&b, scale).unwrap(), u, "u={u} scale={scale}");
        }
        for u in [0, 1, -1, 10i128.pow(38) - 1, -(10i128.pow(38) - 1)] {
            for scale in [0u8, 1, 17, 38] {
                let mut b = Vec::new();
                encode(u, scale, &mut b);
                assert_eq!(decode(&b, scale).unwrap(), u);
            }
        }
    }
}
