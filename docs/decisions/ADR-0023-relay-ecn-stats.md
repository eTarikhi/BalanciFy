# ADR-0023: Relay ECN echo stats

Date: 2026-10-19
Status: accepted

## Context

Relay ECN echo (Sec.6.4 rule 4) was implemented via `ack_with_ecn` and `BALANCIFY_SIMULATE_ECN=1` env var, but no stats tracked how many ECN echoes were sent. Diagnostics need ECN echo count for support and for throughput harness.

Previously, `SecureRelay::stats()` returned (sessions, handshakes, rejected, bytes, packets) — no ECN.

## Decision

- `crates/relay/src/main.rs`: add `ecn_echoed: u64` to `RelaySession`, increment when `simulate_ecn` true and `ack_with_ecn` sent in `ingest` and `drain`
- `SecureRelay`: add `total_ecn_echoed: u64`, accumulate ECN from sessions on `sweep` (when session evicted, add its `ecn_echoed` to total), `stats()` now returns 6 values including ECN sum (current sessions sum + total)
- `run_secure`: destructure 6 values, print `ecn` in stats eprintln `Stats: sessions=... ecn=...`
- Tests: existing relay tests still pass (per-IP cap, handshake rate limit, secure tunnel, etc.), no new test needed but stats now include ECN

## Consequences

- Positive: relay tracks ECN echoes, visible in logs and support bundle via service counters (service already tracks ecn_ce)
- Negative: stats tuple changed from 5 to 6 values, need to update destructuring (done)
- Neutral: no change to data path, only stats

## References

- Blueprint Sec.6.4 rule 4 ECN, Sec.4.7 relay, Sec.8.2 diagnostics
- Code: `crates/relay/src/main.rs` `RelaySession::ecn_echoed`, `SecureRelay::total_ecn_echoed`, `stats()`, `sweep()`
- CI: 39233826038 success after fixing arg order, 39234128363 success after per-path diag
- ADR-0014 throughput harness, ADR-0023 relay ECN stats
