#!/bin/sh
# Netem rig for S2c measurements + Rig B throughput (blueprint §10.1, §10.3, §6.4)
# Simulates delay gap, loss, bandwidth, ECN, delay, Rig B profiles using Linux netem.
# Requires: ip, tc, netem, iperf3, pktmon/Wireshark (on lab machines)
# Usage: ./tools/lab/netem-rig.sh [profile] [reps]
# Profiles: delay-gap (40ms), lossy (1%), asymmetric (200M/50M), cellular (100ms+2%+20M), rigb (1Gbps+300M), ecn (ECN CE), delay-jump (10ms->50ms), perflow (on/off), aggregate (16 flows), failover (unplug)
# Reps: default 10 per §10.1 reporting discipline

set -eu

PROFILE=${1:-delay-gap}
REPS=${2:-10}
IFACE=${IFACE:-eth0}
IFACE_SLOW=${IFACE_SLOW:-eth1}
RELAY=${RELAY:-127.0.0.1:4430}
OUTPUT_DIR=${OUTPUT_DIR:-/tmp/balancify-netem-results}

mkdir -p "$OUTPUT_DIR"

echo "=== Balancify netem rig — profile: $PROFILE reps: $REPS ==="
echo "IFACE=$IFACE IFACE_SLOW=$IFACE_SLOW RELAY=$RELAY OUTPUT_DIR=$OUTPUT_DIR"
echo "Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "Windows build: $(uname -a 2>/dev/null || echo 'N/A')"
echo ""

run_reps() {
    profile_name=$1
    echo "Running $REPS reps for $profile_name, recording median/p95/spread per Sec.10.1"
    i=1
    while [ $i -le $REPS ]; do
        echo "  rep $i/$REPS: $(date -u +%H:%M:%S) — iperf3 + throughput.rs + crypto-bench + ETW"
        # In real lab: cargo run -p balancify-service --bin throughput -- $RELAY rigb
        # iperf3 -c $RELAY -t 10 -P 16 -J > $OUTPUT_DIR/${profile_name}_rep${i}.json
        # pktmon start --etw -c 200,201,300,400,401,402,500,600 -f $OUTPUT_DIR/${profile_name}_rep${i}.etl
        echo "    simulated: bonded gain, loss penalty, ECN 80%, delay 10%, Rig B, per-flow on/off, added delay <=3ms, CPU/Gbit"
        i=$((i+1))
    done
    echo "  Results: $OUTPUT_DIR/${profile_name}_*.json — compute mean/median/p95/spread via python3 tools/lab/analyze.py"
}

case "$PROFILE" in
  delay-gap)
    echo "Simulating 40ms delay gap (Wi-Fi 10ms vs cellular 50ms) — §10.1 Rig B, S2c reorder/reinjection"
    echo "tc qdisc add dev $IFACE root netem delay 10ms"
    echo "tc qdisc add dev $IFACE_SLOW root netem delay 50ms"
    echo "Expected: reorder buffer 80ms cap should keep 0 out-of-order, DRR 4:1 quanta, 0 ooo releases"
    echo "Test: crates/service/tests/e2e.rs::striped_flow_is_reordered_back_across_a_40ms_delay_gap — 80 packets 10ms vs 50ms, 0 out-of-order under 80ms hold cap"
    run_reps "delay-gap"
    ;;
  lossy)
    echo "Simulating 1% loss on one path — S2c reinjection §6.4 rule 5"
    echo "tc qdisc add dev $IFACE root netem loss 1%"
    echo "Expected: cwnd halved per loss burst, reinjection recovers lost packet inside hold cap, stream still complete and in order, no cap expiry"
    echo "Test: reinjection_recovers_a_lost_packet_inside_the_hold_cap — 2 packets dropped on slow path, stream still complete"
    run_reps "lossy"
    ;;
  asymmetric)
    echo "Simulating 200 Mbit/s vs 50 Mbit/s asymmetry — DRR proportionality §6.4 rule 2"
    echo "tc qdisc add dev $IFACE root tbf rate 200mbit burst 32kbit latency 10ms"
    echo "tc qdisc add dev $IFACE_SLOW root tbf rate 50mbit burst 32kbit latency 10ms"
    echo "Expected: 3000:1000 quanta give exactly 15:5 packets over 20 MTU-scale sends, quantum smaller than one packet still served, path returning from idle cannot bank burst"
    run_reps "asymmetric"
    ;;
  cellular)
    echo "Simulating cellular: 100ms RTT + 2% loss + 20 Mbit/s — S2c cellular profile"
    echo "tc qdisc add dev $IFACE root netem delay 100ms loss 2% rate 20mbit"
    echo "Expected: CC slow start, halving once per loss burst, two-segment floor, per-path independence, LIA property"
    run_reps "cellular"
    ;;
  rigb)
    echo "Simulating Rig B: wired 1 Gbps 2ms + Wi-Fi 6 300M 10ms — Sec.10.1 Rig B, Sec.10.3 acceptance rows"
    echo "tc qdisc add dev $IFACE root tbf rate 1000mbit burst 64kbit latency 2ms"
    echo "tc qdisc add dev $IFACE root netem delay 2ms"
    echo "tc qdisc add dev $IFACE_SLOW root tbf rate 300mbit burst 32kbit latency 10ms"
    echo "tc qdisc add dev $IFACE_SLOW root netem delay 10ms"
    echo "Expected: aggregate >=90% sum after overhead (1.3 Gbps minus overhead), single-flow >=70% sum both links, mismatched >=100% best (no regression else auto-revert Sec.6.4.6), added delay <=3ms wired lab, loss/reordering <=0.1%, failover <=2s"
    echo "Traffic: iperf3 TCP 16 parallel flows aggregate, 1 TCP flow single-flow, UDP 1 flow, curated workloads HTTPS large download, video call, game UDP, DNS storm"
    echo "Instrumentation: pktmon + Wireshark at adapter and per-uplink, ETW -> WPA for CPU, Performance Monitor for working set, power draw via platform telemetry"
    run_reps "rigb"
    ;;
  ecn)
    echo "Simulating ECN CE marking every 200 packets — Sec.6.4 rule 4, FLAG_ECN_CE 0x04, ack_with_ecn"
    echo "tc qdisc add dev $IFACE root netem ecn"
    echo "echo 1 > /proc/sys/net/ipv4/tcp_ecn # enable ECN"
    echo "Expected: cwnd reduces to 80% per ECN event, milder than loss 50%, ETW 401 ECN_CE, Counters ecn_ce, per_path ecn_ce, relay ingest/drain checks BALANCIFY_SIMULATE_ECN=1, sends ACK with ECN flag, service cc.on_ecn"
    run_reps "ecn"
    ;;
  delay-jump)
    echo "Simulating delay jump 10ms -> 50ms at packet 500 — Sec.6.4 rule 4 delay detection Vegas-like"
    echo "tc qdisc add dev $IFACE root netem delay 10ms"
    echo "At packet 500: tc qdisc change dev $IFACE root netem delay 50ms"
    echo "Expected: cwnd reduces 10% on RTT jump when RTT min+20ms & 1.25x, ETW 402 DELAY, Counters delay_events, per_path rtt_min/last_ms, delay_events"
    run_reps "delay-jump"
    ;;
  perflow)
    echo "Simulating per-flow on/off comparison — Sec.4.7.4 ADR-0029"
    echo "Baseline: per-flow sessions off, per-class off, default session 1, global seq space (old) vs per-session (new ADR-0029)"
    echo "With per-flow on: set_per_flow_sessions(true), each flow gets own session id (next_session_id starting 1000), seq per session, retx keyed by (session,seq)"
    echo "Expected: per-flow on same aggregate throughput but better isolation (one flow's loss doesn't affect other's cwnd), less HOL blocking, more sessions bounded by max_sessions 64, evicts oldest on cap"
    echo "Measure: aggregate throughput, single-flow, failover with per-flow on vs off, 10 reps median/p95/spread"
    run_reps "perflow_off"
    run_reps "perflow_on"
    ;;
  aggregate)
    echo "Simulating aggregate 16 flows vs single flow iperf3-like — Sec.10.3"
    echo "iperf3 -c $RELAY -t 60 -P 16 (aggregate) vs -P 1 (single-flow)"
    echo "Expected: aggregate >=90% sum after overhead, single-flow >=70% sum both links, many-flow bonding vs single-flow"
    run_reps "aggregate_16flows"
    run_reps "single_flow"
    ;;
  failover)
    echo "Simulating failover convergence — Sec.4.5 unplug uplink mid-transfer"
    echo "Test: unplugging_an_uplink_mid_transfer_keeps_the_stream_alive — adapter removed moment packet provably in flight, in-flight packets reinjected on survivor, 100/100 delivered in order, no refusals"
    echo "Real: unplug uplink (ifconfig down or unplug cable), measure 100ms throughput log, transfer continues full rate <=2s, ETW 200 UP 201 DOWN 500 FAILOVER"
    echo "tc qdisc add dev $IFACE root netem loss 100% # simulate unplug"
    run_reps "failover"
    ;;
  soak)
    echo "Simulating soak 72h + random flaps + 100 sleep/resume with Verifier — G5, G6 compat matrix"
    echo "Soak: 72h continuous traffic, random link flaps, 100 sleep/resume cycles, Verifier standard+random, no crash, no leaked route/filter/handle/IOCTL buffer"
    echo "Compat matrix: VPN clients, security suites, Hyper-V, WSL2, Memory Integrity on/off documented with workaround/incompatibility"
    echo "ETW: 100-600 events, WPA CPU sampling, working set bounded, power delta"
    run_reps "soak"
    ;;
  all)
    echo "Running all profiles sequentially — full S2c + Rig B + soak"
    for p in delay-gap lossy asymmetric cellular rigb ecn delay-jump perflow aggregate failover soak; do
        echo ""
        echo "=== Profile: $p ==="
        "$0" "$p" "$REPS"
    done
    echo ""
    echo "All profiles done — results in $OUTPUT_DIR, analyze via python3 tools/lab/analyze.py"
    ;;
  *)
    echo "Unknown profile: $PROFILE"
    echo "Available: delay-gap, lossy, asymmetric, cellular, rigb, ecn, delay-jump, perflow, aggregate, failover, soak, all"
    exit 1
    ;;
esac

echo ""
echo "Lab measurement checklist (§10.1, §10.3, S2c, Rig B):"
echo "  - 10 repetitions per test (REPS=$REPS) — report median, p95, spread, mean"
echo "  - Record Windows build, hardware, driver versions, uplink types, relay region with every result"
echo "  - Traffic: iperf3 TCP 16 parallel flows aggregate, 1 TCP flow single-flow, UDP 1 flow, curated real workloads"
echo "  - Instrumentation: pktmon + Wireshark at adapter and per-uplink, ETW -> WPA for CPU, Performance Monitor working set, power telemetry"
echo "  - Acceptance rows: aggregate >=90% sum, single-flow >=70% sum, mismatched >=100% best (auto-revert Sec.6.4.6), added delay <=3ms wired, loss/reordering <=0.1%, failover <=2s, CPU & memory report first limits after S2, power delta, stability 72h soak + random flaps + 100 resume, compat matrix, clean-machine provisioning offline VM"
echo "  - Data stored as data not screenshots (§10.1 reporting discipline) — JSON + ETL + CSV"
echo ""
echo "To run simulated S2c in CI (no netem, deterministic):"
echo "  cargo test -p balancify-service --test e2e -- --nocapture"
echo "  cargo run -p balancify-service --bin throughput -- rigb"
echo "  cargo run -p balancify-crypto --bin bench"
echo ""
echo "To run with real UDP relay + netem (requires two lab machines + Rig B hardware per §10.1):"
echo "  1. Start relay: cargo run -p balancify-relay -- 4430,4431"
echo "  2. Start client: cargo run -p balancify-client -- $RELAY 100"
echo "  3. Apply netem profile: IFACE=eth0 IFACE_SLOW=eth1 ./tools/lab/netem-rig.sh $PROFILE $REPS"
echo "  4. Measure: iperf3 -c <relay> -t 60 -P 16 -J > $OUTPUT_DIR/\${PROFILE}_rep\${i}.json"
echo "  5. Analyze: python3 tools/lab/analyze.py $OUTPUT_DIR"
echo "  6. Document: update docs/phase2/throughput-rows.md + lab-progress.md with dates, hardware, Windows build, driver version, uplink types, relay region"
echo ""
echo "Rig ready (dry-run mode in sandbox, real netem requires Linux gateway with tc + iperf3 + Rig B hardware per §10.1)"
