import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class WorkflowContractTests(unittest.TestCase):
    def test_linux_workflows_use_a_pinned_runner_image(self):
        workflows = [
            ROOT / ".github/workflows/ci.yml",
            ROOT / ".github/workflows/static.yml",
        ]
        for workflow in workflows:
            with self.subTest(workflow=workflow.name):
                text = workflow.read_text(encoding="utf-8")
                self.assertNotIn("runs-on: ubuntu-latest", text)
                self.assertIn("runs-on: ubuntu-24.04", text)

    def test_pages_actions_use_exact_node24_compatible_releases(self):
        text = (ROOT / ".github/workflows/static.yml").read_text(encoding="utf-8")
        expected = {
            "actions/checkout": "v7.0.1",
            "actions/configure-pages": "v6.0.0",
            "actions/upload-pages-artifact": "v5.0.0",
            "actions/deploy-pages": "v5.0.1",
        }
        for action, version in expected.items():
            with self.subTest(action=action):
                self.assertRegex(text, rf"uses:\s+{re.escape(action)}@{re.escape(version)}\b")

    def test_wdk_build_job_pins_tooling_and_builds_x64_driver(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("driver-build-x64:", workflow)
        self.assertIn("runs-on: windows-2022", workflow)
        self.assertIn("version: 10.0.26100.6584", workflow)
        self.assertIn("vs-version: '[17.0,18.0)'", workflow)
        self.assertRegex(
            workflow,
            r"uses:\s+microsoft/windows-drivers-rs/\.github/actions/install-wdk@c4ae6815484398dc9d28fbe97e6c02d44f9f49a8\b",
        )
        self.assertIn("msbuild driver\\balancify.vcxproj", workflow)
        self.assertIn("/p:SignMode=Off", workflow)
        self.assertIn("name: balancify-driver-unsigned-x64", workflow)
        self.assertIn("path: driver/x64/Release/balancify.sys", workflow)
        project = (ROOT / "driver/balancify.vcxproj").read_text(encoding="utf-8")
        self.assertIn("<WindowsTargetPlatformVersion>10.0.26100.0</WindowsTargetPlatformVersion>", project)
        self.assertIn("<AdditionalOptions>/W4 /WX /Qspectre %(AdditionalOptions)</AdditionalOptions>", project)
        linker = re.search(r"<Link>.*?</Link>", project, re.S)
        self.assertIsNotNone(linker)
        self.assertNotIn("/Qspectre", linker.group(0))
        self.assertNotIn("$(DDK_LIB_PATH)\\wdf", project)


if __name__ == "__main__":
    unittest.main()
