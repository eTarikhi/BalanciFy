# Roadmap next — what remains for G2/G3

Date: 2026-10-23
Status: in progress; PR #19 remains open at `02409cf` (IPv6 boundary regressions `9be616f`; deterministic parser stress `02409cf`). Push CI `39922008067` and PR CI `39922014929` passed all six jobs, including unsigned x64/ARM64 WDK compile/link, PE machine-type, mitigation-flag, and unsigned-status verification, Ubuntu/Windows Rust tests, lab tests, and dependency/crypto audit. Hardware/product acceptance gates remain pending.

## Completed since last green (39191895800)

- ECN CE + delay-based CC (PathWindow ecn_ce, rtt_min, rtt_last, delay_events; CC ecn_events/delay_events; on_ecn 80% reduction, on_rtt/on_ack_with_rtt 10% Vegas-like, FLAG_ECN_CE=0x04, service ingest_path checks flag, probe RTT feeds cc.on_rtt)
- Per-flow sessions (BTreeMap<FlowKey,u64>, session_for_flow, evict oldest on cap, open rx session, data path via frame_with_session)
- Per-class sessions (per_class_sessions toggle, class_sessions map, session_for_class FlowClass -> session)
- Per-session seq tracking (session_seqs map, global unique seq for retx simplicity)
- Rate limiting service egress (TokenBucket, RateLimiter per-uplink, try_send, delay_for, wired into send_app_packet -> Hold on limit)
- Flow-level capacity uses authenticated per-packet FlowAck receipts in wire v2; sender-side pending ACK state is bounded and capacity samples are credited to the original uplink.
- Wintun fallback ADR-0009 (feature-gated, exception file check, raw FFI placeholder)
- Installer transactional (journaled, 9 steps including SPDX 2.3 + 3.0.1 sidecars, reverse rollback, dry-run, tests); default journals are exclusively reserved and private (Unix mode `0600`), with collision regression coverage.
- IPC v1 string fields now reject malformed UTF-8 instead of accepting replacement-character aliases; tests cover both short and u32-length strings. CI also tests pinned Ubuntu runner and Pages action release contracts.
- Rust lab CLI IPC client (`balancify-app` crate; home/rules/diagnostics/tray and support modes, 1Hz event subscription, ECN/delay rendering); the production WinUI 3/.NET shell remains unimplemented
- Service IPC v1 loopback adapter handles requests and rate-limited event push; service snapshots combine warning and counter events. The shell's bundle mode requests opcode 11 and receives the complete redacted support JSON. Concurrency is capped at 16, idle unsubscribed clients close after 60 seconds, and nonblocking lab UDP polling avoids stalling the dispatcher. Production named-pipe authentication remains pending.
- Netem runner defaults to no-side-effect dry-run; explicit `--apply` supports seven raw link profiles with JSON capture and safety tests. It is not BalanciFy/driver acceptance evidence.
- Driver control source is hardened to SYSTEM/Admin access and owned NBL packet buffers; `MiniportInitializeEx` registers the adapter, submits revision-2 general attributes, and advertises a minimal OID surface. Miniport and INF consistently use NdisMediumIP; INJECT_NBL validates IP lengths and MTU, while pause waits for active indications, returned NBLs, and send callback chains. PR #19 adds a bounded 64-record reverse TX queue, atomic NBL batch publication, read-access `DEQUEUE_TX`, optional service polling into `Service::send_ip_packet` (ADR-0040), and WDK-independent TCP/UDP/IP boundary checks aligned with the service parser, including exact unfragmented UDP/IP lengths and IPv6 UDP regression cases (ADR-0041). Latest push run `39922008067` and PR run `39922014929` compiled and linked unsigned x64/ARM64 targets, verified PE machine types, mitigation flags, empty certificate tables, and passed INF contract tests. This is source/build evidence only: no signed install, Verifier, HVCI, VM, or packet-path behavior has been verified.
- File-backed key storage uses unique private atomic temporaries, Unix 0600-at-create, zeroized plaintext copies, and Windows DPAPI by default (with legacy raw-key migration tests).
- Tunnel wire v2 retains frame-kind AEAD nonce separation and adds authenticated selective FlowAck; FlowData has its own bounded replay-protected sequence lane, keepalives no longer repeat sequence zero, control frames authenticate and pass replay checks before changing service state, and retransmission/ACK state is keyed by `(session, seq)` with per-session Data sequence spaces.
- Flow-level capacity now uses peer FlowAck receipts, with bounded per-session pending bytes/entries, replay verification, duplicate re-ACK, rate-limit cleanup, and a regression test for service sampling.
- Commit `7bb90d7` implements ETW diagnostics through Windows EventRegister/EventWrite using a stable provider GUID, nonzero level/keyword, event IDs 100-600 and bounded UTF-16 payloads; non-Windows uses stderr. CI `39602034920` passed with Windows compilation/tests and a regression for endpoint-free failover messages. Support bundle includes UplinkDiag cwnd/in_flight and CongestionDiag loss/ecn/delay+paths. Actual Windows trace collection/decoding remains unvalidated; no manifest is installed.
- Software benchmark + benches (throughput.rs local work rate/per-call latency only; drr-bench, pacing-bench, crypto-bench). Real network performance and G3/G4 acceptance remain pending.
- SPDX 2.3 remains the compatibility inventory; commit `d86ad4f` adds a parallel SPDX 3.0.1 JSON-LD generator for the same Cargo-locked production graph, official JSON Schema + SHACL CI validation, artifact publication, and installer copy/rollback for both sidecars. CI `39600224242` passed; reviewed VEX and actual clean-machine installation remain pending.
- ADRs: 0009 Wintun fallback, 0010 rate limiting+flow capacity, 0011 per-session sequencing, 0012 ETW+support bundle, 0013 per-class sessions, 0014 throughput harness, 0015 continuous development

## Next for G2 (gate: tunnel end-to-end through miniport on test-signed rig)

1. **Complete driver runtime validation**: review NDIS/WDF lifecycle and test the source-level bidirectional queue/injection path on Windows. The MTU/configuration contract remains fail-closed pending a supported NDIS reconfiguration path; `SET_QUANTA` returns not-supported until scheduler integration.
2. **S2b socket test through miniport**: obtain a test-signed driver, then run `ordered_stream_survives_a_real_udp_socket` through the NDIS path on an isolated Hyper-V VM with Driver Verifier and Memory Integrity configured. The unsigned CI artifact cannot be installed as acceptance evidence.
3. **Real S2c/throughput validation:** use the explicit `--apply` netem runner only on disposable interfaces with a separate iperf3 server; the script currently measures raw link paths for seven profiles. Then add/run adapter-level BalanciFy traffic capture and record 10-repetition statistics. No such lab run has been completed in this environment.

## Next for G3 (gate: performance + HVCI)

1. **Throughput rows on Rig B**: run `throughput.rs` + iperf3 on real hardware (wired 1 Gbps, Wi-Fi 6 300 Mbit/s), secure mode, pacing 80% capacity, per-flow off/on, measure aggregate/single-flow/failover/added delay (target ≤3ms) + CPU/Gbit/s via crypto-bench + ETW.
2. **Driver packaging**: bundle self-contained .NET 10 + WinAppSDK runtimes, transactional installer with atomic rollback, clean-machine offline test per `clean-machine-test.md`.
3. **HVCI readiness trio**: Driver Verifier with code-integrity, HLK HVCI readiness test, run on VBS/Memory-Integrity machine (S15).
4. **SBOM/VEX S13**: SPDX 2.3 and 3.0.1 inventories are generated from the locked Cargo graph; the remaining work is reviewed VEX from current advisory data and validating both installer sidecars on a clean machine.

## Next for Phase 4 (secure + GUI)

1. **Client binary + GUI IPC**: CLI now uses pinned authenticated Noise_IK or explicit debug-only loopback lab mode; remaining work is production packet-adapter integration, WinUI 3 shell (Home, Rules, Relay & account, Diagnostics, tray), and authenticated production IPC.
2. **ETW trace validation**: the Windows EventRegister/EventWrite sink and event IDs are implemented; validate collection and useful decoding on a Windows service install, then decide whether a manifest/TraceLogging schema is needed. This cannot be verified in the current sandbox.

## Known gaps (deliberate)

- `Cargo.lock` is committed and checked by the dependency audit.
- Simulated paths are not netem — S2c/Rig B measurements remain pending; raw iperf3 link baselines are not end-to-end BalanciFy measurements.
- Miniport source compiles/links for x64 and ARM64 in CI, but remains non-operational: unsigned artifacts only, no test-signed install, Verifier/HVCI, VM, or packet-path run. The bounded TX queue and service polling are source-level only; `SET_QUANTA` remains `STATUS_NOT_SUPPORTED` until scheduler integration, and `SET_MTU` refuses changes that would diverge from NDIS-cached interface attributes.
- The local service adapter uses loopback TCP only; production named-pipe ACLs and Windows caller authentication remain pending.
- Single session per service default — multi-session demux exists but service opens one session unless per-flow/per-class toggled.
- Wire-v2 FlowData uses authenticated selective FlowAck, and bounded RTO repair resends unacknowledged payload on an alternate path. CI tests the tunnel and service integration; real network loss/reordering and driver-path behavior remain unmeasured.

## References

- Blueprint §4.5 failover, §4.7.4 per-flow sessions, §4.8 dependency policy, §6.1 TAP-like adapter, §6.2 WRH + flow classes, §6.3 capacity, §6.4 DRR + pacing + CC, §6.5 health hysteresis, §6.6 NAT keepalive, §8.2 Diagnostics, §8.3 installer, §9.2 IPC + handshake, §9.3 signing, §9.5 HVCI, §10.1 Rig A/B, §10.3 acceptance rows, §11.2 Wintun exception, S13 SBOM/VEX, S15 HVCI trio.
- ADRs 0003-0041.

## Current validation snapshot

- PR head `02409cf`; PR #19 is open. Latest push run `39922008067` and PR run `39922014929` passed all six CI jobs. Rust CI summaries: 305 Ubuntu / 304 Windows tests.
- Local checks after the documentation refresh: 24 Python CI tests, 12 driver packet/OID/INF test binaries under strict warnings plus AddressSanitizer/UndefinedBehaviorSanitizer; parser test includes 12,000 deterministic length mutations, generated portal checks, SVG XML parsing, and `git diff --check`.
- Runtime acceptance remains outstanding: signed installation, Driver Verifier, HVCI, real NDIS packet path, hardware performance, clean-machine packaging, reviewed VEX, and production GUI/IPC.
