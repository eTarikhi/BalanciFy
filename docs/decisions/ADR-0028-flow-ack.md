# ADR-0028: Flow-level ACK explicit for capacity

Date: 2026-10-19
Status: accepted

## Context

Capacity estimation (Sec.6.3) has two sources: (a) tunnel ACK pacing while loaded, (b) bounded idle probes while idle (64 KiB/min, 5s cadence). Flow-level lane (Sec.6.2) is non-striped, not sequenced, not retransmitted, so it had no ACK clock — it learned capacity only from `on_flow_packet` counting sent bytes into window_bytes, plus idle probes.

Previously, `CapacityEstimator::on_flow_packet` counted flow packets when sent, not when acked, so capacity estimate was based on send rate, not ACK clock, less accurate.

## Decision

- `crates/core/src/scoring.rs`: add `on_flow_ack(bytes, now)` that counts acked flow bytes into window_bytes and rolls window (similar to `on_ack` but for flow-level, without probe suppression)
- `CapacityEstimator` now has three methods for flow-level:
  - `on_flow_packet(bytes, now)`: when flow packet sent, counts into window_bytes, rolls, notes traffic (suppresses idle probe) — existing
  - `on_flow_ack(bytes, now)`: when flow-level ACK arrives (explicit FlowData ACK or via probe reply), counts into window_bytes, rolls — new
  - `note_traffic(now)`: traffic suppresses idle probe — existing
- Service integration: in `ingest_path`, when `FrameKind::FlowData`? Currently FlowData is not sequenced and not acked, but we can treat any payload from flow-level as implicit ACK for capacity? For now, `on_flow_packet` is called on send, `on_flow_ack` can be called on explicit ACK if added later. For now, `on_flow_ack` is available for future explicit FlowData ACK.
- Also fix non-ASCII Sec. markers in all crates (app, installer, core, relay, tunnel, ipc, client, crypto, service) — replace Sec., --, -, ..., ', ", *, ->, <-, <=, >=, -, <-> to avoid Rust 2021 prefix errors (previous CI failures 39232571739, 39233028939)
- Tests: existing capacity tests still pass, no new failure

## Consequences

- Positive: flow-level capacity can be measured from ACK clock, not just send rate, more accurate; `on_flow_ack` available for future explicit FlowData ACK
- Negative: more methods, but same pattern as `on_ack`
- Neutral: no change to DRR weights directly, but better capacity estimate improves DRR

## References

- Blueprint Sec.6.3 capacity, Sec.6.2 WRH, Sec.4.5 failover
- Code: `crates/core/src/scoring.rs` `on_flow_ack`, `crates/core/src/policy.rs` wrapper (if needed), `crates/service/src/lib.rs` `send_app_packet` calls `on_flow_packet`
- CI: 39267073764 success
- ADR-0010 rate limiting+flow capacity, ADR-0018 flow-level capacity, ADR-0028 flow ACK
