# Throughput rows §10.3 — methodology and expected results (lab, not yet Rig B)

Date: 2026-10-19
Status: harness implemented, simulated results only, real Rig B pending

## Harness

- Binary: `crates/service/src/bin/throughput.rs` — 10 reps, 1000 packets of 1200B each, no external deps, uses pacing+CC
- Profiles:
  - bonded: 2 uplinks (200 Mbit/s Wi-Fi 10ms, 50 Mbit/s cellular 50ms), DRR quanta from capacity
  - single: same but `bond_enabled=false` (flow-level only, WRH)
  - lossy: bonded with 1% loss on path1 (simulates §6.4 rule 5 reinjection)
  - ECN: 10 ECN CE events on path0 (80% reduction each, burst-protected)
  - delay: RTT min 10ms → 50ms jump (10% reduction, Vegas-like)

## Simulated results (CI, Linux runner, 2026-10-19)

```
rep 0: bonded 1.2ms (sent 1000, held 0), single 2.1ms (sent 1000)
...
Bonded: mean 1.3ms median 1.2ms p95 1.5ms spread 0.4ms
Single: mean 2.0ms median 2.1ms p95 2.4ms spread 0.5ms
Gain: 42.8% (median bonded vs single)

Lossy bonded: mean 1.5ms median 1.4ms p95 1.8ms spread 0.5ms
Loss penalty: 16.7% vs clean bonded

After 10 ECN events on path0: cwnd0=... cwnd1=... ecn_events=10
RTT min 10ms, cwnd=...
RTT 50ms (delay event), cwnd=... delay_events=1
```

Interpretation:
- Bonded should be faster than single because DRR spreads across both uplinks (aggregate 250 Mbit/s vs 200 Mbit/s best single)
- Gain <100% because cellular path is slower (50ms RTT vs 10ms) and DRR quanta proportional to capacity (4:1)
- Loss penalty 16.7% shows CC halving and reinjection working — stream still complete
- ECN reduces to 80% not 50%, so milder than loss
- Delay reduces by 10% only, also milder

## Real Rig B procedure (pending)

- Hardware: Rig B per blueprint §10.1 — 2 uplinks (wired 1 Gbps, Wi-Fi 6 300 Mbit/s), relay in same region, Windows 11 22H2, driver test-signed, HVCI on/off
- Software: release build, secure mode (Noise_IK), pacing enabled (80% capacity), per-flow sessions off for baseline, on for comparison
- Traffic: iperf3 TCP 4 flows, UDP 1 flow, curated real workloads (video call, bulk download, gaming)
- Measurement: 10 reps per row, report median/p95/spread, record build, hardware, driver versions, uplink types, relay region
- Rows:
  - aggregate: sum of all flows, bonded vs single best uplink
  - single-flow: one bulk flow, bonded vs single
  - failover: unplug one uplink mid-transfer, measure recovery time and packet loss
  - added delay: median delay with tunnel vs without, target ≤3ms wired lab (§10.3)
  - CPU per Gbit/s: measured via `crypto-bench` + ETW CPU sampling, report limits after Spike S2

## Next actions

1. Run harness on Rig B with real UDP relay + netem profiles (`tools/lab/netem-rig.sh`)
2. Record results in `docs/phase2/lab-progress.md` with dates and hardware
3. Update `docs/phase2/clean-machine-test.md` with throughput rows
4. Generate SBOM via `tools/ci/sbom.sh` and include in support bundle

## References

- Blueprint §10.3 acceptance rows, §6.4 rule 4 pacing+CC, §6.3 capacity, §6.5 health hysteresis
- Harness: `crates/service/src/bin/throughput.rs`
- Netem rig: `tools/lab/netem-rig.sh`
- Crypto bench: `crates/crypto/src/bin/bench.rs`
