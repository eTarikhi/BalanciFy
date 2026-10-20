import json
import tempfile
import unittest
from pathlib import Path

import analyze


class AnalyzeTests(unittest.TestCase):
    def test_p95_uses_nearest_rank(self):
        stats = analyze.compute_stats(list(range(1, 21)))
        self.assertEqual(stats["p95"], 19)

    def test_loads_only_iperf3_json_and_groups_repetitions(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            for rep, mbps in ((1, 100), (2, 120)):
                (root / f"bonded_rep{rep}.json").write_text(
                    json.dumps({"end": {"sum_received": {"bits_per_second": mbps * 1_000_000}}}),
                    encoding="utf-8",
                )
            (root / "local-throughput.log").write_text(
                "bonded: mean=999 median=999 p95=999\n", encoding="utf-8"
            )
            self.assertEqual(analyze.load_results(root), {"bonded": [100.0, 120.0]})

    def test_missing_directory_does_not_substitute_simulated_results(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            missing = Path(temp_dir) / "not-created"
            with self.assertRaises(FileNotFoundError):
                analyze.load_results(missing)

    def test_empty_directory_is_an_error(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            with self.assertRaisesRegex(ValueError, "no iperf3 JSON"):
                analyze.load_results(Path(temp_dir))

    def test_rejects_nonpositive_and_nonfinite_throughput(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            for name, value in (("zero", 0), ("nan", float("nan"))):
                (root / f"{name}_rep1.json").write_text(
                    json.dumps({"end": {"sum_received": {"bits_per_second": value}}}),
                    encoding="utf-8",
                )
            with self.assertRaisesRegex(ValueError, "no valid iperf3"):
                analyze.load_results(root)


if __name__ == "__main__":
    unittest.main()
