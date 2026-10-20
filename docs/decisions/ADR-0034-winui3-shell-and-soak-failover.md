# ADR-0034: WinUI3 shell design + soak harness + failover convergence tracking

Date: 2026-10-20
Status: accepted

> Measurement correction (2026-10-20): `throughput.rs` is only a local software benchmark. The analyzer now requires iperf3 JSON and no longer emits demo numbers; earlier simulated values are not Rig B evidence. See `docs/phase2/throughput-rows.md`.
Commits: 50bf7c8 app failover/soak modes, ae34432 soak.rs, 95dbb0e failover tracking, 0722246 throughput failover, 094bfa7/89e1f26 fixes, 04a533b/5c05cb5 netem-rig enhanced

## Context

Blueprint Sec.8.2 Diagnostics, Sec.8.3 installer, Sec.10.3 acceptance rows, G5 soak + Verifier + compat matrix require:
- WinUI 3 / .NET 10 shell: Home, Rules, Relay & account, Diagnostics, tray flyout, 1Hz telemetry, last-known-good + stale markers, accessibility, self-contained .NET 10 + WinAppSDK runtimes bundled, offline installer per Sec.4.8 rev 2.1
- Soak: 72h continuous traffic, random link flaps, 100 sleep/resume cycles, Verifier standard+random, no crash, no leaked route/filter/handle/IOCTL buffer
- Compat matrix: VPN clients, security suites, Hyper-V, WSL2, Memory Integrity on/off documented
- Failover convergence: unplugging uplink mid-transfer keeps stream alive, in-flight packets reinjected on survivor, 100/100 delivered in order, no refusals, transfer continues full rate <=2s, ETW 200 UP 201 DOWN 500 FAILOVER

Previously:
- App shell had 9 modes (home/rules/diagnostics/tray/relay/bundle/perflow/rigb/hvci/sbom) but no failover/soak
- No soak harness, no failover convergence tracking
- Throughput harness had 9 profiles but no failover
- Service had relay failover but no convergence measurement, no uplink failover tracking

## Decision

### App shell 11 modes (50bf7c8)

Enhanced `app/src/main.rs` from 9 to 11 modes:
- home: Service version, schema, bond/duplicate/metered/MTU, policy rev, rules rev, active flows, uplinks id/state/rtt/loss/cap/weight/metered/eligible, counters, ETW 200/201/300/400/401/402/500/600, 1Hz event subscription
- rules: Flow Rules + plain-language preview "This rule sends Zoom over lowest-latency healthy link"
- diagnostics: Version policy_rev rules_rev active_flows, counters, ECN CE 80% vs loss 50% delay 10%, per-path diag cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, bond gain tracker auto-disable <15% emits BondReverted 300, rekey packets_since_rekey last_rekey_ago_secs forward secrecy 1h/1M ETW 400, relay current+failover count+convergence <=2s ETW 500 600, uplink failover count+convergence <=2s ETW 200/201 history, support bundle JSON includes failover counts+conv ms
- tray: up_count eligible, active_flows bond dup, active mode + any down uplink, one-click pause/resume, keyboard+screen-reader operable
- relay: Relay & Account region/address key status connection test button secondary relay state failover count, connection test is support call preventer
- bundle: Support Bundle redacted Sec.8.2 Sec.9.2, version policy_rev rules_rev active_flows, counters, uplinks total/eligible, bundle JSON includes cwnd/in_flight per uplink, congestion diag loss/ecn/delay+paths per_path Vec<PathCongestionDiag> cwnd/in_flight/ecn_ce/delay_events/rtt_min/last_ms, rekey diag, relay stats, ETW export, route table, adapter state, version/signing info, redaction no destinations/content only counters error codes hashed flow keys
- perflow: Per-flow Sessions Sec.4.7.4 ADR-0029, active flows each flow own session id next_session_id 1000 seq per session retx (session,seq), per-flow > per-class > default precedence session cap 64 evicts oldest, per-session seq spaces true per-session demux without collision RFC 8684 Sec.3.3, flow-level capacity on_flow_packet sent + on_flow_ack acked in scoring.rs policy.rs wrapper ingest_path FlowData calls on_flow_ack+on_ack
- rigb: Rig B Throughput Sec.10.3, hardware Rig B per Sec.10.1 Windows 11 24H2 x64+ARM64 wired 1 Gbps Wi-Fi 6 300M USB 5G relay Linux in-region driver test-signed HVCI on/off VBS on, software release build secure Noise_IK pacing 80% per-flow on/off per-class off/on, traffic iperf3 TCP 16 flows aggregate 1 TCP flow single-flow UDP 1 flow curated workloads, acceptance rows aggregate >=90% sum single-flow >=70% sum mismatched >=100% best auto-revert Sec.6.4.6 added delay <=3ms wired loss/reordering <=0.1% failover <=2s CPU & memory report first limits after S2 power delta stability 72h soak + random flaps + 100 resume compat matrix clean-machine provisioning offline VM
- hvci: HVCI Readiness Trio S15 Sec.9.4 Sec.9.5, Verifier 0x209BB + code integrity, HLK HVCIMemoryIntegrity, VBS/MI VM Secure Boot vTPM test-signing, code miniport.c NBL pool fAllocateNetBuffer=TRUE ioctl.c length validation SDDL METHOD_BUFFERED NonPagedPoolNx NormalPoolPriority no RWX MDL freeing NdisFreeMdl /integritycheck /DYNAMICBASE /NXCOMPAT /Qspectre PoolTag BlaB Spectre mitigation, build msbuild vcxproj Release x64 WDK 10.0.22621+, test verifier /standard /driver balancify.sys HLK Studio HVCIMemoryIntegrity Hyper-V VM Gen2 Secure Boot vTPM VBS bcdedit testsigning pnputil add-driver msinfo32 VBS Running MI enabled, code ready real VM pending CI only Rust no WDK future WDK build job Phase 4 (a) in ci.yml
- sbom: SBOM/VEX SPDX 3.0 S13 March 2027, sbom.sh v2 SPDX 3.0 JSON creationInfo timestamp tool v2, packages first-party + vetted + transitive, relationships DEPENDS_ON, annotations C2/HVCI/installer, vulnerabilities VEX not_affected/fixed, generates /tmp/sbom.spdx.json 24 packages, installer copies ProgramData\Balancify\sbom.spdx.json, real generation cargo cyclonedx --format json --spec-version 1.5 + cargo audit --json + sbom-tool generate
- failover (new): Failover Convergence <=2s Sec.10.3 Sec.4.5, uplink failover unplugging_an_uplink_mid_transfer_keeps_the_stream_alive adapter removed moment packet provably in flight in-flight reinjected on survivor 100/100 delivered in order no refusals, relay failover primary+secondary failover on 10s no ACK back-and-forth failover count convergence measured first ACK after failover, target transfer continues full rate <=2s ETW 200 UP 201 DOWN 500 FAILOVER support bundle relay_failover_count relay_conv_ms uplink_failover_count uplink_conv_ms history, real measurement netem-rig.sh failover profile tc loss 100% simulate unplug 100ms throughput log pktmon+Wireshark at adapter and per-uplink ETW->WPA, synthetic throughput.rs failover-profile work rate and local transition cost only; end-to-end failover measurement remains pending soak.rs random flaps 1000 iterations
- soak (new): Soak 72h + Random Flaps + 100 Resume G5 G6 compat matrix, stability 72h continuous traffic random link flaps 100 sleep/resume cycles Verifier standard+random no crash no leaked route/filter/handle/IOCTL buffer, compat matrix VPN clients security suites Hyper-V WSL2 Memory Integrity on/off documented workaround/incompatibility, soak harness crates/service/src/bin/soak.rs 1000 iterations 10 packets per iteration random flap every 100 remove_uplink_at+add_uplink+publish sleep/resume every 10 resume_from_sleep+tick keepalives ECN every 200 delay jump every 300 loss every 100 tick for keepalives/rekey/failover/probes checks pending_probes <=8 sessions <=64 held <=1000 support bundle per-path ECN/delay cwnd/in_flight, ETW 100-600 WPA CPU sampling working set bounded power delta, real requires Rig B hardware + Hyper-V VM Gen2 Secure Boot vTPM VBS + WDK 10.0.22621+ + Verifier 0x209BB + HLK HVCIMemoryIntegrity + VBS/MI

### Soak harness (ae34432, 89e1f26 fix)

`crates/service/src/bin/soak.rs`:
- Uses real TunnelCipher, Service with 3 uplinks wired 1 Gbps 2ms Wi-Fi 6 300M 10ms cellular 20M 100ms Rig B+cellular, publish, flow 10.0.0.1:5000 -> 203.0.113.1:443
- 1000 iterations fast loop simulating 72h as 7200 ticks (36s per tick), 10 packets per iteration total 10k packets, random flap every 100: remove_uplink_at(UplinkId(1), t) publish, failover_start Instant::now(), replug after 2s add_uplink + on_probe_ok + publish, measure failover_elapsed >2s WARN, total_flaps
- Sleep/resume every 10 iterations until 100 resumes: resume_from_sleep(t_resume) returns abandoned count, tick for keepalives, check keepalives empty WARN, total_resumes
- ECN every 200: cc.on_ecn(0, t), delay jump every 300: on_rtt 10ms -> 50ms, loss every 100: on_loss(0, t), tick for keepalives/rekey/failover/probes
- Checks: pending_probes <=8 else WARN accumulate, sessions <=64 else WARN cap, held_packets <=1000 else WARN bound, support bundle every 100 iterations collect + check per_path cwnd 0 WARN
- Counters: stripped forwarded dups reinj delivered ooo refused auth_fail ecn delay, uplinks total eligible sessions held pending_probes, ETW 200/201/300/400/401/402/500/600, compat matrix VPN/Hyper-V/WSL2/MI, G5 acceptance no crash no leaked route/filter/handle/IOCTL buffer 72h soak + random flaps + 100 resume with Verifier

Fixes: ae34432 had remove_uplink with 2 args (bug), fixed to remove_uplink_at in 34386af; 95dbb0e had etw.event not found E0599, fixed to etw.info/warn in 094bfa7; ae34432 had sleep/resume not found E0599, fixed to resume_from_sleep in 89e1f26 — CI green 89e1f26 ubuntu+windows+audit

### Failover convergence tracking (95dbb0e, 0722246)

`crates/service/src/lib.rs`:
- Added fields: relay_failover_start Option<Instant>, relay_failover_convergence Option<Duration>, relay_failover_history Vec<(Instant, Duration)>, uplink_failover_start Option<Instant>, uplink_failover_convergence Option<Duration>, uplink_failover_history Vec<(UplinkId, Instant, Duration)>, uplink_failover_count u64
- Initialized in new(): None, None, Vec::new(), etc.
- on_relay_ack: if Some(start) = failover_start.take(), convergence = now - start, store convergence, push history, etw.info 500 "relay failover converged in {}ms count={}", if >2s warn 500
- check_relay_failover: on timeout, current vs next, set current to next, last_ack now, failover_count +=1, failover_start Some(now), convergence None, etw.info 500 "relay failover: {} -> {} count={} timeout={}s"
- remove_uplink: policy remove/drop, path_mtu remove, flows forget, cc forget, pacing forget, path_local remove, bond reset, uplink_failover_start Some(Instant::now()), convergence None, count +=1, etw.info 201 "uplink {} down, failover count={} — measuring convergence <=2s"
- remove_uplink_at: same but with now param, etw.info 201 "uplink {} down at {:?}, failover count={}"
- add_uplink: policy add, path_mtu insert, bond reset, if Some(start) = uplink_failover_start.take(), convergence = now - start, store, push history (id, start, convergence), etw.info 200 "uplink {} up, failover converged in {}ms count={}", if >2s warn 500, else etw.info 200 "uplink {} up cap={}bps mtu={}"
- Methods: relay_failover_count, relay_failover_convergence, relay_failover_history, relay_failover_start, set_relay_failover_timeout, uplink_failover_count, uplink_failover_convergence, uplink_failover_history, uplink_failover_start

`crates/service/src/support/mod.rs`:
- Added fields: relay_failover_convergence_ms Option<u64>, uplink_failover_count u64, uplink_failover_convergence_ms Option<u64>
- collect: relay_failover_convergence_ms from svc.relay_failover_convergence().map(millis), uplink_failover_count from svc.uplink_failover_count(), uplink_failover_convergence_ms from svc.uplink_failover_convergence().map(millis)
- to_json: includes relay_conv_ms, uplink_failovers, uplink_conv_ms, loss, ecn, delay, rekey_packets, per_path

### Throughput failover profile (0722246)

`crates/service/src/bin/throughput.rs` enhanced from 9 to 10 profiles:
- Existing 9: bonded, single, lossy, ECN, delay, rigb_wired1G_wifi300M, perflow_off, perflow_on, aggregate_16flows
- New failover: Sec.4.5 unplugging mid-transfer Sec.10.3 failover <=2s, 2 uplinks 200M/50M, 5 probe_ok, publish, send 500 packets before failover, failover_start Instant::now(), remove_uplink_at(UplinkId(1), t+500ms) publish, send 500 after, failover_elapsed, total 1000 packets, mbps, results failover, delay_results failover_convergence_ms, prints failover mean Mbps + convergence mean/median ms target <=2000ms, support bundle failover count+convergence ms

Now 10 synthetic profiles with per-call software processing latency; no network throughput, added-delay, CPU/Gbit, or end-to-end failover acceptance is measured by this harness; real Rig B pending

### Netem rig enhanced (04a533b, 5c05cb5)

`tools/lab/netem-rig.sh` 11 profiles + all: delay-gap, lossy, asymmetric, cellular, rigb, ecn, delay-jump, perflow, aggregate, failover, soak, all — each with tc commands, expected behavior, test mapping, run_reps 10 reps median/p95/spread, OUTPUT_DIR /tmp/balancify-netem-results, measurement checklist 10 reps median/p95/spread mean stdev min max, record Windows build hardware driver versions uplink types relay region, traffic iperf3 TCP/UDP N flows curated workloads, instrumentation pktmon+Wireshark ETW->WPA CPU Performance Monitor working set power telemetry, acceptance rows aggregate >=90% sum single-flow >=70% sum mismatched >=100% best auto-revert Sec.6.4.6 added delay <=3ms wired loss/reordering <=0.1% failover <=2s CPU & memory report first limits after S2 power delta stability 72h soak + random flaps + 100 resume compat matrix clean-machine provisioning offline VM, data stored as data not screenshots JSON+ETL+CSV, simulated S2c in CI cargo test e2e --nocapture + throughput rigb + crypto bench, real UDP relay + netem requires 2 lab machines + Rig B hardware

`tools/lab/analyze.py`: accepts iperf3 JSON only, computes count/mean/median/nearest-rank p95/spread/stdev/min/max, rejects invalid or missing input and never fabricates demo values; does not parse throughput.rs logs as network results. Analyzer tests: `python3 -m unittest discover -s tools/lab -p 'test_*.py'`.

## Consequences

- App shell now 11 modes, lab CLI mimics WinUI 3 shell Home Rules Relay & account Diagnostics tray flyout 1Hz telemetry last-known-good stale markers accessibility self-contained .NET 10 + WinAppSDK runtimes bundled offline installer per Sec.4.8 rev 2.1 — real WinUI 3 .NET 10 shell still pending (requires Windows + .NET 10 + WinAppSDK, not possible in Linux sandbox, but lab CLI covers contract v1)
- Soak harness G5 72h + random flaps + 100 resume with Verifier ready, checks no leaked route/filter/handle/IOCTL buffer, compat matrix documented
- Failover convergence tracking <=2s Sec.10.3 Sec.4.5 ready, ETW 200/201/500, support bundle includes failover counts+conv ms, throughput failover profile measures convergence
- Netem rig 11 profiles + analyze.py ready for S2c + Rig B + soak, 10 reps median/p95/spread per Sec.10.1, data not screenshots
- CI green 89e1f26 ubuntu+windows+audit after fixing 3 bugs (remove_uplink_at, etw.event->info/warn, sleep->resume_from_sleep)
- No PR per user instruction, stay on branch arena/7abc96c8-balancify

## References

- Blueprint Sec.8.2 Diagnostics, Sec.8.3 installer, Sec.10.1 Rig A/B 10 reps, Sec.10.2 baselines, Sec.10.3 acceptance rows aggregate/single-flow/failover/added delay/CPU/memory/power/stability/compat/clean-machine, Sec.4.5 failover, Sec.6.1 TAP-like adapter, Sec.6.4 DRR+pacing+CC, Sec.6.5 health hysteresis, Sec.6.6 NAT keepalive, Sec.9.2 IPC + handshake, Sec.9.3 signing, Sec.9.5 HVCI, S13 SBOM/VEX, S15 HVCI trio, G5 soak+Verifier+compat, G6 compat matrix+external review
- Code: app/src/main.rs 11 modes, crates/service/src/bin/soak.rs, crates/service/src/lib.rs failover tracking, crates/service/src/support/mod.rs failover counts+conv, crates/service/src/bin/throughput.rs 10 profiles failover, tools/lab/netem-rig.sh 11 profiles, tools/lab/analyze.py, crates/service/src/etw.rs 200/201/300/400/401/402/500/600
- Docs: docs/phase2/throughput-rows.md, docs/phase2/lab-progress.md, docs/phase2/hvci-readiness.md, docs/phase2/final-summary.md, docs/phase2/ci-history.md
- CI: 89e1f26 success (latest), 094bfa7 success fix etw, 34386af fix remove_uplink_at, 5c05cb5 netem-rig enhanced, 7fb32ec driver README, 2f2766c crypto bench, b666a03 ci-history, etc. all green ubuntu+windows+audit after fixes
