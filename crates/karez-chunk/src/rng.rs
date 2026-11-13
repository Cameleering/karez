//! A tiny deterministic PRNG (SplitMix64) for randomised tests across the workspace.
//!
//! Not cryptographic. It exists so tests need no dependency.

/// SplitMix64, from Steele, Lea and Flood, "Fast splittable pseudorandom number generators"
/// (OOPSLA 2014).
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// A generator with the given seed.
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// The next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// A value in `0..bound` (`bound` must be non-zero; slight modulo bias is acceptable here).
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0);
        self.next_u64() % bound
    }

    /// A value in `lo..=hi`.
    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        assert!(lo <= hi);
        lo + self.below((hi - lo + 1) as u64) as usize
    }

    /// True with probability `num / den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    /// `n` random bytes.
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next_u64() as u8).collect()
    }

    /// A random valid UTF-8 string of up to `max_chars` characters, mixing ASCII and multi-byte.
    pub fn string(&mut self, max_chars: usize) -> String {
        let n = self.range(0, max_chars);
        let mut s = String::new();
        for _ in 0..n {
            let c = match self.below(4) {
                0 => char::from_u32(0x20 + self.below(0x5f) as u32),
                1 => char::from_u32(0xA0 + self.below(0x500) as u32),
                2 => char::from_u32(0x4E00 + self.below(0x1000) as u32),
                _ => char::from_u32(0x1F300 + self.below(0x100) as u32),
            };
            s.push(c.unwrap_or('?'));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_sequence_for_seed_zero() {
        // Reference values for SplitMix64 seeded with 0 (widely published test vector).
        let mut r = SplitMix64::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(r.next_u64(), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn range_is_inclusive_and_bounded() {
        let mut r = SplitMix64::new(7);
        for _ in 0..1000 {
            let v = r.range(3, 5);
            assert!((3..=5).contains(&v));
        }
    }
}
