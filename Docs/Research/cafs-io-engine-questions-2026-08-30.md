# I/O Engine — Questions to Close Before Alpha 1 (Linux 64-bit)

Ordered by dependency — later questions assume earlier ones are answered where noted.

## 1. Process architecture

Document 9 mentioned a "Master" (oldest process, background handlers) and a possible separate "Allocator daemon," both still marked open by their own admission last session (IPC transport, permit/deny protocol, Master resurrection — none settled).

**For Alpha 1 specifically, Linux 64-bit only:** does it need the full Master + separate Allocator daemon model on day one, or can Alpha 1 ship as a single mntproc with the allocator as a linked-in library call, and the daemon split deferred to whenever multi-drive/multi-process coordination actually gets exercised? Everything below — the C↔Python locking mechanism, how the allocator gets its usage stats, how SMART updates get dispatched — has a different concrete shape depending on this answer. This is the one question I'd want settled before anything else here.

## 2. Type-A / Type-B on-disk formats

Conceptually locked (2KiB fixed Type-A, variable Type-B extents, background-converted). Not yet byte-specified:

- **Zone Header** (the format `zone_table_lba` has been missing since the first fs.info). Needs: `zone_type` (0=A/1=B), what else — extent count, free-space summary, a back-pointer to its own bitmap/index?
- **Type-A bitmap** — where does it live relative to the zone it tracks? Inline at a fixed offset within the zone, or referenced by a pointer in the Zone Header?
- **Type-A checksum placement** — document 4 said "per-block table" for Tier-1/CRC32C. Concretely: a separate structure (parallel array indexed by block number, one per zone), or reserved bytes at the end of each 2KiB block itself (making a Type-A block e.g. 2044 bytes data + 4 bytes CRC32C)? These have different space and I/O-pattern costs and need picking now, not discovered while writing the read path.
- **Type-B extent header** — `byte_length`, checksum (Tier-2, 8 or 32 bytes depending on xxhash64 vs. BLAKE3), `file_id`, `offset_in_file` are named; what are their actual sizes and byte offsets?

## 3. Block size vs. extent granularity — a concrete inconsistency

Document 4's original phrasing: Type-A fixed at 2KiB, but Type-B extents specified as "multiples of 1KiB." If the physical unit is 2KiB, what does a 1KiB-granularity extent actually mean — does Type-B address at a finer grain than Type-A's own block size, or should this just read "multiples of 2KiB" to match? If Type-B really can address at 1KiB, that has real implications for how Type-A and Type-B zones can neighbor each other on disk.

## 4. Global Hash Table / Dedup Table — concrete format

Settled conceptually as two related structures (a hash inventory at configurable granularity, and the dedup subset). Byte format not written: entry size, what a lookup actually returns (offset into a separate location table, or the location inline?), collision handling for the "binary hash table, O(1)" lookup you described — open addressing, chaining, something else?

## 5. CoW chunk metadata — concrete format

`chunk_id, file_id, offset, LBA, flags` are named; sizes and byte layout aren't. Also: where does this live — inline with the extent it describes, or its own indexed structure like the Zone Header?

## 6. WAL EXTENT_B (op 0x06) — concrete payload

Confirmed to correctly follow the ANCHOR_DIFF pattern (fixed 48-byte entry, value field points to the real Type-B extent header elsewhere). Needs the actual value-field layout: presumably `extent_header_lba` + `extent_header_size`, mirroring ANCHOR_DIFF's `anchor_diff_block_lba` + `anchor_diff_block_size` exactly — confirm, or is there a reason it should differ?

## 7. Type-A/Type-B checksum failure — what actually happens

If a Tier-1 or Tier-2 checksum fails on read, is Alpha 1's behavior simply "return EIO, log to `total_checksum_mismatch`," with no attempt at data-level recovery (there's no mirroring/RAID here, so there may be nothing else to try)? Confirming this is the intended floor for alpha, not an oversight — "won't die easily" is about the filesystem staying mountable and metadata-consistent through this, not about the unreadable bytes coming back.

## 8. C↔Python locking mechanism

You mentioned the C process needs a locking mechanism for the Smart Update Handler path (a series of locks, then the Python script gets the full state via stdin). What's actually being locked — the V-SMART scratch buffer specifically, or something broader? Depends on #1 above if Python's role or the process boundary changes shape.

## 9. Allocator's statistics feed

You said the allocator demands the trend/usage data for its own decisions. Concretely: does the allocator read the Python trend script's *output* (JSON, same shape as the SMART health output), or does it read the raw SMART counters itself and do its own math independent of Python? If the former, is that a file, shared memory, or a direct call — same dependency on #1 as above.

## 10. Recovery Order — does Type-A/Type-B corruption need a fourth track

fs.info §14 has three tracks, all scoped to the anchor region (Superblock/Config/Meta-Table/Function-Table). Given #7 above (checksum failure → EIO, no data recovery attempted), I don't think Type-A/B corruption needs its own recovery track — it's not a structural/anchor failure, it's a data-integrity failure with a defined, simple response. Flagging for confirmation rather than assuming.

---

Answer what you can, and for anything that's a "needs data I don't have" the way the SMART formulae were — say so and I'll research it rather than guess. Once #1 is settled the rest mostly falls into place quickly; it's the one genuinely load-bearing unknown left.
