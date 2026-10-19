# Final summary — continuous development without stopping to report

Date: 2026-10-19
Status: continuous, CI green 39237683235, 215+ tests, all benches compile

User instruction: "continue the development process without stopping to report" — keep building features, fixing bugs, pushing CI, not stopping after each small change to report. This is the second time user said it, so we continue even more.

## What was done in this entire continuous session (from ADR-0015 to now)

### Bug fixes (from CI failures)

- `pacing-bench.rs` E0599 `delay_for` not found -> `delay_until_next(0, t)` (PacingController API: `delay_until_next`, `next_send_time`, `on_send(path, bytes, now)`, `set_rate(path, rate)`)
- `drr-bench.rs` unused imports `UplinkId, FlowKey, SenderConfig, Receiver, ReceiverConfig` -> cleaned to `Sender, Crypto`
- `service/src/lib.rs` non-ASCII Sec. markers (`§`, `—`, `–`, `…`, `’`, `‘`, `“`, `”`, `•`, `→`, `←`, `≤`, `≥`, `−`, `↔`) replaced with ASCII `Sec.`, `--`, `-`, `...`, `'`, `"`, `*`, `->`, `<-`, `<=`, `>=`, `-`, `<->` to avoid Rust 2021 prefix errors (E0308, unknown prefix, unknown start of token \u{a7})
- `service/src/lib.rs` format! escaping `&format!(\\"ECN CE...` -> `&format!("ECN CE...` by replacing backslash+quote with quote globally
- `service/src/lib.rs` arg order `on_ack_with_rtt(path, bytes, now, None)` -> `on_ack_with_rtt(path, bytes, None, now)` (rtt Option before now, per congestion.rs:134)
- `driver/src/miniport.c` and `ioctl.c` and `README.md` non-ASCII replaced similarly

CI progression: 39232571739 failure non-ASCII, 39233028939 failure prefix, 39233548995 failure arg order, 39233826038 success, 39234128363 success per-path diag, 39234403709 success, 39234666177 success, 39234937199 success, 39235214291 success, 39235545738 success driver, 39235817239 success, 39236091314 success app, 39236360963 success, 39236639218 success ECN bench, 39236895693 success, 39237159469 success, 39237431952 success test coverage, 39237683235 success next steps

### Features added

- **ETW wiring** (Service etw field, ingest_path ECN CE 401 + delay 402 + auth_failed 600, tick rekey 400 + failover 500, relay ecn_echoed + total_ecn_echoed + sweep + stats 6-tuple) — ADR-0021
- **Support bundle per-path ECN/delay diag** (cc methods ecn_ce, delay_events_path, rtt_min, rtt_last, CongestionDiag per_path Vec<PathCongestionDiag> with cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, test bundle_includes_per_path_ecn_delay) — ADR-0022
- **Relay ECN echo stats** (RelaySession ecn_echoed, SecureRelay total_ecn_echoed, sweep accumulation, stats 6-tuple, print ecn) — ADR-0023
- **Driver WDF control device + IOCTLs** (SET_MTU 0x800, GET_STATS 0x801, INJECT_NBL 0x802, SET_QUANTA 0x803, length-validated, WdfDriverCreate, BalancifyCreateControlDevice in MiniportInitializeEx) — ADR-0024, for G2 S2b
- **App shell relay & bundle modes** (Sec.8.2) — render relay stats, support bundle redacted, ECN/delay counters — ADR-0025
- **ECN bench** (cwnd 80% ECN vs 50% loss, delay 10% reduction, per-path ECN/delay/rtt_min/last) — ADR-0026
- **Docs**: ADR-0021 ETW wiring, ADR-0022 support bundle per-path, ADR-0023 relay ECN stats, ADR-0024 driver WDF control, ADR-0025 app shell relay & bundle, ADR-0026 ECN bench, plus continuous-development-log, security-hardening, test-coverage, next-steps

### Test coverage

- 215+ tests, 0 failures, identical on Linux and Windows runners (windows had transient failures fixed: E0503 borrow checker in delay detection, E0063 Counters missing ecn_ce/delay_events, truncated test with optional tail, FlowClass vs BondClass, WindowsEtwHandle case, bench imports, rate limiter burst 1000<1036, non-ASCII Sec. markers, format! escaping, on_ack_with_rtt arg order)
- Core: 85+ tests (bond, congestion 13, flowtable, health, keepalive, mtu, pacing 7, placement, policy, ratelimit 3, rules, scoring)
- Crypto: 36 tests (AEAD 8, handshake 12, keys, store, replay 5, rekey 3)
- IPC: 9 tests (contract v1, truncation, Counters 10 fields)
- Relay: 12 tests (Endpoint 6, SecureRelay 6)
- Service: 30+ tests (bonded, flow-level, duplicate, no healthy, idle session, unanswered probe, capacity sample, sleep/resume, NAT rebind, full window Hold, ack reopen, keepalives+probe, probe reply RTT+capacity, IPC status, rule edits, event rate, self-test, bond revert, MTU, metered, loss failover, secure handshake, pinning, failover, rekey auto, per-flow, per-class, rate limiter, ECN/delay wire, bundle per-path)
- Tunnel: 31 tests (DRR proportionality 3000:1000, quantum < packet, idle cannot bank burst, sender numbers, without paths refuses, ack frees retx, retx bounded, ECN flag 0x04)
- E2E: 9 tests (delay-gap 40ms reorder 0 ooo, reinjection recovers lost, unplugging keeps alive, flow-level not held up, duplicate delivers once, IPC contract v1 over stream, secure handshake over real UDP, etc.)
- Fuzz: 6 tests (20k rounds each, no panic, no invented payload, caps bound memory, length checks, replay cannot manufacture)
- App: 0, Installer: 2, plus benches (drr, pacing, ecn, crypto) and throughput.rs

## Next (still without stopping)

- Driver S2b socket test through miniport (via NDIS path, test-signed + Verifier)
- Cargo.lock generation (gen-lock.sh)
- Real netem S2c (netem-rig.sh profiles, 10 reps median/p95/spread Sec.10.1)
- Throughput rows on Rig B (wired 1 Gbps, Wi-Fi 6 300 Mbit/s, secure mode, pacing 80% capacity, per-flow off/on, iperf3, added delay <=3ms, CPU/Gbit/s via crypto-bench + ETW)
- Driver packaging (self-contained .NET 10 + WinAppSDK, transactional installer, clean-machine test)
- HVCI trio (Driver Verifier code-integrity, HLK HVCI readiness, VBS/Memory-Integrity S15)
- SBOM/VEX SPDX 3.0 (sbom.sh + cargo cyclonedx, S13)
- Client binary + GUI IPC (WinUI 3 Home, Rules, Relay & account, Diagnostics, tray, IPC v1, .NET 10)
- Per-flow session demux retx keyed by (session, seq) (currently global unique seq for retx simplicity)
- Flow-level ACK explicit (currently on_flow_packet counts flow packets into window_bytes, suppresses probe)
- More ETW events (bond revert 300, uplink up/down 200/201)

## References

- Blueprint Sec.4.5 failover, Sec.4.7.4 per-flow sessions, Sec.4.8 dependency policy, Sec.6.1 TAP-like adapter, Sec.6.2 WRH + flow classes, Sec.6.3 capacity, Sec.6.4 DRR + pacing + CC, Sec.6.5 health hysteresis, Sec.6.6 NAT keepalive, Sec.8.2 Diagnostics, Sec.8.3 installer, Sec.9.2 IPC + handshake, Sec.9.3 signing, Sec.9.5 HVCI, Sec.10.1 Rig A/B, Sec.10.3 acceptance rows, Sec.11.2 Wintun exception, S13 SBOM/VEX, S15 HVCI trio, C2/C4
- ADRs 0003-0026
- CI: 39237683235 success
