//! DRR benchmark -- measures proportionality and throughput of DeficitRoundRobin
//! No external deps.

use balancify_tunnel::{Sender, Crypto};
use std::time::Instant;

#[derive(Clone)]
struct NoopCrypto;
impl Crypto for NoopCrypto {
    fn seal(&self, _path: u8, _seq: u64, _aad: &[u8], plain: &[u8], out: &mut Vec<u8>) {
        out.extend_from_slice(plain);
    }
    fn open(&self, _path: u8, _seq: u64, _aad: &[u8], cipher: &[u8]) -> Option<Vec<u8>> {
        Some(cipher.to_vec())
    }
}

fn main() {
    let payload = vec![0u8; 1200];
    let iters = 100_000;

    // Test proportionality 3000:1000 quanta
    let mut sender = Sender::new(1, NoopCrypto, vec![(0, 3000), (1, 1000)]);
    let mut counts = [0u64; 2];
    let start = Instant::now();
    for _ in 0..iters {
        if let Some(sent) = sender.send(&payload, start) {
            counts[sent.path as usize] += 1;
        }
    }
    let elapsed = start.elapsed();
    println!("DRR 3000:1000 over {} sends: path0={} path1={} ratio={:.2} (expected 3.0)",
        iters, counts[0], counts[1], counts[0] as f64 / counts[1].max(1) as f64);
    println!("Throughput: {:.2}k pps, {:.2} MB/s",
        iters as f64 / elapsed.as_secs_f64() / 1000.0,
        (iters * payload.len()) as f64 / elapsed.as_secs_f64() / (1024.0*1024.0));

    // Test with blocked paths (congestion)
    let mut sender2 = Sender::new(1, NoopCrypto, vec![(0, 2000), (1, 2000), (2, 2000)]);
    let mut counts2 = [0u64; 3];
    let blocked = [1u8];
    for _ in 0..iters {
        if let Some(sent) = sender2.send_excluding(&payload, start, &blocked) {
            counts2[sent.path as usize] += 1;
        }
    }
    println!("DRR with path1 blocked: path0={} path1={} path2={} (path1 should be 0)", counts2[0], counts2[1], counts2[2]);

    println!("DRR bench done");
}
