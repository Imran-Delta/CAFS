# CAFS — Spec Changes Needed (grouped by function, format-breaking first)
**Date:** 2026-09-30
**Status:** Change list only. Nothing here has been applied to any file.
**Group numbers** match `cafs-draft-decisions` and `cafs-research-consolidated`.

**Status key:** **READY** = backed by an Imran decision, can be written now · **BLOCKED(x)** = needs the open decision named in `x` · **PROPOSED** = my suggestion, not decided · **WORDING** = text-only fix.
**ADR numbers are provisional:** the last formalized ADR I know of is 065, so new ones start at 066. Existing ADR numbers below are as cited in the 09-19 and 09-30 material; check them against `ADR1-55.json` and `cafs-adrs-job4-2026-09-19.json` before editing (ADR-054/055 references come from the DeepSeek draft).

---

## One breaking format bump (G1–G5 land together)
Do these as **one** format revision so existing volumes face a single migration decision (reformat vs migrate; old 48 B volumes can't take the new fields):
- `fs.info` v22 → next version (provisional v23), `FORMAT_VERSION` 7 → 8 in `layout.rs`.
- Identifier Table row 48 → 128 B with Superblock `row_size`.
- Function Table: WAL chunk list, remove `temp_cache_lba`.
- New tables: ACL table, overflow table (and Meta Hash Table if it gets a format).
- Encryption nonce field (inside the row).

---

# PART 1 — FORMAT-BREAKING

## G1. Identifier row
| File / section | Change | Status |
|---|---|---|
| `fs.info` §7a | Row size 48 → fixed 128 B, power of two, set at format time; direct-index formula `identifier_table_lba + identifier × row_size` | **READY** |
| `fs.info` §7a | Row field table: add `optimization_flags` bits (already present), `mtime/ctime/btime` (s64 + u32 ns), `posix_acl_id`, `nt_acl_id`, `overflow_id` (u32, null = none), 4 B crc32c, 16 B nonce; remainder undefined/reserved | **BLOCKED(atime in row?, checksum width, nonce location)** |
| `fs.info` §7a | Remove any generation-counter language; state that identifier safety rests on refcount + Recently-Zeroed Queue and that the queue's "refcount 0" check also consults an in-memory holder count | **READY** (holder-count wording **PROPOSED**) |
| `fs.info` §7a | Add per-row checksum behaviour (covers torn writes; what happens on mismatch) | **BLOCKED(checksum width)** |
| `fs.info` | Document migration stance for 48 B volumes | **BLOCKED(migration path)** |
| `layout.rs` | Add `SB_OFF_ROW_SIZE`; row-size constants; bump `FORMAT_VERSION`; update the module header comment ("anchor layout hasn't moved since v19" stays true but the version list needs the new entry) | **BLOCKED(row_size offset)** |
| ADR-047 | Amend: identifier reuse via immediate retire + async reuse needs **no** generation counter, but needs the in-memory holder count; note the NFS-export assumption (none) | **READY** (amend) |
| New ADR | "Identifier row: 128 B fixed, format-time `row_size`, no generation counter" | **READY** |
| New ADR | Timestamp fields: mtime/ctime/btime s64+u32ns; atime opt-in flag | **READY** |

## G2. Function Table and Superblock
| File / section | Change | Status |
|---|---|---|
| `fs.info` §8, `layout.rs` | Replace single `wal_lba` with a WAL chunk list: either widen the Function Table prefix and move its checksum (currently @64 over bytes 0–63, minimum size 80) or keep `wal_lba` as a pointer to a chunk-list block | **BLOCKED(list layout)** |
| `fs.info` §8, `layout.rs` | Remove `temp_cache_lba` (`FT_OFF_TEMP_CACHE_LBA`@32); re-check checksum offsets and `FT_MIN_REAL_SIZE` afterwards | **READY** (remove vs reserve: lean remove — confirm) |
| `fs.info` §4, `layout.rs` | Add hash-algorithm ID beside `root_hash_algo_id`@80 | **READY** |
| `fs.info` §4, `layout.rs` | Add `row_size` field | **BLOCKED(offset)** |
| `fs.info` §4 | Superblock `flags`: assign bit 0 = pending-freeze, reserve the rest | **PROPOSED** |
| `fs.info` §4 | `format_complete` as a multi-valued field (not boolean); define proof format and its location | **BLOCKED(proof format, location)** |
| `fs.info` §4 | Confirm `namespace_structure_id`@84 in the spec text (already decided 09-13) | **READY** |
| ADR-046 | Move the WAL sizing formula into the ADR; clarify `region_size` = total active WAL footprint across all chunks, not one chunk | **READY** (wording confirmed in the draft; sizing numbers still open) |
| ADR-057 | Record the outcome for `temp_cache_lba` (removed outright vs reserved) | **BLOCKED(remove vs reserve)** |

## G3. WAL
| File / section | Change | Status |
|---|---|---|
| `fs.info` WAL section | Chunk header layout | **BLOCKED(Imran, held)** |
| `fs.info` WAL section | Entry format: fixed header + length-prefixed payload (ANCHOR_DIFF is variable-length); checksum/ECC level declared once per chunk | **PROPOSED**, **BLOCKED(entry format)** |
| `fs.info` WAL section | Recovery start point, tail semantics, retirement: only after entry format; physical vs logical entry decision first (revoke hazard) | **BLOCKED(physical vs logical)** |
| `fs.info` / ADR-062 | Journal primitive: one primitive, separate instances (main WAL, relocation log, reformat WAL), per-instance integrity level (ECC for small high-stakes, checksum for high-volume) | **READY** |
| ADR-046 | Name a single authority for chunk membership (Function Table list vs chunk chaining) | **BLOCKED(authority)** |
| New ADR | Apply-atomicity model (A per-block, B per-entry, C transaction, D append-and-forget) | **BLOCKED(Claude analysis owed, then Imran)** |

## G4. New tables
| File / section | Change | Status |
|---|---|---|
| `fs.info` new section | ACL table: separate from overflow; same blob machinery; reclaim (not never-delete); entry layout | **READY** (shape) / **BLOCKED(entry layout, reclaim mechanism)** |
| `fs.info` new section | Overflow table: any non-bool per-file attribute that doesn't fit the header (`extent_limit` is the first); reclaim; bundle vs pair | **READY** (scope) / **BLOCKED(bundle vs pair)** |
| `fs.info`, ADR-029 | Global Hash Table modes = `off` / `per-file` / `per-LBA` only; remove the 4th mode text | **READY** |
| ADR-029 | Supersede with an ADR creating the **Global Meta Hash Table** as a separate structure | **READY** (existence) / **BLOCKED(entry format, lifecycle, granularity)** |
| ADR-054 (as cited) | `extent_limit` storage moved from extent-list blob header to the overflow table | **READY** |
| Allocator spec (`allocator.md`, not reviewed) | `CA-1002` and the extent-limit path now read from the overflow table (cached in open-file context) | **WORDING** after review |

## G5. Encryption
| File / section | Change | Status |
|---|---|---|
| `fs.info` §7a | 16 B per-file nonce field plus a per-file "encrypted" presence flag | **BLOCKED(nonce location)** |
| New ADR | Encryption scope, key derivation, key storage, filename scope | **BLOCKED(entire design open)** |
| `fs.info`/config | Interaction with dedup: encrypted files excluded from cross-file dedup | **PROPOSED** |

---

# PART 2 — FORMAT-TIME / FLAG-LEVEL

## G6. Hash and dedup
| File / section | Change | Status |
|---|---|---|
| `fs.info` §Superblock, `config.fs` | Hash algorithm is format-time only (BLAKE3-256 default, SHA-256 selectable); not runtime-configurable; changing it requires reformat | **READY** |
| `config.fs` | Add bucket-count config for `[dedup_table]` equivalent to `[hash_table]`'s | **BLOCKED(Meta Hash Table decision)** |
| `config.fs` | Hash-table toggle token names | **WORDING** (needs Imran's naming) |

## G7. Flags
| File / section | Change | Status |
|---|---|---|
| `fs.info` §7a / `config.fs` | `optimization_flags` bit assignments: immutable-data, immutable-meta, atime, raw tag, snapshot-of-raw opt-in | **BLOCKED(bit allocation)** |
| `config.fs` | Stale optimization-flags comment: rewrite once the flag set settles | **WORDING** after bit assignments |
| `config.fs` | `flag_group`: decide documentation-only vs code-adjacent location | **BLOCKED(Imran call)** |
| `config.fs` | Option for whether both immutable flags force the raw bundle (default off — **PROPOSED**) | **PROPOSED** |
| `fs.info` | Raw = advisory computed bundle; WAL chunks are the explicit non-snapshot exception | **READY** |
| `fs.info`/allocator | Audit assumptions that raw blocks have refcount 1 (lazy CoW-break and per-file snapshot opt-in break it) | **PROPOSED** (audit task) |

## G8. Redundancy
| File / section | Change | Status |
|---|---|---|
| ADR-065 | Revise: zone ECC is a plain opt-in toggle on any media; no rotational-detection auto-disable; state scope (localized media defects, not device failure) | **READY** (policy) / **BLOCKED(warning mechanism, checksum-trailer interaction)** |
| `fs.info`, anchor spec | Make Superblock Backup and SMART backup differ byte-for-byte from their primaries (copy index inside the checksummed region) | **PROPOSED**; check they're really identical first |
| `config.fs` | Zone ECC toggle (per zone/volume) | **READY** (existence) |

---

# PART 3 — PROCESS / RUNTIME

## G9. Relocation, resize, reformat, mkfs
| File / section | Change | Status |
|---|---|---|
| ADR-061 | Confirm relocation-log entry format (file hash, old logical LBA, old physical LBA, new destination, ECC) | **READY** (confirm only) |
| New ADR | Resize/relocation safety rule: plan committed first → copy → verify → atomic pointer switch → checkpoint → retire old; container-boundary ordering (shrink: data then boundary; grow: boundary then metadata) | **PROPOSED** |
| New ADR | In-place whole-volume shift: window ≤ shift distance, durable next-chunk marker | **PROPOSED** (my derivation) |
| New ADR | Reformat machinery: offline, own WAL instance, hash-algorithm change is a rebuild, not a block move | **READY** (outline) |
| mkfs spec | Scope sentence "mkfs fails → rerun" limited to fresh format | **WORDING** |
| mkfs spec | Allocator bootstrap, reformat reads existing Zone Table, verify-then-format_complete, FAT32 rescue default OFF, no resize headroom | **READY** (per DS) |
| mkfs spec | Add: existing-signature refusal without force, mounted-device check, whole-device TRIM with opt-out | **PROPOSED** |
| Zone Table byte layout | Allocator's own job; unchanged | **DEFERRED** |

## G10. ACL and permissions
| File / section | Change | Status |
|---|---|---|
| `fs.info`, `config.fs` | ACL mapping config (`none` / `posix_to_nt` / `nt_to_posix` / `bidirectional`); granularity | **BLOCKED(granularity, default name)** |
| `fs.info` | NT default: admin-level when the Linux permission requires root | **READY** |
| `fs.info` | Null NT ID = computed from mode; NT→POSIX fails closed | **PROPOSED** |
| `config.fs` | uid/gid ↔ SID user map section (or explicit scope limit to local accounts) | **BLOCKED(idmap design)** |
| `io_engine.md` | Permission section: add `FUSE_POSIX_ACL` negotiation; "default_permissions + mode/uid/gid is all mntproc owes" holds only for mode bits, not ACLs; Linux must serve ACL xattrs and implement default-ACL inheritance on create | **READY** (wording) |
| New ADR | Permission-delegation layering (POSIX check → allocator grant → I/O Engine execution): proposed across several turns, never written as an ADR | **BLOCKED(#28 case list, ownership boundary)** |
| New ADR | Dual ACL storage (both canonical) | **READY** |

## G11. Errors
| File / section | Change | Status |
|---|---|---|
| `error_id.md` | Rewrite the scheme: POSIX name where exact, `CA_Name`/`CA-Name` otherwise, numbers for obscure ones, three layers, 16-bit cap, per-platform mapping; remove the stale 4-digit ranges 1000–4999 | **READY** (scheme) / **BLOCKED(spelling `CA-` vs `CA_`)** |
| `error_id.md` | Migrate `CA-1001`–`1003` | **BLOCKED(migration)** |
| `error_id.md` | Add every named error's numeric Windows Event ID mapping | **BLOCKED(implementation)**; ranges stay advisory |
| ADR-055 (as cited) | Revise error-range allocation | **READY** (revise) |
| `errors.md` | Create/populate the catalog | **DEFERRED** |
| New error variants | Permit-validation failure: `CafsError` variant and code | **BLOCKED(retry vs fail)** |

## G12. TRIM and Windows kernel helper
| File / section | Change | Status |
|---|---|---|
| TRIM relay spec / `io_engine.md` | Re-audit against new free events (WAL drain/shrink, dedup-redirect loser, raw/snapshot refcounts, relocation-log old-LBA retention) | **BLOCKED(ADR set review)** |
| `io_engine.md` | No change to kernel-mode constraints | — |

## G13. I/O Engine
| File / section | Change | Status |
|---|---|---|
| `io_engine.md` | Wording pass: `mntproc` is Rust, not C; several ADRs and docstrings still say "C process"/"C engine" | **WORDING** |
| `io_engine.md` | Add permit table size; permit-validation failure; `CafsCtx` locking; misaligned-write + permit as one unit; checksum-mismatch handling on ordinary read (read the existing PoC auto-repair path first); `heat_caches()` size threshold | **BLOCKED(Imran decisions)** |
| `syscalls.rs` | Wire the I/O counters (`io_placeholder` in `smart_handler.py`) | **DEFERRED** (implementation) |

---

# PART 4 — DEFERRED / DOCUMENTATION
- **New ADR-056-style roll-up** of everything locked once the above settle; the user-facing document (09-19 issue #37).
- **README:** no change needed from this session; keep "Alpha" and the AI-disclosure text.
- **Workload-dependent settings:** per-setting documentation (workloads served, automatic logic, override, production vs server) — last batch.
- **Copy-operation fork philosophy:** deferred by Imran.
- **`cafs-concerns-opinions-2026-09-19.md`** items not listed above (extent-limit math, 4th-mode framing) are superseded by this change list and the decisions file.

---

## Counts (for planning)
**READY now:** the row size/formula, no-generation-counter text and ADR-047 amendment, `temp_cache_lba` removal, hash-algorithm ID, Global Hash Table three modes, extent_limit relocation, raw advisory bundle, journal-primitive framing, zone-ECC policy text, NT default rule, `io_engine.md` ACL wording, dual-ACL ADR, error-scheme rewrite, `mntproc` wording pass.
**Biggest blockers to clear first (settle order):** atime / checksum width / nonce location / overflow shape (G1) → Function Table WAL-list layout (G2) → physical vs logical WAL entries and apply model (G3) → ACL table layout and reclaim (G4) → encryption design (G5).
