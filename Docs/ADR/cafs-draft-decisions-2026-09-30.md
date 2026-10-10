# CAFS — Draft Decisions (grouped by function, format-breaking first)
**Date:** 2026-09-30
**Status:** Draft. AI-compiled from this chat, the DeepSeek batch summary and project files. Not canon until Imran confirms.
**Supersedes:** `cafs-state-by-category-2026-09-30.md` (same content, old ordering).

**Tags:** **[I]** Imran stated it (in this chat or the memory notes) · **[DS]** from the DeepSeek batch summary, unverified by me · **[P]** my proposal, unconfirmed · **[?]** open · **[→C]** owed to Claude · **[!]** audit correction.
**Group numbers match** `cafs-research-consolidated` and `cafs-changes`.

---

## HANDOFF — start of next session

**Upload:** `cafs-draft-decisions`, `cafs-changes`, `cafs-research-consolidated` (all 2026-09-30) **plus** `fs-corrected-2026-09-19.info`, `config-corrected-2026-09-19.fs`, `layout-corrected-2026-09-19.rs`, and the ADR JSONs (`ADR1-55.json`, `cafs-adrs-job4-2026-09-19.json`). `allocator.md`, `io_engine.md`, `error_id.md` only when their groups open (G10–G13).
**Not needed:** the batch 3/4 files and the ACL file (folded into the research file); the 09-13 session files (folded into G13/G14 below).

**Settle order (most format-breaking first):**
1. **G1 row** — four calls unblock everything: atime in row or flag-gated; checksum width; nonce location; overflow ID shape (one pair vs bundle).
2. **G2** — Function Table WAL-list layout; Superblock fields; migration stance for 48 B volumes.
3. **G3 WAL** — physical vs logical entries, apply-atomicity model, chunk header, entry format.
4. **G4** — ACL table entry layout; reclaim mechanism; Meta Hash Table shape.
5. **G5 encryption** — scope and key hierarchy (largest open area).
Then G6–G8 (flag bits, hash, redundancy), then G9–G13.

---

# PART 1 — FORMAT-BREAKING

## G1. Identifier row and lifecycle
**Decided**
- **[I]** Row grows. Fixed **128 B**, power of two, set at format time via Superblock `row_size`. Spare bytes stay undefined for future fields. **[I]** "Non-negotiable" partition-resize safety applies; no per-row variable size.
- **[I/DS]** **No generation counter.** Refcount + Recently-Zeroed Queue provide ABA protection, conditional on no NFS export and correct FUSE `FORGET` handling. Keep the reserved bytes.
- **[I]** `extent_limit` lives in the **overflow table**, not the extent-list blob header (standardization 09-30; supersedes the earlier blob-header choice).
- **[DS]** Three u32 IDs in the row: `posix_acl_id`, `nt_acl_id`, `overflow_id` (null = none).
- **[I]** Timestamps: mtime, ctime, btime, each s64 seconds + u32 nanoseconds (12 B). atime is an opt-in flag.

**Row (DS arithmetic, re-checked): 120 B used, 8 B spare — only if atime is in the row**

| Field | B |
|---|---|
| filename_blob_offset | 8 |
| extent_list_offset | 8 |
| extent_count | 4 |
| unix_mode + uid + gid | 12 |
| identifier_refcount | 4 |
| optimization_flags | 4 |
| mtime, ctime, btime, atime | 48 |
| posix_acl_id, nt_acl_id, overflow_id | 12 |
| row checksum (crc32c) | 4 |
| encryption nonce | 16 |
| **Used / spare / total** | **120 / 8 / 128** |

Without atime: 108 used, 20 spare. Direct-index formula becomes `identifier_table_lba + identifier × row_size`.

**[P]** crc32c 4 B (ext4 precedent; torn writes and bitrot need no cryptographic strength). 16 B nonce in the row (a lookup per access if moved out; deriving it is unsafe per the fscrypt lesson). 128 B also fixes the sector-straddling problem 48 B had.
**[?]** atime in row vs flag-gated · checksum width · nonce location (Imran's wording "nonce as file flag, storage in a bit" isn't reconciled with a 16 B row field) · overflow ID: one pair or a bundle of pairs · migration path for existing 48 B volumes (or "only newly formatted volumes get these fields") · Superblock offset for `row_size`.
**[!]** The refcount argument needs one addition: the Recently-Zeroed Queue's "verify refcount 0" step must also check an **in-memory holder count** (kernel lookups, open handles); the on-disk count can't see them.

---

## G2. Function Table and Superblock
**Decided**
- **[I]** ADR-046: `wal_lba` becomes a list of active WAL chunk locations.
- **[I]** `temp_cache_lba` is retired (ADR-057). **[?]** remove outright or leave reserved — lean remove (a reserved field with no purpose documents worse than none). Fold into the format bump.
- **[I]** `namespace_structure_id`@84 confirmed in the 09-13 decisions (drop it from open lists).
- **[I/DS]** Hash algorithm is recorded as an algorithm ID in the Superblock beside `root_hash_algo_id`.
- **[DS]** `format_complete` is a value, not a boolean, somewhat temporary.

**[P]** Superblock flags: bit 0 = pending-freeze, rest reserved (draft item; unconfirmed).
**[?]** WAL list layout: the checksummed 64-byte Function Table prefix can't hold 16–32 slots. Either move the checksum (format change) or keep `wal_lba` as a pointer to a separate chunk-list block. Format version for the bump (fs.info v23 / format 8 provisional).
**[?]** (mkfs proof) proof format (magic vs derived checksum); Superblock location of the proof (reserved field vs flags bit); backup Superblock proof consistency; whether Superblock write needs two phases; whether `hash_table_lba` / `recent_zero_queue_lba` are initialized at mkfs or lazy (lazy needs an explicit per-region marker).

---

## G3. WAL and shared journal primitive
**Decided**
- **[I]** Journals stay **split** per subsystem on a shared primitive (ADR-062); reformat gets its own offline WAL instance. Unifying for its own sake rejected.
- **[I]** Entry integrity per instance: relocation log and reformat WAL → ECC; main WAL → regular checksum. Rule: small high-stakes journals ECC, large high-volume checksum.
- **[DS]** Shape: chained ranges; chunks are arrays; Function Table holds a bounded list of chunk LBAs (16–32 slots) and chunks also chain; chunk size configurable (target ~512 MB, smaller if allocation is constrained); initial WAL block carries chunk header + block header.
- **[DS]** WAL chunks are raw-tagged, tagged per-file as not-snapshotted, allocator-allocated, permit-given, time-limited scratch blocks; refcount stays 1.

**Held by Imran:** chunk header layout; entry format (direct discussion).
**Answerable-now items [DS], 11 not 10:** head/tail semantics; rollover; drain; fsync policy; tail pointer; CHECKPOINT meaning; ANCHOR_DIFF vs anchor commit; scratch/grant integration; recovery start point; chunk integrity uniformity; entry ordering. **Partial:** journal-primitive scope; append protocol; entry retirement.
**[→C] still owed:** (1) apply atomicity, four models (A per-block, B per-entry, C transaction-based, D append-and-forget) with pros/cons; (3) sizing — draft says `region_size` = total active footprint across chunks, so 512 MB × 16–32 slots caps the slot-list footprint at 8–16 GiB unless chunks chain; (4) entry format (Imran direct). **Answered:** revoke case, which area holds the list, current format (research file G2/G3).
**[P]** fixed entry header (magic, type, seq, len, csum) + length-prefixed payload (ANCHOR_DIFF is variable-length, which broke a fixed slot in an earlier audit); linear tail scan stopping at first bad checksum or non-consecutive seq; checksum type in chunk header; seed with `volume_uuid`; chunk header carries first seq.
**[?]** physical vs logical entries (decides whether the revoke hazard exists).
**[!]** "Chunks chain" and "Function Table lists them" are two sources of truth. Name the authority (I'd make the Function Table list authoritative, chaining a cross-check, or drop chaining).

---

## G4. New tables: ACL, overflow, Global Meta Hash Table
**Decided**
- **[I]** ACL table and overflow table are two separate tables on the same blob machinery.
- **[I]** Overflow holds any non-bool per-file attribute that doesn't fit the header; bools go in `optimization_flags`.
- **[I]** Both **reclaim** entries; NTFS-style never-delete explicitly rejected.
- **[I]** Directory-metadata dedup is a separate **Global Meta Hash Table**, not a hash-table mode. Entry format, lifecycle and granularity open; server-tier benefit; default undecided.
**[P]** Reclaim by background scan with a last-touched epoch, not per-operation refcounts (shared-ACL write hotspot). **[DS]** had overflow using refcount + Recently-Zeroed Queue; mechanism for both tables is **[?]**.
**[P]** Ship the Meta Hash Table default-off.
**[?]** ACL table entry layout; where the POSIX ACL physically lives (row / overflow / inline-with-spill / blob); overflow bundle vs pair; what repeats in a directory entry (if (name, identifier), copies differ and hits ≈ hard links); whether ACL and overflow share an entry shape.

---

## G5. Encryption
**Decided:** **[I]** per-file encryption; per-file nonce with a presence flag.
**[P]** keys derived from a volume master key (HKDF-style, context per purpose), content-only scope; never derive from identifier or LBA.
**[?]** nonce location; key hierarchy and storage; whether filenames are in scope; encrypted files' exclusion from cross-file dedup; per-file field layout. The largest remaining open area.

---

# PART 2 — FORMAT-TIME / FLAG-LEVEL

## G6. Hash algorithm and dedup modes
**Decided:** **[I]** hash algorithm is a **format-time** choice: BLAKE3-256 default, SHA-256 selectable; changing it needs the reformat machinery. **[I]** Global Hash Table modes stay `off` / `per-file` / `per-LBA`.
**[?]** `[hash_table]` bucket config and `[dedup_table]` equivalents (coupled to G4); hash-table toggle token names (mechanical).
**[!]** ADR-029 (4-mode hash table) is obsolete → superseding ADR.

## G7. Per-file flags and attributes
**Decided**
- **[I]** Snapshot-of-raw is **per-file** opt-in. `immutable` is a flag (draft: **two** — Immutable Data, Immutable Meta; neither binds physical location).
- **[I]** `raw` is an **advisory bundle**: it suggests flag values, the tag is computed and re-evaluated, users can edit any bundled flag. WAL chunks are the explicit exception (raw-tagged, never snapshotted).
**[?]** Whether both immutable flags set auto-applies raw — draft makes it a config option. **[P]** keep raw separate by default (auto-forcing silently drops the file from snapshots and from dedup). Check that nothing assumes raw blocks always have refcount 1.
**[?]** bit assignments in `optimization_flags`; DOS attributes (hidden/system/archive/read-only) have no stored home.

## G8. Redundancy and integrity
**Decided:** **[I]** zone ECC is a plain **opt-in toggle on any media** (no auto-disable, no special SSD case); ECC for relocation log and reformat WAL; main WAL uses checksums.
**[?]** warning mechanism, if any; interaction with the 16 B checksum trailer on every block (layered, or does one subsume the other in protected zones); explicit scope statement: zone ECC protects localized media defects, not whole-device failure; ADR-065 revision.
**[P]** make byte-identical redundant copies differ (Superblock vs Backup, SMART main vs backup) so a deduping SSD controller can't merge them; confirm they're actually identical. Parity survival under firmware dedup is untested.

---

# PART 3 — PROCESS / RUNTIME

## G9. Relocation, resize, reformat, mkfs
**Decided**
- **[I]** Safe resize/relocation is non-negotiable. Reformat = changing format-time data without losing data, reusing the block-copy/move primitive; offline; own WAL instance. A hash-algorithm change is not a block move.
- **[DS]** ADR-061 relocation log entry: file hash, old logical LBA, old physical LBA, new destination, ECC (locked).
- **[DS]** mkfs: allocator scope = everything except the LBA0–5 anchor pointers; bootstrap "LBA0–5 used, rest free"; reformat mode reads the existing Zone Table; the allocator allocates the root B-tree; initial WAL = one block; verify (light fsck / mount steps) first, then set format-complete; FAT32 rescue default OFF; no resize headroom (rely on the block-move primitive).
**[?]** mount-check frequency; verification scope; first-append behaviour after mkfs; DS's 14-step write order and minimal-set list (I can't see them).
**[P]** minimal mkfs set: anchor region, Superblock, Config Snapshot + Parity, Function Table, SMART main/backup, root row, B-tree root, WAL chunk header; format-time values `row_size` and hash algorithm.
**[?]** chunk-level record for whole-volume shifts; whether addresses are partition-relative (if so, moving the partition needs no pointer rewrites).
**[!]** "mkfs fails → rerun, no atomicity" is safe for a **fresh** format only; a data-preserving reformat needs its own WAL (already decided).

## G10. ACL and permission semantics
**Decided**
- **[I]** Both ACL forms stored natively and authoritative for their platform. Config selects mapping: `none` / `posix_to_nt` / `nt_to_posix` / `bidirectional`.
- **[I]** NT default: if the Linux permission requires sudo/root, the NT side defaults to admin-level permission (name TBD).
- **[I]** Permission checks delegated: `default_permissions` + `FUSE_POSIX_ACL` on Linux; WinFsp `AccessCheck` on Windows. The allocator owns state rejections only. Access check ≠ grant.
**[P] (listed as decided in the DS summary — confirm):** null NT ID = computed from mode; POSIX→NT is the safe direction; NT→POSIX fails closed.
**[?]** WinFsp uid 0 (held; source suggests `S-1-5-0`, not Administrators); idmap / user map design and SID-mapping consistency; config granularity (volume / directory / file / cascade); bidirectional conflict handling; default name; whether a file always has both fields or only as needed (my read: as needed); #28 case list [DS] (emergency mode, out of space, raw/snapshot conflict, grant boundary, cross-platform ACL divergence, immutable flags, mount-time state, recovery in progress) and the allocator-vs-master ownership boundary.
**[!]** The Batch 1 line "Default `none`" contradicts the later ACL default and Imran's own NT-admin rule.

## G11. Errors and Event IDs
**Decided**
- **[I]** Identity: POSIX name where Linux has an exact one (ENOSPC when the drive is genuinely 100% full); `CA_Name` otherwise (Imran wrote `CA-Name`); numbers only for errors too obscure to name.
- **[I]** Three layers: syscall return (errno / NTSTATUS), diagnostic identity, message text. Windows numbers are a different mapping; users read the application's error message.
- **[I]** Event IDs capped at 65,535. Ranges are **advisory** until implementation: I/O Engine 10,000–19,999; Allocator 20,000–34,999; Process + Rust 35,000–54,999; Python 55,000–59,999; reserved 60,000–65,535. A range is a category (space-related; relocation; permissions + grants + concurrency as one family; core allocation).
**[?]** migration of `CA-1001`–`1003`; `CA-` vs `CA_`; `error_id.md` is stale (4-digit, 1000–4999).
**[!]** Every logged Windows event needs a numeric ID, so "numbers only for obscure" must mean *user-facing* numbers; the per-platform mapping table assigns one to every named error.

## G12. TRIM and Windows kernel helper
**Decided:** **[I]** narrow kernel-mode TRIM relay driver (user mode can't issue the trim action); kernel development per `io_engine.md` constraints; full Windows kernel filesystem driver is post-V1.
**[?]** TRIM relay re-audit against every new free event (needs the ADR set; checklist in the research file G12).

## G13. I/O Engine mechanism gaps (09-19 concerns file, untouched since)
**[?]** permit table size; permit-validation failure (error variant and `CA-` code; retry or fail); `CafsCtx` concurrency beyond the counters (shared Dedup/Hash tables need locking; recommend a lock per cached table); misaligned write + permit as one unit (one permit for the bounce-buffer read and write); checksum-mismatch handling on an ordinary read (read the PoC's existing corruption/auto-repair path first); `heat_caches()` at scale (size threshold for partial heat-up); identifier-slot reuse via the permit-table pattern (plausible, unconfirmed); `syscalls.rs` counter wiring.

---

# PART 4 — DEFERRED
**[I]** Workload-dependent settings batch: each documents which workloads it serves, the automatic system's decision logic, user override, and where "production" vs "server" practicality draws. Deferred to the final batch.
**[I]** Copy-operation "fork philosophy" (don't change until it differs): deferred. Open sub-question: do copies share the identifier until modified, or get a new identifier and share only extents (a shared identifier makes a copy look like a hard link, and open handles change meaning on first write).
**09-13 open items still standing:** Tasker's final shape; `avg` mode formula; NVMe tasker cadence; scratch structures (`fsck_scratch_lba` resumable repair state unconfirmed; `scratch_lba` purpose unknown); hard-floor scope (global vs zone) and severity gradient; "Balanced" preset daemon-vs-library default; lock granularity; migration handler identity; relocation-log execution owner; consensus-window agreement range; grant-revocation mechanism; placeholder thresholds (`health_composite` `warn_at`, `pending_trajectory`, `write_size_cv`, EWMA α constants, A0–A5 preset cutoffs); research gaps for WinFsp trim and ntfs-3g locking.

---

## Audit corrections to the DeepSeek batch summary (one place)
1. Batch 4 was delivered (`cafs-research-batch4`); ZFS `copies`, overlapping-move and firmware-dedup research exist (consolidated file G8, G9).
2. My proposals were recorded as decisions: computed-until-set, POSIX→NT direction, NT→POSIX fail-closed.
3. "Default `none`" contradicts the later ACL default.
4. "mkfs fails → rerun" must be scoped to fresh format.
5. "10 answerable" WAL items are 11; chunk chain + Function Table list need one authority.
6. Row 120/8 depends on atime living in the row.
7. Refcount sufficiency also needs the in-memory holder count.
8. "Numbers only for obscure" needs a numeric Windows ID per named error.
9. `extent_limit` is in the overflow table.
10. Proposals that were acknowledged ("Fair") without being explicitly adopted stay tagged [P] here until Imran restates them.
