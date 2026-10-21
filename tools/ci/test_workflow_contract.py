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


if __name__ == "__main__":
    unittest.main()
