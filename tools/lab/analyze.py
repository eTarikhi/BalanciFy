#!/usr/bin/env python3
"""Summarize real iperf3 JSON runs for the Sec.10.1/10.3 lab reports.

Only iperf3 JSON files are accepted as network measurements. The software-path
benchmark output from throughput.rs is deliberately not parsed as network data.
Usage: python3 tools/lab/analyze.py /path/to/iperf3-results
"""

import json
import math
import statistics
import sys
from pathlib import Path


def compute_stats(vals):
    if not vals:
        return None
    sorted_vals = sorted(vals)
    # Nearest-rank percentile: rank=ceil(p*N), converted to zero-based index.
    p95_idx = max(0, math.ceil(len(sorted_vals) * 0.95) - 1)
    return {
        "count": len(sorted_vals),
        "mean": statistics.mean(sorted_vals),
        "median": statistics.median(sorted_vals),
        "p95": sorted_vals[p95_idx],
        "min": sorted_vals[0],
        "max": sorted_vals[-1],
        "spread": sorted_vals[-1] - sorted_vals[0],
        "stdev": statistics.stdev(sorted_vals) if len(sorted_vals) > 1 else 0.0,
    }


def parse_iperf3_json(path):
    """Return received throughput in Mbps, or None for an invalid run file."""
    try:
        with path.open(encoding="utf-8") as f:
            data = json.load(f)
        bps = data["end"]["sum_received"]["bits_per_second"]
        bps = float(bps)
        if not math.isfinite(bps) or bps <= 0:
            raise ValueError("sum_received.bits_per_second must be finite and positive")
        return bps / 1_000_000.0
    except (OSError, UnicodeError, json.JSONDecodeError, KeyError, TypeError, ValueError) as e:
        print(f"WARN: failed to parse iperf3 result {path}: {e}", file=sys.stderr)
        return None


def load_results(output_dir):
    if not output_dir.exists():
        raise FileNotFoundError(f"results directory does not exist: {output_dir}")
    if not output_dir.is_dir():
        raise NotADirectoryError(f"results path is not a directory: {output_dir}")

    files = sorted(output_dir.glob("*.json"))
    if not files:
        raise ValueError(f"no iperf3 JSON result files found in {output_dir}")

    results = {}
    for json_file in files:
        profile = json_file.stem.split("_rep", 1)[0]
        if not profile:
            print(f"WARN: skipping result with empty profile name: {json_file}", file=sys.stderr)
            continue
        mbps = parse_iperf3_json(json_file)
        if mbps is not None:
            results.setdefault(profile, []).append(mbps)

    if not results:
        raise ValueError(f"no valid iperf3 JSON results found in {output_dir}")
    return results


def print_summary(results, output_dir):
    for profile, vals in sorted(results.items()):
        stats = compute_stats(vals)
        print(
            f"{profile}: count={stats['count']} mean={stats['mean']:.1f} "
            f"median={stats['median']:.1f} p95={stats['p95']:.1f} "
            f"spread={stats['spread']:.1f} stdev={stats['stdev']:.1f} "
            f"min={stats['min']:.1f} max={stats['max']:.1f} Mbps"
        )

    if "bonded" in results and "single" in results:
        bonded_mean = statistics.mean(results["bonded"])
        single_mean = statistics.mean(results["single"])
        gain = (bonded_mean - single_mean) / single_mean * 100.0
        print(f"\nMeasured bonded-vs-single difference: {gain:.1f}% (bonded {bonded_mean:.1f} vs single {single_mean:.1f} Mbps)")

    print(
        f"\nParsed iperf3 network results from {output_dir}. "
        "Record date, hardware, OS/driver versions, uplink types, relay region, "
        "and test configuration in throughput-rows.md before interpreting acceptance."
    )


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) != 1:
        print("Usage: python3 tools/lab/analyze.py /path/to/iperf3-results", file=sys.stderr)
        return 2
    output_dir = Path(argv[0])
    try:
        results = load_results(output_dir)
    except (FileNotFoundError, NotADirectoryError, ValueError) as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 2
    print_summary(results, output_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
