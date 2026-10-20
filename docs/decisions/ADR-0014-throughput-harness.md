# ADR-0014: Throughput harness §10.3

Date: 2026-10-19
Status: accepted

## Context

Blueprint §10.3 requires throughput rows: aggregate, single-flow, failover, added delay, CPU per Gbit/s, with 10 reps, median/p95/spread, recorded hardware/build/driver/uplink/relay region.

Previously, only simulated e2e tests existed, no harness for throughput.

## Decision

- Add `crates/service/src/bin/throughput.rs` — deterministic harness:
  - 10 reps, 1000 packets of 1200B each, no external deps, uses pacing+CC
  - Profiles: bonded (200 Mbit/s Wi-Fi 10ms + 50 Mbit/s cellular 50ms, DRR quanta from capacity), single (bond_enabled=false), lossy (1% loss on path1), ECN (10 CE events), delay (RTT min 10ms → 50ms)
  - Measures mean/median/p95/spread, gain %, loss penalty
  - Prints ECN/delay diag, cwnd after events
- Add `crates/core/src/bin/drr-bench.rs` — DRR proportionality 3000:1000, throughput k pps and MB/s, blocked path handling
- Add `crates/crypto/src/bin/bench.rs` — seal/open/rekey throughput, MB/s, us/op, for CPU per Gbit/s reporting
- Document methodology and simulated results in `docs/phase2/throughput-rows.md`
- Real Rig B procedure documented in same file, pending hardware

## Consequences

- Positive: can run simulated throughput in CI, no netem needed, deterministic
- Negative: simulated, not real netem — real measurements still pending on Rig B
- Neutral: no change to data path, only measurement

## References

- Blueprint §10.3 acceptance rows, §6.4 rule 2 DRR, §6.4 rule 4 pacing+CC, §6.3 capacity
- Code: `throughput.rs`, `drr-bench.rs`, `crypto/bench.rs`
- Docs: `throughput-rows.md`, `lab-progress.md`, `clean-machine-test.md`
