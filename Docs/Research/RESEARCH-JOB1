# Research Log — Job 1 (On-Disk Binary Data)

Scope: external research performed during Job 1, as distinct from internal design decisions (those are recorded in the ADRs). Each entry states the question, what was actually checked, the finding, and what it fed into.

---

## 1. FUSE `max_read` / readahead ceiling

**Question**: is a 128KiB-per-request ceiling a hard, unavoidable FUSE limitation, and is there a known mechanism to raise it for large sequential files?

**Checked**: current `libfuse` documentation and source for `max_read`, `max_readahead`, and the 2022 LKML thread proposing `FUSE_TRUST_MAX_RA`.

**Findings**:
- `max_read` caps the size of a single FUSE-level read operation. Historically 128KiB (32 pages) regardless of configured value — this was never an artificial limit imposed by configuration, it was the kernel's own ceiling.
- `FUSE_TRUST_MAX_RA` (proposed March 2022) was checked directly against current `libfuse` source (`fuse_common.h`, `master` branch) — it does **not** exist. Capability bits at the positions it would have occupied are in use by unrelated, later features (`FUSE_CAP_NO_EXPORT_SUPPORT`, `FUSE_CAP_OVER_IO_URING`, `FUSE_CAP_ALLOW_IDMAP`). The patch never landed under that name.
- However: `max_readahead` in `struct fuse_conn_info` has no hardcoded clamp in the struct itself — settable directly by a filesystem's `init()` handler or via mount option. And the real modern per-request ceiling comes from a different, already-merged mechanism: `FUSE_MAX_MAX_PAGES = 256` pages, negotiated via `max_write`/`max_read`, which is 1MiB at a 4KiB page size — well above the old 128KiB default.

**Fed into**: the recommendation to key `max_read`/`max_readahead` off the existing `sequential`/`random` optimization hint rather than adding a new signal or a file-size-probing syscall — the capability to raise the ceiling for sequential files already exists via `max_pages` negotiation, no rejected/nonexistent kernel feature required.

---

## 2. BTRFS transparent compression chunk size

**Question**: what compression chunk size does BTRFS actually use, and why — needed to check an initial 64MiB–1GiB proposal against real precedent before adopting a number.

**Checked**: BTRFS documentation and technical references on compression extent size and `zstd`/`lzo` handling.

**Finding**: BTRFS uses **128KiB**, not 1GiB or anywhere near it. Kept deliberately small for three concrete reasons: it bounds the RAM required to decompress on any single read; it prevents one small random read from forcing decompression of a much larger unit than needed; and it allows decompression to be parallelized across CPU cores by chunk.

**Fed into**: ADR-025. The 128KiB default was adopted as-is rather than reinvented; the reasoning directly informed the decision to keep 1GiB as an explicit, narrow opt-in rather than a recommended value, since a 1GiB chunk would reproduce exactly the random-read amplification problem BTRFS's small default exists to avoid.

---

## 3. Windows ACL compatibility without NTFS IP exposure

**Question**: is there a legally clean, already-proven way to support Windows-style ACL permissions without reverse-engineering or infringing on Microsoft's unpublished native NTFS on-disk format?

**Checked**: Microsoft's published specification program, and real-world implementations that already provide Windows-compatible ACLs on non-NTFS storage (Samba, ZFS).

**Findings**:
- The native NTFS on-disk format is correctly identified as unpublished/proprietary — not a safe implementation target.
- Separately, and safely: **MS-DTYP** (`SECURITY_DESCRIPTOR`, ACL, ACE, SID wire structures) is published openly by Microsoft under its Open Specification Promise, which includes an explicit patent non-assert covenant for implementers. This is not the on-disk format — it's the protocol-level structure definition used for interoperability.
- Real production precedent implementing exactly this exists: Samba's `vfs_acl_xattr` and `nfs4acl_xattr` modules, and ZFS's native ACL support, both store Windows-compatible permission semantics via these published structures (or NFSv4 ACL semantics, an IETF-published open standard with comparable expressiveness) without touching NTFS internals.
- WinFsp — already the stated Windows FUSE-equivalent for this project — operates at its API boundary using these same MS-DTYP structures directly.

**Fed into**: ADR-024. `acl_blob_offset` stores the self-relative `SECURITY_DESCRIPTOR` blob verbatim as handed off by WinFsp, rather than a custom or reverse-engineered format — avoiding both the legal risk and the need to build a translation layer.
