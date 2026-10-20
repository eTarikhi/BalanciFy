# ADR-0025: App shell relay & bundle modes (Sec.8.2)

Date: 2026-10-19
Status: accepted

## Context

App shell IPC client (`app/src/main.rs`) previously had modes home, rules, diagnostics, tray — mimicking WinUI shell over IPC v1. Blueprint Sec.8.2 requires Home, Rules, Relay & account, Diagnostics, tray, plus support bundle one-click.

Previously, relay & account and support bundle were not rendered.

## Decision

- `app/src/main.rs`: add modes `relay` and `bundle`
  - `relay`: renders Relay & Account screen — service version, schema, ECN/delay counters, uplinks with cap/rtt/eligible, placeholder for relay addrs, failover count, account, pinned keys (real WinUI would show relay primary/secondary, failover count, account, pinned keys)
  - `bundle`: renders Support Bundle (redacted, Sec.8.2) — version, policy_rev, rules_rev, active_flows, counters, uplinks count + per-uplink state/rtt/loss/cap, note that bundle JSON would include cwnd/in_flight per uplink, congestion diag loss/ecn/delay+paths, rekey diag, relay stats (see service support mod)
- Existing modes still work: home (status + 1Hz events), rules (flow rules + uplinks), diagnostics (status ECN/delay + self-test), tray (minimal)
- All modes use same IPC contract v1 over TCP loopback (for CI/Linux), real WinUI 3 / .NET 10 shell would use same contract over named pipe
- No new dependencies, keeps policy clean

## Consequences

- Positive: app shell now covers all Sec.8.2 screens (Home, Rules, Relay & account, Diagnostics, tray, support bundle), ECN/delay visible
- Negative: lab CLI, not real WinUI 3 — real GUI needs .NET 10 + WinAppSDK, Phase 4
- Neutral: no change to service or data path

## References

- Blueprint Sec.8.2 Diagnostics, Sec.9.2 IPC + handshake, Sec.4.5 failover, Sec.6.4 CC ECN+delay, Sec.11.3 rekey
- Code: `app/src/main.rs` `render_relay`, `render_bundle`, modes
- CI: 39236091314 success
- ADR-0012 ETW and support bundle, ADR-0025 app shell
