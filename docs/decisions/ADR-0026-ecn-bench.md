# ADR-0026: ECN bench for cwnd reduction verification

Date: 2026-10-19
Status: accepted

## Context

Throughput harness (ADR-0014) measures bonded gain, loss penalty, ECN 80% reduction, delay 10% reduction via simulated profiles, but no dedicated bench for ECN vs loss cwnd reduction.

Previously, DRR bench and pacing bench existed, but no ECN bench.

## Decision

- `crates/core/src/bin/ecn-bench.rs`: measures cwnd before/after ECN CE (80% reduction vs 50% loss), delay detection (10ms -> 50ms triggers 10% reduction), per-path ECN CE count, delay events, rtt_min, rtt_last
  - Steps: grow window with 10 RTTs, record before, trigger `on_ecn`, record after ECN 80%, grow again 5 RTTs, trigger `on_loss` 50%, trigger `on_rtt` 10ms -> 50ms delay
  - Outputs cwnd bytes, reduction %, ECN events, loss events, delay events, per-path ECN/delay/rtt_min/last
  - Verifies ECN 80% vs loss 50% (ECN milder), delay 10% reduction
- `crates/core/Cargo.toml`: add `[[bin]] ecn-bench` entry
- No new dependencies, uses existing `CongestionController`

## Consequences

- Positive: dedicated bench for ECN vs loss cwnd reduction, per-path ECN/delay/rtt, useful for Sec.10.3 CPU/Gbit/s and for verifying Sec.6.4 rule 4 ECN extension
- Negative: more bench binary, but same pattern as DRR and pacing benches
- Neutral: no change to data path

## References

- Blueprint Sec.6.4 rule 4 ECN+delay, Sec.10.3 acceptance rows, Sec.8.2 diagnostics
- Code: `crates/core/src/bin/ecn-bench.rs`, `crates/core/Cargo.toml`
- CI: 39236639218 success
- ADR-0014 throughput harness, ADR-0026 ECN bench
