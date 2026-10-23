import subprocess
import tempfile
import unittest
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class LinkCollector(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
        self.assets = []
        self.external_assets = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "a" and attrs.get("href"):
            self.links.append(attrs["href"])
        if tag == "img" and attrs.get("src"):
            self.assets.append(attrs["src"])
        if tag in {"script", "link"}:
            source = attrs.get("src") or attrs.get("href", "")
            if source.startswith(("http://", "https://")):
                self.external_assets.append(source)


class DocsSiteTests(unittest.TestCase):
    def test_pages_artifact_contains_expected_site_and_visuals(self):
        with tempfile.TemporaryDirectory(prefix="balancify-site-test-") as temporary:
            output = Path(temporary) / "published"
            subprocess.run(
                ["sh", str(ROOT / "tools/ci/build-docs-site.sh"), str(output)],
                cwd=ROOT,
                check=True,
            )
            for relative in (
                "index.html",
                "404.html",
                ".nojekyll",
                "assets/progress/status-dashboard-2026-10-23.svg",
                "assets/progress/test-evidence-2026-10-23.svg",
                "assets/progress/delivery-signal-2026-10-23.svg",
                "assets/progress/roadmap-sequence-2026-10-23.svg",
                "assets/progress/strategic-alignment-2026-10-23.svg",
            ):
                with self.subTest(path=relative):
                    self.assertTrue((output / relative).is_file(), relative)
            html = (output / "index.html").read_text(encoding="utf-8")
            parser = LinkCollector()
            parser.feed(html)
            self.assertIn('lang="en"', html)
            self.assertIn("not release-ready", html)
            self.assertIn("304 passed", html)
            self.assertIn("303 passed", html)
            self.assertIn("39908817177", html)
            self.assertIn("8d98b60", html)
            self.assertIn("TCP/UDP/IP structural-boundary checks", html)
            self.assertIn("not claimed live", html)
            self.assertNotIn("has not been WDK-built", html)
            self.assertFalse(parser.external_assets, parser.external_assets)
            self.assertEqual(len(parser.assets), 5)
            for asset in parser.assets:
                self.assertTrue((output / asset).is_file(), asset)
            for href in parser.links:
                if href.startswith(("https://", "mailto:", "#")):
                    continue
                target = output / href.split("#", 1)[0]
                self.assertTrue(target.is_file(), href)

    def test_pages_workflow_uploads_only_built_site(self):
        workflow = (ROOT / ".github/workflows/static.yml").read_text(encoding="utf-8")
        self.assertIn("tools/ci/build-docs-site.sh", workflow)
        self.assertIn("balancify-site", workflow)
        self.assertNotIn("path: '.'", workflow)


if __name__ == "__main__":
    unittest.main()
