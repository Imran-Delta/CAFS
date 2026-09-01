# Session Evaluation — Block/Checksum/Dedup Decisions, the Disturbance, and I/O Engine Gaps

## Part 1 — Document 4's decisions, verified against the actual fs-v19.info / config-v19.fs

**1. Block size hybrid (Type-A fixed 2KiB / Type-B variable extents).** Sound in principle, but "Change default block_size to 2048 for Type-A" is a real, consequential parameter change buried in a resolution-table cell — worth its own explicit confirmation, not something to wave through. Also: this isn't really a *new* structure. `zone_table_lba` has existed in the Meta/User Pointer Table since the first fs.info I read — it's just never had a format. "Zone Header" is the overdue definition of that pointer, not an addition alongside it. Recommend naming it accordingly (Zone Table = array of Zone Headers) so it doesn't read as a second, competing concept.

**2. CoW chunk = 64KiB; compression batch = 1GiB.** 64KiB is reasonable (close to ZFS's own default recordsize, which we already discussed as a well-tested value). 1GiB is the one I'd push back on: it's an *atomic* compression unit, meaning reading one byte 900MB into a file requires decompressing up to the full gigabyte around it (unless the format is internally seekable, which isn't mentioned), and any single corrupted byte inside that batch risks the whole batch — a bigger blast radius than "won't die easily" suggests you want. Worth asking DeepSeek to justify 1GiB specifically, or considering something in the 16–64MB range that trades some ratio for real fault isolation and random-access granularity.

**3. Two-tier checksum (CRC32C fast/Tier-1 on Type-A, xxhash64-or-BLAKE3 strong/Tier-2 in background for dedup + Type-B).** This is genuine progress on a real, previously-flagged gap (fs.info's own §18 already lists "data-block checksum storage" as unresolved). But there's a hole in the design as described: Tier-1 is explicitly scoped to Type-A only, and Tier-2 is explicitly "computed in background" — so what protects a Type-B extent in the window between it being written and its background Tier-2 checksum landing? As written, it reads like zero coverage during that window, for exactly the data path (defrag/compression/dedup output) that's least disposable. Needs either an inline fast checksum for Type-B too (Tier-2 upgrades it later), or the WAL needs to cover the gap.

**4. Global Hash Table for dedup, "stored in Function Table."** Two problems. First, precision: Function Table entries are 8-byte pointers to structures living elsewhere (same pattern as `wal_lba`, `smart_main_lba` — it points, it doesn't hold). Second, and more important: the Meta/User Pointer Table *already has* `dedup_table_root_lba` — this has been in fs.info since the first version I read. A new `global_hash_table_lba` entry in the Function Table, introduced without acknowledging that field, is either a duplicate of something that already exists or needs to explain how the two relate. Given the Meta/User Table is where every other large, data-adjacent structure lives (`btree_root_lba`, `compaction_checkpoint_lba`), the dedup hash table belongs there too — recommend treating this as the long-overdue *format* for `dedup_table_root_lba`, not a new Function Table entry.

**5. Checksum storage location.** Same "genuine gap being closed" note as #3 — good progress, same Type-B timing hole applies.

**6. `[cow].chunk_size = "64KB"` addition.** Verified: config.fs's `[cow]` really does only have `default_mode` today. Accurate gap, clean addition.

**7. `[data].compression_batch_size` / `compression_min_file_size` addition.** Verified: `[data]` really has no batch-size field today. Accurate gap. (Batch size itself — see #2.)

**8. WAL op 0x06 = EXTENT_B, value points to the Type-B extent header.** This is the one I'd call out *positively*: it correctly follows the pattern we fought hard to establish for ANCHOR_DIFF (fixed 48-byte entry, value field is a pointer to a separately-stored variable-length structure) rather than reintroducing variable-length WAL entries — the exact zombie design this session already killed once. Whoever wrote this got that part right.

**9. Dedup depth/index-type config addition.** The proposed additions (`depth`, `index_type`, `max_entries_hard_limit`, `eviction_policy`) are genuinely missing. But the framing — "only basic `enabled` and `algo`" — is inaccurate: verified `[dedup]` already has `min_file_size_for_dedup`, `scan_interval_seconds`, `max_ram_table_mb`, `skip_dedup_for_raw`. Worth correcting so nothing gets accidentally duplicated when this is added.

**Non-conflicts claim (background handlers, allocator knobs, mount_hints, checksum algo IDs, SMART/WAL):** verified `[background].priority_order = ["dedup", "defrag", "balance"]` really does already exist as claimed. This part of the document is accurate.

---

## Part 2 — The disturbance, and why "the allocator needs research" is right

You flagged these documents might be out of order and asked me to treat them with appropriate suspicion. Doing that surfaced two real, unreconciled contradictions — not just possible reordering, but actual conflicting content.

**The allocator now has three unreconciled designs, not one.**

1. What's actually in the uploaded allocator file: five static named algorithms (A0–A4), selected once at mount, live epoch-switching explicitly rejected by an independent audit for a concurrency data race.
2. What we built together earlier this session: seven *independent* knobs, each scored from live metrics, most-specific-wins tiebreak (with an explicit config fallback for the one cross-variable case), concurrency fixed at mount via a new Service Data structure, the other six knobs safe to reconfigure live via an atomic generation-counter swap.
3. Document 9's "Core Model": back to five static named algorithms (A0–A4, same names as #1) with a *numeric priority list* tiebreak (`[4,3,2,1]`), concurrency fixed at mount (matches #2), but "Placement, read-ahead, prealloc, flush interval, bad-block relocation — safe to change live" grafted on top (borrowed from #2's spirit, applied to #1's structure).

#3 doesn't cleanly merge #1 and #2 — if Placement and read-ahead can change live independent of which A0–A4 "algorithm" is active, the named bundles aren't really monolithic algorithms anymore, they're just an initial preset for knobs that get overridden anyway, which is #2 wearing #1's naming. It also isn't internally consistent on its own terms: A4 is named "Fallback," but the stated fallback rule is "if no policy eligible → A2 or A3 depending on drive type" — never A4. What A4 actually is, if not the thing its own name says, isn't answered anywhere in what's here.

None of this is resolved by picking one — it's a real design question with three genuinely different concurrency-selection philosophies in play, which is exactly why "the allocator needs research" rather than "the allocator needs a decision file."

**SMART Handler: Python or not?**

Documents 5, 6, and 7 are substantial, serious work on exactly one question — how Python integrates (embedded and IPC-free vs. optional-and-detected, one-shot vs. daemon, socket vs. shared memory) — and your own message just now restates a live position on it (one-shot via args/stdin, or daemon via socket, or a memory-map fallback; packages auto-included with compile-time exclusion). Documents 8 and 9 both instead say the SMART Handler is "a thread inside the Master... No Python process. No sockets. No pipes," with no reference back to any of that work. Those two claims can't both be current. Given you just restated the Python-process position yourself, I'm treating *that* as live and documents 8/9's "no Python process" line as the one that's stale or from a different branch of discussion — flag it if that's backwards.

---

## Part 3 — I/O Engine: what's still missing, given all of the above

- **Data-block checksum storage** — direction is good (two-tier), but the Type-B coverage-window gap above needs closing before this is specifiable.
- **Zone Table format** — needs to be written for the first time, as `dedup_table_root_lba`'s neighbor `zone_table_lba` has needed since the start; document 4's "Zone Header" is that work, just needs the naming fixed so it doesn't read as a second structure.
- **B-tree node format** — still completely undefined anywhere, not something document 4 introduced or fixed. The "(LBA, block_count) only" claim in document 4's conflict table isn't accurate — there's nothing to conflict with, because no B-tree node format has ever been written. This needs to happen regardless of which allocator model wins.
- **Global Hash Table** — direction is right, needs to land as `dedup_table_root_lba`'s format rather than a new pointer.
- **Whichever allocator model wins** determines what "read/write" and "checksum" mean in the engine's hot path — this is the one gap that blocks the others from being finalized, not just documented.
- **Python/SMART boundary** — needs the contradiction above resolved before the I/O Engine ↔ SMART Handler protocol (already an open roadmap item) can be written at all.

---

## Recommendation

Ready for a decision file now, independent of the allocator/process questions: Zone Table format (naming-corrected), Global Hash Table as `dedup_table_root_lba`'s format, WAL op 0x06 (EXTENT_B), `[cow].chunk_size`, `[data].compression_batch_size` — with the 1GiB batch size and the Type-B checksum-window gap flagged for your call before they're locked rather than waved through.

Not ready: allocator model (three live candidates, needs the reconciliation above, not a vote), C-process/Master architecture (document 9's own "Open" list — IPC transport, permit/deny protocol, Master resurrection — is still genuinely open by its own admission), Python/SMART integration point (contradiction above unresolved).

Given that split, I'd hold off on the full decision file and check.py until the allocator and process questions get their own pass — writing checks against a foundation that's still forking three ways would just mean rewriting check.py's assertions once it settles. Say which of the three allocator models you want to actually pursue (or how to merge them) and I'll go from there, in this session or the next one, your call.
