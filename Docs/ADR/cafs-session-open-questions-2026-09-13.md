# CAFS Session Open Questions — 2026-09-13

Things I asked this session that didn't get a direct answer, or that were explicitly deferred. Listed so nothing gets silently assumed resolved.

# CAFS Session Open Questions — 2026-09-13

Things asked this session that didn't get a direct answer, or that were explicitly deferred. Pruned at end-of-session — resolved items removed, not just left stale. Cross-check against the Decisions file before assuming something here is still open; this list reflects only what's left.

## Explicitly deferred by you (have a home, just not now)
- **Tasker's final shape** — master-dispatches-on-interval / intent-notification model, to be dealt with right after the allocator (allocator's now largely closed, so this is next up).
- **`avg` mode's exact computation** — known to exist, feeds `smart`/`trends`, cadence 1–50 min confirmed, but the actual window/formula was never specified.
- **NVMe-specific tasker cadence behavior** — flagged back in the Job 3 overview, never revisited.

## Asked, never answered
- **Scratch structures** — `fsck_scratch_lba` (critical tier): hypothesis was resumable in-progress repair state, never confirmed. `scratch_lba`/`temp_cache_lba` (non-critical): no description found anywhere, purpose genuinely unknown.
- **Hard-floor response scope** — global (whole volume) or zone-scoped (quarantine just the affected zone)? Never resolved which governs. Separately, and deferred by you specifically: smartctl-derived signals are inherently whole-device, so this may not always be answerable even in principle — logged as its own harder problem, not the same as the scope question itself.
- **Hard-floor severity gradient** — what determines warning vs. error? Never specified.
- **"Balanced" preset's daemon-vs-library default** — Safety biases daemon, Performance assumed library; Balanced specifically never confirmed either way.
- **Lock granularity** — are "same file, same block, same leaf, same zone" four genuinely separate lock domains an operation might need simultaneously, or does this collapse to one (zone-level) lock in practice?
- **Migration handler identity** — `[migration]`/`[cold_storage]` config implies a timer-driven cold-data mover; never confirmed as its own process vs. a mode of balance.
- **Relocation-log execution owner** — detection-triggers-an-intent is clear; who actually drains the log and executes the move isn't named anywhere.
- **`wal_lba` exact format** — agreed it needs to be a list (proposed reusing the 16-byte extent-entry shape), never formally locked.
- **WAL "extra entry" structural placement** — the head-based rotation model answered the conceptual crash-safety question; whether granular applied/not-applied tracking lives in the Function Table's fast-path list or the WAL Block Header itself was never pinned.
- **Extent-limit relaxation scope (ENOSPC absolute-effort)** — is dropping the extent-limit constraint strictly per-allocation-attempt, or does it persist as temporary state for some duration? Asked, not yet answered.
- **Consensus-window agreement range** — 3 consecutive readings need to agree "within a range, not exactly," per your own recovered draft. The range width is explicitly still open, logged in your own draft as needing on-disk testing.
- **Grant revocation mechanism** — independent-verification requirement is locked in as a hard constraint; the actual mechanism is deferred to when the I/O Engine and allocator are actually implemented.
- **Error-code numeric scheme** — see tonight's hash-collision/range finding, unresolved as of this message.

## Research gaps (not design questions — things I looked for and couldn't confirm)
- **WinFsp's own trim-relay capability** — searched specifically, found nothing either way.
- **ntfs-3g's exact internal lock primitive** — confirmed it never runs FUSE multithreaded, but the specific mechanism protecting its internal data structures beyond "never run concurrently" wasn't found.

## Numeric thresholds still placeholder, not measured (flagged throughout, consolidated here)
- `health_composite` per-attribute `warn_at` values (10/5/5/5/3) — reasonable starting shapes, not tuned against real data.
- `pending_trajectory` "trending upward" threshold for triggering Proactive Bad-Block Relocation.
- `write_size_cv` low/high split for Preallocation On/Off.
- EWMA α constants (0.18 health, 0.3 trajectory) — derived from the α=2/(n+1) relationship for a chosen window, not validated.
- A0–A5 preset table's own thresholds (15% free space, healthy/degraded cutoffs for Flush Interval).

