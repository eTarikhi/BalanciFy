# Roadmap next — what remains for G2/G3

Date: 2026-10-19
Status: in progress, continuous development

## Completed since last green (39191895800)

- ECN CE + delay-based CC (PathWindow ecn_ce, rtt_min, rtt_last, delay_events; CC ecn_events/delay_events; on_ecn 80% reduction, on_rtt/on_ack_with_rtt 10% Vegas-like, FLAG_ECN_CE=0x04, service ingest_path checks flag, probe RTT feeds cc.on_rtt)
- Per-flow sessions (BTreeMap<FlowKey,u64>, session_for_flow, evict oldest on cap, open rx session, data path via frame_with_session)
- Per-class sessions (per_class_sessions toggle, class_sessions map, session_for_class FlowClass -> session)
- Per-session seq tracking (session_seqs map, global unique seq for retx simplicity)
- Rate limiting service egress (TokenBucket, RateLimiter per-uplink, try_send, delay_for, wired into send_app_packet -> Hold on limit)
- Flow-level capacity (on_flow_packet counts flow packets into window_bytes, suppresses idle probe, policy wrapper)
- Wintun fallback ADR-0009 (feature-gated, exception file check, raw FFI placeholder)
- Installer transactional (journaled, 7 steps, reverse rollback, dry-run, tests)
- App shell IPC client (balancify-app crate, home/rules/diagnostics/tray modes, 1Hz event subscription, renders ECN/delay)
- ETW stub + support bundle enhanced (EtwProvider, event IDs 100-600, UplinkDiag cwnd/in_flight, CongestionDiag loss/ecn/delay+paths, JSON)
- Software benchmark + benches (throughput.rs local work rate/per-call latency only; drr-bench, pacing-bench, crypto-bench). Real network performance and G3/G4 acceptance remain pending.
- SBOM + clean-machine test docs (sbom.sh SPDX 3.0, clean-machine-test.md offline bundle, rollback, HVCI trio)
- ADRs: 0009 Wintun fallback, 0010 rate limiting+flow capacity, 0011 per-session sequencing, 0012 ETW+support bundle, 0013 per-class sessions, 0014 throughput harness, 0015 continuous development

## Next for G2 (gate: tunnel end-to-end through miniport on test-signed rig)

1. **Driver WDF control device + IOCTLs**: create WDFDEVICE in MiniportInitializeEx, implement `src/ioctl.c` with minimal IOCTLs (set MTU, inject NBL, query stats), length-validated, fuzzed same gate as tunnel. Build with WDK 10.0.22621+, test-signed install on Hyper-V VM with Driver Verifier.
2. **S2b socket test through miniport**: same as `ordered_stream_survives_a_real_udp_socket` but via NDIS path — service sends through miniport, relay receives, reorder, deliver.
3. **Cargo.lock**: generate with `cargo generate-lockfile` once toolchain available, commit, audit checks lock file if present.
4. **Real netem S2c**: replace simulated rig with `tools/lab/netem-rig.sh` profiles (delay-gap 40ms, lossy 1%, asymmetric 200M/50M, cellular 100ms+2%+20Mbit), record 10 reps median/p95/spread per §10.1.

## Next for G3 (gate: performance + HVCI)

1. **Throughput rows on Rig B**: run `throughput.rs` + iperf3 on real hardware (wired 1 Gbps, Wi-Fi 6 300 Mbit/s), secure mode, pacing 80% capacity, per-flow off/on, measure aggregate/single-flow/failover/added delay (target ≤3ms) + CPU/Gbit/s via crypto-bench + ETW.
2. **Driver packaging**: bundle self-contained .NET 10 + WinAppSDK runtimes, transactional installer with atomic rollback, clean-machine offline test per `clean-machine-test.md`.
3. **HVCI readiness trio**: Driver Verifier with code-integrity, HLK HVCI readiness test, run on VBS/Memory-Integrity machine (S15).
4. **SBOM/VEX SPDX 3.0**: generate via `sbom.sh` + cargo cyclonedx, publish sidecar, S13.

## Next for Phase 4 (secure + GUI)

1. **Client binary + GUI IPC**: wire Service handshake into CLI client (done lab mode), then WinUI 3 shell (Home, Rules, Relay & account, Diagnostics, tray) via IPC v1, self-contained .NET 10.
2. **Per-flow session demux**: retx keyed by (session, seq) for true per-session seq spaces, not just global unique.
3. **Flow-level ACK**: add FlowData ACK or at least count for capacity (done via on_flow_packet, but could add explicit ACK).
4. **ETW wiring**: log ECN, delay, failover, rekey, auth_failed via EtwProvider in Service tick/ingest.

## Known gaps (deliberate)

- No Cargo.lock (toolchain not reachable in sandbox, crates.io blocked, only github/pypi/npm allowed) — must be generated first time toolchain available.
- Simulated paths are not netem — S2c real measurements pending.
- Miniport not in loop — driver is functional stub, not yet WDF control device.
- Single session per service default — multi-session demux exists but service opens one session unless per-flow/per-class toggled.
- Flow-level capacity now counts flow packets + probes, but no explicit FlowData ACK.

## References

- Blueprint §4.5 failover, §4.7.4 per-flow sessions, §4.8 dependency policy, §6.1 TAP-like adapter, §6.2 WRH + flow classes, §6.3 capacity, §6.4 DRR + pacing + CC, §6.5 health hysteresis, §6.6 NAT keepalive, §8.2 Diagnostics, §8.3 installer, §9.2 IPC + handshake, §9.3 signing, §9.5 HVCI, §10.1 Rig A/B, §10.3 acceptance rows, §11.2 Wintun exception, S13 SBOM/VEX, S15 HVCI trio.
- ADRs 0003-0015.
