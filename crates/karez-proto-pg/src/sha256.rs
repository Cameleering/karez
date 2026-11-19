//! SHA-256 (FIPS 180-4), HMAC-SHA-256 (RFC 2104 / RFC 4231) and PBKDF2-HMAC-SHA-256 (RFC 8018).
//!
//! The round constants are derived at compile time from their definition (the first 32 bits of
//! the fractional parts of the cube roots of the first 64 primes, and of the square roots of the
//! first 8 primes), so there is no table to mistype. Tests check NIST and RFC vectors.

const fn first_primes<const N: usize>() -> [u64; N] {
    let mut out = [0u64; N];
    let mut found = 0;
    let mut n = 2u64;
    while found < N {
        let mut is_prime = true;
        let mut d = 2u64;
        while d * d <= n {
            if n % d == 0 {
                is_prime = false;
                break;
            }
            d += 1;
        }
        if is_prime {
            out[found] = n;
            found += 1;
        }
        n += 1;
    }
    out
}

/// floor(cbrt(x)) by binary search; `x < 2^108` so the cube of the midpoint fits in u128.
const fn icbrt(x: u128) -> u128 {
    let mut lo = 0u128;
    let mut hi = 1u128 << 37;
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if mid * mid * mid <= x {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

const fn isqrt(x: u128) -> u128 {
    let mut lo = 0u128;
    let mut hi = 1u128 << 40;
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if mid * mid <= x {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

const K: [u32; 64] = {
    let p = first_primes::<64>();
    let mut k = [0u32; 64];
    let mut i = 0;
    while i < 64 {
        // cbrt(p) * 2^32 = cbrt(p * 2^96); the low 32 bits are the fractional part.
        k[i] = (icbrt((p[i] as u128) << 96) & 0xFFFF_FFFF) as u32;
        i += 1;
    }
    k
};

const H0: [u32; 8] = {
    let p = first_primes::<8>();
    let mut h = [0u32; 8];
    let mut i = 0;
    while i < 8 {
        h[i] = (isqrt((p[i] as u128) << 64) & 0xFFFF_FFFF) as u32;
        i += 1;
    }
    h
};

/// Incremental SHA-256.
#[derive(Clone, Debug)]
pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// A fresh hasher.
    pub fn new() -> Self {
        Sha256 {
            state: H0,
            buf: [0; 64],
            buf_len: 0,
            total: 0,
        }
    }

    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
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

    /// Absorb more input.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                Self::compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }
        let mut blocks = data.chunks_exact(64);
        for b in &mut blocks {
            let mut block = [0u8; 64];
            block.copy_from_slice(b);
            Self::compress(&mut self.state, &block);
        }
        let rest = blocks.remainder();
        if !rest.is_empty() {
            self.buf[..rest.len()].copy_from_slice(rest);
            self.buf_len = rest.len();
        }
    }

    /// Finish and return the digest.
    pub fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total.wrapping_mul(8);
        let mut pad = [0u8; 72];
        pad[0] = 0x80;
        let pad_len = if self.buf_len < 56 {
            56 - self.buf_len
        } else {
            120 - self.buf_len
        };
        pad[pad_len..pad_len + 8].copy_from_slice(&bit_len.to_be_bytes());
        // `update` would add to `total`, which no longer matters.
        self.update(&pad[..pad_len + 8]);
        debug_assert_eq!(self.buf_len, 0);
        let mut out = [0u8; 32];
        for (o, s) in out.chunks_exact_mut(4).zip(self.state) {
            o.copy_from_slice(&s.to_be_bytes());
        }
        out
    }
}

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize()
}

/// HMAC-SHA-256 with precomputed key pads, cheap to reuse (as PBKDF2 does).
#[derive(Clone, Debug)]
pub struct HmacSha256 {
    inner: Sha256,
    outer: Sha256,
}

impl HmacSha256 {
    /// Key the MAC.
    pub fn new(key: &[u8]) -> Self {
        let mut block = [0u8; 64];
        if key.len() > 64 {
            block[..32].copy_from_slice(&sha256(key));
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        let mut ipad = [0x36u8; 64];
        let mut opad = [0x5cu8; 64];
        for i in 0..64 {
            ipad[i] ^= block[i];
            opad[i] ^= block[i];
        }
        let mut inner = Sha256::new();
        inner.update(&ipad);
        let mut outer = Sha256::new();
        outer.update(&opad);
        HmacSha256 { inner, outer }
    }

    /// MAC of the concatenation of `parts`.
    pub fn mac(&self, parts: &[&[u8]]) -> [u8; 32] {
        let mut inner = self.inner.clone();
        for p in parts {
            inner.update(p);
        }
        let ih = inner.finalize();
        let mut outer = self.outer.clone();
        outer.update(&ih);
        outer.finalize()
    }
}

/// HMAC-SHA-256 of `data` under `key`.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    HmacSha256::new(key).mac(&[data])
}

/// PBKDF2 with HMAC-SHA-256, filling `out`. `iterations` must be at least 1.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    assert!(iterations >= 1);
    let prf = HmacSha256::new(password);
    for (block_index, chunk) in out.chunks_mut(32).enumerate() {
        let counter = (block_index as u32 + 1).to_be_bytes();
        let mut u = prf.mac(&[salt, &counter]);
        let mut t = u;
        for _ in 1..iterations {
            u = prf.mac(&[&u]);
            for (a, b) in t.iter_mut().zip(u) {
                *a ^= b;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

#[cfg(test)]
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_constants_match_the_standard() {
        // First and last round constants and first/last initial hash values from FIPS 180-4.
        assert_eq!(K[0], 0x428a2f98);
        assert_eq!(K[1], 0x71374491);
        assert_eq!(K[63], 0xc67178f2);
        assert_eq!(H0[0], 0x6a09e667);
        assert_eq!(H0[7], 0x5be0cd19);
    }

    #[test]
    fn sha256_nist_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let mut h = Sha256::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            h.update(&chunk);
        }
        assert_eq!(
            hex(&h.finalize()),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn sha256_incremental_equals_one_shot_at_every_split() {
        let data: Vec<u8> = (0..300u32).map(|i| (i * 7 + 3) as u8).collect();
        let want = sha256(&data);
        for split in 0..=data.len() {
            let mut h = Sha256::new();
            h.update(&data[..split]);
            h.update(&data[split..]);
            assert_eq!(h.finalize(), want, "split {split}");
        }
        // Lengths around the padding boundaries.
        for len in 0..140 {
            let d = &data[..len.min(data.len())];
            let mut h = Sha256::new();
            for b in d {
                h.update(&[*b]);
            }
            assert_eq!(h.finalize(), sha256(d));
        }
    }

    #[test]
    fn hmac_rfc4231_vectors() {
        // Test case 1
        assert_eq!(
            hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        // Test case 2
        assert_eq!(
            hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 3
        assert_eq!(
            hex(&hmac_sha256(&[0xaa; 20], &[0xdd; 50])),
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe"
        );
        // Test case 6: key longer than the block size
        assert_eq!(
            hex(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn pbkdf2_vectors() {
        let mut out = [0u8; 32];
        pbkdf2_hmac_sha256(b"password", b"salt", 1, &mut out);
        assert_eq!(
            hex(&out),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        pbkdf2_hmac_sha256(b"password", b"salt", 2, &mut out);
        assert_eq!(
            hex(&out),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
        pbkdf2_hmac_sha256(b"password", b"salt", 4096, &mut out);
        assert_eq!(
            hex(&out),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
    }
}
