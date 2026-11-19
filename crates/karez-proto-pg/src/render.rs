//! Binary values that have no native column type, rendered into PostgreSQL's own text form.
//!
//! `numeric`, `json`, `jsonb`, `interval`, `time`, `timetz`, `inet`, `cidr`, `macaddr`,
//! `macaddr8`, `bit`, `varbit`, `xml`, `"char"` and one-dimensional or multi-dimensional arrays
//! of these and of the native types are requested in **binary** format and rendered here to a
//! string column. The rendering is exact: `numeric` keeps every digit, `interval` uses the
//! `iso_8601` form, arrays use the `{a,b,NULL}` literal form. Floating-point array elements use
//! Rust's shortest round-trip form, which can differ from the server's text (`1e+20`).

use crate::error::{ProtoError, Result};
use crate::types::oid;
use std::fmt::Write as _;

fn bad(o: u32, what: &'static str) -> ProtoError {
    ProtoError::BadValue { oid: o, what }
}

fn fix<const N: usize>(v: &[u8], o: u32) -> Result<[u8; N]> {
    <[u8; N]>::try_from(v).map_err(|_| bad(o, "wrong length"))
}

/// True if [`render_scalar`] can render this type OID.
pub(crate) fn is_renderable(o: u32) -> bool {
    matches!(
        o,
        oid::BOOL
            | oid::INT2
            | oid::INT4
            | oid::INT8
            | oid::FLOAT4
            | oid::FLOAT8
            | oid::TEXT
            | oid::VARCHAR
            | oid::BPCHAR
            | oid::NAME
            | oid::CHAR
            | oid::XML
            | oid::JSON
            | oid::JSONB
            | oid::BYTEA
            | oid::UUID
            | oid::OID
            | oid::NUMERIC
            | oid::TIME
            | oid::TIMETZ
            | oid::INTERVAL
            | oid::INET
            | oid::CIDR
            | oid::MACADDR
            | oid::MACADDR8
            | oid::BIT
            | oid::VARBIT
            | oid::VOID
    )
}

/// Element type of a supported array type, from the array's OID.
pub(crate) fn array_elem(o: u32) -> Option<u32> {
    Some(match o {
        1000 => oid::BOOL,
        1001 => oid::BYTEA,
        1002 => oid::CHAR,
        1003 => oid::NAME,
        1005 => oid::INT2,
        1007 => oid::INT4,
        1009 => oid::TEXT,
        1014 => oid::BPCHAR,
        1015 => oid::VARCHAR,
        1016 => oid::INT8,
        1021 => oid::FLOAT4,
        1022 => oid::FLOAT8,
        1028 => oid::OID,
        1040 => oid::MACADDR,
        1041 => oid::INET,
        1183 => oid::TIME,
        1187 => oid::INTERVAL,
        1231 => oid::NUMERIC,
        1270 => oid::TIMETZ,
        1561 => oid::BIT,
        1563 => oid::VARBIT,
        199 => oid::JSON,
        651 => oid::CIDR,
        2951 => oid::UUID,
        3807 => oid::JSONB,
        143 => oid::XML,
        775 => oid::MACADDR8,
        _ => return None,
    })
}

fn float(x: f64, out: &mut String) {
    if x.is_nan() {
        out.push_str("NaN");
    } else if x.is_infinite() {
        out.push_str(if x > 0.0 { "Infinity" } else { "-Infinity" });
    } else {
        let _ = write!(out, "{x}");
    }
}

fn utf8(v: &[u8]) -> Result<&str> {
    std::str::from_utf8(v).map_err(|_| ProtoError::InvalidUtf8)
}

/// Render one binary value of type `o` as text.
pub(crate) fn render_scalar(o: u32, v: &[u8], out: &mut String) -> Result<()> {
    match o {
        oid::BOOL => match v {
            [0] => out.push('f'),
            [1] => out.push('t'),
            _ => return Err(bad(o, "not 0 or 1")),
        },
        oid::INT2 => {
            let _ = write!(out, "{}", i16::from_be_bytes(fix(v, o)?));
        }
        oid::INT4 => {
            let _ = write!(out, "{}", i32::from_be_bytes(fix(v, o)?));
        }
        oid::INT8 => {
            let _ = write!(out, "{}", i64::from_be_bytes(fix(v, o)?));
        }
        oid::OID => {
            let _ = write!(out, "{}", u32::from_be_bytes(fix(v, o)?));
        }
        oid::FLOAT4 => {
            let x = f32::from_be_bytes(fix(v, o)?);
            if x.is_finite() {
                let _ = write!(out, "{x}");
            } else {
                float(f64::from(x), out);
            }
        }
        oid::FLOAT8 => float(f64::from_be_bytes(fix(v, o)?), out),
        oid::TEXT | oid::VARCHAR | oid::BPCHAR | oid::NAME | oid::CHAR | oid::XML | oid::JSON => {
            out.push_str(utf8(v)?)
        }
        oid::JSONB => match v.split_first() {
            Some((1, rest)) => out.push_str(utf8(rest)?),
            _ => return Err(bad(o, "unknown jsonb version")),
        },
        oid::BYTEA => {
            out.push_str("\\x");
            for b in v {
                let _ = write!(out, "{b:02x}");
            }
        }
        oid::UUID => {
            let u: [u8; 16] = fix(v, o)?;
            for (i, b) in u.iter().enumerate() {
                if matches!(i, 4 | 6 | 8 | 10) {
                    out.push('-');
                }
                let _ = write!(out, "{b:02x}");
            }
        }
        oid::NUMERIC => numeric(v, out)?,
        oid::TIME => {
            let us = i64::from_be_bytes(fix(v, o)?);
            if !(0..=86_400_000_000).contains(&us) {
                return Err(bad(o, "time out of range"));
            }
            time_of_day(us, out);
        }
        oid::TIMETZ => {
            let b: [u8; 12] = fix(v, o)?;
            let us = i64::from_be_bytes(b[..8].try_into().unwrap());
            let west = i32::from_be_bytes(b[8..].try_into().unwrap());
            if !(0..=86_400_000_000).contains(&us) {
                return Err(bad(o, "time out of range"));
            }
            time_of_day(us, out);
            let off = -i64::from(west);
            out.push(if off < 0 { '-' } else { '+' });
            let a = off.unsigned_abs();
            let _ = write!(out, "{:02}", a / 3600);
            if a % 3600 != 0 {
                let _ = write!(out, ":{:02}", a / 60 % 60);
                if a % 60 != 0 {
                    let _ = write!(out, ":{:02}", a % 60);
                }
            }
        }
        oid::INTERVAL => {
            let b: [u8; 16] = fix(v, o)?;
            interval(
                i64::from_be_bytes(b[..8].try_into().unwrap()),
                i32::from_be_bytes(b[8..12].try_into().unwrap()),
                i32::from_be_bytes(b[12..].try_into().unwrap()),
                out,
            );
        }
        oid::INET | oid::CIDR => inet(o, v, out)?,
        oid::MACADDR => mac(v, 6, o, out)?,
        oid::MACADDR8 => mac(v, 8, o, out)?,
        oid::BIT | oid::VARBIT => bits(o, v, out)?,
        oid::VOID => {
            if !v.is_empty() {
                return Err(bad(o, "void with data"));
            }
        }
        _ => return Err(bad(o, "no renderer for this type")),
    }
    Ok(())
}

fn time_of_day(us: i64, out: &mut String) {
    let _ = write!(
        out,
        "{:02}:{:02}:{:02}",
        us / 3_600_000_000,
        us / 60_000_000 % 60,
        us / 1_000_000 % 60
    );
    fraction(us % 1_000_000, out);
}

/// `.ffffff` with trailing zeros removed; nothing when zero.
fn fraction(f: i64, out: &mut String) {
    if f != 0 {
        let s = format!("{f:06}");
        out.push('.');
        out.push_str(s.trim_end_matches('0'));
    }
}

fn interval(micros: i64, days: i32, months: i32, out: &mut String) {
    if micros == 0 && days == 0 && months == 0 {
        out.push_str("PT0S");
        return;
    }
    out.push('P');
    let (years, mons) = (months / 12, months % 12);
    if years != 0 {
        let _ = write!(out, "{years}Y");
    }
    if mons != 0 {
        let _ = write!(out, "{mons}M");
    }
    if days != 0 {
        let _ = write!(out, "{days}D");
    }
    if micros != 0 {
        out.push('T');
        let hours = micros / 3_600_000_000;
        let mins = micros / 60_000_000 % 60;
        let sec = micros % 60_000_000;
        if hours != 0 {
            let _ = write!(out, "{hours}H");
        }
        if mins != 0 {
            let _ = write!(out, "{mins}M");
        }
        if sec != 0 {
            if sec < 0 {
                out.push('-');
            }
            let a = sec.unsigned_abs();
            let _ = write!(out, "{}", a / 1_000_000);
            fraction((a % 1_000_000) as i64, out);
            out.push('S');
        }
    }
}

fn numeric(v: &[u8], out: &mut String) -> Result<()> {
    let o = oid::NUMERIC;
    if v.len() < 8 {
        return Err(bad(o, "too short"));
    }
    let nd = usize::from(u16::from_be_bytes([v[0], v[1]]));
    let weight = i32::from(i16::from_be_bytes([v[2], v[3]]));
    let sign = u16::from_be_bytes([v[4], v[5]]);
    let dscale = usize::from(u16::from_be_bytes([v[6], v[7]]));
    if v.len() != 8 + 2 * nd {
        return Err(bad(o, "wrong length"));
    }
    match sign {
        0xC000 => {
            out.push_str("NaN");
            return Ok(());
        }
        0xD000 => {
            out.push_str("Infinity");
            return Ok(());
        }
        0xF000 => {
            out.push_str("-Infinity");
            return Ok(());
        }
        0 | 0x4000 => {}
        _ => return Err(bad(o, "unknown sign")),
    }
    if dscale > 0x3FFF {
        return Err(bad(o, "scale out of range"));
    }
    let mut digits = Vec::with_capacity(nd);
    for c in v[8..].chunks_exact(2) {
        let d = u16::from_be_bytes([c[0], c[1]]);
        if d >= 10_000 {
            return Err(bad(o, "digit out of range"));
        }
        digits.push(d);
    }
    let dig = |i: i32| -> u16 {
        usize::try_from(i)
            .ok()
            .and_then(|i| digits.get(i).copied())
            .unwrap_or(0)
    };
    if sign == 0x4000 {
        out.push('-');
    }
    if weight < 0 {
        out.push('0');
    } else {
        let mut s = String::with_capacity((weight as usize + 1) * 4);
        for i in 0..=weight {
            let _ = write!(s, "{:04}", dig(i));
        }
        let t = s.trim_start_matches('0');
        out.push_str(if t.is_empty() { "0" } else { t });
    }
    if dscale > 0 {
        out.push('.');
        let mut f = String::with_capacity(dscale + 4);
        let mut k = 0;
        while f.len() < dscale {
            let _ = write!(f, "{:04}", dig(weight + 1 + k));
            k += 1;
        }
        f.truncate(dscale);
        out.push_str(&f);
    }
    Ok(())
}

fn inet(o: u32, v: &[u8], out: &mut String) -> Result<()> {
    if v.len() < 4 {
        return Err(bad(o, "too short"));
    }
    let (family, bits, nb) = (v[0], v[1], usize::from(v[3]));
    let addr = &v[4..];
    if addr.len() != nb {
        return Err(bad(o, "wrong length"));
    }
    let full = match (family, nb) {
        (2, 4) => 32,
        (3, 16) => 128,
        _ => return Err(bad(o, "unknown address family")),
    };
    if bits > full {
        return Err(bad(o, "netmask out of range"));
    }
    if family == 2 {
        let _ = write!(out, "{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3]);
    } else {
        ipv6(addr, out);
    }
    if o == oid::CIDR || bits != full {
        let _ = write!(out, "/{bits}");
    }
    Ok(())
}

/// The compression rules of PostgreSQL's `inet_net_ntop`.
fn ipv6(a: &[u8], out: &mut String) {
    let w: Vec<u16> = a
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let (mut best, mut best_len) = (usize::MAX, 0usize);
    let (mut cur, mut cur_len) = (usize::MAX, 0usize);
    for (i, x) in w.iter().enumerate() {
        if *x == 0 {
            if cur == usize::MAX {
                cur = i;
                cur_len = 1;
            } else {
                cur_len += 1;
            }
        } else if cur != usize::MAX {
            if best == usize::MAX || cur_len > best_len {
                best = cur;
                best_len = cur_len;
            }
            cur = usize::MAX;
        }
    }
    if cur != usize::MAX && (best == usize::MAX || cur_len > best_len) {
        best = cur;
        best_len = cur_len;
    }
    if best != usize::MAX && best_len < 2 {
        best = usize::MAX;
    }
    for i in 0..8 {
        if best != usize::MAX && i >= best && i < best + best_len {
            if i == best {
                out.push(':');
            }
            continue;
        }
        if i != 0 {
            out.push(':');
        }
        if i == 6
            && best == 0
            && (best_len == 6 || (best_len == 7 && w[7] != 1) || (best_len == 5 && w[5] == 0xffff))
        {
            let _ = write!(out, "{}.{}.{}.{}", a[12], a[13], a[14], a[15]);
            return;
        }
        let _ = write!(out, "{:x}", w[i]);
    }
    if best != usize::MAX && best + best_len == 8 {
        out.push(':');
    }
}

fn mac(v: &[u8], n: usize, o: u32, out: &mut String) -> Result<()> {
    if v.len() != n {
        return Err(bad(o, "wrong length"));
    }
    for (i, b) in v.iter().enumerate() {
        if i > 0 {
            out.push(':');
        }
        let _ = write!(out, "{b:02x}");
    }
    Ok(())
}

fn bits(o: u32, v: &[u8], out: &mut String) -> Result<()> {
    if v.len() < 4 {
        return Err(bad(o, "too short"));
    }
    let n = i32::from_be_bytes([v[0], v[1], v[2], v[3]]);
    let n = usize::try_from(n).map_err(|_| bad(o, "negative length"))?;
    let data = &v[4..];
    if data.len() != n.div_ceil(8) {
        return Err(bad(o, "wrong length"));
    }
    for i in 0..n {
        out.push(if data[i / 8] >> (7 - i % 8) & 1 == 1 {
            '1'
        } else {
            '0'
        });
    }
    Ok(())
}

fn push_element(s: &str, out: &mut String) {
    let quote = s.is_empty()
        || s.eq_ignore_ascii_case("NULL")
        || s.chars().any(|c| {
            matches!(c, '"' | '\\' | '{' | '}' | ',') || c.is_ascii_whitespace() || c == '\x0b'
        });
    if !quote {
        out.push_str(s);
        return;
    }
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

/// Render a binary array whose element type is `elem` as a PostgreSQL array literal.
pub(crate) fn render_array(elem: u32, v: &[u8], out: &mut String) -> Result<()> {
    let o = elem;
    let int = |at: usize| -> Result<i32> {
        v.get(at..at + 4)
            .map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| bad(o, "array too short"))
    };
    let ndim = int(0)?;
    let eoid = int(8)? as u32;
    if !(0..=6).contains(&ndim) {
        return Err(bad(o, "array dimensions out of range"));
    }
    if eoid != elem {
        return Err(bad(o, "array element type mismatch"));
    }
    let ndim = ndim as usize;
    let mut at = 12;
    let mut lens = Vec::with_capacity(ndim);
    let mut lbs = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        let (len, lb) = (int(at)?, int(at + 4)?);
        at += 8;
        if len < 1 {
            return Err(bad(o, "bad array dimension"));
        }
        lens.push(len as usize);
        lbs.push(lb);
    }
    if ndim == 0 {
        out.push_str("{}");
        return Ok(());
    }
    let mut total = 1usize;
    for l in &lens {
        total = total
            .checked_mul(*l)
            .filter(|t| *t <= v.len())
            .ok_or_else(|| bad(o, "array too large for message"))?;
    }
    if lbs.iter().any(|lb| *lb != 1) {
        for (len, lb) in lens.iter().zip(&lbs) {
            let hi = i64::from(*lb) + *len as i64 - 1;
            let _ = write!(out, "[{lb}:{hi}]");
        }
        out.push('=');
    }
    fn walk(elem: u32, lens: &[usize], v: &[u8], at: &mut usize, out: &mut String) -> Result<()> {
        out.push('{');
        for i in 0..lens[0] {
            if i > 0 {
                out.push(',');
            }
            if lens.len() > 1 {
                walk(elem, &lens[1..], v, at, out)?;
                continue;
            }
            let hdr = v
                .get(*at..*at + 4)
                .ok_or_else(|| bad(elem, "array too short"))?;
            let n = i32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
            *at += 4;
            if n == -1 {
                out.push_str("NULL");
                continue;
            }
            let n = usize::try_from(n).map_err(|_| bad(elem, "bad element length"))?;
            let bytes = v
                .get(*at..*at + n)
                .ok_or_else(|| bad(elem, "array too short"))?;
            *at += n;
            let mut s = String::new();
            render_scalar(elem, bytes, &mut s)?;
            push_element(&s, out);
        }
        out.push('}');
        Ok(())
    }
    walk(elem, &lens, v, &mut at, out)?;
    if at != v.len() {
        return Err(bad(o, "trailing bytes in array"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(nd: &[u16], weight: i16, sign: u16, dscale: u16) -> Vec<u8> {
        let mut b = (nd.len() as u16).to_be_bytes().to_vec();
        b.extend_from_slice(&weight.to_be_bytes());
        b.extend_from_slice(&sign.to_be_bytes());
        b.extend_from_slice(&dscale.to_be_bytes());
        for d in nd {
            b.extend_from_slice(&d.to_be_bytes());
        }
        b
    }

    fn r(o: u32, v: &[u8]) -> String {
        let mut s = String::new();
        render_scalar(o, v, &mut s).unwrap();
        s
    }

    #[test]
    fn numeric_cases() {
        let n = |v: Vec<u8>| r(oid::NUMERIC, &v);
        assert_eq!(n(num(&[], 0, 0, 0)), "0");
        assert_eq!(n(num(&[], 0, 0, 3)), "0.000");
        assert_eq!(n(num(&[1], 0, 0, 0)), "1");
        assert_eq!(n(num(&[1, 2345, 6789], 2, 0, 0)), "123456789");
        assert_eq!(n(num(&[1, 2345, 6789], 0, 0, 8)), "1.23456789");
        assert_eq!(n(num(&[1, 5000], 0, 0x4000, 1)), "-1.5");
        assert_eq!(n(num(&[1], -3, 0, 12)), "0.000000000001");
        assert_eq!(n(num(&[5, 1000], 0, 0, 2)), "5.10");
        assert_eq!(n(num(&[1], 1, 0, 0)), "10000");
        assert_eq!(n(num(&[], 0, 0xC000, 0)), "NaN");
        assert_eq!(n(num(&[], 0, 0xD000, 0)), "Infinity");
        assert_eq!(n(num(&[], 0, 0xF000, 0)), "-Infinity");
        assert!(render_scalar(oid::NUMERIC, &num(&[10_000], 0, 0, 0), &mut String::new()).is_err());
        assert!(render_scalar(oid::NUMERIC, &[0; 5], &mut String::new()).is_err());
    }

    #[test]
    fn time_and_interval() {
        assert_eq!(
            r(oid::TIME, &3_723_000_001i64.to_be_bytes()),
            "01:02:03.000001"
        );
        assert_eq!(r(oid::TIME, &0i64.to_be_bytes()), "00:00:00");
        let mut tz = 3_600_000_000i64.to_be_bytes().to_vec();
        tz.extend_from_slice(&(-19_800i32).to_be_bytes());
        assert_eq!(r(oid::TIMETZ, &tz), "01:00:00+05:30");
        let iv = |us: i64, d: i32, m: i32| {
            let mut b = us.to_be_bytes().to_vec();
            b.extend_from_slice(&d.to_be_bytes());
            b.extend_from_slice(&m.to_be_bytes());
            r(oid::INTERVAL, &b)
        };
        assert_eq!(iv(0, 0, 0), "PT0S");
        assert_eq!(iv(14_706_789_000, 3, 14), "P1Y2M3DT4H5M6.789S");
        assert_eq!(iv(-500_000, 0, 0), "PT-0.5S");
    }

    #[test]
    fn inet_and_friends() {
        let v4 = |bits: u8, cidr: u8| vec![2, bits, cidr, 4, 192, 168, 1, 0];
        assert_eq!(r(oid::INET, &v4(24, 0)), "192.168.1.0/24");
        assert_eq!(r(oid::INET, &v4(32, 0)), "192.168.1.0");
        assert_eq!(r(oid::CIDR, &v4(32, 1)), "192.168.1.0/32");
        let mut a = vec![3, 128, 0, 16];
        a.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(r(oid::INET, &a), "::1");
        let mut b = vec![3, 64, 0, 16];
        b.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(r(oid::INET, &b), "2001:db8::1/64");
        assert_eq!(r(oid::MACADDR, &[8, 0, 0x2b, 1, 2, 3]), "08:00:2b:01:02:03");
        assert_eq!(r(oid::BIT, &[0, 0, 0, 5, 0b1010_0000]), "10100");
    }

    #[test]
    fn arrays() {
        // int4[] {1,NULL,3}, one dimension.
        let mut b = Vec::new();
        for x in [1i32, 1, 23, 3, 1] {
            b.extend_from_slice(&x.to_be_bytes());
        }
        for e in [Some(1i32), None, Some(3)] {
            match e {
                Some(x) => {
                    b.extend_from_slice(&4i32.to_be_bytes());
                    b.extend_from_slice(&x.to_be_bytes());
                }
                None => b.extend_from_slice(&(-1i32).to_be_bytes()),
            }
        }
        let mut s = String::new();
        render_array(oid::INT4, &b, &mut s).unwrap();
        assert_eq!(s, "{1,NULL,3}");
        // text[] needing quotes, and the empty array.
        let mut t = Vec::new();
        for x in [1i32, 0, 25, 3, 1] {
            t.extend_from_slice(&x.to_be_bytes());
        }
        for e in ["a b", "", "NULL"] {
            t.extend_from_slice(&(e.len() as i32).to_be_bytes());
            t.extend_from_slice(e.as_bytes());
        }
        s.clear();
        render_array(oid::TEXT, &t, &mut s).unwrap();
        assert_eq!(s, r#"{"a b","","NULL"}"#);
        let mut e = Vec::new();
        for x in [0i32, 0, 23] {
            e.extend_from_slice(&x.to_be_bytes());
        }
        s.clear();
        render_array(oid::INT4, &e, &mut s).unwrap();
        assert_eq!(s, "{}");
        // Wrong element type and truncation are errors.
        assert!(render_array(oid::INT8, &b, &mut String::new()).is_err());
        assert!(render_array(oid::INT4, &b[..b.len() - 1], &mut String::new()).is_err());
    }
}
