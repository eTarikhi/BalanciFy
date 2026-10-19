# ADR-0027: ETW for uplink up/down and bond revert

Date: 2026-10-19
Status: accepted

## Context

ETW wiring (ADR-0021) covered ECN CE (401), delay congestion (402), rekey (400), relay failover (500), auth_failed (600), but not uplink up/down (200/201) and bond revert (300) which are defined in etw.rs as EVT_UPLINK_UP=200, EVT_UPLINK_DOWN=201, EVT_BOND_REVERTED=300.

Previously, `publish()` updated pacing rates but did not log uplink state changes, and `observe_bond_gain` returned event but did not log ETW.

## Decision

- `crates/service/src/lib.rs` `publish()`: clone `prev_uplinks` before overwriting snapshot, after publish iterate prev vs curr, if `prev.state != curr.state` and new state is Up or Down, log `ETW EVT_UPLINK_UP (200)` or `EVT_UPLINK_DOWN (201)` with id, prev->curr, rtt, cap
- `observe_bond_gain()`: when `d.flipped` true (bond revert), log `EVT_BOND_REVERTED (300)` with class, gain, bonded_bps, best_bps, then return `BondReverted` event
- No new dependencies, keeps policy clean
- Tests: existing tests still pass, no new failure

## Consequences

- Positive: ETW now covers all Sec.8.2 events (100-600): service start/stop, uplink up/down, bond revert, rekey, ECN, delay, relay failover, auth_failed
- Negative: ETW stub on non-Windows is eprintln, not real ETW — acceptable for lab
- Neutral: no change to data path

## References

- Blueprint Sec.8.2 Diagnostics, Sec.6.5 health hysteresis, Sec.6.4 rule 6 bond auto-disable, Sec.4.5 failover
- Code: `crates/service/src/lib.rs` `publish()` ETW uplink, `observe_bond_gain()` ETW bond revert, `crates/service/src/etw.rs` event IDs
- CI: 39266364959 success
- ADR-0021 ETW wiring, ADR-0027 uplink/bond
