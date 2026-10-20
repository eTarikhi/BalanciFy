# ADR-0020: Throughput harness and benches (§10.3)

Date: 2026-10-19
Status: accepted

## Context

§10.3 requires throughput rows: aggregate, single-flow, failover, added delay, CPU/Gbit/s. Previously, no harness existed, only unit tests.

Need deterministic simulated harness + benches for DRR, pacing, crypto, plus real Rig B procedure.

## Decision

- `crates/service/src/bin/throughput.rs`: 10 reps, 1000×1200B, profiles:
  - bonded: 200 Mbps Wi-Fi 10ms + 50 Mbps cellular 50ms — measures gain vs single, target 42.8% mean 1.3ms vs 2.0ms single
  - lossy: 1% loss — measures penalty 16.7% vs clean
  - ECN: 80% cwnd reduction on CE — measures reduction
  - delay: 10% reduction on RTT min+20ms & 1.25x — measures Vegas-like
  - outputs mean/median/p95/spread per §10.1
- `crates/core/src/bin/drr-bench.rs`: 100k sends, 3000:1000 quanta ratio check (target 3:1), throughput k pps MB/s, blocked path exclusion
- `crates/core/src/bin/pacing-bench.rs`: rate 10 Mbps, burst 14KB, 10k sends, measures pacing delay, burst spread ~11ms
- `crates/crypto/src/bin/bench.rs`: seal/open/rekey MB/s, us/op, Gbit/s for CPU/Gbit/s metric
- `docs/phase2/throughput-rows.md`: simulated results, Rig B procedure (wired 1 Gbps, Wi-Fi 6 300 Mbit/s, secure mode, pacing 80% capacity, per-flow off/on, 10 reps, iperf3)
- `docs/phase2/clean-machine-test.md`: offline bundle, journaled rollback, HVCI trio, 10 reps checklist, Hyper-V automation
- `tools/ci/sbom.sh`: SPDX 3.0 stub lists 6 vetted crates + first-party, no external deps

## Consequences

- Positive: deterministic harness for CI, real Rig B procedure documented, CPU/Gbit/s via crypto-bench + ETW
- Negative: simulated not netem — real measurements pending, but harness exists
- Neutral: no change to data path

## References

- Blueprint §10.3 acceptance rows, §10.1 10 reps median/p95/spread, §6.4 DRR+pacing+CC, S13 SBOM/VEX
- Code: `crates/service/src/bin/throughput.rs`, `crates/core/src/bin/drr-bench.rs`, `pacing-bench.rs`, `crates/crypto/src/bin/bench.rs`
- Docs: `docs/phase2/throughput-rows.md`, `clean-machine-test.md`, `sbom.sh`
- CI: green 39207636405 (ADR-0014) + 39208040465 (bench fixes)
- ADRs: ADR-0014 throughput harness, ADR-0020 enhanced
