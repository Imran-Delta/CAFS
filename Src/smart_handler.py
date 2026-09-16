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

FS Tasker (Rust, not C -- corrected from an earlier draft of this
docstring) invokes this three times per cycle: --type=avg to fold the
current hour's samples, (default) to get a health score, --type=trends
to get what the allocator actually consumes.
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
                "allocator": {"emergency_mode": True, "caution": True},
                "signals": compute_signals(payload, primary=None)}

    result = compute_trend_signal(primary, averages)
    result["consistency"] = consistency
    result["primary_table"] = primary_name
    result["signals"] = compute_signals(payload, primary=primary)
    return result


# =====================================================================
# Signals layer (added this session) -- allocation_chs.rs's knob inputs
# =====================================================================
#
# Different job from compute_trend_signal() above. That mechanism asks
# "is CAFS's own error-counter history degrading relative to itself."
# This layer asks "what does each allocator knob's live-swap decision
# need to see, right now." Both live under --type=trends because both
# genuinely belong to trends' scope -- kept side by side, not merged,
# since they answer different questions from different data.
#
# Every value returned here is already bounded/normalized on purpose --
# allocation_chs.rs is meant to do nothing but threshold comparisons
# against these, no computation of its own.
#
# PLACEHOLDER INPUTS. Two real data sources this logic depends on
# don't exist in the codebase yet:
#   - io_placeholder: per-operation I/O Engine counters (consecutive
#     write LBA distances, write-size samples). Needs syscalls.rs to
#     actually populate -- not built yet.
#   - smartctl_placeholder: real drive smartctl attribute reads
#     (5, 187, 188, 197, 198 / NVMe critical_warning). No smartctl
#     shell-out exists anywhere in this file yet -- this is standing
#     in for that missing integration, not a finished one.
# The math below is real and reviewable now; wiring these two sources
# in for real is a separate, later change to this file's payload
# handling, not to the formulas themselves.

FIXED_CAP_BLOCKS = 262144   # 1GiB at 4KB blocks -- R_write normalization reference. Provisional, needs real tuning against actual workloads.
BIAS_THRESHOLD = 2.0        # workload_bias >= this -> "sequential-friendly". Recalibrated this session -- the original threshold of 15 needed a 960KB average write at zero randomness to ever fire, unreachable for realistic sequential I/O.
FREE_SPACE_LOW = 0.15       # Reused from the existing Urgent-B routine-tasker threshold rather than inventing a second number for the same concept.


def compute_r_write(avg_consecutive_distance_blocks):
    """Mean |LBA_new - LBA_prev| across writes in the window, normalized
    against a fixed cap rather than total device blocks -- keeps
    R_write's meaning consistent across different device sizes, so the
    same physical write pattern scores the same on a 128GB SSD and a
    20TB HDD."""
    if avg_consecutive_distance_blocks is None:
        return None
    return min(avg_consecutive_distance_blocks / FIXED_CAP_BLOCKS, 1.0)


def compute_workload_bias(mean_write_size_bytes, r_write):
    """B = (mean write size in 64KB units) / denominator(r_write).
    Denominator recalibrated this session: the original 1/(1+R_write)
    can only ever halve B even at maximum randomness, too weak to
    meaningfully penalize large-but-scattered writes. Replaced with a
    shape whose denominator genuinely grows large as r_write -> 1 --
    the *0.99 factor keeps (1 - r_write*0.99) bounded away from zero
    (minimum 0.01 at r_write=1.0), so no real divide-by-zero risk
    despite the harsh penalty near maximum randomness."""
    if mean_write_size_bytes is None or r_write is None:
        return None
    size_component = mean_write_size_bytes / 65536.0
    denominator = 1.0 / (1.0 - r_write * 0.99)
    return size_component / denominator


def normalize_attribute(raw_value, warn_at):
    """0.0 = no concern, 1.0 = at or past the warn threshold. Straight
    linear ramp -- a placeholder shape, not validated against real
    drive-population failure data the way the 5/187/188/197/198
    attribute *selection* itself was (Backblaze)."""
    if raw_value is None or warn_at <= 0:
        return 0.0
    return min(raw_value / warn_at, 1.0)


def compute_ewma(bucket_history, alpha=0.18):
    """Fold an oldest-first bucket history into a single recency-
    weighted value -- each point's influence decays exponentially the
    further back it is, rather than every point counting equally
    (a raw average) or only the endpoints mattering (a first-to-last
    delta). alpha=0.18 targets roughly the last ~10 buckets dominating
    the result (alpha = 2/(n+1) for n=10) -- a real starting number,
    not tuned against actual drive data yet. A length-1 history just
    returns that single point, which is the deliberate degrade path
    for callers that don't have real history yet (see compute_signals)."""
    if not bucket_history:
        return None
    ewma = bucket_history[0]
    for value in bucket_history[1:]:
        ewma = alpha * value + (1 - alpha) * ewma
    return ewma


def compute_health_composite(attr_5_history, attr_197_history, attr_188_history,
                              checksum_mismatch_history=None, host_relocations_history=None):
    """EWMA-smoothed per attribute, not raw-value-scored -- a single
    isolated error nudges the score and decays back out over
    subsequent buckets; a sustained climb doesn't. Matches real
    disk-failure-prediction research's core finding for cumulative
    counters: recent *change* is more informative than the raw
    accumulated total, and matches the stated design goal directly --
    drives don't fail instantly and occasional isolated errors are
    normal, so the score has to distinguish noise from a genuine trend
    rather than reacting to either identically.

    Five inputs, not three: attr_5/197/188 (real ATA attributes) plus
    checksum_mismatch and host_relocations, CAFS's own on-disk
    counters. Those two aren't redundant with the ATA data -- they
    catch a silent bit-flip that trips CAFS's own checksum without
    ever crossing whatever internal threshold makes drive firmware
    report an ATA error at all. Real smartctl evidence and CAFS-proven
    evidence, combined, not overlapping.

    187/198 never reach this function at all, at any input position --
    they go straight to check_hard_floor and bypass scoring entirely,
    on purpose. Combined via max(), not average -- one climbing input
    shouldn't be diluted by four clean ones into a falsely reassuring
    blend."""
    inputs = [
        (compute_ewma(attr_5_history), 10),
        (compute_ewma(attr_197_history), 5),
        (compute_ewma(attr_188_history), 5),
        (compute_ewma(checksum_mismatch_history), 5),   # warn_at placeholder, same as the ATA ones -- needs its own tuning pass, not yet validated
        (compute_ewma(host_relocations_history), 3),    # a relocation already succeeded in saving the data, but it's still real degradation evidence -- lower warn_at than checksum_mismatch since even a few relocations is worth noticing
    ]
    scores = [normalize_attribute(ewma, warn_at) for ewma, warn_at in inputs if ewma is not None]
    return max(scores) if scores else 0.0


def compute_pending_trajectory(attr_197_bucket_history):
    """EWMA of the bucket-to-bucket *rate of change*, not a raw
    first-to-last delta (the original shape) -- the delta version was
    noisy with few buckets and couldn't tell a single-bucket blip from
    a steady climb; this smooths that out while still reacting faster
    than health_composite's own slower alpha, deliberately, since this
    knob's whole job is catching deterioration before it becomes a
    hard-floor event, not confirming it after the fact."""
    if not attr_197_bucket_history or len(attr_197_bucket_history) < 2:
        return None
    deltas = [b - a for a, b in zip(attr_197_bucket_history, attr_197_bucket_history[1:])]
    return compute_ewma(deltas, alpha=0.3)


def compute_write_size_cv(write_size_average, write_size_std_dev):
    """Deliberately separate from workload_bias -- bias conflates size
    with sequentiality; Preallocation only cares whether write sizes
    are *consistent*, not how big or how sequential they are."""
    if write_size_average is None or write_size_std_dev is None or write_size_average == 0:
        return None
    return write_size_std_dev / write_size_average


def check_hard_floor(attr_187_uncorrectable, attr_198_offline_uncorrectable,
                      nvme_critical_warning_bits=None):
    """Filesystem-wide override. Monotonic within a mount by design --
    once True here, the caller is expected to keep hard_floor_active
    True for the rest of the session regardless of what later reads
    say; clearing it is a hot-remount concern, not something this
    function does. 187/198 (both literally "uncorrectable" -- the
    drive's own firmware already exhausted its retries before
    reporting either) are the ATA hard triggers. NVMe critical_warning
    bit 2 (reliability degraded) and bit 3 (media read-only) mirror
    them -- bit 2 specifically matches VMware vSAN's own automatic-
    action trigger, not an invented threshold."""
    if attr_187_uncorrectable and attr_187_uncorrectable > 0:
        return True
    if attr_198_offline_uncorrectable and attr_198_offline_uncorrectable > 0:
        return True
    if nvme_critical_warning_bits is not None:
        if nvme_critical_warning_bits & 0b0100:   # bit 2
            return True
        if nvme_critical_warning_bits & 0b1000:   # bit 3
            return True
    return False


def compute_signals(payload, primary=None):
    """Assembles every value allocation_chs.rs needs, each already
    normalized/bounded. See module-level note above on io_placeholder
    and smartctl_placeholder -- both stand in for data sources that
    don't exist in the codebase yet.

    primary: the already-reconciled on-disk SMART array from
    reconcile_smart_tables() (same data compute_trend_signal already
    uses) -- real, not a placeholder. Supplies checksum_mismatch and
    host_relocations' *current* values directly. Their multi-hour
    bucket history (for real EWMA smoothing, not just a single-point
    read) isn't available from anywhere yet -- same gap as the ATA
    attributes' history. Until that plumbing exists, every history
    list here degrades to length-1 (just the current value) when a
    fuller history isn't supplied in the payload -- compute_ewma of a
    single point is just that point, so this runs correctly today and
    improves automatically once real history is wired in, with no
    further code change needed here."""
    io = payload.get("io_placeholder", {})
    smartctl = payload.get("smartctl_placeholder", {})

    r_write = compute_r_write(io.get("avg_consecutive_write_distance_blocks"))
    workload_bias = compute_workload_bias(io.get("mean_write_size_bytes"), r_write)

    def history_or_single(explicit_history_key, single_value):
        history = smartctl.get(explicit_history_key)
        if history:
            return history
        return [single_value] if single_value is not None else None

    checksum_mismatch_current = primary[CKSUM_ERR] if primary is not None else None
    host_relocations_current = primary[RELOC] if primary is not None else None

    health_composite = compute_health_composite(
        history_or_single("attr_5_bucket_history", smartctl.get("attr_5_reallocated")),
        history_or_single("attr_197_bucket_history", smartctl.get("attr_197_pending")),
        history_or_single("attr_188_bucket_history", smartctl.get("attr_188_timeout")),
        history_or_single("checksum_mismatch_bucket_history", checksum_mismatch_current),
        history_or_single("host_relocations_bucket_history", host_relocations_current),
    )
    pending_trajectory = compute_pending_trajectory(
        history_or_single("attr_197_bucket_history", smartctl.get("attr_197_pending"))
    )
    write_size_cv = compute_write_size_cv(
        io.get("write_size_average"), io.get("write_size_std_dev")
    )

    # Zone Table pass-through -- not computed here, just carried along
    # so allocation_chs.rs has one place to read every signal from.
    free_space_percent = payload.get("free_space_percent")

    hard_floor_active = check_hard_floor(
        smartctl.get("attr_187_uncorrectable"),
        smartctl.get("attr_198_offline_uncorrectable"),
        smartctl.get("nvme_critical_warning_bits"),
    )

    return {
        "workload_bias": workload_bias,
        "workload_bias_sequential": workload_bias is not None and workload_bias >= BIAS_THRESHOLD,
        "r_write": r_write,
        "health_composite": health_composite,
        "pending_trajectory": pending_trajectory,
        "write_size_cv": write_size_cv,
        "free_space_percent": free_space_percent,
        "free_space_low": free_space_percent is not None and free_space_percent < FREE_SPACE_LOW,
        "hard_floor_active": hard_floor_active,
    }


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
