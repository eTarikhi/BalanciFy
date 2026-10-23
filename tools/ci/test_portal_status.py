import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class PortalStatusTests(unittest.TestCase):
    def test_hvci_and_sbom_modes_report_current_ci_evidence(self):
        source = (ROOT / "app/src/main.rs").read_text(encoding="utf-8")
        self.assertIn("pinned WDK 10.0.26100.6584", source)
        self.assertIn("pinned x64/ARM64 WDK compile/link and unsigned PE checks pass in CI", source)
        self.assertIn("SPDX 2.3 and SPDX 3.0.1 inventories", source)
        self.assertIn("clean-machine install and reviewed VEX remain pending", source)
        self.assertNotIn("CI currently only Rust workspace", source)
        self.assertNotIn("future add WDK build job", source)
        self.assertNotIn("SPDX 3.0 and reviewed VEX remain pending", source)
        self.assertNotIn("WDK 10.0.22621+", source)

    def test_rig_b_portal_does_not_present_simulated_values_as_measurements(self):
        source = (ROOT / "app/src/main.rs").read_text(encoding="utf-8")
        self.assertIn("Synthetic harness/placeholder output only: not measured on Rig B", source)
        self.assertNotIn("Rig B 1Gbps/300M mean ~X Mbps", source)


if __name__ == "__main__":
    unittest.main()
