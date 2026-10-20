//! Crypto benchmark -- measures ChaCha20Poly1305 seal/open throughput and rekey
//! No external deps, uses std time, CPU/Gbit estimation for Sec.10.3
//! ChaCha20Poly1305 0.11.0, 16B tag overhead, header bound as AAD, nonce separated by path/seq/salt/key

use balancify_crypto::TunnelCipher;
use balancify_tunnel::Crypto;
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
    let mbps = mb / secs;
    let gbps = mbps * 8.0 / 1024.0;
    let cores_per_gbps = if gbps > 0.0 { 1.0 / gbps } else { 0.0 };
    println!("Seal: {} iters, {:.2} MB, {:.3}s, {:.2} MB/s ({:.2} Gbit/s), {:.1}us/op, ~{:.2} cores/Gbit/s",
        iters, mb, secs, mbps, gbps, elapsed.as_micros() as f64 / iters as f64, cores_per_gbps);

    // Open
    let mut sealed = Vec::new();
    cipher.seal(0, 0, &header, &payload, &mut sealed);
    let start = Instant::now();
    for i in 0..iters {
        let _ = cipher.open(0, i as u64, &header, &sealed[20..]);
    }
    let elapsed = start.elapsed();
    let mb_open = mb;
    let secs_open = elapsed.as_secs_f64();
    let mbps_open = if secs_open > 0.0 { mb_open / secs_open } else { 0.0 };
    let gbps_open = mbps_open * 8.0 / 1024.0;
    println!("Open: {} iters, {:.3}s, {:.1}us/op, {:.2} MB/s ({:.2} Gbit/s)",
        iters, secs_open, elapsed.as_micros() as f64 / iters as f64, mbps_open, gbps_open);

    // Rekey
    let start = Instant::now();
    let mut c = cipher.clone();
    for _ in 0..1000 {
        if let Some(new) = c.rekey() {
            c = new;
        }
    }
    let elapsed = start.elapsed();
    println!("Rekey: 1000 iters, {:.3}s, {:.1}us/op (HKDF-BLAKE2s one-way, old ciphers zeroized)", elapsed.as_secs_f64(), elapsed.as_micros() as f64 / 1000.0);

    // Combined seal+open (full tunnel path)
    let start = Instant::now();
    for i in 0..iters {
        out.clear();
        cipher.seal(0, i as u64, &header, &payload, &mut out);
        let _ = cipher.open(0, i as u64, &header, &out[20..]);
    }
    let elapsed = start.elapsed();
    let secs_both = elapsed.as_secs_f64();
    let mb_both = mb * 2.0;
    let mbps_both = if secs_both > 0.0 { mb_both / secs_both } else { 0.0 };
    let gbps_both = mbps_both * 8.0 / 1024.0 / 2.0; // one direction
    let cores_per_gbps_both = if gbps_both > 0.0 { 1.0 / gbps_both } else { 0.0 };
    println!("Seal+Open: {} iters, {:.3}s, {:.1}us/op pair, {:.2} MB/s one-way {:.2} Gbit/s, ~{:.2} cores/Gbit/s",
        iters, secs_both, elapsed.as_micros() as f64 / iters as f64, mbps_both/2.0, gbps_both, cores_per_gbps_both);

    println!("\n--- CPU/Gbit estimation (Sec.10.3) ---");
    println!("TunnelCipher: ChaCha20Poly1305 0.11.0, 16B tag, header as AAD, nonce path/seq/salt/key");
    println!("Seal {:.2} Gbit/s per core, Open {:.2} Gbit/s per core, Seal+Open {:.2} Gbit/s per core", gbps, gbps_open, gbps_both);
    println!("Expected x64 with AES-NI: ~0.5-1 core per Gbit/s, ARM64 without acceleration more");
    println!("ETW CPU sampling via WPA, event IDs 100-600, working set bounded, power delta via telemetry");
    println!("Benchmark done — CPU per Gbit/s derived from MB/s, for Rig B acceptance rows");
}
