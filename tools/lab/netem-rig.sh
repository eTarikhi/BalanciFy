#!/usr/bin/env bash
# Rig B (blueprint §10.1): two emulated uplinks with independent delay/loss/rate,
# for reorder-buffer and lane-choice measurements (Spike S2c / G4).
# Usage: sudo ./netem-rig.sh [delayA ms] [delayB ms] [rateA mbit] [rateB mbit]
set -euo pipefail
DA=${1:-10}; DB=${2:-50}; RA=${3:-300}; RB=${4:-80}

cleanup() { ip netns del balc-client 2>/dev/null || true; ip netns del balc-relay 2>/dev/null || true; }
cleanup

ip netns add balc-client
ip netns add balc-relay

# path A
ip link add vA-c type veth peer name vA-r
ip link set vA-c netns balc-client; ip link set vA-r netns balc-relay
ip netns exec balc-client ip addr add 10.0.1.1/24 dev vA-c
ip netns exec balc-relay  ip addr add 10.0.1.2/24 dev vA-r
# path B
ip link add vB-c type veth peer name vB-r
ip link set vB-c netns balc-client; ip link set vB-r netns balc-relay
ip netns exec balc-client ip addr add 10.0.2.1/24 dev vB-c
ip netns exec balc-relay  ip addr add 10.0.2.2/24 dev vB-r

for ns in balc-client balc-relay; do ip netns exec $ns ip link set lo up; done
ip netns exec balc-client ip link set vA-c up; ip netns exec balc-client ip link set vB-c up
ip netns exec balc-relay  ip link set vA-r up; ip netns exec balc-relay  ip link set vB-r up

# emulate link characteristics both directions (the 40 ms gap pair default: 10 vs 50 ms)
ip netns exec balc-client tc qdisc add dev vA-c root netem delay ${DA}ms rate ${RA}mbit
ip netns exec balc-client tc qdisc add dev vB-c root netem delay ${DB}ms rate ${RB}mbit
ip netns exec balc-relay  tc qdisc add dev vA-r root netem delay ${DA}ms rate ${RA}mbit
ip netns exec balc-relay  tc qdisc add dev vB-r root netem delay ${DB}ms rate ${RB}mbit

echo "Rig B ready: client ns=balc-client (10.0.1.1 / 10.0.2.1), relay ns=balc-relay (10.0.1.2 / 10.0.2.2)"
echo "Run:  ip netns exec balc-relay  cargo run -p balancify-relay -- '4430,4431'"
echo "Then: ip netns exec balc-client <sender harness>  # Spike S2a/S2c"
