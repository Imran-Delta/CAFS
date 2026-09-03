#!/usr/bin/env python3
"""
Tools/Src/smart_handler.py — unified SMART telemetry processor.

One file, three modes, all stdin JSON in / stdout JSON out:

  (no --type, or --type=smart)   Reconcile S/M/B SMART tables, compute
                                   health score and recommendation.
  --type=avg                      Combine raw or pre-aggregated arrays
                                   into bucket statistics (mean/variance/
                                   std_dev/min/max/count). Supports
                                   --aggregate-only for building the
                                   current, still-accumulating hour
                                   without finalizing it into a bucket.
  --type=trends                   Weighted degradation signal for the
                                   allocator: z-scores each error
                                   counter against its historical
                                   average/std_dev, weights them, and
                                   produces a single trend judgement.

C invokes this three times per cycle: --type=avg to fold the current
hour's samples, (default) to get a health score, --type=trends to get
what the allocator actually consumes.
"""

import sys
import json
import argparse

# =====================================================================
# --type=avg  (from avg_maker.py, verified last session — unchanged)
# =====================================================================

METRICS = [
    "total_reads", "total_writes", "total_read_errors", "total_write_errors",
    "total_checksum_mismatch", "total_host_relocations",
]


def normalize_array(arr):
    """A raw sample becomes a one-item aggregate: count=1, sum=value,
    sum_sq=value^2, min=max=value. Lets raw samples and pre-aggregated
    buckets be combined through the same code path."""
    out = {}
    for key in METRICS:
        if key not in arr:
            continue
        v = arr[key]
        out[key] = {"count": 1, "sum": float(v), "sum_sq": float(v) * float(v),
                     "min": v, "max": v}
    return out


def combine_arrays(arrays):
    """Merge any mix of raw samples and pre-aggregated buckets into one
    running total per metric: count, sum, sum_sq, min, max."""
    totals = {}
    for arr in arrays:
        # Decide raw vs. pre-aggregated by inspecting any metric actually
        # present, not by assuming one fixed key exists in every array.
        is_aggregate = any(isinstance(arr.get(k), dict) for k in METRICS if k in arr)
        agg = arr if is_aggregate else normalize_array(arr)

        for key in METRICS:
            m = agg.get(key)
            if not isinstance(m, dict):
                continue
            t = totals.setdefault(key, {"count": 0, "sum": 0.0, "sum_sq": 0.0,
                                         "min": None, "max": None})
            t["count"] += m.get("count", 0)
            t["sum"] += m.get("sum", 0.0)
            t["sum_sq"] += m.get("sum_sq", 0.0)
            mn, mx = m.get("min"), m.get("max")
            if mn is not None:
                t["min"] = mn if t["min"] is None else min(t["min"], mn)
            if mx is not None:
                t["max"] = mx if t["max"] is None else max(t["max"], mx)
    return totals


def compute_averages(totals):
    """Turn combined totals into average/variance/std_dev/min/max/count."""
    out = {}
    for key, t in totals.items():
        count = t["count"]
        if count <= 0:
            continue
        average = t["sum"] / count
        variance = (t["sum_sq"] / count) - (average * average)
        variance = max(0.0, variance)  # clamp floating-point noise near zero
        out[key] = {
            "average": average,
            "variance": variance,
            "std_dev": variance ** 0.5,
            "min": t["min"],
            "max": t["max"],
            "count": count,
        }
    return out


def run_avg(payload, aggregate_only):
    arrays = payload.get("arrays", [])
    totals = combine_arrays(arrays)
    if aggregate_only:
        return {"aggregate": totals}
    return {"averages": compute_averages(totals)}


# =====================================================================
# --type=smart (default) — S/M/B reconciliation, health score
# =====================================================================

# fs.info Section 11.1 array layout (13 elements):
# [magic, version, sequence, total_reads, total_writes, total_read_errors,
#  total_write_errors, total_checksum_mismatch, total_host_relocations,
#  last_access_lba, last_access_timestamp, mount_count, clean_unmount_flag]
SEQ, READS, WRITES, RD_ERR, WR_ERR, CKSUM_ERR, RELOC = 2, 3, 4, 5, 6, 7, 8
LAST_TS, MOUNT_COUNT, CLEAN_UNMOUNT = 10, 11, 12


def reconcile_smart_tables(scratch, main, backup):
    """
    scratch/main/backup: each is a 13-element list, or None if the C
    side already determined it failed its BLAKE3-128 checksum. Python
    never sees raw checksums — that verification happens on the C side
    before this is called; None is the "invalid" signal into Python.
    Returns (primary_table, primary_name, consistency_status).
    """
    main_ok, backup_ok, scratch_ok = main is not None, backup is not None, scratch is not None

    if not main_ok and not backup_ok:
        if scratch_ok:
            return scratch, "scratch", "critical"
        return None, None, "critical"

    if not main_ok:
        return backup, "backup", "main_backup_mismatch"
    if not backup_ok:
        return main, "main", "main_backup_mismatch"

    if backup[SEQ] > main[SEQ]:
        chosen, chosen_name = backup, "backup"
    else:
        chosen, chosen_name = main, "main"

    if not scratch_ok:
        return chosen, chosen_name, "ok"

    if scratch[SEQ] > chosen[SEQ]:
        return scratch, "scratch", "scratch_mismatch"
    return chosen, chosen_name, "ok"


def compute_health_score(primary, averages):
    total_reads, total_writes = primary[READS], primary[WRITES]
    total_read_errors, total_write_errors = primary[RD_ERR], primary[WR_ERR]
    total_checksum_mismatch = primary[CKSUM_ERR]
    total_relocations = primary[RELOC]
    mount_count = primary[MOUNT_COUNT]
    clean_unmount = primary[CLEAN_UNMOUNT]

    total_ops = total_reads + total_writes
    total_errors = total_read_errors + total_write_errors + total_checksum_mismatch
    error_ratio = total_errors / total_ops if total_ops > 0 else 0.0

    avg_reads = averages.get("total_reads", {}).get("average")
    avg_writes = averages.get("total_writes", {}).get("average")
    avg_rd_err = averages.get("total_read_errors", {}).get("average")
    avg_wr_err = averages.get("total_write_errors", {}).get("average")
    avg_ck_err = averages.get("total_checksum_mismatch", {}).get("average")

    avg_ops = (avg_reads + avg_writes) if avg_reads is not None and avg_writes is not None else None
    avg_errors = (
        (avg_rd_err or 0) + (avg_wr_err or 0) + (avg_ck_err or 0)
        if avg_rd_err is not None or avg_wr_err is not None or avg_ck_err is not None
        else None
    )
    avg_error_ratio = avg_errors / avg_ops if avg_ops and avg_ops > 0 else None

    health_score = 100
    if error_ratio > 0.001:
        health_score -= 40
    elif error_ratio > 0.0001:
        health_score -= 20

    health_score -= min(total_relocations / 10, 20)

    health_score += 5 if clean_unmount == 1 else -10

    if mount_count > 1000:
        health_score -= 5
    elif mount_count > 500:
        health_score -= 2

    health_score = max(0, min(100, health_score))

    return {
        "health_score": health_score,
        "error_ratio": error_ratio,
        "avg_error_ratio": avg_error_ratio,
        "total_errors": total_errors,
        "total_ops": total_ops,
        "relocations": total_relocations,
        "mount_count": mount_count,
        "clean_unmount": clean_unmount == 1,
    }


def detect_simple_trend(latest_error_ratio, avg_error_ratio, threshold=0.1):
    if avg_error_ratio is None or avg_error_ratio == 0:
        return "unknown"
    ratio = latest_error_ratio / avg_error_ratio
    if ratio > (1 + threshold):
        return "degrading"
    if ratio < (1 - threshold):
        return "improving"
    return "stable"


def get_recommendation(health_score):
    if health_score >= 90:
        return "none"
    if health_score >= 60:
        return "monitor"
    if health_score >= 40:
        return "backup"
    if health_score >= 20:
        return "emergency"
    return "replace_drive"


def run_smart(payload):
    scratch = payload.get("scratch")
    main = payload.get("main")
    backup = payload.get("backup")
    averages = payload.get("averages", {})

    primary, primary_name, consistency = reconcile_smart_tables(scratch, main, backup)

    if primary is None:
        return {
            "health_score": 0,
            "emergency_flag": True,
            "consistency": "critical",
            "recommendation": "replace_drive",
            "primary_table": None,
        }

    result = compute_health_score(primary, averages)
    trend = detect_simple_trend(result["error_ratio"], result["avg_error_ratio"])
    recommendation = get_recommendation(result["health_score"])

    return {
        "health_score": result["health_score"],
        "emergency_flag": result["health_score"] < 40,
        "error_ratio": result["error_ratio"],
        "avg_error_ratio": result["avg_error_ratio"],
        "error_ratio_trend": trend,
        "total_errors": result["total_errors"],
        "total_ops": result["total_ops"],
        "relocations": result["relocations"],
        "mount_count": result["mount_count"],
        "clean_unmount": result["clean_unmount"],
        "consistency": consistency,
        "recommendation": recommendation,
        "timestamp": primary[LAST_TS],
        "primary_table": primary_name,
        "allocator": {
            "emergency_mode": result["health_score"] < 40,
            "health_bonus": max(0, min(10, (result["health_score"] - 50) // 5)),
        },
    }


# =====================================================================
# --type=trends — weighted degradation signal for the allocator
# =====================================================================
#
# Weights below follow Backblaze's published SMART failure-correlation
# ranking (reallocated-sector count is the single strongest individual
# predictor of near-term failure; uncorrectable/pending-sector counts
# next; raw read/write errors as supporting signal, not primary),
# mapped onto CAFS's own counters. These specific numbers are a
# reasoned starting point, not independently validated against
# consumer-drive failure data — revisit if real data says otherwise.

RELOCATION_WEIGHT = 0.5
CHECKSUM_WEIGHT = 0.3
READ_ERROR_WEIGHT = 0.1
WRITE_ERROR_WEIGHT = 0.1

DEGRADING_THRESHOLD = 1.0    # weighted z-score above this -> degrading
IMPROVING_THRESHOLD = -0.25  # below this -> improving
EMERGENCY_THRESHOLD = 2.0    # above this -> allocator emergency_mode


def zscore(current, avg_entry):
    if not avg_entry:
        return None
    avg = avg_entry.get("average")
    std = avg_entry.get("std_dev")
    if avg is None or std is None or std == 0:
        return None
    return (current - avg) / std


def compute_trend_signal(primary, averages):
    z_reloc = zscore(primary[RELOC], averages.get("total_host_relocations"))
    z_checksum = zscore(primary[CKSUM_ERR], averages.get("total_checksum_mismatch"))
    z_read = zscore(primary[RD_ERR], averages.get("total_read_errors"))
    z_write = zscore(primary[WR_ERR], averages.get("total_write_errors"))

    components = [
        (z_reloc, RELOCATION_WEIGHT),
        (z_checksum, CHECKSUM_WEIGHT),
        (z_read, READ_ERROR_WEIGHT),
        (z_write, WRITE_ERROR_WEIGHT),
    ]
    known = [(max(0.0, z), w) for z, w in components if z is not None]
    total_w = sum(w for _, w in known)
    weighted_degradation = (sum(z * w for z, w in known) / total_w) if total_w > 0 else None

    if weighted_degradation is None:
        trend = "unknown"
    elif weighted_degradation > DEGRADING_THRESHOLD:
        trend = "degrading"
    elif weighted_degradation < IMPROVING_THRESHOLD:
        trend = "improving"
    else:
        trend = "stable"

    return {
        "z_scores": {
            "total_host_relocations": z_reloc,
            "total_checksum_mismatch": z_checksum,
            "total_read_errors": z_read,
            "total_write_errors": z_write,
        },
        "weighted_degradation": weighted_degradation,
        "trend": trend,
        "allocator": {
            "emergency_mode": weighted_degradation is not None and weighted_degradation > EMERGENCY_THRESHOLD,
            "caution": weighted_degradation is not None and weighted_degradation > DEGRADING_THRESHOLD,
        },
    }


def run_trends(payload):
    scratch = payload.get("scratch")
    main = payload.get("main")
    backup = payload.get("backup")
    averages = payload.get("averages", {})

    primary, primary_name, consistency = reconcile_smart_tables(scratch, main, backup)
    if primary is None:
        return {"trend": "unknown", "weighted_degradation": None, "consistency": "critical",
                "allocator": {"emergency_mode": True, "caution": True}}

    result = compute_trend_signal(primary, averages)
    result["consistency"] = consistency
    result["primary_table"] = primary_name
    return result


# =====================================================================
# CLI
# =====================================================================

def main():
    parser = argparse.ArgumentParser(description="CAFS SMART telemetry processor")
    parser.add_argument("--type", choices=["smart", "avg", "trends"], default="smart")
    parser.add_argument("--aggregate-only", action="store_true",
                         help="(--type=avg only) return running totals without finalizing averages")
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()

    try:
        payload = json.load(sys.stdin)
    except json.JSONDecodeError as e:
        print(json.dumps({"error": f"invalid JSON on stdin: {e}"}))
        sys.exit(1)

    if args.type == "avg":
        output = run_avg(payload, args.aggregate_only)
    elif args.type == "trends":
        output = run_trends(payload)
    else:
        output = run_smart(payload)

    print(json.dumps(output))


if __name__ == "__main__":
    main()
