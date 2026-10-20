# ADR-0028: Flow-level ACK explicit for capacity

Date: 2026-10-19
Status: accepted

## Context

Capacity estimation (Sec.6.3) has two sources: (a) tunnel ACK pacing while loaded, (b) bounded idle probes while idle (64 KiB/min, 5s cadence). Flow-level lane (Sec.6.2) is non-striped, not ordered in the shared Data sequence, and not retransmitted, so it has no ACK clock — it learns capacity only from `on_flow_packet` counting sent bytes into window_bytes, plus idle probes. Wire v1 adds a dedicated FlowData nonce/replay sequence, not an ACK sequence.

Previously, `CapacityEstimator::on_flow_packet` counted flow packets when sent, not when acked, so capacity estimate was based on send rate, not ACK clock, less accurate.

## Decision

- `crates/core/src/scoring.rs`: add `on_flow_ack(bytes, now)` that counts acked flow bytes into window_bytes and rolls window (similar to `on_ack` but for flow-level, without probe suppression)
- `CapacityEstimator` now has three methods for flow-level:
  - `on_flow_packet(bytes, now)`: when flow packet sent, counts into window_bytes, rolls, notes traffic (suppresses idle probe) — existing
  - `on_flow_ack(bytes, now)`: when flow-level ACK arrives (explicit FlowData ACK or via probe reply), counts into window_bytes, rolls — new
  - `note_traffic(now)`: traffic suppresses idle probe — existing
- Initial service integration was deferred at this ADR's creation. Follow-up commit `d96d656` replaces send-side-only capacity sampling in the Service flow lane with authenticated selective FlowAck accounting; `on_flow_packet` remains an estimator API but is no longer the Service's flow-level sampling path.
- Also fix non-ASCII Sec. markers in all crates (app, installer, core, relay, tunnel, ipc, client, crypto, service) — replace Sec., --, -, ..., ', ", *, ->, <-, <=, >=, -, <-> to avoid Rust 2021 prefix errors (previous CI failures 39232571739, 39233028939)
- Tests: existing capacity tests still pass, no new failure

## Consequences

- Positive: Service flow-level capacity can be sampled from authenticated peer receipt rather than merely local sends.
- Follow-up implementation is recorded above; frame kind `FlowAck` is part of wire v2 and v1 peers are explicitly rejected.
- Negative: more methods, but same pattern as `on_ack`
- Neutral: no change to DRR weights directly, but better capacity estimate improves DRR

## References

- Blueprint Sec.6.3 capacity, Sec.6.2 WRH, Sec.4.5 failover
- Code: `crates/core/src/scoring.rs` `on_flow_ack`, `crates/core/src/policy.rs` wrapper (if needed), `crates/service/src/lib.rs` `send_app_packet` calls `on_flow_packet`
- CI for original estimator work: 39267073764 success
- Follow-up implementation (2026-10-20, commit `d96d656`): wire v2 adds authenticated selective `FlowAck` kind 6; sender tracks bounded per-session sequence/path/byte records, receiver ACKs fresh FlowData and re-ACKs duplicates, and service samples capacity only after authenticated peer receipt. Rate-limited unsent FlowData removes pending state. Wire v1 is rejected to avoid silent loss of ACK-based sampling. CI 39476560004 passed on Ubuntu and Windows.
- ADR-0010 rate limiting+flow capacity, ADR-0018 flow-level capacity, ADR-0028 flow ACK, ADR-0003 wire v2 amendment
