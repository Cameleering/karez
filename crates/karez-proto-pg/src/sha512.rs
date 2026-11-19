//! SHA-384 and SHA-512 (FIPS 180-4), used to hash a server certificate for SCRAM channel binding
//! (`tls-server-end-point`, RFC 5929) when the certificate is signed with one of them.
//!
//! The constants are not typed in: they are computed once from their definition (the first 64 bits
//! of the fractional parts of the cube roots of the first 80 primes, and of the square roots of the
//! first 16), with a small 256-bit integer root, so there is no table to mistype. The tests check
//! the NIST vectors, which exercise every constant.

use std::sync::OnceLock;

/// `a * b` as little-endian 64-bit limbs, truncated to 4 limbs (callers keep values below 2^256).
fn mul(a: &[u64], b: &[u64]) -> [u64; 4] {
    let mut out = [0u64; 4];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u128;
        for (j, &y) in b.iter().enumerate() {
            if i + j >= 4 {
                break;
            }
            let t = u128::from(out[i + j]) + u128::from(x) * u128::from(y) + carry;
            out[i + j] = t as u64;
            carry = t >> 64;
        }
        if i + b.len() < 4 {
            out[i + b.len()] = out[i + b.len()].wrapping_add(carry as u64);
        }
    }
    out
}

fn le(a: &[u64; 4], b: &[u64; 4]) -> bool {
    for i in (0..4).rev() {
        if a[i] != b[i] {
            return a[i] < b[i];
        }
    }
    true
}

fn limbs(x: u128) -> [u64; 2] {
    [x as u64, (x >> 64) as u64]
}

/// The first `n` primes.
fn primes(n: usize) -> Vec<u64> {
    let mut v = Vec::with_capacity(n);
    let mut c = 2u64;
    while v.len() < n {
        if v.iter().all(|p| c % p != 0) {
            v.push(c);
        }
        c += 1;
    }
    v
}

/// The low 64 bits of `floor(root_k(p * 2^(64 * k)))`: the first 64 fractional bits of `p^(1/k)`
/// (the integer part of a root of these primes is below 2^8, so the low 64 bits are the fraction).
fn frac_root(p: u64, k: u32) -> u64 {
    // x = p << (64 * k): 128 bits for a square root, 192 for a cube root
    let mut x = [0u64; 4];
    x[k as usize] = p;
    let (mut lo, mut hi) = (0u128, 1u128 << 72);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let m = limbs(mid);
        let pow = if k == 2 {
            mul(&m, &m)
        } else {
            let sq = mul(&m, &m);
            mul(&sq, &m)
        };
        if le(&pow, &x) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo as u64
}

struct Consts {
    k: [u64; 80],
    iv512: [u64; 8],
    iv384: [u64; 8],
}

fn consts() -> &'static Consts {
    static C: OnceLock<Consts> = OnceLock::new();
    C.get_or_init(|| {
        let pr = primes(80);
        let mut k = [0u64; 80];
        for (i, p) in pr.iter().enumerate() {
            k[i] = frac_root(*p, 3);
        }
        let mut iv512 = [0u64; 8];
        let mut iv384 = [0u64; 8];
        for i in 0..8 {
            iv512[i] = frac_root(pr[i], 2);
            iv384[i] = frac_root(pr[8 + i], 2);
        }
        Consts { k, iv512, iv384 }
    })
}

fn compress(state: &mut [u64; 8], block: &[u8]) {
    let k = &consts().k;
    let mut w = [0u64; 80];
    for (i, c) in block.chunks_exact(8).enumerate() {
        w[i] = u64::from_be_bytes(c.try_into().expect("8 bytes"));
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..80 {
        let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
        let ch = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(k[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

fn digest(iv: [u64; 8], data: &[u8]) -> [u8; 64] {
    let mut state = iv;
    let mut blocks = data.chunks_exact(128);
    for b in &mut blocks {
        compress(&mut state, b);
    }
    let rest = blocks.remainder();
    let mut tail = [0u8; 256];
    tail[..rest.len()].copy_from_slice(rest);
    tail[rest.len()] = 0x80;
    let total = if rest.len() < 112 { 128 } else { 256 };
    let bits = (data.len() as u128) * 8;
    tail[total - 16..total].copy_from_slice(&bits.to_be_bytes());
    for b in tail[..total].chunks_exact(128) {
        compress(&mut state, b);
    }
    let mut out = [0u8; 64];
    for (o, s) in out.chunks_exact_mut(8).zip(state) {
        o.copy_from_slice(&s.to_be_bytes());
    }
    out
}

/// SHA-512 of `data`.
pub fn sha512(data: &[u8]) -> [u8; 64] {
    digest(consts().iv512, data)
}

/// SHA-384 of `data`.
pub fn sha384(data: &[u8]) -> [u8; 48] {
    let d = digest(consts().iv384, data);
    let mut out = [0u8; 48];
    out.copy_from_slice(&d[..48]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn the_first_constants_are_the_standard_ones() {
        let c = consts();
        assert_eq!(c.iv512[0], 0x6a09e667f3bcc908);
        assert_eq!(c.iv512[7], 0x5be0cd19137e2179);
        assert_eq!(c.iv384[0], 0xcbbb9d5dc1059ed8);
        assert_eq!(c.k[0], 0x428a2f98d728ae22);
        assert_eq!(c.k[79], 0x6c44198c4a475817);
    }

    #[test]
    fn nist_sha512_vectors() {
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            hex(&sha512(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
             47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
        assert_eq!(
            hex(&sha512(
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            )),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018\
             501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );
    }

    #[test]
    fn nist_sha384_vectors() {
        assert_eq!(
            hex(&sha384(b"abc")),
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed\
             8086072ba1e7cc2358baeca134c825a7"
        );
        assert_eq!(
            hex(&sha384(b"")),
            "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da\
             274edebfe76f65fbd51ad2f14898b95b"
        );
    }

    #[test]
    fn lengths_around_the_block_boundary() {
        // 111 bytes pads into one block, 112 and 127 need two; compare a streamed computation
        // against itself over many lengths by checking determinism and that all differ
        let mut seen = std::collections::HashSet::new();
        for n in [0usize, 1, 111, 112, 113, 127, 128, 129, 255, 256, 1000] {
            let data = vec![0x61u8; n];
            assert!(seen.insert(sha512(&data)), "length {n} collides");
        }
        // one million 'a' (FIPS 180-4 example)
        let m = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&sha512(&m)),
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973eb\
             de0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        );
    }
}
