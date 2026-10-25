# CAFS — Open items

**Status:** CURRENT (living document)
**Updated:** 2026-10-24
**Replaces the open lists in:** `JOB3-OVERVIEW`, `cafs-closeout-2026-08-29-night`,
`cafs-roadmap-2026-08-29`, `cafs-io-engine-questions-2026-08-30`,
`cafs-session-evaluation-2026-08-29`, and the unresolved parts of
`cafs-draft-decisions-2026-09-30`. `fs.info` section 18 should point here instead of
keeping its own copy.
**Rule:** when an item closes, move it to section F with its resolution. Never delete one
silently, and never open a second list elsewhere.

**Needed by:** **PoC-format** = must settle before format 8 freezes · **PoC-impl** = needed
to build the PoC · **Post-PoC** · **Later**.

---

## A. Blocks the PoC format

| ID | Item | Needed by | Notes |
|---|---|---|---|
| A1 | Function Table: confirm 4096 B layout, WAL slot count (32), ring K (default 8) | PoC-format | Proposed in `DECISIONS-FORMAT-8` G2 |
| A2 | Superblock additions: confirm field list and offsets (88-135) | PoC-format | `row_size`, `row_checksum_algo_id`, feature words, `format_complete`, sector sizes, `sector_trust_mode` |
| A3 | `pending-freeze` flag: what it means (my read: "format not frozen yet", better as an incompat feature bit) | PoC-format | |
| A4 | `root_hash_algo_id` @80 exists in v22: is it the dedup hash selector, or does G6 need a new field? | PoC-format | `fs.info` section 4 |
| A5 | Sector-trust mismatch default: read-write with read-back verify if physical <= 4096, else read-only unless overridden | PoC-format | Override levels are decided |
| A6 | Accept WAL apply model C (transaction runs in sealed sector-sized groups), logical-only addressing, epoch, absolute entries, backward-only chain | PoC-format | Held while docs are cleaned |
| A7 | WAL byte layouts: group header, entry, chunk header; CHECKPOINT meaning; `region_size` sizing; head-hint cadence; rediscovery scan bounds; `last_applied_wal_seq` semantics; replay-idempotency rule | PoC-format | Depends on A6 |
| A8 | Row: confirm self-identifier, crc32c coverage, `row_checksum_algo_id` default, whether to reserve a `generation` slot for post-PoC versioning, atime update semantics | PoC-format | |
| A9 | Overflow table: bundle vs pair (the row's `overflow_id` is a u32 either way) | PoC-format | |
| A10 | Meta/User Table: table-pointer slots for ACL, overflow, Meta Hash, key block; confirm free reserved bytes | PoC-format | |
| A11 | ACL in the PoC: confirm null IDs (NT computed from mode); where the POSIX ACL physically lives | PoC-format | |
| A12 | Encryption: reserve the presence flag and null `key_block_lba`; hierarchy, key storage, filename scope and dedup exclusion stay open | PoC-format | |
| A13 | `optimization_flags`: 32-bit word, zero = default; bit assignments; DOS attributes (hidden/system/archive/read-only) have no stored home | PoC-format | |
| A14 | ECC off in the PoC; ECC x 16 B trailer layering decided later | PoC-format | |
| A15 | Feature words (`compat`/`ro_compat`/`incompat`) and refuse-unknown-bits mount rule: confirm | PoC-format | |
| A16 | Old 48 B volumes: refuse at mount, no migration | PoC-format | |

## B. Mechanical spec and repo fixes (no decision needed)

| ID | Item | Where |
|---|---|---|
| B1 | Fix "format_version = 6" text | `fs.info` section 4 |
| B2 | Import ADR 056-065 into the ADR store (the job4 JSON is not in the repo) | `ADR/` |
| B3 | Rewrite the error scheme (stale 4-digit ranges); decide `CA-` vs `CA_`; migrate `CA-1001`-`1003` | `error_id.md` |
| B4 | Remove "flags stored in inodes" claim until the row actually carries `optimization_flags` | `config.fs` |
| B5 | Wording: `mntproc` is Rust; add `FUSE_POSIX_ACL` negotiation | `io_engine.md` |
| B6 | `CA-1002` extent-limit path reads the overflow table | `allocator.md` |
| B7 | Move format code (`layout`, `anchor`, `checksum`, `error`) out of the Linux x86-64 crate into a shared top-level crate; `mkfs_cafs.rs` still reimplements `blake3_128`; add a Rust/C layout conformance test | `Src/` |
| B8 | `layout.rs` `FORMAT_VERSION` to 8 | `layout.rs` |
| B9 | README: say `Src/Kernel` not "the kernel folder"; WinFsp notice; `THIRD-PARTY-NOTICES`; SPDX headers and a `LICENSES/` directory | repo root |
| B10 | Extend `check.py`: flag "LOCKED"/"Accepted" outside the spec files and ADR store; flag duplicate hashes; add the semantic cross-document layer; show a diff before `--update` accepts changes | `check.py` |
| B11 | Check the **[verify]** items in `io_engine.md` v2 (salvaged from the 08-08 spec) against `block_io.rs`: operation list and flags, scratch-buffer ownership, retry rules, counter batching numbers, 32-bit x86 scope | `io_engine.md` |
| B12 | Define the unit of an LBA in one place in `fs.info` (the old engine spec says block index; the anchor table lists byte offsets for slots) | `fs.info` |

## C. Post-PoC design

| ID | Item | Notes |
|---|---|---|
| C1 | Hard links and names: names live only in the row (one per identifier); directory trees are hash-only | ADR-034; tree-mode fix in `DECISIONS-FORMAT-8` section 7 |
| C2 | Snapshot versioning of rows/extents (tree mode vs Row Version overlay) | |
| C3 | Zone Table refcount granularity (per block, per extent run, or birth epochs); per-block transid | |
| C4 | CoW safety: adopt "committed blocks are immutable" vs ADR-031's `refcount > 1` trigger | |
| C5 | Accounting audit tool (recompute counts from all reachable extents, diff against the Zone Table) and a power-failure simulation run after every operation | The PoC's central test |
| C6 | Open-unlinked files after a crash (orphan handling); holder-count semantics | ADR-047 |
| C7 | Copy-operation "fork" philosophy: do copies share the identifier until modified? | Deferred by Imran |
| C8 | Metadata dedup | Deferred 2026-09-08 |
| C9 | B-tree rebalancing (implementation question) | `fs.info` 7b |

## D. Runtime, process and format-detail items

| ID | Item |
|---|---|
| D1 | Hash Table bucket/entry layout (toggle and 256-bit width are decided) |
| D2 | Recently-Zeroed Queue entry format, including the retired-identifier tag |
| D3 | Snapshot Table `retention_tier` encoding |
| D4 | GPT partition GUID question; FAT32 rescue partition sizing and trigger (ADR-028) |
| D5 | Purpose of `fsck_scratch_lba` and `scratch_lba`; `uuid_sequence`, `volume_label`, `volume_label_strict` treated as inert passthrough |
| D6 | mkfs: mount-check frequency, verification scope, first-append behaviour, DeepSeek's write order (not visible), chunk record for whole-volume shifts, partition-relative addresses |
| D7 | ACL semantics: WinFsp uid 0 mapping, idmap design, config granularity, bidirectional conflicts, default name, both fields vs as-needed, the #28 case list, allocator-vs-master boundary; reconcile the "default `none`" line with the NT-admin rule |
| D8 | TRIM: re-audit against new free events; define `trim_mode` and `reserve_percent`; test user-mode ATA/NVMe pass-through on Windows before building the helper; some USB bridges drop TRIM |
| D9 | I/O Engine gaps: permit table size, permit-validation failure (variant, retry or fail), `CafsCtx` locking, misaligned write + permit as one unit, checksum-mismatch handling on ordinary reads, `heat_caches()` threshold, identifier-slot reuse via the permit pattern, wiring I/O counters |
| D10 | 09-13 items: Tasker final shape, `avg` formula, NVMe tasker cadence, hard-floor scope and severity gradient, "Balanced" preset daemon-vs-library default, lock granularity, migration handler identity, relocation-log owner, consensus-window range, grant revocation, placeholder thresholds (`health_composite`, `pending_trajectory`, `write_size_cv`, EWMA alphas, A0-A5 cutoffs), ntfs-3g locking research |
| D11 | Workload-dependent settings documentation (final batch) |
| D12 | Global Meta Hash Table: entry format, lifecycle, granularity, default |
| D13 | Windows Event ID mapping for every named error |
| D14 | Config: `[dedup_table]` bucket count, hash-table toggle token names, `flag_group` placement, option for both immutable flags forcing `raw` (proposed default off) |
| D15 | Redundancy: byte-identical backup copies vs deduping controllers; ECC warning mechanism; ADR-065 revision; audit assumptions that raw blocks have refcount 1 |

## E. Licensing and platform (see `Research/LICENSE_RESEARCH.md` section 6)

| ID | Item |
|---|---|
| E1 | `winfsp` crate (GPL-3.0) vs own FFI vs thin C shim for the Windows glue |
| E2 | Are the KMDF stub libraries in the WDK's REDIST list? (matters for a GPL-2.0-only helper) |
| E3 | Read GPL-2.0 section 3 for the binary-branch release process |
| E4 | Does a bundled Microsoft runtime breach WinFsp's "no proprietary software" condition? (link the CRT statically) |
| E5 | Licence of any WDK sample you copy |
| E6 | Contribution policy before the first PR (DCO sign-off, inbound = outbound per folder, relicensing right) |
| E7 | Clean-room rule: no verbatim GPL code in the BSD core, including AI-drafted code |
| E8 | Legal review before a wide public release |

## F. Closed (the old document that raised it is safe to delete)

| Old item | Resolution |
|---|---|
| `io-engine-data.txt` "LOCKED v1.0" (assembly, block-index LBA) | Salvaged into `io_engine.md` v2 (2026-10-24); the rest superseded: ADR-009 -> ADR-011 (Rust), `fs.info` |
| C-process / Master architecture | ADR-015, ADR-040, ADR-050 |
| SMART Handler: Python vs C, IPC | ADR-001, ADR-004 (embedded interpreter); `ADR-006_pseudo_draft.md` contradicts it |
| Three competing allocator models | ADR-017, ADR-042, ADR-043; `allocator.md` |
| Data-block / B-tree node checksum storage | `fs.info` section 15a (16 B trailer on every block) |
| B-tree node format | `fs.info` 7b; ADR-048 (254 slots) |
| Zone Table format | ADR-021, ADR-035 (byte layout is the allocator's job) |
| Global Hash Table / dedup table | ADR-023 |
| `namespace_structure_id` placement | Superblock 84-87 |
| Two independent `emergency_mode` signals | `fs.info` 17e: acute signal wins; hard floor (ADR-044) |
| Identifier slot reuse vs retire | ADR-047 (immediate retire, async reuse) |
| `raw`: zero-fill vs uninitialized | ADR-049 (UNWRITTEN flag) |
| `raw` in snapshots | ADR-027, ADR-049: outside snapshot scope; per-file opt-in (G7) |
| SMART trend script (script 3) | `--type=trends` exists; ADR-045 |
| Compression batch "64 MiB" (closeout) | ADR-025 governs chunk default/max/floor |
| Type-A / Type-B zones (hot/cold recording), 2 KiB blocks, Block Map | Dropped (confirmed 2026-10-24); history kept in `DECISIONS-FORMAT-8` section 8 |
| Block size approach A/B/C | Current spec is uniform `block_size` plus a 16 B trailer (option A); confirm once |
| Python hot-path hashing tension | Hash daemon over a socket (ADR-037); see ADR-004 |
| `EXTENT_B` WAL op 0x06 / ANCHOR_DIFF pattern | Revisit inside G3 (ANCHOR_DIFF proposed for removal) |
