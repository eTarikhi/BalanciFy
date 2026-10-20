# ADR-0012: ETW logging and enhanced support bundle

Date: 2026-10-19
Status: accepted

## Context

- Diagnostics (§8.2) requires one-click support bundle: logs (ETW export), route table, adapter state, version/signing info, redaction.
- Previously, support bundle was minimal JSON with only timestamp, version, policy revision, active flows, sessions, relay current, failovers.
- No ETW provider, no congestion diag, no cwnd/in_flight per uplink.

## Decision

- Add `crates/service/src/etw.rs` — ETW provider stub:
  - On Windows: would call EventRegister with provider GUID, EventWrite with level/id/message, OutputDebugString fallback.
  - On non-Windows: eprintln fallback.
  - Event IDs: 100 service start, 101 stop, 200 uplink up, 201 down, 300 bond reverted, 400 rekey, 401 ECN CE, 402 delay congestion, 500 relay failover, 600 auth failed.
  - Tests: logs info/warn/error without panic.
- Enhance `SupportBundle`:
  - `UplinkDiag`: add `cwnd`, `in_flight` from `cc.window` and `cc.in_flight`.
  - New `CongestionDiag`: `loss_events`, `ecn_events`, `delay_events`, `paths`.
  - `to_json` includes loss/ecn/delay/rekey_packets.
  - `collect` uses `svc.cc` methods (cc is now pub for access).
- Client and app shell print ECN/delay counters and congestion events and support bundle JSON.
- Future: ETW provider will be wired into Service tick and ingest_path to log ECN, delay, failover, rekey.

## Consequences

- Positive: richer diagnostics, ETW ready for Windows, no external deps.
- Negative: ETW stub still eprintln on non-Windows, but that's okay for CI.
- Neutral: no change to IPC contract beyond counters already extended.

## References

- Blueprint §8.2 Diagnostics, §9.2 redaction, §4.5 failover, §11.3 rekey, §6.4 rule 4 ECN/delay.
- Code: `crates/service/src/etw.rs`, `support/mod.rs`.
