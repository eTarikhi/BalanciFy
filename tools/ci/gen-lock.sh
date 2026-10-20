#!/bin/sh
# Generate Cargo.lock and refresh transitive allowlist.
# Requires a Rust toolchain with network access to crates.io.
# In the sandboxed CI this is not available (crates.io blocked), so the
# lock file is not committed yet. Run this locally once toolchain is available.
set -eu
cd "$(dirname "$0")/../.."

echo "1/3 cargo generate-lockfile"
cargo generate-lockfile

echo "2/3 cargo tree -p balancify-crypto --prefix none (for allowlist)"
cargo tree -p balancify-crypto --prefix none | sort -u > /tmp/tree.txt
echo "--- tree ---"
cat /tmp/tree.txt

echo "3/3 audit"
sh tools/ci/audit-deps.sh

echo
echo "If audit passed, commit Cargo.lock and update tools/ci/crypto-allowlist.txt"
echo "with the transitive closure from the tree output (excluding workspace members)."
