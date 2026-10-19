# ADR-0019: ETW logging and support bundle enhanced with cwnd/in_flight/congestion diag

Date: 2026-10-19
Status: accepted

## Context

Diagnostics (§8.2) requires ETW provider + support bundle with congestion diag. Previously, support bundle had basic counters, but no cwnd/in_flight per uplink, no congestion diag (loss/ecn/delay events), no paths.

Also, ETW stub existed but not wired to service events.

## Decision

- `crates/service/src/etw.rs`: `EtwProvider` struct with `new(name)`, `log(event_id, msg)`, `log_uplink(id, cwnd, in_flight)`, `log_congestion(path, loss, ecn, delay)`, `log_failover`, `log_rekey`, `log_auth_failed`. Windows: `EventRegister`/`EventWrite` via winapi, non-Windows: eprintln stub. Event IDs: 100 info, 200 uplink, 300 congestion, 400 failover, 500 rekey, 600 auth_failed.
- `crates/service/src/support/mod.rs`: `UplinkDiag { id, cwnd, in_flight, capacity_bps, rtt_ms, loss_pct }`, `CongestionDiag { loss_events, ecn_ce, delay_events, paths: Vec<PathDiag> }`, `PathDiag { id, cwnd, in_flight, min_cwnd }`, `SupportBundle { version, timestamp, uplinks, congestion, counters, relay_stats }` with `to_json()` that includes new fields.
- Service integration: `Service::support_bundle()` collects `scoring` + `cc` + `counters` + `relay_stats` into bundle, `etw` logs on `on_ecn`, `on_rtt` delay, `check_relay_failover`, `maybe_rekey`, `ingest` auth failed.
- App shell: `balancify-app` prints ECN/delay from `sample_status()` counters, `Counters` now 10 fields (loss_events, ecn_ce, delay_events added).
- Tests: `crates/service/src/support/mod.rs` test `bundle_json_includes_cwnd` checks JSON contains cwnd/in_flight/congestion, `crates/service/src/etw.rs` test `etw_logs_without_panic`.

## Consequences

- Positive: diagnostics include cwnd/in_flight per uplink, congestion events, paths — useful for support
- Negative: ETW stub on non-Windows is eprintln, not real ETW — acceptable for lab, real ETW needs Windows build
- Neutral: no change to data path

## References

- Blueprint §8.2 Diagnostics, §6.4 CC, §4.5 failover, §11.3 rekey, §9.1b auth_failed
- Code: `crates/service/src/etw.rs`, `crates/service/src/support/mod.rs`, `crates/ipc/src/lib.rs` Counters 10 fields, `crates/service/src/lib.rs` support_bundle()
- Tests: `crates/service/src/support/mod.rs` + `etw.rs`
- CI: green 39207242665+
- ADRs: ADR-0012 ETW and support bundle, ADR-0019 enhanced
