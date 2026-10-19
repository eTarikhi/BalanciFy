# ADR-0017: Rate limiter per-uplink in service egress

Date: 2026-10-19
Status: accepted

## Context

Service egress must not exceed uplink capacity or configured rate limit. Previously, service sent directly without rate limiting, relying only on congestion window and pacing. But user may configure per-uplink rate limit (e.g., cellular capped at 50 Mbps), and service must enforce it, holding packets when over limit rather than dropping.

Blueprint §4.5 requires failover, §6.4 rule 4 pacing in front of cwnd, §6.3 capacity estimation, but also rate limiting is needed for user-configured caps and for abuse prevention.

## Decision

- `crates/core/src/ratelimit.rs`: TokenBucket with `rate_bps` + `burst_bytes`, `try_consume(bytes, now) -> bool`, `delay_for(bytes, now) -> Duration`
- `RateLimiter` per-uplink: map UplinkId -> TokenBucket, default 1 Gbps/1MB burst, `set_uplink_rate(id, rate_bps, burst)`, `try_send(id, bytes, now)`, `delay_for`
- Service integration: `Service::set_uplink_rate(id, rate_bps, burst)`, `try_send` called in `send_app_packet` after placement, before tunnel frame creation — if over limit, returns `Action::Hold` (same as cwnd full), packet stays queued, service will retry on next tick when tokens refill
- Tests: 3 core tests (allow within burst, hold when over, refill over time) + 1 service test `rate_limiter_holds_when_over_limit` (8000 bps, burst 2000 to cover sealed overhead 20+16+1000=1036, first burst passes, second immediate fails, after 1s refill passes)

## Consequences

- Positive: enforces user caps, prevents abuse, holds rather than drops, integrates with existing Hold path
- Negative: adds latency when over limit, but correct — better than drop
- Neutral: no change to DRR or CC, just additional check

## References

- Blueprint §4.5 failover, §6.4 pacing + cwnd, §6.3 capacity
- Code: `crates/core/src/ratelimit.rs`, `crates/service/src/lib.rs` `try_send`, `set_uplink_rate`
- Tests: `crates/core/tests/ratelimit.rs`, `crates/service/src/lib.rs` `rate_limiter_holds_when_over_limit`
- CI: 39207060537 failure burst 1000 < 1036, 39207242665 success after burst 2000
