//! Many threads on a small pool: how many gets a second get through, and how long the slowest waits.
//!
//! `cargo run --release -p karez-pool --example contention -- THREADS POOL HOLD_US SECONDS`
//! (defaults 512, 32, 200, 3). A thread loops: get a connection, hold it for HOLD_US microseconds
//! (a sleep, as a server round trip would be), give it back. This is the shape of S8 in bench/: 512
//! workers on a pool of 32. The pool is the mock below, so no server is needed.

use karez_pool::{Manager, Pool};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Mock;
impl Manager for Mock {
    type Connection = ();
    type Error = ();
    fn connect(&self) -> Result<(), ()> {
        Ok(())
    }
    fn recycle(&self, _: &mut ()) -> bool {
        true
    }
}

fn main() {
    let a: Vec<u64> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let (threads, size, hold_us, secs) = (
        a.first().copied().unwrap_or(512) as usize,
        a.get(1).copied().unwrap_or(32) as usize,
        a.get(2).copied().unwrap_or(200),
        a.get(3).copied().unwrap_or(3),
    );
    let pool = Pool::new(Mock, size).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicU64::new(0));
    let worst = Arc::new(AtomicU64::new(0));
    let hs: Vec<_> = (0..threads)
        .map(|_| {
            let (p, s, d, w) = (pool.clone(), stop.clone(), done.clone(), worst.clone());
            std::thread::spawn(move || {
                while !s.load(Ordering::Relaxed) {
                    let t = Instant::now();
                    let g = p.get().unwrap();
                    w.fetch_max(t.elapsed().as_micros() as u64, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_micros(hold_us));
                    drop(g);
                    d.fetch_add(1, Ordering::Relaxed);
                }
            })
        })
        .collect();
    std::thread::sleep(Duration::from_secs(1)); // warm up
    let n0 = done.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_secs(secs));
    let n1 = done.load(Ordering::Relaxed);
    stop.store(true, Ordering::Relaxed);
    for h in hs {
        h.join().unwrap();
    }
    println!(
        "{threads} threads, pool {size}, hold {hold_us} us: {:.0} gets/s (ideal {:.0}), slowest wait {} ms",
        (n1 - n0) as f64 / secs as f64,
        size as f64 * 1e6 / (hold_us as f64 + 60.0),
        worst.load(Ordering::Relaxed) / 1000
    );
}
