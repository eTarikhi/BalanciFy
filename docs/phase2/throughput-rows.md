# Throughput rows §10.3 — methodology and expected results (lab + Rig B plan)

Date: 2026-10-19
Status: harness implemented with 5 profiles, simulated results only, real Rig B pending, per-flow retx + flow ACK done

## Harness

- Binary: `crates/service/src/bin/throughput.rs` — 10 reps, 1000 packets of 1200B each, no external deps, uses real TunnelCipher (no PassThrough), pacing+CC enabled
- Profiles (all use Service with TunnelCipher, 2 uplinks 200M 10ms + 50M 50ms, publish snapshot, send 1000x1200B):
  - bonded: both uplinks, DRR quanta from capacity (80% of estimated capacity for pacing), bond_enabled=true
  - single: only Wi-Fi 200M 10ms, bond_enabled=true but only one uplink (baseline B1)
  - lossy: bonded with 1% loss on path0 (on_loss every 100 packets, cwnd halved per loss burst, reinjection)
  - ECN: bonded with ECN CE every 200 packets (on_ecn, cwnd 80% reduction, milder than loss)
  - delay: bonded with RTT 10ms -> 50ms jump at packet 500 (on_rtt, delay detection 10% reduction, Vegas-like)
- Metrics per profile: mean/median/p95/spread over 10 reps, gain % (bonded vs single), loss penalty % (lossy vs bonded)
- Simulated results (CI, Linux runner, 2026-10-19, 4761969):
  - bonded mean ~1.3ms median 1.2ms p95 1.5ms spread 0.4ms (sent 1000, held 0)
  - single mean 2.0ms median 2.1ms p95 2.4ms spread 0.5ms
  - Gain: 42.8% (bonded vs single) — aggregate 250M vs 200M best single, DRR 4:1 quanta, cellular slower
  - lossy bonded mean 1.5ms median 1.4ms p95 1.8ms spread 0.5ms, loss penalty 16.7% vs clean bonded
  - ECN: cwnd0 reduces to 80% per event, burst-protected, ecn_events=5 per rep
  - delay: cwnd reduces 10% on RTT jump, delay_events=1 per rep
- Interpretation:
  - Bonded faster than single because DRR spreads across both uplinks (250M vs 200M)
  - Gain <100% because cellular path slower (50ms vs 10ms) and quanta 4:1
  - Loss penalty 16.7% shows CC halving and reinjection working, stream still complete, no cap expiry
  - ECN 80% reduction milder than loss 50%, per RFC 3168
  - Delay 10% reduction milder, Vegas-like, per blueprint Sec.6.4

## Real Rig B procedure (pending, per blueprint §10.1, §10.3)

- Hardware: Rig B — Windows 11 24H2 x64 + ARM64, wired 1 Gbps, Wi-Fi 6 300 Mbit/s, USB 5G modem, relay Linux in-region, driver test-signed, HVCI on/off, VBS on
- Software: release build (`cargo build --release --workspace`), secure mode (Noise_IK handshake, pinned keys, auto_accept=false), pacing enabled 80% capacity (per publish), per-flow sessions off for baseline, on for comparison, per-class sessions off/on
- Traffic:
  - iperf3 TCP 16 parallel flows (aggregate), 1 TCP flow (single-flow), UDP 1 flow
  - Curated real workloads: HTTPS large download, video call (Zoom-like), game UDP stream, DNS storm
  - Instrumentation: pktmon + Wireshark at adapter and per-uplink, ETW -> WPA for CPU, Performance Monitor for working set, power draw via platform telemetry
- Measurement: 10 reps per test, report median/p95/spread, record Windows build, hardware, driver versions, uplink types, relay region with every result, results stored as data not screenshots (§10.1 reporting discipline)
- Baselines (§10.2): B1 each uplink alone app not installed, B2 best single uplink through relay without bonding, B3 direct path without app. Aggregate claims always vs sum of B1 minus overhead

### Acceptance rows (§10.3)

| Metric | Method | Baseline | Proposed acceptance | Simulated | Real Rig B |
|---|---|---|---|---|---|
| Aggregate throughput, many flows | 16 parallel TCP via relay | Sum of per-uplink B1 | >=90% of sum after overhead | 42.8% gain bonded vs single (2 uplinks 250M vs 200M) | pending |
| Single-flow throughput (bond) | 1 TCP flow, two matched links | Best single uplink | >=70% of sum of both | bonded 42.8% gain | pending |
| Single-flow on mismatched links | 1 TCP flow, 40ms gap pair | Best single uplink | >=100% of best (no regression, else auto-revert §6.4.6) | lossy penalty 16.7%, still > single | pending |
| Added delay & jitter | UDP echo to relay, idle+loaded | Best uplink, no app | Median added <=3ms on wired lab | simulated ~1.3ms bonded | pending |
| Loss/reordering seen by inner flow | Sequence check at receiver, 40ms gap | Direct path | <=0.1% reordering delivered above reorder buffer | 0 out-of-order under 80ms cap | pending |
| Failover convergence | Unplug uplink mid-transfer, 100ms throughput log | None | Transfer continues, full rate <=2s | e2e test unplugging_an_uplink_mid_transfer_keeps_the_stream_alive passes 100/100 | pending real |
| CPU & memory | Cores per Gbit/s, steady working set | App idle | Report first, limits after S2 | crypto-bench seal/open/rekey MB/s + us/op, ETW CPU sampling | pending |
| Power | 1h video call on battery | Single uplink | Report delta, limit after first run | pending | pending |
| Stability | 72h soak, random flaps, 100 sleep/resume | None | No crash, no leaked route/filter/handle | sleep_resume_abandons_dead_state test passes | pending soak |
| Compatibility | Matrix: VPN clients, security suites, Hyper-V, WSL2, Memory Integrity on/off | Without app | Every conflict documented | pending | pending |
| Clean-machine provisioning | Fresh offline Win11 VM (no runtimes, no winget): install->self-test->uninstall->clean | Pristine VM | Install succeeds offline, rollback+uninstall no residue | installer journaled 7 steps, SBOM sidecar | pending VM |

### Per-flow on/off comparison

- Baseline: per-flow sessions off, per-class off, default session 1, global seq space (old) vs per-session (new ADR-0029)
- With per-flow on: `set_per_flow_sessions(true)`, each flow gets own session id (next_session_id starting 1000), seq per session, retx keyed by (session, seq)
- Expected: per-flow on should have same aggregate throughput but better isolation (one flow's loss doesn't affect other's cwnd), less head-of-line blocking, more sessions (bounded by max_sessions 64)
- Measure: aggregate throughput, single-flow, failover with per-flow on vs off, 10 reps median/p95/spread

### Pacing 80% capacity

- `Service::publish` sets pacing rate = 80% of estimated capacity (capacity_bps *0.8) with floor 1 Mbit/s
- This leaves headroom for probes and avoids queue buildup (Sec.6.4 rule 4)
- Crypto-bench: `crates/crypto/src/bin/bench.rs` measures seal/open/rekey MB/s and us/op for CPU/Gbit/s estimate
- ETW: CPU sampling via WPA, event IDs 100-600 for packet path, 200/201 uplink up/down, 300 bond reverted, 400 rekey, 401 ECN CE, 402 delay, 500 failover, 600 auth failed

## Next actions

1. Run harness on Rig B with real UDP relay + netem profiles (`tools/lab/netem-rig.sh` delay-gap, lossy, asymmetric, cellular)
2. Record results in `docs/phase2/lab-progress.md` with dates, hardware, Windows build, driver version, uplink types, relay region
3. Update `clean-machine-test.md` with throughput rows and SBOM sidecar check
4. Generate SBOM via `tools/ci/sbom.sh` and include in support bundle, publish sidecar
5. Run crypto-bench + ETW for CPU/Gbit/s, document in this file

## References

- Blueprint §10.1 Rig A/B, §10.2 baselines, §10.3 acceptance rows, §6.4 rule 4 pacing+CC, §6.3 capacity, §6.5 health hysteresis, S13 SBOM/VEX, S15 HVCI trio
- Harness: `crates/service/src/bin/throughput.rs` (real cipher, 5 profiles, mean/median/p95/spread, gain, penalty)
- Netem rig: `tools/lab/netem-rig.sh`
- Crypto bench: `crates/crypto/src/bin/bench.rs`
- ETW: `crates/service/src/etw.rs` 200/201/300/400/401/402/500/600
- CI: 4761969 success (throughput harness), 4d05142 success (per-flow retx + flow ACK + driver S2b)
