# CAFS I/O Engine

**Document version:** v2 (2026-10-24)
**Status:** CURRENT. Constraints, plus the engine rules salvaged from the 2026-08-08
spec (`io-engine-data.txt`, now removed; see section 13 for what was dropped).
Not a full architecture.
**Tags:** items marked **[verify]** come from the 08-08 spec and have not been re-checked
against `block_io.rs` or `fs.info` v22.
**Related:** `fs.info` (on-disk format), `config.fs` (settings), `allocator.md`,
`error_id.md`, `OPEN_ITEMS.md` (B11, B12).

---

## 1. Role and boundaries

The I/O Engine is the bottom layer: the only component that opens the block device and
reads or writes it. Everything above it (FUSE worker, allocator, cleaner, scrubber,
recovery) goes through its operations.

**It does:**
- Open and close the block device.
- Read and write blocks at given addresses.
- Compute and verify checksums.
- Keep host-level health counters (SMART Buffer feed, section 8).
- Handle direct-I/O alignment.
- Hide platform differences (Linux, Windows).

**It delegates upwards:**
- Filesystem semantics: identifiers, extents, directories, B-trees.
- Block allocation and relocation (allocator).
- Drive firmware S.M.A.R.T. (the drive's own job).
- WAL replay and recovery logic.
- SMART table reconciliation (Smart Handler).
- Zone Table, Dedup Table, relocation log, caches.

## 2. Naming correction

`mntproc` (the FUSE request handler) is **Rust**, not C. Several existing ADRs and
docstrings still say "C process" or "C engine" from an earlier point in the project.
Worth a wording pass wherever that appears; not a design change.

## 3. Settings the engine receives

The engine does **not** parse `config.fs`. The upper layer parses it and passes the
values in. Keys currently present in `config.fs`:

| Key | Section | Meaning |
|---|---|---|
| `block_size` | `[physical]` | Expected block size; validated against the on-disk value, mismatch refuses to open |
| `direct_io` | `[mount_hints]` | Use direct I/O (`O_DIRECT` / no-buffering) |
| `direct_io_required` | `[mount_hints]` | `false`: fall back to buffered I/O if direct I/O is unsupported; `true`: fail |
| `sync_open` | `[mount_hints]` | Open with `O_SYNC` / write-through; `false`: durability comes from explicit flushes |
| `destroy_flush` | `[io]` | Flush (with a warning) when the context is destroyed |
| `misaligned_action` | `[io]` | `copy` (default) or reject a misaligned caller buffer |
| `pointer_checksum_algo`, `data_checksum_algo`, `anchor_checksum_algo` | `[physical]` / `[integrity]` | Algorithm per structure class (currently crc32c / xxhash64 / blake3-128) |

If a config is passed and an on-disk Config Snapshot exists, the engine validates the
passed config against it. **[verify]**

## 4. Operations

Language-neutral list from the 08-08 spec. The Rust names and signatures are whatever
`block_io.rs` exports; reconcile this table with that file (OPEN_ITEMS B11).

| Operation | Purpose |
|---|---|
| init / destroy / destroy-with-flush | Open the device and validate it; release it, optionally flushing |
| read block | Read one block; flag `verify` checks the checksum |
| write block | Write one block; flags `compute` (engine computes the checksum vs caller supplied) and `flush` |
| verify checksum | Tri-state result: match / mismatch / I/O error |
| read metadata | Read a block **without** checksum verification, for recovery or when the checksum itself is corrupt |
| total blocks, block size | Device geometry |
| flush | Make written data durable |
| compute checksum | Pure helper for any algorithm |
| zero block | Convenience write of zeros |
| increment relocation | Allocator reports a host-level relocation so the counter stays right |

## 5. Direct I/O, alignment, scratch buffers

- With direct I/O on, the buffer address, file offset and length must all be aligned.
  Offsets and lengths are block multiples automatically.
- Windows: the alignment unit is the physical sector size (queried through the disk
  geometry IOCTL). Use `block_size` if it is a multiple of the sector size, otherwise
  the sector size.
- A misaligned caller buffer is handled by copying through a scratch buffer
  (`misaligned_action = copy`, default) or rejected with an invalid-argument error.
- **Scratch buffer**: provided by the caller per call; size at least
  `max(block_size, 4096)`; aligned for direct I/O; each concurrent call needs its own
  (or the caller serialises). The engine neither allocates nor shares scratch buffers.
  **[verify]** whether the Rust API keeps caller-provided scratch.
- Anchor-region structures are written as whole 4096-byte aligned units regardless of
  `block_size` (see `fs.info` and `DECISIONS-FORMAT-8` G2).

## 6. Platform access

| | Linux | Windows |
|---|---|---|
| Open | `openat`; `O_DIRECT` and `O_SYNC` per settings; retry without `O_DIRECT` on `EINVAL` if allowed | `CreateFile` with read+write, no-buffering and write-through flags |
| Read / write | `pread64` / `pwrite64` | `ReadFile` / `WriteFile` with `OVERLAPPED` offsets |
| Flush | `fdatasync`, fallback `fsync` | `FlushFileBuffers` |
| Device size | `ioctl(BLKGETSIZE64)` | `IOCTL_DISK_GET_LENGTH_INFO` |

**Retries** **[verify]**: retry on `EINTR` up to 3 times. Retry short reads and writes only
on regular files; on a block device a short transfer is an I/O error.

**Sector sizes**: the logical and physical sector sizes are recorded in the Superblock at
format time and compared with what the OS reports at mount (G2 trust rule; a mismatch
always warns; format-time and mount-time overrides, mount wins).

## 7. Thread safety **[verify]**

- Offset-based `pread`/`pwrite` give concurrent I/O to different blocks without a shared
  file position.
- Health counters use atomic operations; batch flushing is lock-free (section 8).
- No internal locks; callers own scratch-buffer serialisation.

## 8. Counters

Two distinct counter sets exist; both are needed.

### 8a. Workload counters for the allocator (`syscalls.rs`, non-blocking)

Every filesystem-atomic read or write bumps plain atomic counters as part of the call
(`AtomicU64::fetch_add`, relaxed ordering). Read count, write count, bytes read, bytes
written, and write-size sum + count (enough for a mean, no histogram) are what the
allocator's Signals layer depends on (`allocator.md` Section 4).

This cannot be replaced by kernel statistics. Only here does the LBA distance between
consecutive writes exist, which `R_write` needs. `/proc/diskstats` or
`IOCTL_DISK_PERFORMANCE` answer a different question (total device load, idle detection),
since OS write coalescing and readahead sit between CAFS and the drive.

**Not yet implemented:** a placeholder in `smart_handler.py`'s payload (`io_placeholder`)
pending real `syscalls.rs` wiring.

### 8b. Host health counters (SMART Buffer feed) **[verify]**

Path: engine -> SMART Buffer -> Smart Handler -> B-SMART -> WAL -> M-SMART
(`fs.info` section 11.2). Table layout: `fs.info` section 11 (`mount_count` @68,
`clean_unmount_flag` @72).

| Counter | Update trigger | Batch rule |
|---|---|---|
| total reads / writes | every successful read / write | 1024 operations |
| read / write errors | every failure | 2 events or 16 seconds |
| checksum mismatches | every verification failure | 2 events or 16 seconds |
| host relocations | allocator call | same as writes |
| last access LBA | every I/O | same as writes |
| last access timestamp | every I/O | rounded down to 30 minutes: `(unix_time / 1800) * 1800`, applied before entering the accumulator |
| mount count | init | written immediately |
| clean-unmount flag | destroy | written immediately |

**Lock-free batch flush:** each counter has an atomic accumulator; when one crosses its
limit, a thread tries to compare-and-swap a "flush in progress" flag from 0 to 1. The
winner copies counters to scratch, writes, and resets the accumulators; losers carry on.
Timed error flushes use the same mechanism.

## 9. Errors **[verify]**

Return values follow the scheme in `error_id.md` (POSIX name where exact, `CA_Name`
otherwise). Semantics carried over from the 08-08 spec:
- Invalid argument: null, bad address, invalid algorithm, `block_size` mismatch.
- Checksum mismatch on a read or on verify is an I/O error (and bumps the mismatch
  counter).
- Device full on write, permission denied, device not found at init.
- Interrupted calls are retried internally, never surfaced.
- Verify-checksum is tri-state: match, mismatch, or a negative error for an I/O failure.

## 10. Permission enforcement: delegated, not implemented here

Linux: mount with `default_permissions`. The kernel's own `generic_permission()` does the
check before a request reaches mntproc. mntproc's only obligation is reporting accurate
`st_mode`/`uid`/`gid` via `getattr` (the Identifier Table carries these). **ACLs are not
covered by that:** negotiate `FUSE_POSIX_ACL`, serve ACL xattrs, and implement
default-ACL inheritance on create.

Windows/WinFsp: same shape. WinFsp passes the file's security descriptor to Windows'
`AccessCheck`; the Security Reference Monitor enforces. WinFsp translates POSIX
uid/gid/mode into a compatible security descriptor; a `FileSecurity` (SDDL) option gives
full native ACL control.

**Consequence:** neither the I/O Engine nor the allocator re-checks permissions. This does
not extend to filesystem-*state* rejections (no free space, `hard_floor_active`, a raw
file hitting a snapshot conflict); those stay the allocator's job (`allocator.md`
Section 7).

## 11. Windows kernel code

Confirmed direction: Windows gets a kernel-mode helper, not a pure user-mode
implementation for everything. `Src/Kernel/Windows/helper.rs` is the placeholder for all
such helper files (currently the TRIM relay: `IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES`
with `Action=Trim` is unavailable to user-mode callers). The helper is optional, a
separate binary, and unsigned; CAFS runs without it (`trim_mode`, `reserve_percent`).

**Constraint on how it is developed:** a kernel-mode bugcheck (`KeBugCheckEx`) is not a
catchable exception. SEH (`__try`/`__except`) cannot intercept it, nor an IRQL violation
(one of the most common real driver bugs). No code-level wrapper makes kernel iteration as
safe as user mode. The safety net is environmental: develop and test in a VM with a kernel
debugger attached, snapshot before each run, revert after a crash. Keeping the surface
minimal (a narrow trim relay, not the filesystem) follows directly from this.

## 12. V1 scope

- FUSE (Linux x86-64) and WinFsp (Windows, user mode) are the V1 release target.
- Other architectures and macOS are out of scope for V1. The 08-08 spec also listed Linux
  and Windows x86 (32-bit); the repository currently has only `x86-64`. **[verify]** whether
  32-bit is intentionally dropped.
- A full Windows kernel-mode filesystem driver (the whole of mntproc, not just the TRIM
  helper) is a post-V1 experiment, contingent on the VM-based testing constraint above.

## 13. History: what the 2026-08-08 spec said that no longer holds

| 08-08 spec | Now |
|---|---|
| "LOCKED v1.0", engine written in assembly with a C ABI (`cafs_io_*`) | Rust; the API is whatever `block_io.rs` exports |
| LBA is a block index; byte offset = LBA x block_size | Not defined in one place in `fs.info`; anchor slots are listed by byte offset (OPEN_ITEMS B12) |
| Superblock at LBA 0 with a pointer table at Superblock offset 16 (512 entries; entry 7 = SMART Table) | Six anchor pointer blocks (LBA0-5), Superblock with fixed fields, Meta/User Pointer Table, Function Table (`fs.info` sections 2-8) |
| 4-byte checksum at the end of every block; CRC32C or XXHASH32 only | 16-byte checksum trailer on every data block; per-structure algorithms (`fs.info` 15a, `config.fs`) |
| Dedicated SMART Table with its own magic and layout; engine increments sequence at init | `fs.info` section 11 (M-SMART / B-SMART) |
| Init sequence (20 steps) | Anchor discovery and recovery order (`fs.info` sections 2, 14); `mount.rs` |
| Error codes as raw negative errno | `error_id.md` scheme |
| License: BSD-3-Clause | Unchanged (userspace); `Src/Kernel` is GPL-2.0-only |
