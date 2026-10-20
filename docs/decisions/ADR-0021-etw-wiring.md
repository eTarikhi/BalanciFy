# ADR-0021: ETW wiring for ECN/delay/rekey/failover/auth

Date: 2026-10-19
Status: accepted

## Context

ETW provider stub existed (ADR-0012) but was not wired to service events. Diagnostics (Sec.8.2) requires ETW events for ECN CE, delay congestion, rekey, relay failover, auth failed, plus per-path cwnd/in_flight.

Previously, service had eprintln for failover only, no ETW.

## Decision

- `crates/service/src/lib.rs`: add `etw: EtwProvider` field to `Service`, init in `new()` as `EtwProvider::new("BalancifyService")`
- `ingest_path`: when ECN CE flag detected (`FLAG_ECN_CE`), after incrementing `ecn_ce` counter, log `ETW EVT_ECN_CE (401)` with path, uplink, cwnd
- `ingest_path`: when `on_ack_with_rtt` triggers delay event (RTT jump), log `EVT_DELAY_CONGESTION (402)`; same for probe reply `on_rtt` delay
- `ingest_path`: when `Event::Dropped AuthFailed`, log `EVT_AUTH_FAILED (600)` with uplink
- `tick`: when `should_rekey` triggers `maybe_rekey`, log `EVT_REKEY (400)` with session and time
- `tick`: when `check_relay_failover` returns new relay, log `EVT_RELAY_FAILOVER (500)` with old->new and count, plus existing eprintln
- `crates/service/src/etw.rs`: already has event IDs 100-600, `EtwProvider::new`, `info/warn/error/verbose` that eprintln on non-Windows, EventRegister/EventWrite on Windows (stub)
- Tests: existing ETW test `etw_provider_logs` still passes, no new failure

## Consequences

- Positive: diagnostics include ECN/delay/rekey/failover/auth via ETW, visible in support bundle and shell
- Negative: ETW stub on non-Windows is eprintln, not real ETW — acceptable for lab, real ETW needs Windows build
- Neutral: no change to data path, only logging

## References

- Blueprint Sec.8.2 Diagnostics, Sec.6.4 CC ECN+delay, Sec.11.3 rekey, Sec.4.5 failover, Sec.9.1b auth_failed
- Code: `crates/service/src/lib.rs` `etw` field, `ingest_path`, `tick`, `crates/service/src/etw.rs`
- CI: 39233826038 success after fixing arg order `on_ack_with_rtt(path, bytes, None, now)` vs previous `now, None`
- ADR-0012 ETW and support bundle, ADR-0019 enhanced, ADR-0021 wiring
