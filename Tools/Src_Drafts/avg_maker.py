#!/usr/bin/env python3
"""
avg_maker.py — Combine arrays (raw or aggregated) and compute averages.

Reads JSON from stdin: {"arrays": [ ... ]}
Outputs JSON to stdout: either averages (default) or raw aggregate (--aggregate-only).
"""

import sys
import json
import math
import argparse

# Whitelist of metrics to process
METRICS = [
    "total_reads",
    "total_writes",
    "total_read_errors",
    "total_write_errors",
    "total_checksum_mismatch",
    "total_host_relocations",
    "mount_count",
]


def normalize_array(arr):
    """
    Convert an input array element into a normalized aggregate dict.
    - If 'count' is present, treat as aggregated bucket.
    - Otherwise, treat as raw point with count=1.
    Returns dict: { metric: {"count": int, "sum": float, "sum_sq": float|None,
                             "min": float|None, "max": float|None} }
    """
    normalized = {}

    # Determine if aggregated or raw
    if "count" in arr and isinstance(arr["count"], (int, float)):
        bucket_count = arr["count"]
        # Aggregated: each metric should have sum, sum_sq, min, max
        for metric in METRICS:
            if metric in arr:
                m = arr[metric]
                if not isinstance(m, dict):
                    continue
                # Ensure sum exists; if not, skip
                if "sum" not in m:
                    continue
                sum_val = m["sum"]
                sum_sq = m.get("sum_sq", None)
                min_val = m.get("min", None)
                max_val = m.get("max", None)
                normalized[metric] = {
                    "count": bucket_count,
                    "sum": sum_val,
                    "sum_sq": sum_sq,
                    "min": min_val,
                    "max": max_val,
                }
    else:
        # Raw point: each metric is a simple number
        for metric in METRICS:
            if metric in arr:
                value = arr[metric]
                if not isinstance(value, (int, float)):
                    continue
                normalized[metric] = {
                    "count": 1,
                    "sum": value,
                    "sum_sq": value * value,
                    "min": value,
                    "max": value,
                }

    return normalized


def combine_arrays(arrays):
    """
    Combine multiple normalized arrays into a single aggregate.
    Returns dict: { metric: {"count": int, "sum": float, "sum_sq": float|None,
                             "min": float|None, "max": float|None} }
    """
    combined = {}

    for arr in arrays:
        norm = normalize_array(arr)
        for metric, data in norm.items():
            if metric not in combined:
                combined[metric] = {
                    "count": 0,
                    "sum": 0.0,
                    "sum_sq": 0.0,
                    "min": None,
                    "max": None,
                }
                # If sum_sq is None, we need to track that we have no sum_sq
                # We'll use a flag: if any data has sum_sq None, we set final to None
                if data["sum_sq"] is None:
                    combined[metric]["sum_sq"] = None
            # Accumulate count
            combined[metric]["count"] += data["count"]
            # Accumulate sum
            combined[metric]["sum"] += data["sum"]
            # Accumulate sum_sq only if both current and incoming are not None
            if combined[metric]["sum_sq"] is not None and data["sum_sq"] is not None:
                combined[metric]["sum_sq"] += data["sum_sq"]
            elif data["sum_sq"] is None:
                combined[metric]["sum_sq"] = None
            # Update min
            if data["min"] is not None:
                if combined[metric]["min"] is None or data["min"] < combined[metric]["min"]:
                    combined[metric]["min"] = data["min"]
            # Update max
            if data["max"] is not None:
                if combined[metric]["max"] is None or data["max"] > combined[metric]["max"]:
                    combined[metric]["max"] = data["max"]

    return combined


def compute_averages(aggregate):
    """
    Compute average, variance, std_dev, min, max, count from an aggregate.
    Returns dict: { metric: {"average": float, "variance": float|None,
                             "std_dev": float|None, "min": float|None,
                             "max": float|None, "count": int} }
    """
    averages = {}
    for metric, data in aggregate.items():
        count = data["count"]
        if count == 0:
            continue
        average = data["sum"] / count
        if data["sum_sq"] is not None:
            variance = (data["sum_sq"] / count) - (average ** 2)
            # Clamp tiny negative due to floating point
            if variance < 0 and variance > -1e-9:
                variance = 0.0
            if variance < 0:
                variance = None  # shouldn't happen, but guard
                std_dev = None
            else:
                std_dev = math.sqrt(variance)
        else:
            variance = None
            std_dev = None
        averages[metric] = {
            "average": average,
            "variance": variance,
            "std_dev": std_dev,
            "min": data["min"],
            "max": data["max"],
            "count": count,
        }
    return averages


def main():
    parser = argparse.ArgumentParser(description="Combine arrays and compute averages.")
    parser.add_argument("--aggregate-only", action="store_true",
                        help="Output raw aggregate instead of averages")
    parser.add_argument("--verbose", action="store_true",
                        help="Print debug info to stderr")
    args = parser.parse_args()

    if args.verbose:
        print(f"Reading JSON from stdin...", file=sys.stderr)

    try:
        input_data = json.load(sys.stdin)
    except json.JSONDecodeError as e:
        print(f"Error parsing JSON: {e}", file=sys.stderr)
        sys.exit(1)

    if "arrays" not in input_data or not isinstance(input_data["arrays"], list):
        print('Invalid input: expected {"arrays": [...]}', file=sys.stderr)
        sys.exit(1)

    arrays = input_data["arrays"]
    if args.verbose:
        print(f"Received {len(arrays)} arrays.", file=sys.stderr)

    # Combine all arrays
    aggregate = combine_arrays(arrays)

    if args.aggregate_only:
        # Output raw aggregate
        output = {"aggregate": aggregate}
    else:
        # Compute averages
        averages = compute_averages(aggregate)
        output = {"averages": averages}

    # Output JSON
    json.dump(output, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()