#!/bin/sh
# Guardrails for the netem runner: dry-run is side-effect-free, and real
# execution requires explicit endpoints/confirmation rather than fake results.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
RIG="$ROOT/tools/lab/netem-rig.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

OUTPUT_DIR="$TMP/no-results" sh "$RIG" --dry-run rigb 2 >"$TMP/dry-run.txt"
grep -q '^DRY-RUN only' "$TMP/dry-run.txt"
grep -q 'iperf3 -c <IPERF3_HOST>' "$TMP/dry-run.txt"
grep -q 'No measurements were performed and no results were fabricated' "$TMP/dry-run.txt"
test ! -e "$TMP/no-results"

if OUTPUT_DIR="$TMP/apply-results" sh "$RIG" --apply rigb 1 >"$TMP/apply.txt" 2>&1; then
    echo "--apply unexpectedly succeeded without IPERF3_HOST" >&2
    exit 1
fi
grep -q 'Set IPERF3_HOST' "$TMP/apply.txt"
test ! -e "$TMP/apply-results"

if sh "$RIG" --apply soak 1 >"$TMP/unsupported.txt" 2>&1; then
    echo "unsupported soak profile unexpectedly ran in --apply mode" >&2
    exit 1
fi
grep -q 'not an executable netem/iperf3 profile' "$TMP/unsupported.txt"

# User-supplied positionals must remain data (not be evaluated as shell code).
if sh "$RIG" --dry-run "rigb; touch $TMP/injected" 1 >"$TMP/injection.txt" 2>&1; then
    echo "invalid profile unexpectedly accepted" >&2
    exit 1
fi
test ! -e "$TMP/injected"

echo "netem rig safety tests passed"
