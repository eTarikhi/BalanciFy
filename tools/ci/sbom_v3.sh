#!/bin/sh
# Generate an SPDX 3.0.1 JSON-LD inventory from the exact Cargo.lock resolution.
# SPDX 2.3 remains available through sbom.sh; neither format contains VEX claims.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"
if [ "$#" -eq 0 ]; then
  exec python3 tools/ci/sbom_v3.py
elif [ "$#" -eq 1 ]; then
  exec python3 tools/ci/sbom_v3.py "$1"
else
  exec python3 tools/ci/sbom_v3.py "$1" "$2"
fi
