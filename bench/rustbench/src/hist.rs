//! A fixed-layout latency histogram, the same in every adapter language (see bench/README.md).
//!
//! Values are nanoseconds. Index of a value `v`: below 128 the value itself; otherwise
//! `shift = floor(log2 v) - 6`, `idx = shift * 64 + (v >> shift)`. That is 64 buckets per power
//! of two, a relative error of at most 1/64 (1.6%). Lock-free, so any thread may record.

use std::sync::atomic::{AtomicU64, Ordering};

pub const BUCKETS: usize = 64 * 60;

pub fn index(v: u64) -> usize {
    if v < 128 {
        v as usize
    } else {
        let msb = 63 - v.leading_zeros() as u64;
        let shift = msb - 6;
        ((shift << 6) + (v >> shift)) as usize
    }
}

#[cfg(test)]
pub fn lower_bound(idx: usize) -> u64 {
    let idx = idx as u64;
    if idx < 128 {
        idx
    } else {
        let shift = (idx >> 6) - 1;
        (idx - (shift << 6)) << shift
    }
}

#[derive(Debug)]
pub struct Hist {
    b: Vec<AtomicU64>,
}

impl Default for Hist {
    fn default() -> Self {
        Self::new()
    }
}

impl Hist {
    pub fn new() -> Hist {
        Hist {
            b: (0..BUCKETS).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    pub fn record(&self, ns: u64) {
        let i = index(ns).min(BUCKETS - 1);
        self.b[i].fetch_add(1, Ordering::Relaxed);
    }

    pub fn count(&self) -> u64 {
        self.b.iter().map(|c| c.load(Ordering::Relaxed)).sum()
    }

    /// Sparse JSON: `[[idx,count],...]`.
    pub fn to_json(&self) -> String {
        let mut s = String::from("[");
        let mut first = true;
        for (i, c) in self.b.iter().enumerate() {
            let c = c.load(Ordering::Relaxed);
            if c > 0 {
                if !first {
                    s.push(',');
                }
                first = false;
                s.push_str(&format!("[{i},{c}]"));
            }
        }
        s.push(']');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_lower_bound_agree() {
        for v in [0u64, 1, 63, 64, 127, 128, 129, 255, 256, 1000, 12_345, 1_000_000, 987_654_321] {
            let lb = lower_bound(index(v));
            assert!(lb <= v, "{v} {lb}");
            // within 1/64 above
            assert!(v - lb <= (v >> 6) + 1, "{v} {lb}");
        }
        for i in 1..BUCKETS {
            assert!(lower_bound(i) > lower_bound(i - 1), "monotone at {i}");
        }
    }
}
