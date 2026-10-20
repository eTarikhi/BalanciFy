# ADR-0033: Netem rig enhanced with Rig B, ECN, delay-jump, per-flow, aggregate, failover, soak + analyze.py

Date: 2026-10-20
Status: accepted
Commits: 5c05cb5 netem-rig.sh + analyze.py, 0581256 throughput 9 profiles, ef1ee69 original

## Context

Blueprint Sec.10.1 Rig A/B, Sec.10.3 acceptance rows, S2c reorder/reinjection measurements on netem rig require:
- 10 reps per test, median/p95/spread per Sec.10.1 reporting discipline (data not screenshots)
- Profiles: delay-gap 40ms, lossy 1%, asymmetric 200M/50M, cellular 100ms+2%+20Mbit
- Rig B: wired 1 Gbps, Wi-Fi 6 300 Mbit/s, USB 5G modem, relay Linux in-region, driver test-signed, HVCI on/off, VBS on
- Traffic: iperf3 TCP 16 parallel flows aggregate, 1 TCP flow single-flow, UDP 1 flow, curated real workloads HTTPS large download, video call, game UDP, DNS storm
- Instrumentation: pktmon + Wireshark at adapter and per-uplink, ETW -> WPA for CPU, Performance Monitor working set, power draw
- Acceptance rows: aggregate >=90% sum after overhead, single-flow >=70% sum both links, mismatched >=100% best (no regression else auto-revert Sec.6.4.6), added delay <=3ms wired lab, loss/reordering <=0.1%, failover <=2s, CPU & memory report first limits after S2, power delta, stability 72h soak + random flaps + 100 resume, compatibility matrix, clean-machine provisioning offline VM

Previously netem-rig.sh had only 4 profiles (delay-gap, lossy, asymmetric, cellular) and dry-run echo, no reps loop, no analyze.py, no Rig B, ECN, delay-jump, per-flow, aggregate, failover, soak.

Throughput harness already enhanced to 9 profiles (0581256) with Rig B wired 1Gbps/Wi-Fi 6 300M, per-flow on/off, aggregate 16 flows, added delay <=3ms, CPU/Gbit, pacing 80%, secure notes — but netem-rig.sh still needed matching profiles.

## Decision

Enhance `tools/lab/netem-rig.sh` to 11 profiles + all:

**Profiles:**
- delay-gap: 40ms gap Wi-Fi 10ms vs cellular 50ms, tc qdisc add dev IFACE root netem delay 10ms / slow 50ms, expected reorder buffer 80ms cap 0 out-of-order, DRR 4:1 quanta, test striped_flow_is_reordered_back_across_a_40ms_delay_gap
- lossy: 1% loss, tc loss 1%, expected cwnd halved per loss burst, reinjection recovers lost packet inside hold cap, stream still complete, test reinjection_recovers_a_lost_packet_inside_the_hold_cap
- asymmetric: 200M vs 50M, tbf rate 200mbit/50mbit burst 32kbit latency 10ms, expected 3000:1000 quanta exactly 15:5 packets over 20 MTU-scale, quantum < packet still served, idle cannot bank burst
- cellular: 100ms RTT + 2% loss + 20Mbit, netem delay 100ms loss 2% rate 20mbit, expected CC slow start halving once per burst two-segment floor per-path independence LIA
- rigb: wired 1 Gbps 2ms + Wi-Fi 6 300M 10ms, tbf 1000mbit burst 64kbit latency 2ms + netem delay 2ms / slow 300mbit burst 32kbit latency 10ms + delay 10ms, expected aggregate >=90% sum after overhead 1.3 Gbps minus overhead, single-flow >=70% sum, mismatched >=100% best auto-revert Sec.6.4.6, added delay <=3ms wired lab, loss/reordering <=0.1%, failover <=2s, traffic iperf3 TCP 16 flows aggregate 1 flow single-flow UDP 1 flow curated workloads, instrumentation pktmon+Wireshark ETW->WPA CPU Performance Monitor working set power telemetry
- ecn: ECN CE every 200 packets, tc netem ecn + sysctl tcp_ecn 1, expected cwnd 80% per ECN milder than loss 50%, ETW 401 ECN_CE Counters ecn_ce per_path ecn_ce relay ingest/drain BALANCIFY_SIMULATE_ECN=1 sends ACK with ECN flag service cc.on_ecn
- delay-jump: 10ms -> 50ms at packet 500, tc qdisc add delay 10ms then change to 50ms, expected cwnd 10% reduction on RTT jump when RTT min+20ms & 1.25x ETW 402 DELAY Counters delay_events per_path rtt_min/last_ms delay_events
- perflow: per-flow on/off comparison Sec.4.7.4 ADR-0029, baseline per-flow off per-class off default session 1 global seq vs per-session new, with per-flow on set_per_flow_sessions(true) each flow own session id next_session_id 1000 seq per session retx (session,seq), expected same aggregate but better isolation one flow's loss doesn't affect other's cwnd less HOL blocking more sessions bounded max_sessions 64 evicts oldest on cap, measure aggregate single-flow failover 10 reps median/p95/spread
- aggregate: 16 flows vs single flow iperf3-like Sec.10.3, iperf3 -c RELAY -t 60 -P 16 aggregate vs -P 1 single-flow, expected aggregate >=90% sum single-flow >=70% sum many-flow bonding vs single-flow
- failover: unplug uplink mid-transfer Sec.4.5, test unplugging_an_uplink_mid_transfer_keeps_the_stream_alive adapter removed moment packet provably in flight in-flight reinjected on survivor 100/100 delivered in order no refusals, real unplug ifconfig down or cable measure 100ms throughput log transfer continues full rate <=2s ETW 200 UP 201 DOWN 500 FAILOVER tc loss 100% simulate unplug
- soak: 72h soak + random flaps + 100 sleep/resume with Verifier G5 G6 compat matrix, soak 72h continuous traffic random link flaps 100 sleep/resume Verifier standard+random no crash no leaked route/filter/handle/IOCTL buffer, compat matrix VPN clients security suites Hyper-V WSL2 Memory Integrity on/off documented workaround/incompatibility, ETW 100-600 WPA CPU sampling working set bounded power delta
- all: runs all profiles sequentially full S2c + Rig B + soak

**Reps loop:** run_reps function runs REPS (default 10) reps per profile per Sec.10.1, records timestamp, iperf3 + throughput.rs + crypto-bench + ETW, outputs to OUTPUT_DIR default /tmp/balancify-netem-results, computes mean/median/p95/spread via analyze.py

**Measurement checklist:** 10 reps median/p95/spread mean stdev min max, record Windows build hardware driver versions uplink types relay region with every result, traffic iperf3 TCP/UDP N flows curated real workloads, instrumentation pktmon+Wireshark at adapter and per-uplink ETW->WPA CPU Performance Monitor working set power telemetry, acceptance rows aggregate >=90% sum single-flow >=70% sum mismatched >=100% best auto-revert Sec.6.4.6 added delay <=3ms wired loss/reordering <=0.1% failover <=2s CPU & memory report first limits after S2 power delta stability 72h soak + random flaps + 100 resume compat matrix clean-machine provisioning offline VM, data stored as data not screenshots JSON+ETL+CSV

**analyze.py:** `tools/lab/analyze.py` accepts only iperf3 JSON result files:
- Parses `end.sum_received.bits_per_second` as Mbps and rejects missing, non-finite, or non-positive results.
- Groups repetitions by profile and reports count, mean, median, nearest-rank p95, spread, standard deviation, min, and max.
- Missing/empty result directories are errors. It never invents simulated/demo values and never parses `throughput.rs` software-benchmark logs as network measurements.
- `python3 -m unittest discover -s tools/lab -p 'test_*.py'` covers percentile calculation, JSON parsing/grouping, invalid input, and the no-fabricated-data behavior.

**Integration:**
- netem-rig.sh dry-run mode in sandbox (no tc, no iperf3, no Rig B hardware), real netem requires Linux gateway with tc + iperf3 + Rig B hardware per Sec.10.1
- Simulated S2c in CI: cargo test -p balancify-service --test e2e -- --nocapture, cargo run -p balancify-service --bin throughput -- rigb, cargo run -p balancify-crypto --bin bench
- Real UDP relay + netem: cargo run -p balancify-relay -- 4430,4431, cargo run -p balancify-client -- RELAY 100, IFACE=eth0 IFACE_SLOW=eth1 ./tools/lab/netem-rig.sh PROFILE REPS, iperf3 -c relay -t 60 -P 16 -J > OUTPUT_DIR/PROFILE_repI.json, python3 tools/lab/analyze.py OUTPUT_DIR, update docs/phase2/throughput-rows.md + lab-progress.md with dates hardware Windows build driver version uplink types relay region

## Consequences

- Netem rig now has 11 profiles matching throughput harness 9 profiles + soak + all, with 10 reps median/p95/spread per Sec.10.1, measurement checklist, analyze.py
- S2c reorder/reinjection measurements can be run on Linux gateway with tc netem + iperf3 + Rig B hardware, results stored as data not screenshots, documented with hardware/build/driver versions
- Local software-path benchmark outputs are not networking evidence. Real throughput, added-delay, failover, and CPU/Gbit acceptance remains pending Rig B/ETW measurements; see `docs/phase2/throughput-rows.md`
- Real Rig B still pending hardware, but harness + rig script + analyze.py ready
- CI runs analyzer unit tests and Rust workspace checks; real netem/Rig B execution remains a separate hardware gate
- No PR is required for this local development step; push changes only to the active Arena branch

## References

- Blueprint Sec.10.1 Rig A/B, Sec.10.2 baselines, Sec.10.3 acceptance rows, Sec.6.4 DRR+pacing+CC, Sec.6.3 capacity, Sec.6.5 health hysteresis, S13 SBOM/VEX, S15 HVCI trio, C2/C4
- Code: tools/lab/netem-rig.sh enhanced 11 profiles, tools/lab/analyze.py, crates/service/src/bin/throughput.rs 9 profiles, crates/crypto/src/bin/bench.rs CPU/Gbit, crates/service/src/etw.rs 200/201/300/400/401/402/500/600, docs/phase2/throughput-rows.md, docs/phase2/lab-progress.md
- CI: 5c05cb5 success (latest), 7fb32ec, 2f2766c, b666a03, etc. all green ubuntu+windows+audit
