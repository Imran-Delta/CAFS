# CAFS fs.info v17 Consolidation — Audit Findings

**Date:** 2026-08-23
**Documents reviewed:** fs.info (v15, base), fs_info.update (v16, spec suggestion), fs_info.update-2 (v17 change log, "locked"), fs_update.final (v17 consolidated, proposed replacement for fs.info), config.fs (v14, format_version=3)
**Verdict:** fs_update.final should **not** replace fs.info as-is. It implements 6 changes never authorized by the locked v17 change log (2 of them self-contradictory within the document itself), and none of the 4 config.fs-side decisions the change log did lock have actually been applied to config.fs.

---

## Critical

**C1. format_version 3→4 bump — unauthorized.**
fs_update.final §1.8 and Superblock offset 4–7 bump `format_version` to 4. None of the 11 entries in fs_info.update-2 mention this. config.fs `[identity].format_version` (line 29) still reads 3. No document explains why this is a breaking change or coordinates the bump across files.

**C2. root_hash_algo_id silently destroyed — confirmed live field.**
v15/v16 Superblock offset 68–71 = `root_hash_algo_id`. fs_update.final reuses that identical offset for the new `pointer_checksum_algo_id` — it overwrites, not adds. config.fs `[integrity].root_hash_algo = "blake3"` (line 66) is explicitly marked crash-critical / stored in LBA2 and is user-configurable. There is now no on-disk field to persist it. fs_update.final §16 still lists Root Hash algorithm IDs in its table as if the field exists.

**C3. Parity Block redefinition — silent, changes recovery semantics.**
v15/v16: Parity = LBA0 XOR LBA1 (2048-byte pointer blocks), but declared a 4KB block — a pre-existing, never-logged size contradiction. fs_update.final §6 "resolves" this by redefining Parity as XOR of the *real* Superblock and Config blocks (4KB each) instead — a different structure protected, not a fix to the original claim. Not in the change log. Recovery Order (§14) still lists parity reconstruction as step 4 alongside pointer-block recovery steps 1–3 without distinguishing that they now cover different failure domains (pointer blocks vs. real structures).

**C4. Backup Parity references a structure with no defined storage path.**
fs_update.final §6 requires a "backup Config Snapshot" to compute backup parity. §12 (End-of-Device exceptions) only redirects LBA0 → Superblock Backup and LBA2 → Backup Parity Block; LBA1 is never redirected to a Config Snapshot backup anywhere. Self-contradictory within fs_update.final alone, independent of v15/v16.

**C5. Config Snapshot format contradicts its own "locked" change-log entry.**
fs_info.update-2 Entry 4 (locked ✅): Config Snapshot fields "mapped with offsets, sizes, and types" — a binary struct. fs_update.final §5 instead stores raw UTF-8 TOML text in a single opaque byte range, with:
- no bound on TOML length N against the 4080-byte budget,
- no stated algorithm for the inner `config_self_checksum` — confirmed by config.fs (line 34) to be a distinct 64-bit checksum, separate from the block-level BLAKE3-128 checksum described in §5, and never specified anywhere.

**C6. LBA5 / Function Table redundancy reversed.**
v15/v16: LBA5 points to a *separate* backup copy of the real Function Table. fs_update.final §2/§8: "LBA5 is a copy of the LBA4 pointer block, so both point to the same Function Table LBA." Real-structure redundancy for the Function Table is gone — if the real FT block corrupts, both anchor pointers now reference the same corrupted data. Not logged.

**C7. Locked config.fs decisions never applied to config.fs.**
None of the four config.fs-side items marked "✅ Locked" in fs_info.update-2 are reflected in the uploaded config.fs:
| Entry | Locked decision | config.fs actual state |
|---|---|---|
| 5 | `dedup.enabled` → false | line 190: `enabled = true` |
| 6 | `dedup.algo` immutable, stored in LBA2 | line 186: section still headed "RUNTIME ONLY – NOT IN LBA2" |
| 7 | `[buffer].block_size` → `[buffer].granularity` | line 344: still named `block_size` |
| 8 | `[journal]` section removed | lines 125–127: `[journal]` still present, still marked crash-critical |

If fs_update.final replaces fs.info while config.fs is left untouched, the two files describe a filesystem that disagrees with itself.

---

## Major

**M1. Pointer-block checksum configurability — new, unauthorized feature.**
fs_update.final §3.3 introduces CRC32C (default) / XXHASH32 (optional) as a configurable choice for pointer blocks. v16 had this hardcoded to CRC32C. Not in the 11-entry change log. Its new Superblock field is what caused C2.

**M2. WAL Entry Format — change log is self-contradictory; final silently picks a third option.**
fs_info.update-2 Entry 2 locks WAL entries at fixed 48 bytes. Entry 3's own table defines a variable-length, `entry_len`-prefixed format "for all entries" — mutually exclusive with Entry 2. fs_update.final implements neither literally: fixed 48-byte entries (per Entry 2) plus a separate, pointed-to 4KB TLV block for ANCHOR_DIFF (an unlogged third design). Entry 3's table is effectively dead but still marked "Locked ✅."

**M3. Anchor ID enumeration dropped.**
v16's `anchor_id` 0–6 mapping (Superblock/Config/Parity/FT/EOD/M-SMART/B-SMART) — already misaligned by one LBA slot for IDs 0/1/2 versus the actual anchor layout — is entirely absent from fs_update.final §9.3. `anchor_id` is now a 4-byte field with no defined value set anywhere.

**M4. WAL Region Size guidance never matched config.fs; final drops it instead of reconciling.**
v16's "WAL Region Size" section describes config keys `wal_min_size`/`wal_max_size` with a sizing formula. Actual config.fs `[wal]` section (lines 116–119) has none of these — it has `region_size = "512MB"`, `sync_interval_seconds`, `sync_on_every_write`. Pre-existing mismatch, never caught in any change log entry. fs_update.final removes the section rather than reconciling it against real config.fs.

---

## Minor

**m1. §16 Algorithm Identifiers table conflates four independent enumerations.**
Data/Anchor/Root-Hash/Compression algo IDs are unrelated enums in v15/v16 (each with its own meaning per number). fs_update.final merges them into one ID-indexed grid, inviting the false impression that a given ID number means the same thing across categories. `pointer_checksum_algo_id` is a fifth enum, tracked separately in §3.3, not included in this table.

**m2. SMART magic mislabel — quietly fixed, not logged.**
v16 labels `0x534D5254` as `"SMART"`; it actually decodes to `"SMRT"` (4 bytes can't hold 5 ASCII characters). fs_update.final §11.1 correctly relabels it `"SMRT"`. Correct fix, but untraceable — not a change-log entry.

**m3. [recovery] section mixes crash-critical and runtime fields per-field, not per-section.**
`migration_staging_size` is crash-critical; `auto_remount_on_irrecoverable` in the same section is runtime-only (config.fs lines 230–236). fs_update.final §5's TOML-dump approach for the Config Snapshot doesn't state whether mixed sections are dumped whole or field-filtered — ambiguous under the new text-based encoding.

---

## Recommendation

Before fs_update.final replaces fs.info:
1. Resolve C1–C7 explicitly — each needs either a change-log entry with rationale or a rollback to v16 behavior.
2. Update config.fs to match whichever decisions are kept (C7).
3. Reconcile M2 by removing Entry 3's dead table from fs_info.update-2 or updating it to match what was actually implemented.
4. Decide and document anchor_id values (M3) before ANCHOR_DIFF is implementable.
