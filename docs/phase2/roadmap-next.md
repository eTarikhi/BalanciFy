# Roadmap next — what remains for G2/G3

Date: 2026-10-21
Status: in progress; software work continues, hardware gates remain pending

## Completed since last green (39191895800)

- ECN CE + delay-based CC (PathWindow ecn_ce, rtt_min, rtt_last, delay_events; CC ecn_events/delay_events; on_ecn 80% reduction, on_rtt/on_ack_with_rtt 10% Vegas-like, FLAG_ECN_CE=0x04, service ingest_path checks flag, probe RTT feeds cc.on_rtt)
- Per-flow sessions (BTreeMap<FlowKey,u64>, session_for_flow, evict oldest on cap, open rx session, data path via frame_with_session)
- Per-class sessions (per_class_sessions toggle, class_sessions map, session_for_class FlowClass -> session)
- Per-session seq tracking (session_seqs map, global unique seq for retx simplicity)
- Rate limiting service egress (TokenBucket, RateLimiter per-uplink, try_send, delay_for, wired into send_app_packet -> Hold on limit)
- Flow-level capacity uses authenticated per-packet FlowAck receipts in wire v2; sender-side pending ACK state is bounded and capacity samples are credited to the original uplink.
- Wintun fallback ADR-0009 (feature-gated, exception file check, raw FFI placeholder)
- Installer transactional (journaled, 7 steps, reverse rollback, dry-run, tests)
- App shell IPC client (balancify-app crate, home/rules/diagnostics/tray modes, 1Hz event subscription, renders ECN/delay)
- Service IPC v1 loopback adapter handles requests and rate-limited event push; service snapshots combine warning and counter events. The shell's bundle mode requests opcode 11 and receives the complete redacted support JSON. Concurrency is capped at 16, idle unsubscribed clients close after 60 seconds, and nonblocking lab UDP polling avoids stalling the dispatcher. Production named-pipe authentication remains pending.
- Netem runner defaults to no-side-effect dry-run; explicit `--apply` supports seven raw link profiles with JSON capture and safety tests. It is not BalanciFy/driver acceptance evidence.
- Driver control source hardened to SYSTEM/Admin access and owned NBL packet buffers; `MiniportInitializeEx` registers the adapter, submits revision-2 general attributes, and advertises a minimal OID surface. The miniport and INF now consistently use NdisMediumIP for raw packets; OIDs report bounded lookahead and packet counters; the miniport reads the INF Mtu parameter with a safe default; INJECT_NBL validates IPv4/IPv6 declared lengths and the configured MTU, and rejects paused requests. MiniportPause now defers completion until all indicated receive NBLs are returned, with a portable-tested state helper. The KMDF symbolic-link and queue teardown paths were corrected. This remains source-only and unbuilt; WDK/Verifier validation is pending.
- File-backed key storage uses unique private atomic temporaries, Unix 0600-at-create, zeroized plaintext copies, and Windows DPAPI by default (with legacy raw-key migration tests).
- Tunnel wire v2 retains frame-kind AEAD nonce separation and adds authenticated selective FlowAck; FlowData has its own bounded replay-protected sequence lane, keepalives no longer repeat sequence zero, control frames authenticate and pass replay checks before changing service state, and retransmission/ACK state is keyed by `(session, seq)` with per-session Data sequence spaces.
- Flow-level capacity now uses peer FlowAck receipts, with bounded per-session pending bytes/entries, replay verification, duplicate re-ACK, rate-limit cleanup, and a regression test for service sampling.
- ETW stub + support bundle enhanced (EtwProvider, event IDs 100-600, UplinkDiag cwnd/in_flight, CongestionDiag loss/ecn/delay+paths, JSON)
- Software benchmark + benches (throughput.rs local work rate/per-call latency only; drr-bench, pacing-bench, crypto-bench). Real network performance and G3/G4 acceptance remain pending.
- SBOM + clean-machine test docs exist; current `sbom.py` emits SPDX 2.3 from the locked Cargo graph. SPDX 3.0/VEX and clean-machine/installer validation remain pending.
- ADRs: 0009 Wintun fallback, 0010 rate limiting+flow capacity, 0011 per-session sequencing, 0012 ETW+support bundle, 0013 per-class sessions, 0014 throughput harness, 0015 continuous development

## Next for G2 (gate: tunnel end-to-end through miniport on test-signed rig)

1. **Complete and build the driver source**: source now registers attributes, exposes a minimal OID set, aligns the INF/miniport on NdisMediumIP for raw packets, validates injected IP versions/lengths against MTU, and drains the WDF queue before teardown. Review remaining NDIS/WDF lifecycle details and build x64 + ARM64 using WDK 10.0.22621+ (not available in this sandbox).
2. **S2b socket test through miniport**: only after a successful WDK build, run `ordered_stream_survives_a_real_udp_socket` through the NDIS path on a test-signed Hyper-V VM with Driver Verifier and Memory Integrity configured.
3. **Real S2c/throughput validation:** use the explicit `--apply` netem runner only on disposable interfaces with a separate iperf3 server; the script currently measures raw link paths for seven profiles. Then add/run adapter-level BalanciFy traffic capture and record 10-repetition statistics. No such lab run has been completed in this environment.

## Next for G3 (gate: performance + HVCI)

1. **Throughput rows on Rig B**: run `throughput.rs` + iperf3 on real hardware (wired 1 Gbps, Wi-Fi 6 300 Mbit/s), secure mode, pacing 80% capacity, per-flow off/on, measure aggregate/single-flow/failover/added delay (target ≤3ms) + CPU/Gbit/s via crypto-bench + ETW.
2. **Driver packaging**: bundle self-contained .NET 10 + WinAppSDK runtimes, transactional installer with atomic rollback, clean-machine offline test per `clean-machine-test.md`.
3. **HVCI readiness trio**: Driver Verifier with code-integrity, HLK HVCI readiness test, run on VBS/Memory-Integrity machine (S15).
4. **SBOM/VEX S13**: the current CI output is SPDX 2.3 from the locked Cargo graph. Add standards-valid SPDX 3.0 serialization, reviewed VEX from current advisory data, publish the sidecar, and validate installer copy.

## Next for Phase 4 (secure + GUI)

1. **Client binary + GUI IPC**: CLI now uses pinned authenticated Noise_IK or explicit debug-only loopback lab mode; remaining work is production packet-adapter integration, WinUI 3 shell (Home, Rules, Relay & account, Diagnostics, tray), and authenticated production IPC.
2. **ETW coverage**: consider bond-revert and uplink up/down events beyond the existing ECN, delay, failover, rekey, and authentication events.

## Known gaps (deliberate)

- `Cargo.lock` is committed and checked by the dependency audit.
- Simulated paths are not netem — S2c/Rig B measurements remain pending; raw iperf3 link baselines are not end-to-end BalanciFy measurements.
- Miniport source is not operational: NDIS attributes are now registered in source, but no WDK build, test-signed install, or Verifier run has been completed.
- The local service adapter uses loopback TCP only; production named-pipe ACLs and Windows caller authentication remain pending.
- Single session per service default — multi-session demux exists but service opens one session unless per-flow/per-class toggled.
- Flow-level ACK frames are supported in tunnel wire v2, but no FlowData retransmission/repair is provided: loss can suppress capacity samples while delivered payload remains unaffected.

## References

- Blueprint §4.5 failover, §4.7.4 per-flow sessions, §4.8 dependency policy, §6.1 TAP-like adapter, §6.2 WRH + flow classes, §6.3 capacity, §6.4 DRR + pacing + CC, §6.5 health hysteresis, §6.6 NAT keepalive, §8.2 Diagnostics, §8.3 installer, §9.2 IPC + handshake, §9.3 signing, §9.5 HVCI, §10.1 Rig A/B, §10.3 acceptance rows, §11.2 Wintun exception, S13 SBOM/VEX, S15 HVCI trio.
- ADRs 0003-0037.
