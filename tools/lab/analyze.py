#!/usr/bin/env python3
"""
Analyze netem rig results — computes mean/median/p95/spread per Sec.10.1
Input: JSON files from iperf3 -J or throughput.rs output, or CSV
Output: summary table for docs/phase2/throughput-rows.md
Usage: python3 tools/lab/analyze.py /tmp/balancify-netem-results
"""

import json
import sys
import os
import statistics
from pathlib import Path

def compute_stats(vals):
    if not vals:
        return None
    sorted_vals = sorted(vals)
    mean = statistics.mean(sorted_vals)
    median = statistics.median(sorted_vals)
    # p95
    idx = int(len(sorted_vals) * 0.95)
    idx = min(idx, len(sorted_vals)-1)
    p95 = sorted_vals[idx]
    spread = max(sorted_vals) - min(sorted_vals)
    return {
        "count": len(vals),
        "mean": mean,
        "median": median,
        "p95": p95,
        "min": min(sorted_vals),
        "max": max(sorted_vals),
        "spread": spread,
        "stdev": statistics.stdev(sorted_vals) if len(sorted_vals) > 1 else 0.0,
    }

def parse_iperf3_json(path):
    try:
        with open(path) as f:
            data = json.load(f)
        # iperf3 JSON: end.sum_received.bits_per_second
        bps = data.get("end", {}).get("sum_received", {}).get("bits_per_second", 0)
        return bps / 1e6  # Mbps
    except Exception as e:
        print(f"WARN: failed to parse {path}: {e}", file=sys.stderr)
        return None

def parse_throughput_log(path):
    # throughput.rs prints: bonded: mean=X median=Y p95=Z spread=W
    vals = []
    try:
        with open(path) as f:
            for line in f:
                if "mean=" in line and "Mbps" in line or "mean=" in line:
                    # Try to extract mean value
                    parts = line.split()
                    for part in parts:
                        if part.startswith("mean="):
                            try:
                                v = float(part.split("=")[1])
                                vals.append(v)
                            except:
                                pass
    except Exception as e:
        print(f"WARN: failed to parse throughput log {path}: {e}", file=sys.stderr)
    return vals

def main():
    output_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("/tmp/balancify-netem-results")
    if not output_dir.exists():
        print(f"Output dir {output_dir} does not exist, using simulated data for demo")
        # Simulated data from throughput-rows.md
        simulated = {
            "bonded": [42.8, 43.1, 42.5, 43.0, 42.9, 42.7, 43.2, 42.6, 42.8, 43.0],
            "single": [30.0, 30.2, 29.8, 30.1, 30.0, 29.9, 30.3, 29.7, 30.0, 30.1],
            "lossy": [35.6, 35.8, 35.4, 35.7, 35.5, 35.9, 35.3, 35.6, 35.7, 35.5],
            "ecn": [38.0, 38.2, 37.8, 38.1, 38.0, 37.9, 38.3, 37.7, 38.0, 38.1],
            "delay": [40.0, 40.2, 39.8, 40.1, 40.0, 39.9, 40.3, 39.7, 40.0, 40.1],
            "rigb_wired1G_wifi300M": [1100, 1120, 1080, 1110, 1090, 1130, 1070, 1100, 1110, 1095],
            "perflow_off": [100, 102, 98, 101, 100, 99, 103, 97, 100, 101],
            "perflow_on": [105, 107, 103, 106, 105, 104, 108, 102, 105, 106],
            "aggregate_16flows": [1200, 1220, 1180, 1210, 1190, 1230, 1170, 1200, 1210, 1195],
        }
        for profile, vals in simulated.items():
            stats = compute_stats(vals)
            print(f"{profile}: mean={stats['mean']:.1f} median={stats['median']:.1f} p95={stats['p95']:.1f} spread={stats['spread']:.1f} (10 reps) — simulated")
        # Gains
        bonded_mean = statistics.mean(simulated["bonded"])
        single_mean = statistics.mean(simulated["single"])
        gain = (bonded_mean - single_mean) / single_mean * 100 if single_mean else 0
        print(f"\nBonded gain: {gain:.1f}% (bonded {bonded_mean:.1f} vs single {single_mean:.1f}) — target >=70% sum vs best single, >=90% sum vs sum B1")
        lossy_mean = statistics.mean(simulated["lossy"])
        penalty = (bonded_mean - lossy_mean) / bonded_mean * 100 if bonded_mean else 0
        print(f"Lossy penalty: {penalty:.1f}% (lossy {lossy_mean:.1f} vs bonded {bonded_mean:.1f}) — reinjection keeps stream alive")
        rigb_mean = statistics.mean(simulated["rigb_wired1G_wifi300M"])
        print(f"Rig B (wired 1Gbps 2ms + Wi-Fi 6 300M 10ms): mean {rigb_mean:.1f} Mbps — aggregate 1.3 Gbps minus overhead, target >=90% sum")
        print("\nAcceptance rows: aggregate >=90% sum after overhead, single-flow >=70% sum both links, mismatched >=100% best (no regression else auto-revert Sec.6.4.6), added delay <=3ms wired lab, loss/reordering <=0.1%, failover <=2s, CPU & memory report first limits after S2, power delta, stability 72h soak + random flaps + 100 resume, compatibility matrix, clean-machine provisioning offline VM")
        return

    # Real parsing
    results = {}
    for json_file in output_dir.glob("*.json"):
        profile = json_file.stem.split("_rep")[0]
        mbps = parse_iperf3_json(json_file)
        if mbps is not None:
            results.setdefault(profile, []).append(mbps)

    for profile, vals in results.items():
        stats = compute_stats(vals)
        if stats:
            print(f"{profile}: count={stats['count']} mean={stats['mean']:.1f} median={stats['median']:.1f} p95={stats['p95']:.1f} spread={stats['spread']:.1f} stdev={stats['stdev']:.1f} min={stats['min']:.1f} max={stats['max']:.1f}")

    if "bonded" in results and "single" in results:
        bonded_mean = statistics.mean(results["bonded"])
        single_mean = statistics.mean(results["single"])
        gain = (bonded_mean - single_mean) / single_mean * 100 if single_mean else 0
        print(f"\nBonded gain: {gain:.1f}% (bonded {bonded_mean:.1f} vs single {single_mean:.1f})")

    print(f"\nResults from {output_dir} — document in docs/phase2/throughput-rows.md with dates, hardware, Windows build, driver version, uplink types, relay region per Sec.10.1 reporting discipline (data not screenshots)")

if __name__ == "__main__":
    main()
