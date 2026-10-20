#!/usr/bin/env python3
"""Generate an SPDX 2.3 inventory from Cargo's resolved production graph.

This deliberately does not emit VEX or assert that dependencies are unaffected:
those statements require current advisory data and a reviewed impact analysis.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import uuid
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[2]
DOCUMENT_ID = "SPDXRef-DOCUMENT"
DRIVER_ID = "SPDXRef-Package-balancify-driver-source"


def _spdx_id(package_id: str) -> str:
    digest = hashlib.sha256(package_id.encode("utf-8")).hexdigest()[:12]
    slug = re.sub(r"[^A-Za-z0-9.-]+", "-", package_id).strip("-.")[:72]
    return f"SPDXRef-Package-{slug}-{digest}"


def _package(package: dict, workspace_ids: set[str], version_override: str | None) -> dict:
    package_id = package["id"]
    first_party = package_id in workspace_ids
    spdx_id = _spdx_id(package_id)
    name = package["name"]
    version = version_override if first_party and version_override else package["version"]
    source = package.get("source") or ""

    if source.startswith("registry+") and "crates.io-index" in source:
        download = f"https://crates.io/crates/{quote(name, safe='')}/{quote(version, safe='')}"
    else:
        download = "NOASSERTION"

    declared = package.get("license") or "NOASSERTION"
    if first_party and declared == "Proprietary":
        declared = "LicenseRef-Proprietary"

    result = {
        "name": name,
        "SPDXID": spdx_id,
        "versionInfo": version,
        "downloadLocation": download,
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": declared,
        "copyrightText": "NOASSERTION",
        "primaryPackagePurpose": "APPLICATION" if any("bin" in t.get("kind", []) for t in package.get("targets", [])) else "LIBRARY",
        "sourceInfo": "First-party Cargo workspace package" if first_party else "Resolved Cargo package",
    }
    checksum = package.get("checksum")
    if checksum:
        result["checksums"] = [{"algorithm": "SHA256", "checksumValue": checksum}]
    if source.startswith("registry+"):
        result["externalRefs"] = [{
            "referenceCategory": "PACKAGE-MANAGER",
            "referenceType": "purl",
            "referenceLocator": f"pkg:cargo/{quote(name, safe='')}@{quote(version, safe='')}",
        }]
    return result


def build_document(
    metadata: dict,
    *,
    created: str | None = None,
    namespace: str | None = None,
    version_override: str | None = None,
) -> dict:
    """Build a standards-shaped SPDX 2.3 document from Cargo metadata JSON."""
    workspace_ids = set(metadata.get("workspace_members", []))
    packages_by_id = {package["id"]: package for package in metadata.get("packages", [])}
    missing = workspace_ids - packages_by_id.keys()
    if missing:
        raise ValueError(f"workspace package IDs missing from Cargo metadata: {sorted(missing)}")
    resolve = metadata.get("resolve")
    if not isinstance(resolve, dict):
        raise ValueError("Cargo metadata did not provide a resolved dependency graph")

    nodes = {node["id"]: node for node in resolve.get("nodes", [])}
    included = set(workspace_ids)
    pending = list(workspace_ids)
    edges: set[tuple[str, str]] = set()
    while pending:
        parent = pending.pop()
        node = nodes.get(parent)
        if node is None:
            continue
        for dependency in node.get("deps", []):
            kinds = dependency.get("dep_kinds", [])
            # Cargo reports dev dependencies in metadata, but they are not
            # needed to build or run the shipped product.
            if kinds and all(kind.get("kind") == "dev" for kind in kinds):
                continue
            child = dependency.get("pkg")
            if child not in packages_by_id:
                raise ValueError(f"dependency {child!r} is missing from Cargo metadata")
            edges.add((parent, child))
            if child not in included:
                included.add(child)
                pending.append(child)

    if not created:
        created = datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    if not namespace:
        namespace = f"https://balancify.io/spdx/{uuid.uuid4()}"

    cargo_packages = [
        _package(packages_by_id[package_id], workspace_ids, version_override)
        for package_id in sorted(included)
    ]
    package_ids = {package_id: _spdx_id(package_id) for package_id in included}

    # The NDIS driver is a first-party source component, not a Cargo target.
    product_version = version_override
    if not product_version:
        product_version = next(
            (packages_by_id[package_id]["version"] for package_id in workspace_ids
             if packages_by_id[package_id]["name"] == "balancify-service"),
            "0.1.0",
        )
    driver = {
        "name": "balancify-driver-source",
        "SPDXID": DRIVER_ID,
        "versionInfo": product_version,
        "downloadLocation": "NOASSERTION",
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": "LicenseRef-Proprietary",
        "copyrightText": "NOASSERTION",
        "primaryPackagePurpose": "SOURCE",
        "sourceInfo": "First-party NDIS/WDF source; no WDK binary is represented or claimed by this inventory",
    }

    relationships = []
    for package_id in sorted(workspace_ids):
        relationships.append({
            "spdxElementId": DOCUMENT_ID,
            "relationshipType": "DESCRIBES",
            "relatedSpdxElement": package_ids[package_id],
        })
    relationships.append({
        "spdxElementId": DOCUMENT_ID,
        "relationshipType": "DESCRIBES",
        "relatedSpdxElement": DRIVER_ID,
    })
    for parent, child in sorted(edges):
        relationships.append({
            "spdxElementId": package_ids[parent],
            "relationshipType": "DEPENDS_ON",
            "relatedSpdxElement": package_ids[child],
        })

    licenses = [{
        "licenseId": "LicenseRef-Proprietary",
        "name": "Balancify Proprietary License",
        "extractedText": "First-party Balancify source is proprietary. This reference does not grant a license.",
    }]
    return {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": DOCUMENT_ID,
        "name": "Balancify",
        "documentNamespace": namespace,
        "creationInfo": {
            "created": created,
            "creators": ["Tool: balancify-sbom.py", "Organization: Balancify"],
            "comment": "Generated from cargo metadata --locked; licenses are declared metadata, not independent legal conclusions.",
        },
        "packages": cargo_packages + [driver],
        "relationships": relationships,
        "hasExtractedLicensingInfos": licenses,
        "comment": "This document is an SBOM inventory only. It contains no VEX statements and makes no vulnerability-status claims.",
    }


def cargo_metadata() -> dict:
    try:
        completed = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as exc:
        raise RuntimeError("cargo is required to resolve the locked dependency graph") from exc
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or exc.stdout).strip()
        raise RuntimeError(f"cargo metadata --locked failed: {detail}") from exc
    return json.loads(completed.stdout)


def write_atomic(path: Path, document: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(document, indent=2, sort_keys=False) + "\n"
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", nargs="?", help="optional first-party release version override")
    parser.add_argument("output", nargs="?", default="sbom.spdx.json")
    args = parser.parse_args(argv)
    output = Path(args.output)
    if not output.is_absolute():
        output = ROOT / output
    try:
        document = build_document(cargo_metadata(), version_override=args.version)
        write_atomic(output, document)
    except (RuntimeError, ValueError, json.JSONDecodeError) as exc:
        print(f"SBOM generation failed: {exc}", file=sys.stderr)
        return 1
    print(f"SPDX 2.3 SBOM generated: {output}")
    print(f"Packages: {len(document['packages'])}; relationships: {len(document['relationships'])}")
    print("VEX was not generated; no vulnerability status is asserted.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
