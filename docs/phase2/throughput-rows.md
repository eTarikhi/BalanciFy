# Throughput rows §10.3 — synthetic harness and Rig B plan

**Status (2026-10-20):** The current `throughput.rs` binary is a software-path microbenchmark only. It exercises `Service::send_app_packet` with configured link-capacity metadata and a real `TunnelCipher`, but it does not send packets through a driver, socket, relay, or simulated network. Its payload-equivalent work rate, local call latency, congestion-event workloads, and local failover transition cost are **not** network throughput, adapter-added delay, loss penalty, end-to-end failover convergence, or CPU/Gbit. They must not be used to claim that any §10.3 acceptance row passes.

No G3/G4 hardware performance acceptance row is currently demonstrated. Real Rig B and S2c results remain pending.

## Synthetic software-path benchmark

- Binary: `crates/service/src/bin/throughput.rs`
- Workloads: bonded-shaped capacities 200M/50M, single-path-shaped capacity, loss/ECN/RTT event injections, Rig B-shaped capacities (1G/300M), per-flow session on/off, 16-flow workload, and a state-transition scenario.
- Cryptography: `TunnelCipher` (ChaCha20-Poly1305); no PassThrough cipher.
- Reports: payload-equivalent software work rate, plus per-call `send_app_packet` processing latency (mean/median/p95) for bonded, single, and Rig B-shaped configurations. The sample timings include local process/runtime overhead and timer granularity.
- The failover scenario reports only the local elapsed time for `remove_uplink_at` plus `publish`; it does not wait for a remote ACK or verify end-to-end packet continuity.
- The configured RTT/capacity values inform control-plane calculations only; they do not delay packets or constrain the CPU work loop like a real link.
- Any historical output that called these rates “throughput,” software call time “added delay,” or the state-transition timer “failover convergence” is superseded and invalid for acceptance reporting.

## Real Rig B procedure (pending, per blueprint §10.1, §10.3)

- **Hardware:** Rig B — Windows 11 x64 (and ARM64 where available), wired 1 Gbit/s, Wi-Fi 6 300 Mbit/s, optional USB 5G uplink, Linux relay in-region, test-signed Balancify driver; record HVCI/VBS state.
- **Software:** Release build, secure Noise_IK session with pinned keys, driver in the packet path, 80% pacing, per-flow sessions off/on as separate runs. Do not infer these results from the synthetic harness.
- **Traffic:** iperf3 TCP (16 parallel flows and 1 flow), UDP echo, plus curated HTTPS, voice/video, game-like UDP, and DNS workloads.
- **Instrumentation:** pktmon/Wireshark at adapter and per-uplink, ETW/WPA CPU sampling, Performance Monitor working set, and platform power telemetry.
- **Method:** 10 repetitions per row; report median, p95, spread, hardware/driver/Windows versions, uplink type and relay region. Store raw data, not screenshots. Compare against the §10.2 B1/B2/B3 baselines.

### Acceptance rows (§10.3)

| Metric | Method | Proposed acceptance | Current status |
|---|---|---|---|
| Aggregate throughput, many flows | 16 parallel TCP flows through the real driver and relay | ≥90% of summed B1 uplink rates after overhead | Pending Rig B |
| Single-flow throughput (bond) | One TCP flow across matched links | ≥70% of the summed link rates | Pending Rig B |
| Single-flow on mismatched links | One TCP flow with a 40 ms path gap | At least the best single-link baseline; otherwise auto-revert | Pending Rig B |
| Added delay and jitter | UDP echo, idle and loaded, compared with the no-app baseline | Median added delay ≤3 ms on wired lab | Pending Rig B; local harness is not applicable |
| Loss/reordering | Receiver sequence check under 40 ms path gap | ≤0.1% delivered out of order above reorder-buffer allowance | Pending real netem/Rig B |
| Failover convergence | Unplug uplink mid-transfer; log 100 ms throughput and remote ACKs | Transfer continues; convergence ≤2 s | Pending real driver/relay test |
| CPU and memory | ETW/WPA cores per Gbit/s plus steady working set | Report first; set limits after baseline | Pending real traffic |
| Power | One-hour video-call workload on battery | Report delta vs single uplink | Pending hardware |
| Stability | 72-hour soak, random flaps, 100 sleep/resume cycles | No crash or leaked route/filter/handle | Pending real driver soak |
| Compatibility | VPN clients, security suites, Hyper-V, WSL2, HVCI on/off | Document every conflict and workaround | Pending Windows matrix |
| Clean-machine provisioning | Fresh offline Windows VM, install/self-test/uninstall | Offline success, rollback/uninstall leaves no residue | Pending Windows VM |

## Next actions

1. Run S2c profiles on the Linux netem rig and record 10 repetitions per profile.
2. Complete the WDK miniport path and test-signed VM validation before treating Rig B throughput as representative.
3. Run secure-mode Rig B rows with real packet instrumentation and retain raw data.
4. Measure cipher microbenchmarks separately with `crates/crypto/src/bin/bench.rs`; measure end-to-end CPU/Gbit using ETW/WPA under actual traffic.
5. Update this table only from reproducible hardware runs; keep simulated and synthetic results explicitly separate.

## References

- Blueprint §10.1 Rig A/B, §10.2 baselines, §10.3 acceptance rows, §6.4 pacing/congestion control, S13 SBOM/VEX, S15 HVCI readiness.
- Synthetic harness: `crates/service/src/bin/throughput.rs`.
- Netem rig: `tools/lab/netem-rig.sh`.
- Crypto microbenchmark: `crates/crypto/src/bin/bench.rs`.
- Driver guidance: `driver/README.md`, `docs/phase2/hvci-readiness.md`.
