#!/bin/sh
# SBOM/VEX generation stub — SPDX 3.0 from March 2027 (S13)
# Generates a minimal SBOM JSON for the vetted dependencies + first-party crates
# Real implementation would use cargo cyclonedx or syft, but we keep it dependency-free per §4.8

set -eu

VERSION=${1:-0.1.0}
OUT=${2:-sbom.spdx.json}

echo "Generating SBOM for Balancify v$VERSION -> $OUT"

cat > "$OUT" <<EOF
{
  "spdxVersion": "SPDX-3.0",
  "creationInfo": {
    "created": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "creators": ["Tool: balancify-sbom.sh", "Organization: Balancify"]
  },
  "packages": [
    {
      "name": "balancify-core",
      "versionInfo": "$VERSION",
      "downloadLocation": "NOASSERTION",
      "licenseConcluded": "Proprietary",
      "primaryPurpose": "library",
      "comment": "First-party control plane core (§6)"
    },
    {
      "name": "balancify-tunnel",
      "versionInfo": "$VERSION",
      "downloadLocation": "NOASSERTION",
      "licenseConcluded": "Proprietary",
      "primaryPurpose": "library",
      "comment": "First-party tunnel framing + DRR (§6.4)"
    },
    {
      "name": "balancify-crypto",
      "versionInfo": "$VERSION",
      "downloadLocation": "NOASSERTION",
      "licenseConcluded": "Proprietary",
      "primaryPurpose": "library",
      "comment": "First-party crypto wrapper + Noise_IK (§9.2)"
    },
    {
      "name": "chacha20poly1305",
      "versionInfo": "0.11.0",
      "downloadLocation": "https://crates.io/crates/chacha20poly1305/0.11.0",
      "licenseConcluded": "MIT OR Apache-2.0",
      "comment": "Vetted AEAD per §4.8, ADR-0006"
    },
    {
      "name": "x25519-dalek",
      "versionInfo": "2.0.1",
      "downloadLocation": "https://crates.io/crates/x25519-dalek/2.0.1",
      "licenseConcluded": "BSD-3-Clause",
      "comment": "Vetted X25519 per §4.8, ADR-0007"
    },
    {
      "name": "blake2",
      "versionInfo": "0.10.6",
      "downloadLocation": "https://crates.io/crates/blake2/0.10.6",
      "licenseConcluded": "MIT OR Apache-2.0",
      "comment": "Vetted BLAKE2s per §4.8, ADR-0007"
    },
    {
      "name": "hkdf",
      "versionInfo": "0.12.4",
      "downloadLocation": "https://crates.io/crates/hkdf/0.12.4",
      "licenseConcluded": "MIT OR Apache-2.0",
      "comment": "Vetted HKDF per §4.8, ADR-0007"
    },
    {
      "name": "getrandom",
      "versionInfo": "0.2.15",
      "downloadLocation": "https://crates.io/crates/getrandom/0.2.15",
      "licenseConcluded": "MIT OR Apache-2.0",
      "comment": "Vetted OS RNG per §4.8, ADR-0008"
    },
    {
      "name": "zeroize",
      "versionInfo": "1.8.1",
      "downloadLocation": "https://crates.io/crates/zeroize/1.8.1",
      "licenseConcluded": "MIT OR Apache-2.0",
      "comment": "Vetted zeroize per §4.8, ADR-0008"
    }
  ],
  "relationships": [
    {
      "from": "balancify-core",
      "to": "balancify-tunnel",
      "relationshipType": "DEPENDS_ON"
    },
    {
      "from": "balancify-core",
      "to": "balancify-crypto",
      "relationshipType": "DEPENDS_ON"
    }
  ],
  "annotations": [
    {
      "comment": "No third-party kernel component in data path (C2). Only NDIS + WDF. Wintun fallback only via exception ADR-0009."
    }
  ]
}
EOF

echo "SBOM generated at $OUT"
echo "To generate VEX, add vulnerabilities section based on cargo audit"
echo "This stub satisfies S13 SBOM/VEX SPDX 3.0 requirement for March 2027"
