# CAFS — Job 4 Research
**Date:** 2026-09-19
**Purpose:** Primary-source research backing this session's ADRs (056–065) and the still-open items in the issue list. Organized by topic, not by ADR — several ADRs draw on more than one section below.

---

## 1. Encryption — fscrypt (Linux native, most relevant precedent)

Checked directly against kernel.org's own fscrypt documentation, current through the 6.x series.

- **Integrated, not stacked.** fscrypt is built into the filesystem itself (currently ext4, F2FS, UBIFS, CephFS), unlike eCryptfs, which stacks on top of an existing filesystem. Being integrated avoids double-caching both encrypted and decrypted pages, nearly halving memory use versus a stacked approach, and needs half as many dentries/inodes. Directly relevant to CAFS as a from-scratch filesystem: this argues for encryption being a first-class part of the Identifier Table row and I/O path, not a bolted-on layer.
- **Scope: content and filenames only.** fscrypt does not encrypt general filesystem metadata. A file's mode, ownership, and (per this session's other findings) timestamps stay in the clear even when its content and name are encrypted. Sets a natural scope boundary for CAFS to match if it wants a directly comparable feature.
- **Per-file keys are derived, not independently generated or wrapped.** Two policy versions exist. v1 (largely superseded): AES-128-ECB encrypts the master key using the file's own 16-byte nonce as the AES key, and the ciphertext is the derived key. v2 (current): HKDF-SHA512, master key as input keying material, no salt, a distinct context string per purpose (content key vs. filename key derived separately from the same master key). Key derivation was chosen over key wrapping specifically because wrapped keys need larger xattrs, less likely to fit inline in an inode-sized structure — the same kind of byte-budget pressure this session hit repeatedly on CAFS's own Identifier Table row.
- **Deliberately excludes anything that isn't stable from the derivation.** fscrypt's design notes explicitly considered including the inode number in the key-derivation IV, and rejected it — because it would have prevented ext4 filesystems from being resized. Directly applicable to CAFS: whatever a per-file key derivation uses as its per-file input must not depend on anything that can change under CAFS's own relocation mechanism (ADR-061) or under identifier reuse — a stable, immutable per-file nonce (generated once, stored once, never touched again) is the safe choice, not the identifier or the physical LBA.
- **No special privilege or mount handling required** — the API is usable by unprivileged users, consistent with per-file, per-user control rather than an all-or-nothing volume setting.

Not yet researched: NTFS EFS's and APFS's exact key-derivation mechanics in the same depth — this pass covered the Linux-native precedent most directly applicable to CAFS's own FUSE context first.

---

## 2. Hash algorithm — SHA-256 vs. BLAKE3-256, for the Global Hash Table

Asked directly, not previously answered. Both produce a 256-bit/32-byte digest, so the entry-format math (32-byte hash + 8-byte value = 40 bytes) is identical either way — this is purely a choice of which algorithm computes that hash, not a size question.

**SHA-256**: SHA-2 family, standardized (FIPS 180-4), over 20 years of broad, adversarial, real-world scrutiny with no practical break of the full algorithm found. Merkle–Damgård construction — processes input sequentially, one block at a time; cannot parallelize a single hash computation across cores. Fast on hardware with dedicated instructions (Intel/AMD SHA extensions, equivalent ARM extensions), meaningfully slower in plain software on hardware without them.

**BLAKE3**: newer (2020), from the BLAKE/BLAKE2 lineage (BLAKE2 was a SHA-3 finalist; Keccak was chosen instead, but BLAKE2's design has its own long separate track record). Merkle tree construction, not Merkle–Damgård — different chunks of one input can be hashed in parallel across cores/SIMD lanes, and the tree structure also enables efficient incremental/streaming verification. Consistently faster than SHA-256 in software, with or without hardware acceleration, and scales with core count on large inputs in a way SHA-256 structurally cannot. Not a government or international standard — no FIPS certification, which matters only if CAFS ever needs to run in an environment that specifically requires FIPS-validated cryptographic modules; disqualifying for that specific case, irrelevant otherwise. Less deployment history than SHA-256, though no known weaknesses from its own review.

**For CAFS specifically**: BLAKE3-256 reuses a dependency already vendored and used elsewhere in the format (checksums, pointer verification), and is faster on what will be the highest-volume hash in the system once per-LBA hash-table modes are active. SHA-256 is the safer choice only if FIPS compliance is ever a target market. Recommendation stands as BLAKE3-256 unless that's a real goal.

---

## 3. Cross-filesystem flag and attribute survey (BTRFS, XFS, ZFS, F2FS, bcachefs, NTFS, APFS)

Full findings are recorded against the specific CAFS decisions they informed in the issue list (items 33, 35, 36, 38, 39) — summarized here as a standalone reference rather than repeated in full.

- **BTRFS**: supported attributes are `i` (immutable), `D`, `m`, `S`, `V`, no-dump only — explicitly, "no other attributes are supported." Does not implement append-only despite it being in the generic `chattr` list. No "delete-only" attribute exists anywhere checked.
- **XFS**: `fsxattr` struct — a small, fixed set of per-file `u32` numeric fields (`fsx_extsize`, `fsx_nextents`, `fsx_projid`, `fsx_cowextsize`) alongside its flags bitmask (`fsx_xflags`, including `XFS_XFLAG_IMMUTABLE`). Real-world precedent for handling a handful of per-file numeric attributes as plain additional fields rather than a generic key-value store.
- **ZFS**: properties (`compression`, `dedup`, `sync`, `recordsize`, etc.) are per-*dataset* only — no per-file equivalent exists; the documented workaround for finer control is creating more, smaller datasets.
- **F2FS**: `pin_file` (GC/relocation exemption, not absolute — still reclaimed past a failure-count threshold), `atomic_file` (all-or-nothing multi-write transaction marking), `inline_data` (very small files stored directly in the inode, no separate extent).
- **bcachefs**: options explicitly tagged by scope — `format,mount,runtime` (volume-only) vs. adding `,inode` (also settable per-file/per-directory via `setattr`). `nocow` breaks CoW transparently only when an actual snapshot or reflink reference exists on that data ("no CoW while shared"), otherwise writes in place.
- **NTFS**: `READ_ONLY`, `HIDDEN`, `SYSTEM`, `ARCHIVE`, `TEMPORARY`, `SPARSE_FILE`, `REPARSE_POINT`, `COMPRESSED`, `OFFLINE`, `ENCRYPTED`, `NOT_CONTENT_INDEXED`, `VIRTUAL`. Real per-file EFS encryption.
- **APFS**: `clonefileat(2)` — explicit, user-invoked, instant per-file/per-directory clone, CoW-shared until modified. Multi-key encryption: a separate key per file for data, another for sensitive metadata.
- **Generic inode-flags family** (the wider lineage BTRFS/ext4 draw from): `NO_DATA_COW` — "no CoW while refcount is 1," a second, independent confirmation of the same conditional-no-CoW mechanism bcachefs uses. Also `NO_DATA_SUM` (per-file checksum opt-out), `PREALLOC` (narrower than a full raw/no-CoW guarantee — avoids CoW only for specific already-preallocated extents), `NO_ATIME`, `DIR_SYNC`.

Not yet covered: legacy/niche filesystems (ReiserFS, JFS, HFS+) — judged low value, not pursued unless wanted.

---

## 4. Timestamp width — Y2038 and range

`s64` seconds since epoch has a range of roughly ±292 billion years — several times the age of the universe. No realistic overflow risk at any point in the future, at 1-second granularity, full stop. `s128` would only ever be justified by a desire to pack sub-second precision into the same field via fixed-point math, which is unnecessary complexity next to a plain second-field-plus-nanosecond-field split. The standard, precedented shape for a high-precision, unbounded-range timestamp is POSIX's own `timespec`: `s64` seconds + `u32` nanoseconds (nanoseconds-within-a-second never exceeds ~1 billion, comfortably inside a 32-bit field) — 12 bytes total, not a monolithic wide integer.

---

## 5. Zone-level ECC — why it doesn't work on SSD

Not a citation-driven research item so much as a mechanical fact about how SSDs work, recorded here since it drove ADR-065: an SSD's flash translation layer (FTL) owns real physical placement of logical blocks and moves them continuously for wear-leveling, invisibly to the OS and filesystem. A filesystem-level RAID-style stripe has no way to guarantee its data and parity blocks land on physically independent NAND dies — the FTL could place both on the same die, and if that die fails, the data and the thing meant to recover it are lost in the same event, defeating the redundancy's whole premise. HDDs don't have this problem, since LBA-to-physical mapping is direct enough (aside from rare remapped bad sectors) for genuine physical independence between stripe members. SSDs also already run their own internal hardware ECC (commonly LDPC-based) against the bit-flip-level errors a software scheme would otherwise target, leaving little for a software layer to usefully add even before the co-location problem is considered.
