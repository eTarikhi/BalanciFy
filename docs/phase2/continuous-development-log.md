# Continuous development log — without stopping to report

Date: 2026-10-20
Status: continuous; latest CI green on Ubuntu and Windows

User instruction: "continue the development process without stopping to report" — meaning keep building features, fixing bugs, pushing CI, not stopping after each small change to report.

This log records what was done while continuing without stopping, after ADR-0015.

## Continuation (2026-10-20)

- `7920e4a test service telemetry snapshots for IPC` — centralized warning + counter event composition in Service and tested the IPC payload. CI 39426255171 passed.
- `5751d43 close idle unsubscribed IPC clients` — impose a 60-second idle limit for unsubscribed lab clients; retain long-lived event subscriptions. CI 39426687493 passed.
- `93dc6ff refresh IPC coverage notes` and `5e15a4b document responsive service loop` — synchronized roadmap/test coverage with implemented behavior.
- `a548fc2 keep the service dispatcher responsive to IPC` — make lab UDP sockets nonblocking so two-path polling cannot block IPC dispatch for up to 200ms. CI 39427278094 passed.
- `6c1f19f redact relay endpoints from support bundles` — expose relay presence only, with a regression test proving that the relay address and port do not appear in the exported JSON. CI 39431889338 passed.
- `0d0a580 serialize complete redacted support diagnostics` — serialize full counters, uplinks and per-path congestion measurements; add JSON string escaping and tests for control characters. CI 39432345360 passed.

The latest CI also includes Ubuntu/Windows workspace tests, release build, lab safety tests, and dependency-policy audit. Hardware-dependent WDK/NDIS, Driver Verifier, HVCI, and Rig B measurements remain pending.

## Commits since last green 39209903149 (ADR-0020)

- `44942dc feat(service,relay): ETW wiring for ECN/delay/rekey/failover/auth, relay ECN echo stats` — added `etw: EtwProvider` field to Service, init, wired `ingest_path` ECN CE (401) + delay (402) + auth_failed (600), `tick` rekey (400) + failover (500), relay `RelaySession::ecn_echoed` + `SecureRelay::total_ecn_echoed` + sweep accumulation + stats 6-tuple + print ecn. **CI failure 39232571739** due to non-ASCII Sec. markers + format! escaping + arg order.
- `8c2bc1e fix(service): replace non-ASCII Sec. markers to avoid Rust 2021 prefix errors` — replaced `§` with `Sec.`, `—` with `--`, `–` with `-`, `…` with `...`, `’` `‘` `“` `”` etc., plus `−` minus. **CI failure 39233028939** still prefix errors + arg order.
- `4fdded0 fix(service): correct format! string escaping (remove backslash+quote)` — fixed `&format!(\\"ECN CE...` -> `&format!("ECN CE...` by replacing backslash+quote with quote globally. **CI failure 39233548995** arg order `on_ack_with_rtt(path, bytes, now, None)` should be `None, now`.
- `eb3b55a fix(service): on_ack_with_rtt arg order (rtt Option before now)` — fixed `on_ack_with_rtt(path, bytes, None, now)`. **CI success 39233826038** ubuntu+windows+audits green.
- `acdb6a3 feat(core,service): support bundle per-path ECN/delay diag` — added `cc.ecn_ce(path)`, `delay_events_path(path)`, `rtt_min(path)`, `rtt_last(path)` to CongestionController, extended `CongestionDiag` with `per_path: Vec<PathCongestionDiag>` containing cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, `collect` populates per_path, `to_json` includes per_path len, test `bundle_includes_per_path_ecn_delay` verifies ECN+delay via `on_ecn` + `on_rtt` 10ms->50ms. **CI success 39234128363**.
- `88a4db9 docs: ADR-0021 ETW wiring` — documented ETW wiring.
- `054481e docs: ADR-0022 support bundle per-path` — documented per-path diag.
- `9084d95 docs: ADR-0023 relay ECN echo stats` — documented relay ECN stats.

## Current CI

- Last green 39234937199 (ADR-0023) — ubuntu+windows+audits success, 214+ tests (now 215 with bundle test), benches compile, docs.

## Next (still without stopping)

- Driver WDF control device + IOCTLs (`driver/src/ioctl.c`) — create WDFDEVICE in MiniportInitializeEx, minimal IOCTLs (set MTU, inject NBL, query stats), length-validated, fuzzed same gate as tunnel, build with WDK 10.0.22621+, test-signed install on Hyper-V VM with Driver Verifier
- S2b socket test through miniport — same as `ordered_stream_survives_a_real_udp_socket` but via NDIS path
- Cargo.lock generation — `cargo generate-lockfile` once toolchain available, commit, audit checks lock file if present
- Real netem S2c — replace simulated rig with `tools/lab/netem-rig.sh` profiles, record 10 reps median/p95/spread per Sec.10.1
- Throughput rows on Rig B — run `throughput.rs` + iperf3 on real hardware, secure mode, pacing 80% capacity, per-flow off/on, measure aggregate/single-flow/failover/added delay (target <=3ms) + CPU/Gbit/s via crypto-bench + ETW
- Driver packaging — bundle self-contained .NET 10 + WinAppSDK runtimes, transactional installer with atomic rollback, clean-machine offline test
- HVCI readiness trio — Driver Verifier with code-integrity, HLK HVCI readiness test, run on VBS/Memory-Integrity machine (S15)
- SBOM/VEX SPDX 3.0 — generate via `sbom.sh` + cargo cyclonedx, publish sidecar, S13
- Client binary + GUI IPC — wire Service handshake into CLI client (done lab mode), then WinUI 3 shell (Home, Rules, Relay & account, Diagnostics, tray) via IPC v1, self-contained .NET 10
- Per-flow session demux retx keyed by (session, seq) — currently global unique seq for retx simplicity, future per-session seq spaces
- Flow-level ACK — add explicit FlowData ACK for capacity (done via on_flow_packet, but could add explicit ACK)
- ETW wiring already done, but could add more events (bond revert, uplink up/down)

## References

- Blueprint Sec.4.5 failover, Sec.4.7.4 per-flow sessions, Sec.4.8 dependency policy, Sec.6.1 TAP-like adapter, Sec.6.2 WRH + flow classes, Sec.6.3 capacity, Sec.6.4 DRR + pacing + CC, Sec.6.5 health hysteresis, Sec.6.6 NAT keepalive, Sec.8.2 Diagnostics, Sec.8.3 installer, Sec.9.2 IPC + handshake, Sec.9.3 signing, Sec.9.5 HVCI, Sec.10.1 Rig A/B, Sec.10.3 acceptance rows, Sec.11.2 Wintun exception, S13 SBOM/VEX, S15 HVCI trio
- ADRs 0003-0023
- CI runs: 39232571739 failure non-ASCII, 39233028939 failure prefix, 39233548995 failure arg order, 39233826038 success, 39234128363 success, 39234403709 success, 39234666177 success, 39234937199 success
