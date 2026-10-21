# Next steps — continuous development without stopping

Date: 2026-10-21
Status: continuous; client hardening PR #6 is merged. FlowData selective repair is under review in PR #7. CI 39530970838 passed (261 Linux / 260 Windows Rust tests), including SPDX 2.3 validation and lab/policy checks.

User instruction: "continue the development process without stopping to report" — keep building features, fixing bugs, pushing CI, not stopping after each small change.

## What was done in this continuous session (since ADR-0015)

- ETW wiring for ECN/delay/rekey/failover/auth (Service etw field, ingest_path ECN CE 401 + delay 402 + auth_failed 600, tick rekey 400 + failover 500, relay ecn_echoed + total_ecn_echoed + sweep + stats 6-tuple)
- Fixed non-ASCII Sec. markers (Sec. instead of Sec., -- instead of --) to avoid Rust 2021 prefix errors (E0308, unknown prefix, unknown start of token \u{a7})
- Fixed format! escaping (remove backslash+quote) and on_ack_with_rtt arg order (None, now)
- Support bundle per-path ECN/delay diag (cc methods ecn_ce, delay_events_path, rtt_min, rtt_last, CongestionDiag per_path Vec<PathCongestionDiag> with cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, test bundle_includes_per_path_ecn_delay)
- Driver WDF control device + IOCTLs (SET_MTU, GET_STATS, INJECT_NBL, SET_QUANTA) for G2 S2b, length-validated, WdfDriverCreate, BalancifyCreateControlDevice in MiniportInitializeEx, non-ASCII fixed
- App shell relay & bundle modes (Sec.8.2) — render relay stats, support bundle redacted, ECN/delay counters
- ECN bench (cwnd 80% ECN vs 50% loss, delay 10% reduction, per-path ECN/delay/rtt_min/last)
- Docs: ADR-0021 ETW wiring, ADR-0022 support bundle per-path, ADR-0023 relay ECN stats, ADR-0024 driver WDF control, ADR-0025 app shell relay & bundle, ADR-0026 ECN bench, plus continuous-development-log, security-hardening, test-coverage
- Control-frame authentication/replay hardening and relay/session idle protections; wire v2 selective FlowAck for peer-receipt capacity sampling, with bounded per-session accounting and duplicate re-ACK (CI 39476560004).
- Client transport hardening (per-user absolute state roots, relay-source filtering, fatal UDP send errors, and single-use handshake responses) merged in PR #6; prior PR #5 was closed as superseded.
- FlowData selective repair is implemented in PR #7 / ADR-0036: retain bounded plaintext until authenticated FlowAck, reinject once after RTO on an alternate path, and rely on replay rejection plus duplicate re-ACK to preserve at-most-once delivery. Tunnel and service timer regressions pass.
- ETW uplink-up/down and bond-revert events were already implemented (ADR-0027 / commit 655d91d); they are complete, not remaining roadmap work.

CI progression: 39232571739 failure non-ASCII, 39233028939 failure prefix, 39233548995 failure arg order, 39233826038 success, 39234128363 success per-path diag, 39234403709 success, 39234666177 success, 39234937199 success, 39235214291 success, 39235545738 success driver, 39235817239 success, 39236091314 success app, 39236360963 success, 39236639218 success ECN bench, 39236895693 success, 39237159469 success, 39237431952 success test coverage

## What remains (ordered, still without stopping)

1. **Driver S2b socket test through miniport**: validate the existing real-UDP tunnel path through NDIS on a test-signed Hyper-V VM after a successful WDK build; requires Windows/WDK 10.0.22621+, Driver Verifier, and test hardware.
2. **Real S2c/throughput validation**: use the explicitly confirmed netem runner only on disposable interfaces; raw link results alone are not BalanciFy bonding evidence. Add adapter-level capture and record 10-repetition statistics. No lab result is claimed; Rig B remains pending.
3. **Throughput rows on Rig B**: run `throughput.rs` + iperf3 on wired 1 Gbps and Wi-Fi 6 300 Mbit/s, secure mode, pacing at 80% capacity, per-flow off/on; measure aggregate/single-flow/failover/added delay (target <=3ms) and CPU/Gbit/s. Local service-path benchmarks are not network evidence.
4. **Driver packaging and clean-machine validation**: package self-contained .NET 10 + WinAppSDK runtimes, verify installer rollback/offline provisioning and SBOM sidecar on a clean VM.
5. **HVCI readiness trio**: Driver Verifier with code-integrity, HLK HVCI readiness test, and a VBS/Memory-Integrity-enabled machine (S15).
6. **SBOM/VEX S13**: the previous handwritten placeholder is removed; CI now generates an SPDX 2.3 inventory from the exact Cargo metadata graph. Still required: standards-valid SPDX 3.0 serialization, reviewed VEX statements from current advisory data, and installer sidecar verification.
7. **Client binary + GUI IPC**: client handshake now fails closed with pinned identities by default; move from the protocol/lab CLI and loopback adapter to the WinUI 3 shell and production IPC transport/authentication.

## Process

- Keep CI green at all times: each push must pass Ubuntu + Windows + lab checks + dependency/crypto audit (261 Linux / 260 Windows Rust tests; seven SBOM tests and SPDX parser also run)
- Use `gh run list --limit 1 --branch arena/73af0098-balancify --json databaseId,conclusion` + commit check-runs to verify CI.
- Fix bugs using web resources (RFC 8684 Sec.3.3, RFC 6356 LIA, DRR, RFC 6928) + deep thinking, deep learning, repeating, then continue developing app based on criteria (blueprint)
- No third-party kernel component in data path (C2), only vetted/pinned crates, PassThrough test-only must never ship (Phase-4 audit)
- Workspace dependency policy: `[workspace.dependencies]` intentionally empty per blueprint Sec.4.8 — do not add unpinned dependencies casually

## References

- Blueprint Sec.4.5 failover, Sec.4.7.4 per-flow sessions, Sec.4.8 dependency policy, Sec.6.1 TAP-like adapter, Sec.6.2 WRH + flow classes, Sec.6.3 capacity, Sec.6.4 DRR + pacing + CC, Sec.6.5 health hysteresis, Sec.6.6 NAT keepalive, Sec.8.2 Diagnostics, Sec.8.3 installer, Sec.9.2 IPC + handshake, Sec.9.3 signing, Sec.9.5 HVCI, Sec.10.1 Rig A/B, Sec.10.3 acceptance rows, Sec.11.2 Wintun exception, S13 SBOM/VEX, S15 HVCI trio
- ADRs 0003-0036
- Latest feature CI: 39530970838 success
