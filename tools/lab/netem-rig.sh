#!/bin/sh
# Netem rig for S2c measurements (blueprint §10.1, §6.4)
# Simulates delay gap, loss, and bandwidth profiles using Linux netem.
# Requires: ip, tc, netem, iperf3 (on lab machines)
# Usage: ./tools/lab/netem-rig.sh [profile]
# Profiles: delay-gap (40ms), lossy (1% loss), asymmetric (200M/50M), cellular (100ms+2%)

set -eu

PROFILE=${1:-delay-gap}
IFACE=${IFACE:-eth0}
RELAY=${RELAY:-127.0.0.1:4430}

echo "=== Balancify netem rig — profile: $PROFILE ==="
echo "IFACE=$IFACE RELAY=$RELAY"

case "$PROFILE" in
  delay-gap)
    echo "Simulating 40ms delay gap (Wi-Fi 10ms vs cellular 50ms) — §10.1 Rig B"
    # Two virtual interfaces with different delays would be created via
    # veth pairs in full lab; here we document the tc commands:
    echo "tc qdisc add dev $IFACE root netem delay 10ms"
    echo "tc qdisc add dev ${IFACE}_slow root netem delay 50ms"
    echo "Expected: reorder buffer 80ms cap should keep 0 out-of-order"
    ;;
  lossy)
    echo "Simulating 1% loss on one path"
    echo "tc qdisc add dev $IFACE root netem loss 1%"
    ;;
  asymmetric)
    echo "Simulating 200 Mbit/s vs 50 Mbit/s asymmetry"
    echo "tc qdisc add dev $IFACE root tbf rate 200mbit burst 32kbit latency 10ms"
    echo "tc qdisc add dev ${IFACE}_slow root tbf rate 50mbit burst 32kbit latency 10ms"
    ;;
  cellular)
    echo "Simulating cellular: 100ms RTT + 2% loss + 20 Mbit/s"
    echo "tc qdisc add dev $IFACE root netem delay 100ms loss 2% rate 20mbit"
    ;;
  *)
    echo "Unknown profile: $PROFILE"
    echo "Available: delay-gap, lossy, asymmetric, cellular"
    exit 1
    ;;
esac

echo ""
echo "Lab measurement checklist (§10.3):"
echo "  - 10 repetitions per test"
echo "  - Report median, p95, spread"
echo "  - Record Windows build, hardware, driver versions, uplink types, relay region"
echo "  - Traffic: iperf3 TCP/UDP N flows, curated real workloads"
echo ""
echo "To run simulated S2c in CI (no netem, deterministic):"
echo "  cargo test -p balancify-service --test e2e -- --nocapture"
echo ""
echo "To run with real UDP relay + netem (requires two lab machines):"
echo "  1. Start relay: cargo run -p balancify-relay -- 4430,4431"
echo "  2. Start client: cargo run -p balancify-client -- $RELAY 100"
echo "  3. Apply netem profile: IFACE=eth0 ./tools/lab/netem-rig.sh $PROFILE"
echo "  4. Measure: iperf3 -c <relay> -t 60 -P 4"
echo ""
echo "Rig ready (dry-run mode in sandbox)"
