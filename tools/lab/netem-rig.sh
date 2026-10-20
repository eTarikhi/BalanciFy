#!/bin/sh
# Safe, explicit lab runner for real iperf3 JSON measurements.
# Default: dry-run (prints commands only; never claims simulated results).
# Apply:   --apply [profile] [reps]
# WARNING: shaped profiles replace the root qdisc on the named interfaces. Use
# dedicated/disposable interfaces; cleanup removes the test qdisc and does not
# restore a pre-existing custom qdisc. Apply requires explicit confirmation.
#
# Profiles with an implemented runner: delay-gap, lossy, asymmetric, cellular,
# rigb, ecn, aggregate. Other historical profiles remain hardware/orchestration
# tasks and are intentionally rejected in --apply mode.

set -eu

MODE=dry-run
PROFILE=
REPS=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --apply) MODE=apply; shift ;;
        --dry-run) MODE=dry-run; shift ;;
        --help|-h)
            sed -n '1,16p' "$0"
            echo "Usage: $0 [--dry-run|--apply] [profile] [reps]"
            echo "Apply environment: IPERF3_HOST, optional IPERF3_PORT, IFACE, IFACE_SLOW"
            echo "Apply confirmation for shaped profiles: CONFIRM_TC_ROOT_REPLACE=YES"
            exit 0
            ;;
        *)
            if [ -z "$PROFILE" ]; then PROFILE=$1
            elif [ -z "$REPS" ]; then REPS=$1
            else echo "Too many positional arguments" >&2; exit 2
            fi
            shift
            ;;
    esac
done
PROFILE=${PROFILE:-delay-gap}
REPS=${REPS:-10}
IFACE=${IFACE:-eth0}
IFACE_SLOW=${IFACE_SLOW:-eth1}
IPERF3_HOST=${IPERF3_HOST:-}
IPERF3_PORT=${IPERF3_PORT:-5201}
DURATION=${DURATION:-10}
OUTPUT_DIR=${OUTPUT_DIR:-/tmp/balancify-netem-results}

case "$REPS" in ''|*[!0-9]*) echo "reps must be a positive integer" >&2; exit 2 ;; esac
[ "$REPS" -gt 0 ] || { echo "reps must be a positive integer" >&2; exit 2; }
case "$DURATION" in ''|*[!0-9]*) echo "DURATION must be a positive integer" >&2; exit 2 ;; esac
[ "$DURATION" -gt 0 ] || { echo "DURATION must be a positive integer" >&2; exit 2; }
case "$IPERF3_PORT" in ''|*[!0-9]*) echo "IPERF3_PORT must be a port number" >&2; exit 2 ;; esac
[ "$IPERF3_PORT" -ge 1 ] && [ "$IPERF3_PORT" -le 65535 ] || { echo "IPERF3_PORT must be in 1..=65535" >&2; exit 2; }

SUPPORTED="delay-gap lossy asymmetric cellular rigb ecn aggregate"
case " $SUPPORTED " in
    *" $PROFILE "*) ;;
    *)
        case "$PROFILE" in
            all)
                for profile in $SUPPORTED; do
                    "$0" "--$MODE" "$profile" "$REPS"
                done
                exit 0
                ;;
            delay-jump|perflow|failover|soak)
                if [ "$MODE" = apply ]; then
                    echo "$PROFILE is not an executable netem/iperf3 profile; hardware/orchestration measurements remain pending." >&2
                    exit 2
                fi
                echo "DRY-RUN: $PROFILE has no executable runner yet; no measurement or JSON will be produced."
                exit 0
                ;;
            *) echo "Unknown profile: $PROFILE (available: $SUPPORTED, all, delay-jump, perflow, failover, soak)" >&2; exit 2 ;;
        esac
        ;;
esac

shape_required=1
[ "$PROFILE" = aggregate ] && shape_required=0
if [ "$MODE" = apply ]; then
    [ -n "$IPERF3_HOST" ] || { echo "Set IPERF3_HOST to a running iperf3 server; the Rust relay is not an iperf3 server." >&2; exit 2; }
    command -v iperf3 >/dev/null 2>&1 || { echo "iperf3 is required for --apply" >&2; exit 2; }
    if [ "$shape_required" -eq 1 ]; then
        command -v tc >/dev/null 2>&1 || { echo "tc is required for shaped --apply profiles" >&2; exit 2; }
        command -v ip >/dev/null 2>&1 || { echo "iproute2 (ip) is required for shaped --apply profiles" >&2; exit 2; }
        [ "$(id -u)" -eq 0 ] || { echo "shaped --apply profiles require root" >&2; exit 2; }
        [ "${CONFIRM_TC_ROOT_REPLACE:-}" = YES ] || {
            echo "Refusing to replace root qdiscs. Set CONFIRM_TC_ROOT_REPLACE=YES only on dedicated interfaces." >&2
            exit 2
        }
        [ "$IFACE" != "$IFACE_SLOW" ] || { echo "IFACE and IFACE_SLOW must be different" >&2; exit 2; }
        ip link show dev "$IFACE" >/dev/null 2>&1 || { echo "No interface: $IFACE" >&2; exit 2; }
        ip link show dev "$IFACE_SLOW" >/dev/null 2>&1 || { echo "No interface: $IFACE_SLOW" >&2; exit 2; }
    fi
    mkdir -p "$OUTPUT_DIR"
fi

SHAPED_IFACES=""
cleanup() {
    result=$?
    trap - EXIT HUP INT TERM
    if [ "$MODE" = apply ]; then
        for dev in $SHAPED_IFACES; do
            echo "Removing test qdisc from $dev (kernel default will be restored; custom prior qdiscs are not restored)."
            tc qdisc del dev "$dev" root >/dev/null 2>&1 || true
        done
    fi
    exit "$result"
}
trap cleanup EXIT HUP INT TERM

show_tc() {
    dev=$1
    shift
    if [ "$MODE" = dry-run ]; then
        printf '+ tc qdisc replace dev %s root netem' "$dev"
        printf ' %s' "$@"
        printf '\n'
    else
        tc qdisc replace dev "$dev" root netem "$@"
        case " $SHAPED_IFACES " in *" $dev "*) ;; *) SHAPED_IFACES="$SHAPED_IFACES $dev" ;; esac
    fi
}

apply_profile() {
    case "$PROFILE" in
        delay-gap)
            show_tc "$IFACE" delay 10ms
            show_tc "$IFACE_SLOW" delay 50ms
            ;;
        lossy)
            show_tc "$IFACE" loss 1%
            ;;
        asymmetric)
            show_tc "$IFACE" rate 200mbit
            show_tc "$IFACE_SLOW" rate 50mbit
            ;;
        cellular)
            show_tc "$IFACE_SLOW" delay 100ms loss 2% rate 20mbit
            ;;
        rigb)
            show_tc "$IFACE" delay 2ms rate 1000mbit
            show_tc "$IFACE_SLOW" delay 10ms rate 300mbit
            ;;
        ecn)
            show_tc "$IFACE" loss 1% ecn
            ;;
        aggregate) ;;
    esac
}

run_iperf() {
    label=$1
    parallel=$2
    output="$OUTPUT_DIR/${label}_rep${rep}.json"
    if [ "$MODE" = dry-run ]; then
        display_host=${IPERF3_HOST:-<IPERF3_HOST>}
        echo "+ iperf3 -c $display_host -p $IPERF3_PORT -t $DURATION -P $parallel -J > $output"
    else
        echo "  iperf3 $label rep $rep/$REPS ($parallel parallel stream(s), ${DURATION}s)"
        if ! iperf3 -c "$IPERF3_HOST" -p "$IPERF3_PORT" -t "$DURATION" -P "$parallel" -J > "$output"; then
            echo "iperf3 failed; see $output" >&2
            return 1
        fi
        python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$output" || {
            echo "iperf3 output is not valid JSON: $output" >&2
            return 1
        }
    fi
}

if [ "$MODE" = dry-run ]; then
    echo "DRY-RUN only — no tc, iperf3, output directories, or result files will be created."
    [ -n "$IPERF3_HOST" ] || echo "Set IPERF3_HOST for the real iperf3 destination (do not use the Rust relay unless it runs iperf3)."
else
    echo "APPLY mode — profile=$PROFILE reps=$REPS; qdisc and iperf3 commands will execute."
    echo "Target iperf3 server: $IPERF3_HOST:$IPERF3_PORT; output: $OUTPUT_DIR"
fi

interfaces=$IFACE
[ "$shape_required" -eq 0 ] || interfaces="$IFACE,$IFACE_SLOW"
echo "Profile: $PROFILE | interface(s): $interfaces | date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
apply_profile

rep=1
while [ "$rep" -le "$REPS" ]; do
    case "$PROFILE" in
        aggregate)
            run_iperf aggregate_16flows 16
            run_iperf aggregate_single 1
            ;;
        *) run_iperf "$PROFILE" 1 ;;
    esac
    rep=$((rep + 1))
done

if [ "$MODE" = dry-run ]; then
    echo "Dry-run complete. No measurements were performed and no results were fabricated."
else
    echo "iperf3 JSON saved under $OUTPUT_DIR; analyze only these real files with python3 tools/lab/analyze.py $OUTPUT_DIR"
    echo "This is a raw iperf3 link-path measurement, not BalanciFy bonded throughput or driver acceptance evidence."
fi
