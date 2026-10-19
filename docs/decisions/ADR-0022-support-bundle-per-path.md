# ADR-0022: Support bundle per-path ECN/delay diag

Date: 2026-10-19
Status: accepted

## Context

Support bundle (Sec.8.2) had `CongestionDiag` with only aggregate loss/ecn/delay and list of path ids, but no per-path cwnd/in_flight/ecn/delay/rtt_min/rtt_last. Diagnostics need per-path details for support.

Previously, `UplinkDiag` had cwnd/in_flight, but `CongestionDiag` only had aggregate.

## Decision

- `crates/core/src/congestion.rs`: add methods `ecn_ce(path)`, `delay_events_path(path)`, `rtt_min(path)`, `rtt_last(path)` to expose per-path ECN CE count, delay events, min RTT, last RTT from `PathWindow` (private)
- `crates/service/src/support/mod.rs`: extend `CongestionDiag` with `per_path: Vec<PathCongestionDiag>` where `PathCongestionDiag { id, cwnd, in_flight, ecn_ce, delay_events, rtt_min_ms, rtt_last_ms }`
- `SupportBundle::collect`: populate `per_path` from `svc.cc.paths()` using new cc methods, plus existing aggregate
- `to_json`: include `per_path` count (len) for quick check
- Test `bundle_includes_per_path_ecn_delay`: creates service with one uplink, triggers ECN and delay via `on_ecn` and `on_rtt` (10ms -> 50ms), collects bundle, asserts `ecn_events=1`, `delay_events=1`, `per_path.len=1`, `per_path[0].ecn_ce=1`, `delay_events=1`, `rtt_min_ms Some`, json contains `ecn` and `per_path`

## Consequences

- Positive: support bundle includes per-path ECN/delay/rtt_min/rtt_last, useful for diagnosing congestion
- Negative: more fields in JSON, but still redacted (no destinations)
- Neutral: no change to data path

## References

- Blueprint Sec.8.2 Diagnostics, Sec.6.4 CC ECN+delay, Sec.4.5 failover
- Code: `crates/core/src/congestion.rs` new methods, `crates/service/src/support/mod.rs` per_path
- Tests: `support::tests::bundle_includes_per_path_ecn_delay`
- CI: 39234128363 success
- ADR-0019 ETW+support bundle enhanced, ADR-0022 per-path
