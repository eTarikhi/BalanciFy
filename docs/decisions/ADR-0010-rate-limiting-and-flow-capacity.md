# ADR-0010: Rate limiting and flow-level capacity

Date: 2026-10-19
Status: accepted

## Context

- Relay already had RateLimiter (global cap 1024 sessions, per-IP 8, handshake 10/min/IP, per-session token bucket 100 Mbit/s + 10k pps).
- Service egress had no rate limiting — a single flow could overrun a metered uplink.
- Flow-level lane (§6.2) was probe-only for capacity — ACK pacing only measured striped lane, so flow-only links learned capacity only from bounded idle probes (64 KiB/min), which is too slow for user experience.

## Decision

- Add `crates/core/src/ratelimit.rs` — generic token bucket: rate in bps, burst in bytes, refill on demand, `try_consume` and `delay_for`.
  - `RateLimiter`: per-uplink buckets, default 1 Gbps / 1 MB burst, `set_uplink_rate`, `remove_uplink`, `try_send`, `delay_for`.
  - Wired into `Service::send_app_packet`: before sending, check `rate_limiter.try_send(path, bytes_len)`, if fails → `Action::Hold`.
  - Methods `set_uplink_rate_limit`, `remove_uplink_rate_limit`, `check_rate_limit`, `rate_limit_delay` exposed for control plane and tests.
  - 3 unit tests: burst then limit, per-uplink isolation, delay calculation.
- Add flow-level capacity:
  - `CapacityEstimator::on_flow_packet(bytes, now)` — counts flow packets into window_bytes, rolls, notes traffic (suppresses idle probe).
  - `PolicyEngine::on_flow_packet(id, bytes, now)` wrapper.
  - Service flow lane calls `policy.on_flow_packet(uplink, payload_len, now)` after rate limit check.
  - This means flow-only links now learn capacity from both probes and actual flow traffic, not just probes.
- Both changes keep sans-I/O: caller supplies `now`, no sockets.

## Consequences

- Positive: metered uplinks can be capped, service won't overrun them; flow-only links get faster capacity learning.
- Negative: rate limiter adds per-packet check (BTreeMap lookup + float math) — negligible vs crypto.
- Neutral: no change to relay rate limiting, which remains more aggressive.

## Tests

- `ratelimit::tests::token_bucket_allows_burst_then_limits`
- `ratelimit::tests::rate_limiter_per_uplink`
- `ratelimit::tests::delay_for_calculates_wait`
- Existing e2e still green: flow-level mode not held up, lossy path carries less, etc.

## References

- Blueprint §4.5 abuse controls, §6.3 capacity estimation, §6.2 flow-level lane.
