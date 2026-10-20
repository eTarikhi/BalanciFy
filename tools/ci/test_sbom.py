import json
import tempfile
import unittest
from pathlib import Path

from sbom import attach_lock_checksums, build_document, write_atomic


class SbomGenerationTests(unittest.TestCase):
    def setUp(self):
        self.metadata = {
            "workspace_members": ["path+file:///repo#balancify-core@0.1.0"],
            "packages": [
                {
                    "id": "path+file:///repo#balancify-core@0.1.0",
                    "name": "balancify-core",
                    "version": "0.1.0",
                    "source": None,
                    "license": "Proprietary",
                    "checksum": None,
                    "targets": [{"kind": ["lib"]}],
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0",
                    "name": "serde",
                    "version": "1.0.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "license": "MIT OR Apache-2.0",
                    "checksum": "a" * 64,
                    "targets": [{"kind": ["lib"]}],
                },
                {
                    "id": "registry+https://github.com/rust-lang/crates.io-index#test-only@0.1.0",
                    "name": "test-only",
                    "version": "0.1.0",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "license": "MIT",
                    "checksum": "b" * 64,
                    "targets": [{"kind": ["lib"]}],
                },
            ],
            "resolve": {
                "nodes": [
                    {
                        "id": "path+file:///repo#balancify-core@0.1.0",
                        "deps": [
                            {
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0",
                                "dep_kinds": [{"kind": None}],
                            },
                            {
                                "pkg": "registry+https://github.com/rust-lang/crates.io-index#test-only@0.1.0",
                                "dep_kinds": [{"kind": "dev"}],
                            },
                        ],
                    },
                    {"id": "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0", "deps": []},
                    {"id": "registry+https://github.com/rust-lang/crates.io-index#test-only@0.1.0", "deps": []},
                ]
            },
        }

    def test_uses_resolved_runtime_graph_and_excludes_dev_only_packages(self):
        document = build_document(
            self.metadata,
            created="2026-10-20T00:00:00Z",
            namespace="https://example.test/spdx/test",
        )
        self.assertEqual(document["spdxVersion"], "SPDX-2.3")
        names = {package["name"] for package in document["packages"]}
        self.assertIn("serde", names)
        self.assertIn("balancify-driver-source", names)
        self.assertNotIn("test-only", names)
        self.assertEqual(document["creationInfo"]["created"], "2026-10-20T00:00:00Z")

    def test_declared_license_checksum_and_dependency_relationship_are_preserved(self):
        document = build_document(self.metadata, namespace="https://example.test/spdx/test")
        serde = next(package for package in document["packages"] if package["name"] == "serde")
        core = next(package for package in document["packages"] if package["name"] == "balancify-core")
        self.assertEqual(serde["licenseDeclared"], "MIT OR Apache-2.0")
        self.assertEqual(serde["checksums"][0]["checksumValue"], "a" * 64)
        self.assertEqual(core["licenseDeclared"], "LicenseRef-Proprietary")
        self.assertTrue(any(
            edge["spdxElementId"] == core["SPDXID"]
            and edge["relatedSpdxElement"] == serde["SPDXID"]
            and edge["relationshipType"] == "DEPENDS_ON"
            for edge in document["relationships"]
        ))

    def test_attaches_registry_checksums_from_cargo_lock(self):
        checksum = "c" * 64
        lockfile = {
            "package": [{
                "name": "serde",
                "version": "1.0.0",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "checksum": checksum,
            }, {
                "name": "test-only",
                "version": "0.1.0",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "checksum": "d" * 64,
            }]
        }
        metadata = attach_lock_checksums(self.metadata, lockfile)
        serde = next(package for package in metadata["packages"] if package["name"] == "serde")
        self.assertEqual(serde["checksum"], checksum)

    def test_does_not_invent_vulnerability_or_vex_claims(self):
        document = build_document(self.metadata, namespace="https://example.test/spdx/test")
        self.assertNotIn("vulnerabilities", document)
        self.assertIn("no VEX statements", document["comment"])
        self.assertNotIn("CVE-", str(document))

    def test_invalid_cargo_license_is_preserved_without_becoming_a_claim(self):
        self.metadata["packages"][1]["license"] = "MIT/Apache-2.0"
        document = build_document(self.metadata, namespace="https://example.test/spdx/test")
        serde = next(package for package in document["packages"] if package["name"] == "serde")
        self.assertEqual(serde["licenseDeclared"], "NOASSERTION")
        self.assertIn("MIT/Apache-2.0", serde["licenseComments"])

    def test_refuses_metadata_without_resolved_graph(self):
        with self.assertRaisesRegex(ValueError, "resolved dependency graph"):
            build_document({"workspace_members": [], "packages": [], "resolve": None})

    def test_atomic_writer_creates_a_parseable_spdx_document(self):
        document = build_document(self.metadata, namespace="https://example.test/spdx/test")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "sbom.spdx.json"
            write_atomic(output, document)
            parsed = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(parsed["spdxVersion"], "SPDX-2.3")
            self.assertFalse(list(Path(directory).glob("*.tmp")))


if __name__ == "__main__":
    unittest.main()
