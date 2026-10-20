# ADR-0013: Per-class sessions

Date: 2026-10-19
Status: accepted

## Context

Per-flow sessions (§4.7.4) assign a distinct session id per 5-tuple (FlowKey). This is good for isolation but creates many sessions (up to max_sessions cap, evicting oldest). For some use cases, per-class sessions (one session per FlowClass: Bulk, LatencySensitive, Pinned) is a better trade-off: fewer sessions, still isolates bulk from latency-sensitive, and pinned flows get their own session.

## Decision

- Add `per_class_sessions` toggle, `class_sessions: BTreeMap<u8, u64>` (BondClass/FlowClass discriminant -> session id) to `Service`.
- `session_for_class(class, now)` assigns distinct ids per FlowClass, up to next_session_id counter, opens rx session.
- Precedence: per-flow > per-class > default (cfg.session). If both toggles enabled, per-flow wins.
- Methods: `set_per_class_sessions`, `session_for_class`, `class_session_count`.
- Data path: `send_app_packet` uses per-class session when enabled.
- Tests: distinct ids per class, same class same id, disable clears.

## Consequences

- Positive: fewer sessions than per-flow, still isolates classes, useful for Phase-4 when handshake defines session setup.
- Negative: still global seq unique for retx simplicity (future could be per-session seq).
- Neutral: no change to single-session default.

## References

- Blueprint §4.7.4 per-flow sessions, §6.2 flow classes.
- Code: `crates/service/src/lib.rs` per_class_sessions.
