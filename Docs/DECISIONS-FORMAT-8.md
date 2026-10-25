# CAFS — Format 8 decisions log (PoC revision)

**Status:** CURRENT (living document)
**Updated:** 2026-10-24
**Replaces:** `ADR/cafs-changes-2026-09-30.md`, `ADR/cafs-draft-decisions-2026-09-30.md`
**Companion:** `OPEN_ITEMS.md` (everything unresolved lives there, not here)
**Source of truth for the format itself:** `fs.info`. This file records *decisions and
the spec edits they require*; once an edit lands in `fs.info`, mark it APPLIED here.

**Tags:** **DECIDED** = stated by Imran · **PROPOSED** = Claude's proposal, not yet
confirmed (stays PROPOSED until restated) · **SHELVED** = decided, then deferred ·
**APPLIED** = already written into the spec.

---

## 1. Scope and spec bar

- **DECIDED** This revision is the **PoC**. PoC is not Beta, so the on-disk format may
  still change afterwards. Accounting correctness (refcount/CoW) can only be proven by
  practical testing, so the PoC is built to test it.
- **DECIDED** Spec bar ("clear conscience"): detailed enough to avoid format hiccups or
  surprises, without taking long. A format-breaking decision is fine when the change
  itself is mostly defined.
- **PROPOSED** Three tests per format-breaking item: (1) every byte touched is defined or
  reserved-zero; (2) a later change is detected, not silently misread; (3) nothing else
  depends on an undecided detail of it.
- **PROPOSED** Conventions that make those tests cheap: **zero means default / none**
  for every reserved byte, flag and ID; ext4-style `feature_compat`,
  `feature_ro_compat`, `feature_incompat` words in the Superblock; mount refuses an
  unknown incompat bit or unsupported `format_version`.
- **PROPOSED** Version bump: `fs.info` v22 -> v23, `FORMAT_VERSION` 7 -> 8, one revision.
  Old 48 B volumes are refused at mount (no migration; project is pre-Alpha).

## 2. Cross-cutting decisions (2026-10-21 .. 2026-10-24)

| # | Decision | Status |
|---|---|---|
| X1 | The PoC keeps the **table-mode Identifier row** and the **`identifier_refcount`** system. | DECIDED 10-22 |
| X2 | **Hard links are dealt with after the PoC.** PoC refuses `link()` (ENOTSUP); directory `nlink` is synthesized. | DECIDED 10-22 (refusal behaviour PROPOSED) |
| X3 | **Snapshot reads are out of PoC scope.** The Snapshot Table stays in the format, untested. PoC targets CoW redirect, dedup, refcounts and the WAL. | DECIDED 10-22 |
| X4 | Tree mode (btrfs-style: inode records, extents and names as tree items, RAM-only index cache with a config knob) is **SHELVED** to post-PoC. Analysis kept in section 7. | SHELVED (decided 10-21, deferred 10-22) |
| X5 | Anchor-region structures use a **fixed format independent of `block_size`**; a fixed format is required to find the active config. | DECIDED 10-22 |

## 3. Group decisions

### G1 — Identifier row (APPLY to `fs.info` section 7a, `layout.rs`)

- **DECIDED** Row is **256 B by default**, power of two, format-time `row_size`
  (Superblock). No per-row variable size. Spare bytes are reserved-zero.
- **DECIDED** No generation counter. Identifier safety rests on refcount + the
  Recently-Zeroed Queue. **PROPOSED** the queue's "refcount 0" check also consults an
  in-memory holder count (kernel lookups, open handles), and the "no NFS export"
  assumption is written down.
- **DECIDED** `extent_limit` lives in the overflow table, not the extent-list blob header.
- **DECIDED** Timestamps: mtime, ctime, btime (s64 + u32 ns); atime in the row; atime is
  an opt-in flag. Nonce is 16 B in the row. crc32c, 4 B.
- **DECIDED (accepted 10-22)** PoC row:

| Group | Fields | Bytes |
|---|---|---|
| Locate | filename_blob_offset 8, extent_list_offset 8, extent_count 4 | 20 |
| POSIX | unix_mode 4, uid 4, gid 4, size 8, nbytes 8, nlink 4 (fixed 1 in PoC) | 32 |
| Lifetime | identifier_refcount 4, optimization_flags 4 | 8 |
| Time | mtime, ctime, btime, atime (s64 + u32 each) | 48 |
| Shared refs | posix_acl_id, nt_acl_id, overflow_id (u32, 0 = none) | 12 |
| Crypto | nonce | 16 |
| Integrity | self-identifier (8), crc32c (4) | 12 |
| **Total** | | **148 of 256 (108 spare)** |

  *Correction (2026-10-24): an earlier chat table summed the POSIX group as 36 B and the
  total as 152. The correct sums are 32 and 148.*

- **PROPOSED** self-identifier = the row stores its own number (XFS v3 style) to detect
  misdirected writes and wrong slots. **PROPOSED** crc32c covers the whole row with the
  crc field zeroed, selected by `row_checksum_algo_id`.
- **Spec edits:** row size/formula (`identifier_table_lba + identifier * row_size`);
  field table above; `optimization_flags` is **not** in v22's row (config.fs claims flags
  live "in inodes" — fix that comment); ADR-018 amend; ADR-047 amend (no generation
  counter, holder count); ADR-024 superseded by the dual-ACL ADR (`acl_blob_offset`
  removed); new ADR "Identifier row 256 B, format-time `row_size`"; new ADR timestamps.

### G2 — Function Table and Superblock (APPLY to `fs.info` sections 3, 4, 8, 13; `layout.rs`)

- **DECIDED** ADR-046: `wal_lba` becomes a list of active WAL chunk locations.
- **DECIDED** `temp_cache_lba` is retired (ADR-057). **PROPOSED** remove outright.
- **DECIDED** `namespace_structure_id` @84 confirmed in the spec text.
- **DECIDED** Anchor structures are **4096 B, 4096-aligned**; pointer blocks LBA0-3 are
  padded to 4096 (anchor region 16 KiB -> 24 KiB). All anchor writes go through one
  function that patches the RAM image, recomputes the checksum and writes the whole
  4096-aligned unit, so the drive never does a partial-sector read-modify-write.
- **DECIDED** Content updates use **atomic pointer replace**: the new image goes to a
  temp/alt LBA held in the start and end anchor pointers, then the pointers flip
  (end-of-device copy first, then primary).
- **DECIDED** The Superblock is updated **at checkpoints** through the normal system.
- **DECIDED** The Function Table's WAL list is updated **on a cadence**; the WAL has a
  known format, so chunks stay recognizable if the list is stale or damaged.
- **DECIDED** Sector trust: the scheme is trusted only if the Superblock-recorded sector
  size matches what the OS reports. A mismatch **always warns**. Override at two levels,
  **format-time and mount-time; mount-time wins**.
- **PROPOSED** Pointer blocks gain `generation` (u64) and `alt_lba` (u64), covered by the
  checksum. Recovery takes the newest generation whose pointer and image checksums
  verify. The alt image is a previous generation: for diagnosis and fsck rebuild, never
  automatic trust (it can point at freed structures).
- **PROPOSED** Function Table becomes a **K-slot ring** (K format-time, default 8; K = 1
  means no rotation). Its pointer block never changes after mkfs and `real_size` =
  K x 4096. Each update writes slot (generation mod K); recovery takes the highest valid
  generation. Rationale: SD/USB/eMMC wear (SSDs remap writes).
- **PROPOSED** Function Table slot layout (4096 B):

| Offset | Field |
|---|---|
| 0-39 | 5 x u64: smart_main, smart_backup, scratch, hash_table, recent_zero_queue |
| 40-47 | `wal_head_hint` (absolute LBA; lazy hint, written every N blocks and at unmount; may lag, never lead) |
| 48-51 | `wal_chunk_count` u32 |
| 52-55 | reserved |
| 56-439 | 32 x (LBA u64, length-in-blocks u32), oldest first; a hint, not authority |
| 440-447 | `generation` u64 (ring) |
| 448-4079 | reserved, zero |
| 4080-4095 | BLAKE3-128 over bytes 0-4079 |

- **PROPOSED** Superblock additions (offsets 0-87 untouched; `format_version` = 8):

| Offset | Field |
|---|---|
| 88-91 | `row_size` u32 (256) |
| 92-95 | `row_checksum_algo_id` u32 |
| 96-119 | `feature_compat`, `feature_ro_compat`, `feature_incompat` (3 x u64) |
| 120-123 | `format_complete` u32 (0 incomplete, 1 complete; a value, not a boolean) |
| 124-127 | `logical_sector_size` u32 (recorded at format) |
| 128-131 | `physical_sector_size` u32 (recorded at format) |
| 132-135 | `sector_trust_mode` u32 (0 auto-compare, 1 force trust, 2 never trust) |

- **PROPOSED** Mismatch policy: if physical and logical are both <= 4096, allow
  read-write with read-back verification of every anchor write; if physical > 4096,
  mount read-only unless overridden.
- **Spec edits:** fix the stale "format_version = 6" text in section 4; hash-algorithm ID
  field (see OPEN_ITEMS: `root_hash_algo_id` @80 exists, decide whether it is the dedup
  hash selector); ADR-046 wording (`region_size` = total active footprint across chunks);
  ADR-057 outcome; section 13.3 in-place update retired in favour of pointer replace
  and the ring.

### G3 — WAL (analysis delivered 2026-10-24; awaiting Imran's acceptance)

- **DECIDED** Journals stay split per subsystem on a shared primitive (ADR-062); reformat
  gets its own offline WAL instance. Relocation log and reformat WAL use ECC; main WAL
  uses checksums.
- **DECIDED** WAL chunks are raw-tagged, allocator-allocated scratch, never snapshotted,
  refcount 1; chunk size configurable (target ~512 MB).
- **Flaws in `fs.info` sections 9-10 as written** (all PROPOSED fixes below):
  1. Appends rewrite a block that already holds durable entries (torn write loses
     acknowledged entries); the old tail is also rewritten to set `next_lba`.
  2. One checksum per block cannot tell a torn tail from corruption, and section 10.2
     aborts to fsck on a corrupt block.
  3. "Apply idempotently" is only true if entries are **absolute** (set refcount of
     block B to 2), never deltas. `last_applied_wal_seq` now lags by design (G2), so
     replay will repeat entries.
  4. ANCHOR_DIFF is obsolete after G2 (anchors use whole-image writes, rings, pointer
     replace); dropping it removes the variable-length payload.
  5. Stale-tail resurrection: old entries beyond a new tail can keep consecutive
     sequence numbers and valid checksums. Fix with an **epoch** that increments at each
     recovery; checksums seeded with `volume_uuid` + epoch; epoch never decreases along
     the log.
- **PROPOSED** Entries address **stable names** (table + index, or tree + key), never
  reusable block LBAs, so the revoke hazard cannot occur. Full-image entries only for
  fixed-slot tables (Identifier Table etc.).
- **PROPOSED** Apply model **C (transaction)**: a run of entries ending in a `last` flag
  is atomic; recovery drops an incomplete tail run. Write unit = sealed, sector-sized
  **group**, written once and never rewritten; a small transaction = one atomic sector
  write.
- **PROPOSED** Chain links point **backward only** (`prev_chunk_lba`, written once);
  forward order comes from the sequence. Function Table list = hint; backward chain =
  truth. Recovery rediscovery if the list is lost: bounded scan for the chunk magic.
- **PROPOSED** Sketch: group header {magic, epoch, first_seq, entry_count, flags,
  checksum}; entry {type, flags, payload_len, payload}; chunk header {magic, chunk_id,
  first_seq, prev_chunk_lba, creation_epoch, group_size, checksum_algo_id, length,
  checksum}.
- **Models, for the record:** A per-block, B per-entry, C transaction, D
  append-and-forget (definitions are Claude's reading; the draft never defined them).

### G4 — New tables (ACL, overflow, Global Meta Hash Table)

- **DECIDED** ACL table and overflow table are separate, on the same blob machinery.
- **DECIDED** Overflow holds any non-bool per-file attribute that does not fit the header
  (`extent_limit` first); bools go in `optimization_flags`.
- **DECIDED** Both reclaim entries; NTFS-style never-delete rejected.
- **DECIDED** Directory-metadata dedup is a separate **Global Meta Hash Table**, not a
  hash-table mode (ADR-029 superseded by a new ADR; modes stay off/per-file/per-LBA).
- **PROPOSED** PoC reserves table-pointer slots in the Meta/User Table (it has reserved
  bytes) and leaves IDs null (NT ACL computed from mode). Entry layouts, reclaim and
  bundle-vs-pair move to OPEN_ITEMS. Meta Hash Table ships default-off.

### G5 — Encryption

- **DECIDED** Per-file encryption with a per-file nonce and a presence flag.
- **PROPOSED** Content-only scope; keys derived from a volume master key (HKDF-style),
  never from identifier or LBA. The PoC reserves a null `key_block_lba` and the flag bit.
  Everything else (hierarchy, key storage, filename scope, dedup exclusion) is open.

### G6-G8 — Flags, hash, redundancy

- **DECIDED** Hash algorithm is format-time (BLAKE3-256 default, SHA-256 selectable);
  changing it needs the reformat machinery. Global Hash Table modes stay
  `off` / `per-file` / `per-LBA`.
- **DECIDED** Snapshot-of-raw is a per-file opt-in; `raw` is an advisory bundle; WAL
  chunks are the explicit non-snapshot exception; two immutable flags (data, meta).
- **DECIDED** Zone ECC is a plain opt-in toggle on any media; relocation log and reformat
  WAL use ECC.
- **PROPOSED** `optimization_flags` is a 32-bit word, zero = default, bits assigned later.
  ECC stays off in the PoC (avoids the ECC x 16 B-trailer layering question).

### G9-G13 — Process/runtime (no on-disk impact beyond what is listed)

- **DECIDED** Safe resize/relocation is non-negotiable; reformat is offline with its own
  WAL; ADR-061 relocation-log entry confirmed; mkfs scope per the 09-30 draft (allocator
  scope = everything except LBA0-5; verify, then set `format_complete`; FAT32 rescue
  default off).
- **DECIDED** ACL: both forms stored natively, config selects mapping (`none`,
  `posix_to_nt`, `nt_to_posix`, `bidirectional`); NT default is admin-level when Linux
  requires root; checks delegated to `default_permissions` + `FUSE_POSIX_ACL` / WinFsp
  `AccessCheck`.
- **DECIDED** Errors: POSIX name where exact, `CA_Name` otherwise (Imran wrote `CA-Name`;
  spelling open), numbers for obscure ones; three layers; 16-bit IDs; advisory ranges
  (I/O Engine 10,000-19,999; Allocator 20,000-34,999; Process + Rust 35,000-54,999;
  Python 55,000-59,999; reserved 60,000-65,535).
- **DECIDED** TRIM: narrow Windows kernel helper (user mode cannot issue the trim action);
  full Windows kernel filesystem driver is post-V1.

## 4. Known spec problems that exist regardless of the plan above

| Problem | Where | Plan |
|---|---|---|
| Names live only in the identifier row, one per identifier; directory trees are hash-only. Hard links get no per-link name; readdir needs a row read per entry. | `fs.info` 7a/7b, ADR-034 | Deferred with hard links (X2) |
| Snapshots save only the B-tree root while rows are mutated in place, so a snapshot cannot preserve a modified file. | `fs.info` 7e-7g, ADR-030/031 | Out of PoC scope (X3); post-PoC options in section 7 |
| ADR-031 triggers CoW by `refcount > 1`; a count that is too low means an in-place overwrite of a shared block. | ADR-031 | PROPOSED rule: committed blocks are immutable; in place only if born this transaction and count = 1. PoC audit tests count accuracy. |
| Per-block refcounts make "first write after snapshot" O(blocks). | `fs.info` 7c | Deferred: zone-table granularity (per block / per extent / birth epochs) |

## 5. Licensing and platform decisions (details in `Research/LICENSE_RESEARCH.md`)

- **DECIDED** Userspace and shared code: BSD-3-Clause. `Src/Kernel` (Linux and Windows
  helper): GPL-2.0-only. README should say "`Src/Kernel`" rather than "the kernel folder".
- **DECIDED** Linux is userspace only (FUSE) for now. Windows is userspace via WinFsp.
  The TRIM helper is optional, a separate binary, and **unsigned**; no Microsoft contract.
- **DECIDED** Distribution: a zip installer or a GitHub binary branch that an installer
  reads. Kernel code is meant to be public so company improvements reach users.
- **PROPOSED** `trim_mode` config with per-platform backend; `reserve_percent` never
  allocates the tail of the device (works without any driver).
- **OPEN** `winfsp` crate (GPL-3.0) vs own FFI vs C shim — see OPEN_ITEMS.

## 6. Spec-edit checklist (consolidated; tick when APPLIED)

| File | Edit | Group |
|---|---|---|
| `fs.info` 7a | Row layout, `row_size`, formula, field table, remove generation language | G1 |
| `fs.info` 3, 4, 8, 13 | Anchor 4096 rule, pointer blocks, alt/generation, Function Table ring/layout, Superblock additions, retire 13.3, fix version text | G2 |
| `fs.info` 9-10 | WAL rewrite per G3 once accepted | G3 |
| `fs.info` new | ACL table, overflow table sections (reserved pointers for PoC) | G4 |
| `fs.info` 18 | Reconcile known-gaps list with `OPEN_ITEMS.md` | all |
| `layout.rs` | `SB_OFF_*` new fields, `FORMAT_VERSION` 8, Function Table constants | G1, G2 |
| `config.fs` | `[cow]`/hash/flags comments, `sector_trust`, `trim_mode`, `reserve_percent`, ring K | G2, G6, G7 |
| `io_engine.md` | `mntproc` is Rust wording; ACL negotiation wording; permit items | G10, G13 |
| `error_id.md` | Rewrite scheme (stale 4-digit ranges) | G11 |
| `allocator.md` | `CA-1002` extent-limit path reads overflow table | G4 |
| ADRs | Amend 018, 046, 047, 055, 065; supersede 024 (dual ACL), 029 (Global Meta Hash Table); new ADRs for row, timestamps, dual ACL, anchor 4096 rule, pointer replace, sector trust, WAL model | all |
| ADR store | Import ADR 056-065 (currently only in `cafs-adrs-job4-2026-09-19.json`, not in the repo) | — |

## 7. Shelved analysis (post-PoC direction, not binding)

- **Tree mode (btrfs-style).** Inode items, directory entries (with names), extents and
  per-file extras become typed items keyed (identifier, type, offset) in one CoW tree, so
  a snapshot's saved root captures file state and `identifier_refcount` disappears.
  Identifiers become a monotonic u64 (no reuse, no generation question). The on-disk
  Identifier Table is replaced by a RAM-only index cache (`index_cache = off | pointer |
  row`, filled lazily, cleared on snapshot switch). Costs: O(log n) opens, CoW write
  amplification on attribute updates, larger corruption blast radius.
- **Alternative for the direct table.** Keep it, add a `generation` field and a Row
  Version overlay (copy the old row on first change after a snapshot); reversal becomes a
  walk. Novel combination; higher risk.
- **Per-block transid in the Zone Table** (birth stamp, about 4 B per block): enables
  snapshot-sharing checks without refcount bumps and a changed-block scan.
- **Extent stamps** (per-write stamp in extent items, trailer checksum seeded with it) so
  a reference vouches for its block; an owner check that today's in-block trailer cannot
  give.

## 8. Design history (kept on purpose; not current design)

- **Type-A / Type-B zones (hot / cold recording), dropped 2026-10-24.** The August 2026
  design recorded data in two ways: **Type-A**, fixed 2 KiB blocks written first (the
  "hot" path, protected by a fast Tier-1 CRC32C checksum), and **Type-B**, variable-length
  extents produced later by a background handler (the "cold", space-optimized form,
  protected by a stronger Tier-2 checksum such as xxhash64 or BLAKE3). Type-B was defined
  as background-optimized Type-A, so nothing was ever uncovered by a checksum. It was
  dropped because the current spec uses a uniform `block_size` with a 16-byte checksum
  trailer on every block, and no current document refers to Type-A/Type-B. It may return
  if hot/cold recording is revisited, in which case the checksum-coverage argument above
  is the part worth reusing. Sources: `cafs-closeout-2026-08-29-night`,
  `cafs-io-engine-questions-2026-08-30`, `cafs-session-evaluation-2026-08-29`; recover
  them from the `docs-pre-cleanup` git tag.
- **Assembly I/O Engine (2026-08-08 spec), dropped.** The first I/O Engine spec called for
  an assembly implementation with a C ABI, a block-index LBA, and a 4-byte per-block
  checksum. The engine is now Rust and the format is the one in `fs.info`. Still-valid
  rules were salvaged into `io_engine.md` v2; recover the original from the
  `docs-pre-cleanup` tag.
