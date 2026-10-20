#!/bin/sh
# Generate an SPDX 2.3 SBOM from the exact Cargo.lock resolution.
# VEX is intentionally not fabricated: generate it from reviewed advisory data.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"
if [ "$#" -eq 0 ]; then
  exec python3 tools/ci/sbom.py
elif [ "$#" -eq 1 ]; then
  exec python3 tools/ci/sbom.py "$1"
else
  exec python3 tools/ci/sbom.py "$1" "$2"
fi
