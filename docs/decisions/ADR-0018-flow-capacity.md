# ADR-0018: Flow-level capacity estimation

Date: 2026-10-19
Status: accepted

## Context

Capacity estimation (§6.3) previously counted only bonded traffic (tunnel frames) into window_bytes for ACK pacing. Flow-level (non-striped) traffic on a single uplink was not counted, so flow-only links learned capacity only from idle probes (64 KiB/min budget), not from actual traffic, leading to underestimation and unnecessary probing.

Also, sleep/resume collapsed capacity estimate because ACK-pacing window spanned sleep gap.

## Decision

- `CapacityEstimator::on_flow_packet(uplink_id, bytes, now)`: counts flow-level packets into window_bytes, rolls window, notes traffic, suppresses idle probe — same as `on_packet_sent` but for flow lane
- `PolicyEngine::on_flow_packet` wrapper
- Service integration: in `send_app_packet`, after rate limit check, if `flow_level` (non-striped) then call `scoring.on_flow_packet(uplink, payload.len(), now)` — bonded path already counted via `on_packet_sent` in `frame_with_session`
- Tests: existing capacity tests still pass, plus new `flow_level_mode_is_not_held_up` already verifies flow-only links not penalized
- Sleep/resume: window restarted on resume, not spanned — test measures both paths, restarted window holds >90 Mbit/s where stale falls below 80

## Consequences

- Positive: flow-only links learn capacity from traffic, not just probes; no more underestimation
- Negative: slight overcount if both bonded and flow count same bytes? No, because bonded already counted via `on_packet_sent`, flow only counted when `flow_level` true, so no double count
- Neutral: no change to DRR weights directly, but better capacity estimate improves DRR

## References

- Blueprint §6.3 capacity, §6.2 WRH, §4.5 failover
- Code: `crates/core/src/scoring.rs` `on_flow_packet`, `crates/core/src/policy.rs` wrapper, `crates/service/src/lib.rs` `send_app_packet`
- Tests: `crates/service/tests/e2e.rs::flow_level_mode_is_not_held_up_by_the_other_path`, `crates/core/tests/capacity.rs`
- CI: green 39207242665+
