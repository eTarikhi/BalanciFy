//! Crypto benchmark — measures ChaCha20Poly1305 seal/open throughput and rekey
//! No external deps, uses std time.

use balancify_crypto::TunnelCipher;
use std::time::Instant;

fn main() {
    let key = [0x42u8; 32];
    let salt = [0x01u8, 0x02, 0x03];
    let cipher = TunnelCipher::new(&key, salt).unwrap();
    let payload = vec![0u8; 1200];
    let header = [0u8; 20];

    let iters = 100_000;
    let start = Instant::now();
    let mut out = Vec::with_capacity(1500);
    for i in 0..iters {
        out.clear();
        cipher.seal(0, i as u64, &header, &payload, &mut out);
    }
    let elapsed = start.elapsed();
    let mb = (iters * payload.len()) as f64 / (1024.0 * 1024.0);
    let secs = elapsed.as_secs_f64();
    println!("Seal: {} iters, {:.2} MB, {:.3}s, {:.2} MB/s, {:.1}us/op",
        iters, mb, secs, mb/secs, elapsed.as_micros() as f64 / iters as f64);

    // Open
    let mut sealed = Vec::new();
    cipher.seal(0, 0, &header, &payload, &mut sealed);
    let start = Instant::now();
    let mut plain_out = vec![0u8; 1500];
    for i in 0..iters {
        let _ = cipher.open(0, i as u64, &header, &sealed[20..], &mut plain_out);
    }
    let elapsed = start.elapsed();
    println!("Open: {} iters, {:.3}s, {:.1}us/op", iters, elapsed.as_secs_f64(), elapsed.as_micros() as f64 / iters as f64);

    // Rekey
    let start = Instant::now();
    let mut c = cipher.clone();
    for _ in 0..1000 {
        if let Some(new) = c.rekey() {
            c = new;
        }
    }
    let elapsed = start.elapsed();
    println!("Rekey: 1000 iters, {:.3}s, {:.1}us/op", elapsed.as_secs_f64(), elapsed.as_micros() as f64 / 1000.0);

    println!("Benchmark done — CPU per Gbit/s can be derived from MB/s");
}
