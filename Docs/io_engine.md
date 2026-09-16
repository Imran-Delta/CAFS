# CAFS I/O Engine

**Document version:** v1
**Status:** Constraints only — not a design document. The I/O Engine wasn't discussed in depth this session; what follows is what was pinned down, not a full architecture.

---

## Naming correction

`mntproc` (the FUSE request handler) is **Rust**, not C — several existing ADRs and docstrings still say "C process"/"C engine" from an earlier point in the project. Worth a wording pass wherever that appears; not a design change, just stale text.

## Permission enforcement: delegated, not implemented here

Linux: mount with `default_permissions` — the kernel's own `generic_permission()` does the actual check before a request reaches mntproc at all. mntproc's only obligation is reporting accurate `st_mode`/`uid`/`gid` via `getattr` (the Identifier Table already carries these).

Windows/WinFsp: same shape. WinFsp passes the file's security descriptor to Windows' own `AccessCheck` API; the kernel's Security Reference Monitor enforces. WinFsp auto-translates POSIX uid/gid/mode into a compatible security descriptor. A `FileSecurity` (SDDL) option exists for full native ACL control if POSIX-mode mapping is ever insufficient.

**Consequence**: the I/O Engine and the allocator do not need to re-check permissions — that's the kernel's job on both platforms. This does **not** extend to filesystem-*state* rejections (no free space, `hard_floor_active`, a raw file hitting a snapshot conflict) — those remain the allocator's responsibility (see Docs/allocator.md Section 7), a different category from permission checking entirely.

## I/O counters: `syscalls.rs`, non-blocking

Every filesystem-atomic read/write syscall bumps counters as part of the call itself — plain atomic increments (`AtomicU64::fetch_add`, relaxed ordering), genuinely lock-free for values that don't need to update jointly with another value in the same instant. Read count, write count, bytes read, bytes written, and write-size sum+count (sufficient for a mean; no full histogram needed) are the concrete set the allocator's Signals layer currently depends on (Docs/allocator.md Section 4).

This is not optional or replaceable by kernel-level stats — it's the only place workload-shape data (specifically, the LBA distance between consecutive writes that `R_write` needs) exists at all. `/proc/diskstats` or `IOCTL_DISK_PERFORMANCE` answer a different question — total physical device load, useful for idle-detection — not per-operation semantic detail, since OS-level write coalescing and readahead sit between what CAFS requests and what the drive actually does. Both are needed; they don't substitute for each other.

**Not yet implemented** — this is a placeholder in `smart_handler.py`'s payload shape (`io_placeholder`) pending real `syscalls.rs` wiring.

## Windows kernel code

Confirmed direction: Windows gets kernel-mode helper driver code, not a pure user-mode implementation for everything. `Src/Kernel/Windows/helper.rs` is the placeholder location for all such helper files (currently just the TRIM-relay driver, see Docs/error_id.md's context and the session's TRIM discussion — `IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES`'s `Action=Trim` is unavailable to user-mode apps, forcing this).

**Constraint on how this gets developed, not just what gets built**: a kernel-mode bugcheck (`KeBugCheckEx`) is not a catchable exception — SEH (`__try`/`__except`) cannot intercept it, nor can it catch an IRQL-level violation (one of the most common real driver bugs, with two dedicated bug check codes). There is no code-level wrapper that makes kernel-mode iteration as safe as user-mode. The actual safety net is environmental: develop and test in a VM with a kernel debugger attached, snapshot before each run, revert after a crash. Keeping kernel-mode surface area minimal (a narrow trim-relay driver, not the whole filesystem) is a direct consequence of this constraint, not a separate stylistic preference.

## V1 scope

FUSE (Linux, via the above) and WinFsp (Windows, user-mode) are the actual V1 release target. A full Windows kernel-mode filesystem driver (porting the whole of mntproc's logic, not just the TRIM helper) is explicitly a post-V1 PoC/experiment, contingent on the VM-based testing constraint above — not a committed deliverable.
