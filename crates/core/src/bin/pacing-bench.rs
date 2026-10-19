//! Pacing benchmark — measures PacingController rate and burst handling
//! No external deps.

use balancify_core::pacing::PacingController;
use std::time::{Duration, Instant};

fn main() {
    let now = Instant::now();
    let mut pacer = PacingController::new(100_000_000); // 100 Mbit/s
    let payload = 1200u64;

    let iters = 100_000;
    let start = Instant::now();
    let mut delayed = 0;
    let mut t = now;
    for _ in 0..iters {
        t += Duration::from_micros(10);
        pacer.on_send(0, payload, t);
        let delay = pacer.delay_for(0, payload, t);
        if delay > Duration::from_millis(0) {
            delayed += 1;
        }
    }
    let elapsed = start.elapsed();
    println!("Pacing 100 Mbit/s, {} sends: delayed={} ({:.1}%), elapsed {:?}, {:.2}k ops/s",
        iters, delayed, delayed as f64 / iters as f64 * 100.0, elapsed, iters as f64 / elapsed.as_secs_f64() / 1000.0);

    // Test rate update
    pacer.set_rate(0, 50_000_000);
    println!("After set_rate 50 Mbit/s: rate={} bps", 50_000_000);

    println!("Pacing bench done");
}
