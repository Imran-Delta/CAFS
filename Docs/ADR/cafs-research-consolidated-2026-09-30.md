# CAFS — Consolidated Research (grouped by function)
**Date:** 2026-09-30
**Status:** AI-compiled. Informs decisions, is not one. Not canon.
**Replaces for reading purposes:** the 09-19 job-4 research file, `cafs-research-batch3`, `cafs-research-batch4`, and the research halves of the ACL file. Those files stay valid; where they and this file differ, this file wins.
**Convention:** **Found** = from a source listed in §S. **Inference** = my reasoning, not sourced. **Project fact** = read from a CAFS file (`layout.rs`, `io_engine.md`, `error_id.md`, `fs.info`). Nothing here is a decision.
**Group order = format impact.** Groups G1–G5 change the on-disk format; G6–G8 are format-time or flag-level; G9–G13 are process/runtime. Same group numbers are used in `cafs-draft-decisions` and `cafs-changes`.

---

# PART 1 — FORMAT-BREAKING

## G1. Identifier row and identifier lifecycle

**Project fact.** `fs.info` §7a: rows are direct-indexed (`identifier_table_lba + identifier × 48`), no per-row checksum, no generation field. 48 divides neither 512 nor 4096, so rows can straddle sectors and tear on power loss unless the WAL covers row updates (not verified).

**Found — ext4 inode layout.**
- 128 B base inode plus an optional extra region sized by `i_extra_isize`; default inode size 256 B.
- Generation counter lives in the base. Checksum is split lo/hi across base and extra.
- Timestamps pack nanoseconds plus 2 epoch bits into 4 B extras, saving 4 B per timestamp at the cost of range.
- A real checksum-validation bug exists for inodes with a small `i_extra_isize`: variable row size breeds edge cases.
- Power-of-two inode sizes never straddle blocks (general knowledge; XFS inode sizing not researched).

**Found — timestamp width.** `s64` seconds covers about ±292 billion years, so no overflow risk at 1-second granularity. `s128` would only be justified for packing sub-second precision into one fixed-point field, which is needless next to POSIX `timespec`'s shape: `s64` seconds + `u32` nanoseconds = 12 B.

**Found — FUSE inode lifetime** (libfuse low-level header docs).
- Lookup count rises by one per `fuse_reply_entry` / `fuse_reply_create`; `forget(ino, nlookup)` lowers it.
- An inode with non-zero lookup count can still receive requests after `unlink`, `rmdir`, or a rename that overwrites it. The header recommends deferring removal until the count reaches zero. For open files, forget arrives only after `release`/`releasedir`.
- On unmount, counts drop to zero implicitly; forget messages are not guaranteed.
- `generation` in `fuse_entry_param` is needed only for NFS export. The `(ino, generation)` pair must be unique over the filesystem's lifetime; reuse needs a new, never-used generation.

**Found — fscrypt lesson.** fscrypt considered mixing the inode number into key derivation and rejected it because it would have blocked ext4 resize. Direct analogue for CAFS: nothing stable-looking-but-mutable (identifier, LBA) belongs in per-file derivation. See G5.

**Inference.**
- Dropping the generation counter is sound **if** there's no NFS export **and** the slot-reclaim check also consults an in-memory holder count (kernel lookups + open handles). The on-disk refcount can't express "the kernel still holds nodeid N"; without the in-memory check, unlink → refcount 0 → slot reuse → the kernel's cached nodeid resolves to a different file.
- A crash or unmount discards the kernel's cache, so stale nodeids can't survive a mount. Unlinked-but-open files at crash time still need reclaim at next mount (ext4 uses an orphan list; not researched here).
- Not verified: how the kernel reacts when a reply reuses a nodeid with a different generation. WinFsp's equivalent (handle lifetime vs file identity) not researched.

---

## G2. Function Table and Superblock

**Project fact — `layout.rs` (format 7, anchor layout unchanged since v19).**
- Six anchor slots: LBA0 Superblock ptr (0), LBA1 Config Snapshot ptr (2048), LBA2 Parity Block ptr (4096), LBA3 Meta/User ptr (6144), LBA4 Function Table ptr (8192, 4096 B), LBA5 copy of LBA4 (12288). Anchor region = 16384 B. LBA = raw byte offset from the start of the device. The Superblock's `function_table_anchor_lba` field holds a slot **index** (always 4), not a byte offset.
- Superblock (4096 B) offsets: magic@0, format_version@4, volume_uuid@8, config_generation@24, function_table_anchor_slot@32, wal_sequence@40, last_mount_time@48, flags@56, data/anchor/pointer checksum algo@60/64/68, last_applied_wal_seq@72, root_hash_algo@80, `namespace_structure_id`@84, checksum@4080 (16 B over bytes 0–4079). `namespace_structure_id`@84 was confirmed in the 09-13 decisions.
- Function Table: no magic, no fixed total size (size comes from its pointer block's `real_size`). Fields at 8-byte steps: `wal_lba`@0, `smart_main`@8, `smart_backup`@16, `scratch`@24, `temp_cache`@32 (retired by ADR-057, still present in `layout.rs`), 40–63 unassigned, checksum@64 over bytes 0–63 (16 B), minimum real size 80.
- Pointer-block and checksum algorithm IDs, SMART table (144 B) and Parity Block (XOR of Superblock and Config Snapshot) are as in `layout.rs`; not repeated here.

**Found — mke2fs.** Can skip zeroing inode tables and the journal ("lazy init") and flags each group's table as zeroed. Its man page warns a crash before the journal is fully overwritten once carries some risk. Reserves descriptor-table room for a maximum online-resize size.

**Found — mkfs.btrfs.** Refuses to write over an existing filesystem or partition table without a force flag; checks the device isn't mounted; enforces a minimum device size; performs a whole-device TRIM by default (opt-out `-K`). Format-time choices include checksum algorithm (crc32c default; xxhash, sha256, blake2), nodesize, sectorsize, feature flags, profiles, label, UUID.

**Inference.**
- Lazy init needs an explicit per-region "initialized" marker, not assumed zeros.
- A 16–32 slot WAL chunk list (128–256 B) does not fit the checksummed 64-byte Function Table prefix. Options: move the checksum (format change), or keep `wal_lba` as one pointer to a separate chunk-list block.
- ZFS `zpool create` not researched.

---

## G3. WAL and journal primitive

**Found — JBD2.**
- Every journal block carries a fixed 12 B header (magic, type, transaction sequence). The journal superblock records the first expected commit ID and log start, so recovery needs no per-block chain.
- Checksum v2 broke because variable tag sizes caused alignment bugs; v3 fixed the tag size (maximum overhead about 0.1%). The checksum is seeded from the journal UUID.
- **Revoke records exist** because a block freed and reallocated as file data would otherwise be overwritten by a later replay of an older journal entry.

**Found — bcachefs journal.** Entries carry `seq` and `last_seq` (the oldest entry still needed); the checksum nonce is derived from `seq`; when duplicates exist across devices the copy without a checksum error is preferred.

**Project facts.** WAL today = raw-tagged blocks in 512 MB chunks with head-advancement rotation (overview). Past audit found an **ANCHOR_DIFF variable-length payload vs fixed WAL slot** incompatibility, so any entry format needs a fixed header plus a length-prefixed payload.

**Inference — revoke hazard, concrete case.** WAL entry E (seq 10) records an update to block B. B is later freed, reallocated as ordinary data D, and written directly (data writes bypass the WAL). Crash before E is retired. Recovery replays E and overwrites D with stale content. Only **physical** entries (carrying block writes) have this problem; **logical** entries ("set field F of row N") applied to structures never reused as data don't. Fixes: revoke records, or the allocator refuses to reuse a block a live entry names, or entries are retired before the free commits.

**Inference — recovery shape.** Fixed-size entry header (magic, type, seq, len, csum); linear scan from the tail stopping at the first bad checksum or non-consecutive seq; checksum type stored once in the chunk header; checksums seeded with `volume_uuid` so entries from a previous format never validate; chunk header carries its first seq so the Function Table list is cross-checked, not trusted.

**Cross-reference.** `pvmove` checkpoint pattern (G9) is the closest precedent for resumable progress records.

---

## G4. New tables: ACL table, overflow table, Global Meta Hash Table

**Found — 4th-mode survey (what "metadata dedup" looks like in real filesystems).**
- **NTFS `$Secure`:** each unique security descriptor gets an ID, and a hash index detects an existing identical descriptor (full comparison on hash match). Entries are never deleted even when unreferenced.
- **ext4 `mbcache`:** dedups identical xattr blocks, originally built for shared ACLs. A maintainer publicly questioned whether hit rates justify it; no statistics exist.
- **ZFS:** dedup applies to data blocks and doesn't work on encrypted datasets.
- **APFS, bcachefs, btrfs:** thin search found no metadata dedup. **No filesystem found dedups filenames.** No real-world savings numbers found anywhere.

**Concerns file (09-19).** Filenames are short, so repeated-filename savings are marginal. ACL blobs are larger and heavily repeated in real deployments, which is why NTFS shares them by ID.

**Inference.**
- Directory-entry dedup depends entirely on entry shape. If an entry is (name, identifier), a copied tree has different identifiers and hits are roughly limited to hard links. Define what actually repeats before choosing granularity.
- ACL sharing by u32 ID does not need hashing: a hash index only matters for finding an existing identical descriptor at insertion.
- Per-operation refcounts on a popular shared ACL create a write hotspot. Reclaim by background scan (read all rows, tally, free unreferenced) avoids it, at the cost of one table pass. Race guard needed: a last-touched epoch per entry, written at most once per epoch by adopters; the scan frees only entries unreferenced and untouched since it began.
- An overflow ID pointing to a **bundle** of pairs means any single change forks a new bundle and sharing only happens for identical combinations; a **single pair** shares per-attribute. Undecided.

---

## G5. Encryption

**Found — fscrypt (Linux native).**
- Integrated into the filesystem (ext4, F2FS, UBIFS, CephFS), unlike stacked eCryptfs, which double-caches pages; integration roughly halves memory use.
- Scope is **content and filenames only**; mode, ownership and timestamps stay in the clear.
- Per-file keys are **derived**, not wrapped: v1 used AES-128-ECB with the file's 16-byte nonce as key; v2 (current) uses HKDF-SHA512 from the master key with a distinct context per purpose (content vs filename). Derivation was chosen over wrapping because wrapped keys need larger xattrs that wouldn't fit inline in an inode-sized structure.
- The per-file nonce is generated once, stored, never changes. Inode number was rejected as an input (blocks resize).
- Usable by unprivileged users; no special mount handling.

**Found — others.**
- **bcachefs:** stores a nonce and MAC alongside each data pointer, giving a chain of trust up to the superblock. Whole-filesystem only, because metadata blocks don't belong to a particular file.
- **APFS:** a separate key per file (or extent) for data, wrapped by a class key and stored in file metadata; another key for sensitive metadata. Each half of a clone gets a new key for new writes.
- **NTFS EFS:** a random per-file key (FEK) encrypted with the user's public key, stored in the `$EFS` stream; extra copies for additional users; filenames stay visible.
- **ZFS** dedup doesn't work on encrypted datasets. A Fedora btrfs thread notes encryption defeats firmware-level duplicate collapse because identical plaintext becomes different ciphertext.

**Inference.**
- Per-file keys mean encrypted files can't take part in cross-file dedup (identical plaintext → different ciphertext); hashing plaintext instead would leak content equality.
- Extent entries are 16 B with no slot for a nonce or MAC, so a per-file nonce in the row is the cheap route. Wrapped keys don't fit the byte budget; derivation does.
- Not researched: NTFS EFS and APFS key-derivation mechanics in depth.

---

# PART 2 — FORMAT-TIME / FLAG-LEVEL

## G6. Hash algorithm and dedup modes

**Found.** SHA-256 and BLAKE3-256 both give a 32 B digest, so the Hash Table entry layout (32 B hash + 8 B value = 40 B) is identical either way.
- SHA-256: FIPS 180-4, 20+ years of scrutiny, sequential Merkle–Damgård, fast with CPU SHA extensions, slow without.
- BLAKE3: 2020, BLAKE2 lineage, Merkle tree so one input can be hashed in parallel, faster in software, **not FIPS**, shorter deployment history, no known weaknesses.
- mkfs.btrfs already offers crc32c / xxhash / sha256 / blake2 as a format-time choice.

**Inference.** The hash table's role differs from a checksum's: checksums catch accidents (CRC or xxHash suffice), but a hash-table match means "same content", so it needs collision resistance. BLAKE3 reuses a dependency already vendored (checksums, pointer verification). My earlier parallelism argument is weak for 4 KiB inputs. FIPS is the only reason to prefer SHA-256. A different algorithm needs a reformat that re-reads every block and rebuilds the Hash Table (not a plain block move).

---

## G7. Per-file flags and attributes

**Found — cross-filesystem survey.**
- **btrfs:** supports only `i` (immutable), `D`, `m`, `S`, `V`, no-dump; no append-only despite it being in the generic `chattr` list. No "delete-only" attribute anywhere surveyed.
- **XFS:** `fsxattr` struct of small per-file `u32` fields (`fsx_extsize`, `fsx_nextents`, `fsx_projid`, `fsx_cowextsize`) plus a flags bitmask including `XFS_XFLAG_IMMUTABLE`: precedent for plain extra numeric fields.
- **ZFS:** properties are per-dataset only; no per-file equivalent.
- **F2FS:** `pin_file` (relocation exemption, not absolute), `atomic_file`, `inline_data`.
- **bcachefs:** options tagged by scope (`format,mount,runtime` vs also `inode`). `nocow` breaks CoW transparently only when a snapshot or reflink reference exists ("no CoW while shared").
- **Generic inode-flags family:** `NO_DATA_COW` ("no CoW while refcount is 1", independent confirmation of the same idea), `NO_DATA_SUM`, `PREALLOC`, `NO_ATIME`, `DIR_SYNC`.
- **NTFS attributes:** READ_ONLY, HIDDEN, SYSTEM, ARCHIVE, TEMPORARY, SPARSE_FILE, REPARSE_POINT, COMPRESSED, OFFLINE, ENCRYPTED, NOT_CONTENT_INDEXED, VIRTUAL. **APFS:** `clonefileat(2)`, explicit instant clone, CoW-shared until modified.
- **Samba `ignore system acls`:** stores DOS attributes (hidden/system/archive/read-only) separately. CAFS has no decided home for them.

**Inference.** Immutable (data) and immutable (meta) should bind content/metadata, not physical location, or immutable files would block resize, relocation and evacuation. Auto-forcing `raw` when both immutable flags are set would drop the file from raw-opt-in snapshots and from dedup, though never-changing data is the safest to dedup. Lazy CoW-break puts refcount > 1 on raw blocks, so any assumption that raw blocks always have refcount 1 needs checking.

---

## G8. Redundancy and integrity

**Found — SSD mechanics (09-19).** An SSD's flash translation layer places and moves blocks for wear-leveling, invisibly to the OS, so a filesystem-level stripe can't guarantee data and parity land on independent NAND dies; a co-located die failure loses both. HDDs don't have the problem. SSDs also already run internal ECC (commonly LDPC) against bit-flip-level errors.

**Found — redundancy precedent.**
- ZFS `copies=1|2|3`: improves recovery from unrecoverable read faults, works on one disk, copies **may land on the same disk** so a full disk failure can lose all; ZFS tries to separate copies across vdevs but doesn't guarantee it. Metadata gets automatic ditto blocks. Practitioner opinion is split.
- btrfs man page: an SSD can remap duplicate blocks to a single copy, negating DUP. A Fedora thread reports corruption hitting both copies identically and says DUP helps transient problems, not device failure. btrfs raid1 across two SSDs has scrubbed sector errors on a slowly dying drive.
- btrfs: the rotational flag "can be arbitrarily set" by the block-device driver; the default became `dup` regardless since 5.15.
- **Firmware dedup is real for at least one controller:** the SandForce SF-1200 did block-level dedup by default (2011, Wei et al. at FAST '11; shipped in Corsair, ADATA, Mushkin drives). Concern: filesystem-level duplicate metadata (superblock copies, mirrored metadata) collapsing to one physical copy. No real-world failure had been reported then; some controllers (Texas Memory Systems) deliberately don't dedup. Current-generation prevalence unverified. The Arch forum claim that this is "fake" is unsourced and now contradicted for the SF-1200.

**Inference.**
- Duplicate-based redundancy (copies, DUP, mirrored metadata) is the vulnerable kind. Whether **parity** survives (it differs from the data it protects) was never tested by any source found.
- ZFS `copies`/btrfs DUP are scoped by their own docs to media faults, not device failure: the same scope statement Imran asked for on zone ECC.
- CAFS exposure: the Superblock and its Superblock Backup (ADR-006) and the SMART main/backup pair are presumably byte-identical. Making each copy differ at byte level (copy index inside the checksummed region) prevents whole-block collapse. Sub-block dedup granularity unknown. Confirm whether they're actually identical.
- Rotational detection is unreliable (btrfs precedent), supporting zone ECC as an opt-in toggle on any media.

---

# PART 3 — PROCESS / RUNTIME

## G9. Relocation, resize, reformat, mkfs

**Found — LVM `pvmove`.** Creates a temporary pvmove LV recording planned movements; each source range gets a temporary mirror on new space; metadata is committed **before** any copying; one segment mirrored at a time, mirror broken when in sync, a **checkpoint** written to metadata; at the end the temporary LV is removed. Interrupted `pvmove` resumes from the last checkpoint by re-running with no arguments; `--abort` cancels (final placement depends on the `atomic` option). An older man page said moving mirrors/snapshots/origins was unsupported; current status not verified.

**Found — GParted failure reports.** Power loss or I/O error mid-move leaves the filesystem unmountable (a btrfs "can't read tree root" case); in one report the partition table had already been updated so there was no rolling back. A forum reply states GParted's pre-checks verify feasibility but don't account for power failure. Release notes list several fixes for crashes that could lose data while moving/copying. (Anecdotal plus changelog; no design doc found.)

**Found — PowerQuest-era patents** (US 5,675,769; 5,930,831; 6,178,487; expired by age, verify before relying on any technique): replicate in windows; direction depends on whether the partition moves left or right; window size capped at the part of the new partition **outside** the overlap so unread source isn't overwritten; copy only used sectors; lock the partition; journaling or "checkmarking" tracks progress so power failure can resume or reverse.

**Found — resize2fs.** Shrink the filesystem first, then the partition; to grow, enlarge the partition first. It moves blocks, inodes and inode tables as needed. The man page makes no crash-safety claim.

**Found — btrfs relocation:** only a historical patch note (relocation fails with ENOSPC when there's no room). Mechanism not covered.

**Inference.**
- The `pvmove` shape maps onto ADR-061's relocation log: plan committed first → copy → verify → atomically switch the mapping → checkpoint → retire the old copy. The commit point is the WAL-covered pointer switch; the copy is idempotent.
- Ordering rule: at every instant the filesystem's claim must fit inside its container. Shrink: relocate and commit metadata, then move the boundary. Grow: boundary first, then metadata. GParted violates this.
- **Window rule (my derivation of the patent's cap).** For an in-place left shift by `d`, copy ascending with chunk size `n ≤ d`: each chunk's destination lies wholly below its own source, so it never overwrites uncopied data. A crash mid-chunk is fixed by recopying that chunk. Only a durable "next chunk" marker is needed, updated **after** a chunk is durable. Right shift mirrors this, copying descending. Small `d` forces tiny chunks, where a scratch-buffer journal may be better.
- If `layout.rs`'s "device" means CAFS's own partition, all addresses are partition-relative and **moving** the partition start needs no pointer rewrites (data copy plus partition-table update). If addresses were disk-absolute, every pointer would need rewriting. The FAT32 rescue split (ADR-028) makes this worth checking.
- Reformat that changes the hash algorithm is not a block move; changing block size is move plus remap.

---

## G10. ACL and permission semantics

**Project fact (`io_engine.md`).** Linux mounts with `default_permissions` so the kernel's `generic_permission()` checks before a request reaches mntproc; mntproc reports accurate mode/uid/gid. Windows: WinFsp passes the security descriptor to `AccessCheck`; WinFsp auto-translates POSIX uid/gid/mode into a compatible descriptor; a `FileSecurity` (SDDL) option allows full native ACL control. State rejections (no space, hard floor, raw/snapshot conflict) remain the allocator's job.

**Found — FUSE.** `FUSE_POSIX_ACL` is negotiated at init (protocol 7.26, 2016) and implies `default_permissions`. The kernel then enforces ACLs; userspace serves the ACL xattrs, keeps mode in sync, and handles default-ACL inheritance on create.

**Found — Samba `vfs_acl_xattr`.** Stores the NT ACL in an xattr next to POSIX ACLs. With none stored it synthesizes one in one of three styles: `posix` (default; from mode bits plus SYSTEM), `windows` (owner and SYSTEM only), `everyone`; the NFSv4 variant defaults to `everyone`. Warns that user/group→SID mapping must stay consistent or stored ACLs change meaning. NT ACL lives in `security.NTACL` (protected); moving it to `user.*` lets any local user overwrite it. `ignore system acls = yes` skips POSIX mapping, forces create mask 0666 / directory mask 0777 and stores DOS attributes separately. POSIX mapping collapses Windows rights into read/write/execute buckets and merges deny with allow; a Null DACL means everyone has full access and behaves surprisingly under inheritance; inheritance flags are kept in `user.SAMBA_PAI` plus POSIX default ACLs.

**Found — WinFsp POSIX↔SID mapping** (v1.4B3-era source; verify against current code). Arithmetic, no table: small uids map to `S-1-5-<uid>` (so 544 → BUILTIN\Administrators, 18 → SYSTEM), uid 1000 special-cased, local accounts `0x30000 + RID`, domain accounts `0x100000 + RID`. The snippet didn't show uid 0; it appeared to fall to `S-1-5-0`, **not** Administrators. So Linux uid 1000 is not any real Windows user and cross-OS ownership needs a config user map.

**Found — NFSv4 / POSIX / richacl.** NFSv4 ACLs evaluate ACEs in order with explicit deny; with one obscure exception every POSIX ACL maps onto NFSv4, but not the reverse. The IETF draft `draft-ietf-nfsv4-posix-acls` (Jan 2026) says mapping attempts "have been unsuccessful due to semantic incompatibilities" and exposes POSIX ACLs directly to avoid lossy mapping; a 2003 draft reaches the same asymmetry. richacl extends NFSv4-style ACLs with masks: POSIX→richacl is easy, the reverse loses information, and a filesystem supports richacls **or** POSIX ACLs, never both. LWN (2015) expected a 4.4 merge; I believe it wasn't merged, verify.

**Found — NT rights with no POSIX counterpart.** `DELETE` (delete a file regardless of parent-directory write), `DELETE_CHILD`, `WRITE_DAC`, `WRITE_OWNER` (take ownership by grant), `READ_CONTROL`, `SYNCHRONIZE`; attribute rights split differently. Tuxera Fusion SMB: with a POSIX backend, POSIX semantics dictate behaviour; access checks use only allow ACEs; default ACLs correspond to Windows inheritable ACEs. SambaXP 2013 (IBM): `READ_ATTRIBUTES` means stat in NFSv4 but DOS attributes and timestamps on Windows; some bits can't be set independently, so editing in the Windows dialog loses them.

**Inference.**
- Both-canonical storage matches where the IETF landed (expose natively, don't force a lossy map). `posix_to_nt` is the safe direction to automate; `nt_to_posix` is where lossy, security-relevant cases live and must fail closed (dropping a deny ACE would widen access).
- NT-only rights only bite in `posix_to_nt`/`nt_to_posix`/`bidirectional`, not in `none`.
- Samba's `posix` default-style precedent supports "null NT ACL = computed from mode"; after `chmod 600` there's no stale permissive stored copy, unlike derive-once.
- Candidate "kernel can't handle" cases to compare with Imran's #28 list: DELETE/DELETE_CHILD without parent write; WRITE_DAC/WRITE_OWNER as separate rights; take-ownership-by-grant; DOS attributes; Null-DACL semantics; inheritance flags on create.

---

## G11. Errors and Event IDs

**Project fact (`error_id.md`, now stale).** Scheme `CA-NNNN` sequentially assigned, **not** hash-derived: a 4-byte hash lands outside Windows' compatible range essentially every time and collides around 65,000 entries (birthday paradox). Reserved ranges: 1000 allocator, 2000 WAL, 3000 SMART/handler, 4000 reserved. Defined: `CA-1001` per-attempt ENOSPC, `CA-1002` extent fragmentation not allowed (`extent_limit=1`), `CA-1003` identifier slots exhausted. EDQUOT and EFBIG deliberately not adopted (no quota or independent file-size cap exists). Linux surfaces codes in dmesg/syslog/journald text; Windows uses the numeric part as Event ID with `CA-NNNN` in the message text, which needs a registered event source (administrator rights, already required by the kernel helper).

**Found — Windows limits.** Message identifiers are capped at 0–65,535 and the message compiler rejects larger values. The 32-bit EventID splits into qualifiers (high 16 bits) and the ID (low 16 bits). The 32-bit DWORD bit layout: 2 bits severity, 1 customer, 1 reserved, 12 facility, 16 code. Namespace is per event source; messages live in a message file (.mc → .dll) registered with the source.

**Inference.** "4 bytes = 4 billion" was wrong. Leading-digit-equals-subsystem breaks above 65,535 (about 5.5 leading-digit blocks), so subsystem ranges are assigned in blocks within 0–65,535. The application always receives an errno or NTSTATUS, so a CAFS error must map to one even when the CAFS name is more specific (`CA-1001` must return ENOSPC though the device isn't full). Every logged Windows event needs a numeric ID regardless of its name.

---

## G12. Windows TRIM and kernel helper

**Found.** `DeviceDsmAction_Trim` in `IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES` is not supported for user-mode applications. `FSCTL_FILE_LEVEL_TRIM` works on a file inside an existing filesystem: it's a hint, ranges are shrunk to page alignment (4096 B on x86/x64), trimmed data may read back as original data, zeros, ones, or a mix, it fails on EFS-encrypted or compressed files, and it isn't rolled back with an aborted transaction. It doesn't serve a filesystem that owns the raw partition. A WinFsp-specific trim capability: searched twice, nothing found either way.

**Project fact (`io_engine.md`).** A kernel bugcheck is not catchable by SEH and neither is an IRQL violation; the real safety net is a VM with a kernel debugger attached, snapshot before each run. Hence minimal kernel surface (a narrow trim-relay driver). A full Windows kernel filesystem driver is a post-V1 experiment.

**Inference — re-audit checklist** (not performed; needs the ADR set). (1) Read-back of trimmed ranges is nondeterministic, so scrub, ECC, verification and relocation must never expect old contents. (2) Don't trim a relocation log's "old physical LBA" before its entry is retired. (3) A block is trim-eligible only at true refcount 0 (snapshots, lazy CoW-break, dedup-redirect, raw opt-in). (4) New free events: WAL drain/shrink, the losing copy in dedup-redirect. (5) Trim must follow the durable commit of the free.

---

## G13. FUSE concurrency

**Found.** A kernel commit describes "single threaded NTFS-3G" and fixes a hang: on a `fuseblk` mount a delayed RELEASE could trigger a DESTROY request a single-threaded server couldn't service. ntfs-3g's internal lock primitive: still not found (same gap as 09-13).
**Inference.** Only relevant if CAFS ran a single-threaded server on `fuseblk`.
**Project fact.** `syscalls.rs` counters are relaxed atomic increments (read count, write count, bytes, write-size sum/count), not yet implemented. `CafsCtx` concurrency beyond the counters (shared Dedup/Hash tables across FUSE worker threads) is flagged but unresolved in the concerns file.

---

# PART 4 — NOT RESEARCHED (consolidated gaps)
`zpool create`; btrfs relocation internals; WinFsp handle-lifetime analogue and any WinFsp TRIM capability; kernel behaviour on generation change; ext4 orphan-list mechanics; sub-block firmware dedup granularity and current SSD controller behaviour; EFS/APFS key derivation depth; CIFS behaviour; XFS inode sizing; legacy filesystems (ReiserFS, JFS, HFS+); ntfs-3g lock primitive; the TRIM audit itself; the in-place overlapping move beyond the derivation above.

# §S. Sources
- libfuse `fuse_lowlevel.h` (fuse-3.16.2 and AOSP mirrors); Linux kernel commit "fuse: fix hang of single threaded fuseblk filesystem"
- kernel.org fscrypt documentation; ext4 inode documentation; e2fsprogs resize2fs and mke2fs man pages; JBD2 documentation; bcachefs journal documentation
- pvmove(8); Level1Techs, vk4msl.com and Manjaro forum posts on failed partition moves; GParted release notes; USPTO US 5,675,769 / 5,930,831 / 6,178,487
- Microsoft Learn: `FSCTL_FILE_LEVEL_TRIM`, `IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES`, Event Log message identifiers
- Oracle Solaris 11.4 "The copies Property"; TrueNAS and FreeBSD forum threads on `copies=2`; mkfs.btrfs(8) and btrfs-device(8); Fedora btrfs thread; Arch forum thread (unsourced claim)
- ACM Queue 1985003 (Rosenthal, 2011); DSHR's Blog 2011-03-08; StorageMojo; AnandTech SandForce article
- Samba `vfs_acl_xattr(8)`; SambaXP 2011 and 2013 slides; Tuxera Fusion SMB docs; lkml 2000 NT-ACL thread; OpenSolaris `ntaccess.h`; WinFsp `posix.c`; LWN "Rich access control lists" (2015); IETF `draft-ietf-nfsv4-posix-acls-01`, `draft-eriksen-nfsv4-acl`; Fedora COPR richacl
- NTFS `$Secure` and ext4 `mbcache` references from the 4th-mode survey; APFS reference and EFS documentation from the encryption survey
- Project files: `layout.rs`, `io_engine.md`, `error_id.md`, `fs.info` §7a, 09-19 job-4 research and concerns files
