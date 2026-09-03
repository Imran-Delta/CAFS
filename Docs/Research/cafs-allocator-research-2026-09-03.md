# CAFS Allocator — Research: Formulae, Algorithms, and Prior Art

Confirms the earlier finding: no existing crate solves "CoW disk block allocator with health-aware placement" — `slab_allocator_rs`/`block_allocator`/`emballoc` are `no_std` *heap* allocators, a different problem entirely. This has to be built from scratch. What follows is what's actually reusable: concrete algorithms and formulae from four production systems (ZFS, Btrfs, WAFL, XFS), plus a synthesized starting design for CAFS specifically.

## 1. Data structures for tracking free space

**Plain bitmap** — one bit per block. Cheap to update, but answering "is there a free run of N blocks near offset X" means scanning, which gets expensive as the device grows. Fine as a *bottom layer*, poor as the only structure.

**ZFS space maps** (per-metaslab) — a log of allocate/free events, each entry `(offset, run_length)`, replayed to reconstruct free space on load. Two important refinements, both directly reusable:
- **Size-bucketed histogram** alongside the log: `histogram[i]` counts free regions whose size falls in `[2^i, 2^(i+1))`. This is the basis of ZFS's fragmentation-aware weighting (§3).
- **Encoding evolved because of a real bug class**: the original single-word entry format could only address a 16MB run (assuming 512-byte sectors) before needing multiple entries — inefficient for large contiguous frees. Spacemap Encoding V2 added double-word entries for large runs. *Lesson: don't fix your free-run encoding width without checking it against your largest plausible extent.*

**Btrfs block groups** — the device is partitioned into block groups (ZFS's metaslab equivalent). Each group tracks free space with **two red-black trees**: one indexed by *offset*, one indexed by *size*. This dual-index is the single most reusable idea here:
- Offset-tree answers "is there free space at/after hint H" — fast path for sequential/locality-preserving writes.
- Size-tree answers "is there a free run of at least size S" — fallback when the hint misses, and when falling back, Btrfs prefers a slot *close to* the requested size rather than the largest available one, specifically to avoid carving small requests out of large contiguous regions.
- Historical lesson from **space cache v1 → v2**: v1 was a cache *of* the free-space computation, rebuilt from the extent tree every mount — this degraded badly at multi-terabyte scale. v2 (the "free space tree") made free space a first-class persistent B-tree instead of a derived cache. *Don't build a cache of a cache; persist the real structure.*
- Extent-tree-v2's "Global Roots" reorg is worth noting too: block-group metadata used to be interleaved throughout the main extent tree, which meant reading hundreds of scattered leaves at mount time; moving it to its own tree cut that to a handful of reads. *Co-locate metadata that's read together at mount.*

**Buddy system** (classic, e.g. Linux's own physical-page allocator) — memory/space divided into power-of-two blocks. On request, round up to the nearest power of two, split a larger free block in half repeatedly until the right size is reached; on free, check whether the block's "buddy" is also free and merge.
- **Buddy address formula**: `buddy_address = block_address XOR block_size`. O(1) to compute, no search needed — this is *why* the buddy system is fast.
- Implementation: an array of free-lists, one per order (size class); allocation walks up from the target order until it finds a non-empty list, splitting on the way back down.
- Known weakness: **internal fragmentation** — a 66KB request gets a 128KB block, wasting 62KB, because sizes are constrained to powers of two.

## 2. Allocation strategies

- **First-fit**: take the first free region big enough. Fast, tends to fragment the front of the device over time.
- **Best-fit**: take the smallest free region that's still big enough. Less wasted space per allocation, but tends to leave many small, useless slivers behind (visible directly in the buddy-system trade-off above).
- **Hint + fallback** (Btrfs): try near a caller-supplied offset hint first (preserves locality for related writes — e.g. a file's successive extents); if nothing's there, fall back to a size search that prefers a close-to-exact match over the largest available block.
- **Weighted/segment-based selection** (ZFS): each metaslab/block-group gets a *weight*, and allocation picks the highest-weighted group rather than scanning linearly. Two weighting modes exist:
  - *Space-based*: weight ≈ total free bytes (simple, but doesn't distinguish "10GB free in one run" from "10GB free in a million tiny fragments").
  - *Segment-based* (the more interesting one): weight is built from the size-histogram by iterating from the **largest** bucket down, doubling the running total and adding each bucket's count: `segments = 0; for i from max downto min: segments = (segments << 1) + histogram[i]`. Because higher buckets are shifted further, a metaslab with one huge free run dominates the weight over one with many small free runs holding the same total bytes. This is the concrete, implementable answer to "how do I prefer contiguous space over fragmented space, numerically."
  - ZFS also **biases by physical location on HDDs**: outer disk tracks are faster (constant angular velocity means more sectors/revolution at the same rotational speed), so metaslabs on outer tracks get extra weight when `metaslab_lba_weighting_enabled` is set. Not relevant to SSDs, but the *pattern* — let device geometry bias placement — is exactly what CAFS's SMART-fed allocator wants to do for a different reason (§4).
  - A **free-space-percentage threshold** gates whole groups: below it, a group is skipped in favor of emptier ones, unless *every* group is below threshold, in which case the restriction lifts. Prevents pathologically uneven fill across metaslabs/block-groups without ever fully blocking allocation.

## 3. Fragmentation metrics (concrete formulae)

Three real ones, in order of simplicity:

1. **Sum-of-squares** (used in a defragmentation patent for scoring layouts): for free regions of size `s_1..s_n`, score = `Σ s_i²`. Higher = better (favors one big region over many small ones summing to the same total, since squaring rewards size super-linearly). Trivial to compute incrementally as regions merge/split.
2. **XFS's bucketed-percentage approach** (used operationally, with real thresholds): classify free space into buckets (`<8 blocks`, `<64 blocks`, `≥64 blocks`) and report the percentage of total free space in each. Their empirical thresholds: healthy = `<1%` in the smallest bucket and `>5%` of total FS size in the largest bucket; badly fragmented = `>5%` in the smallest bucket and `<5%` in the largest. Useful as-is for a health-dashboard-style "fragmentation score."
3. **Design constraints for a custom metric** (from an independent analysis of memory-fragmentation metrics, directly transferable): a good fragmentation score should (a) be 0 when free space is one contiguous region, (b) approach its maximum when free space is maximally spread across many small holes, (c) *not* depend on the number of allocations/regions directly (a naive `(free_region_count - 1) / allocation_count` formula fails this — small holes between unrelated allocations inflate it without representing "fragmentation" in a meaningful sense), and (d) be independent of the free/used ratio (that's capacity, a different question from fragmentation).

## 4. CoW-specific concerns

This is where WAFL is the most relevant prior art, since it's the one system on this list built around copy-on-write from the ground up rather than in-place update:

- **Reclamation urgency is structurally higher under CoW.** Every write to existing data allocates a *new* block and frees the old one (once no snapshot pins it) — so the write path's throughput is bounded by how fast the allocator can find good free space, not just by raw device bandwidth. WAFL's own framing: *"copy-on-write increases the demand on the file system to find free blocks quickly, which makes rapid free space reclamation essential."*
- **The write allocator is explicitly geometry-aware**: WAFL's allocator factors in underlying RAID geometry, SSD erase-block size, and shingled-HDD zone size when choosing where to place data — not just "is this free," but "is this a *good* free region given the physical medium." Directly analogous to what CAFS's SMART-derived health/degradation signal should feed into: not just avoiding bad regions, but actively preferring healthier ones the same way WAFL prefers erase-block-aligned regions.
- **WAFL evolved from a single free-space algorithm to a scalable, many-core write-allocation architecture ("White Alligator")** as core counts grew — cited speedups up to 274% from removing allocator-side contention. If CAFS's allocator becomes a bottleneck under concurrent writers, sharding free-space tracking (e.g. per-zone, matching the pointer-block-per-zone layout already in `fs.info`) rather than a single global structure is the direction that scales, per this precedent.
- **Snapshot-pinned blocks complicate "free."** A block only becomes truly free once every snapshot referencing it is gone — CoW filesystems generally need a refcount (or equivalent) per block/extent, not just a free/used bit, the same shape as the dedup refcount CAFS's design already has (`fs.info`'s dedup table `refcount` field) — worth checking whether that same mechanism can be shared for snapshot-pinning rather than building a second one.
- **Primary sources** (deep algorithmic detail is behind these, not fully in search-indexed abstracts — worth reading in full before implementing):
  - Kesavan, Singh, Grusecki (NetApp), Patel (UW–Madison), *"Algorithms and Data Structures for Efficient Free Space Reclamation in WAFL,"* USENIX FAST '17 — **Best Paper Award**. The primary reference for this whole section.
  - *"Efficient Search for Free Blocks in the WAFL File System,"* ICPP 2018 — narrower follow-up specifically on the free-block search problem.
  - *"Scalable Write Allocation in the WAFL File System"* — the "White Alligator" many-core architecture.
  - (Separately, the original 1994 WAFL paper — Hitz, Lau, Malcolm, USENIX Winter '94 — is about the filesystem generally, not specifically the allocator; the FAST '17 paper above is the one that's actually about this problem.)

## 5. Synthesis: a starting design for CAFS

Given CAFS already has zones (Type-A/Type-B, per the earlier design discussion) and a real health signal (SMART stats fed to the allocator per `smart_handler.py`'s `emergency_mode`/`caution` flags), the pieces above suggest a concrete starting point rather than a from-scratch blank page:

1. **Per-zone free-space tracking, dual-indexed** — borrow Btrfs's offset-tree + size-tree pair, scoped per zone (matches the zone-per-pointer-block layout already in `fs.info`, and gives WAFL-style shardability for free if concurrent writers ever need it).
2. **Zone weight = ZFS's segment-based histogram formula**, not a raw free-byte count — this is the one piece of this research most directly copy-pasteable as a formula (§2), and it's exactly the "prefer contiguous over fragmented" property an allocator needs.
3. **Bias that weight by SMART health**, the same way ZFS biases by LBA/track position and WAFL biases by RAID/erase-block/zone geometry — multiply or gate zone weight by the zone's current health signal (`caution` → deprioritize, `emergency_mode` → exclude from new allocations entirely until it clears), so degrading zones drain naturally rather than needing a separate migration pass. This is the "great information for the allocator" the SMART stats were built for.
4. **Hint-based allocation with size-search fallback**, Btrfs-style: sequential/related writes get locality via the hint; the fallback prefers close-to-exact size matches over breaking up large contiguous runs.
5. **Report fragmentation as XFS's bucketed percentages**, since it's already human-readable and threshold-able, and reuse the same size-histogram from #2 to compute it — no second data structure needed.
6. **Persist free-space state as a real structure, not a rebuilt-at-mount cache** — Btrfs's v1→v2 lesson. Given CAFS already avoids a "cache of a cache" pattern elsewhere in this design (Config Snapshot backups were dropped for the same write-amplification reason, ADR-006), this is consistent with decisions already made, not a new principle being introduced.
7. **CoW refcounting**: check whether the existing dedup-table refcount mechanism (`fs.info`, `dedup_table_root_lba`) can double as the snapshot-pin mechanism before building a separate one.

This is a proposal, not a lock-in — flag anything here that conflicts with a zone/allocator decision already made elsewhere and it should give way to that.
