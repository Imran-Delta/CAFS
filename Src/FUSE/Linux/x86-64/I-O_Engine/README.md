# CAFS I/O Engine — V1 (Rust, Linux x86-64, kernel 6.12+)

Rust rewrite of the retired C+asm engine (ADR-011/012). Same scope:
base I/O Engine library plus a format tool. No allocator, no
filesystem semantics. FUSE-only. BSD-3-Clause.

**Retire the old C tree**: delete `*.c`, `*.h`, `*.S`, `Makefile`,
`test/` from this directory and `vendor/blake3/` at the repo root —
BLAKE3 is now a normal crate dependency, nothing is vendored anymore.

## Build

```
cargo build --release   # builds libcafs_io and mkfs_cafs
cargo test               # unit tests + the full mkfs->mount->check->remount->confirm integration test
```

## MSRV note

Verified in a sandboxed environment whose only available `rustc` was
an apt-packaged 1.75.0 — old enough that `blake3` 1.8.3+ and
`tempfile` 3.16+ pull in transitive dependencies requiring the
`edition2024` Cargo feature, which 1.75 can't parse. `Cargo.toml`
pins `blake3 = "=1.8.2"` and `tempfile = "=3.15.0"` for that reason,
with the reasoning left in place as comments. A normal toolchain
(`rustup`, or this repo's own CI via `dtolnay/rust-toolchain@stable`)
has no reason to hit this — unpin both to `"1"`/`"3"` once building
outside a similarly old environment.

## Try it by hand

```
./target/release/mkfs_cafs /tmp/cafs.img   # 4 MiB test image, default size
```

Then, from another crate or a small `main.rs`:

```rust
let mut ctx = cafs_io::CafsCtx::mount("/tmp/cafs.img", true)?;
let result = ctx.check()?;
let opts = cafs_io::MountOpts { destroy_flush: true, ..Default::default() };
let snapshot = ctx.remount(&opts)?;
ctx.confirm()?;
ctx.close()?;
```

See `src/lib.rs` for the full API and `tests/pipeline.rs` for a
worked example, including the corruption/auto-repair path.

## Pipeline (unchanged from the C version — ADR-006/007/008 still apply)

1. **mount** — opens the device, reads the anchor blocks. LBA0's own
   pointer-block checksum is the one thing verified here (everything
   else hangs off trusting it); past that, best-effort — a failed
   read just leaves the field unpopulated (`Option`/empty `Vec`), it
   never aborts the mount.
2. **check** — verifies the Superblock's BLAKE3-128 checksum. On
   mismatch: tries Superblock Backup; if good, copies it over the
   primary in place (if writable), fsyncs, and logs the repair to
   stderr. All other anchor structures are not checked or repaired in
   V1 (ADR-007).
3. **remount** — extracts crash-critical fields from the Config
   Snapshot's TOML (targeted key scan, not a general parser), applies
   caller-supplied `MountOpts` on top. Callable multiple times: mount
   read-only -> remount read-write to persist a repair -> remount
   again with the final on-disk config.
4. **confirm** — the only stage that writes unconditionally:
   Superblock `last_mount_time` + recomputed checksum, committed
   primary -> backup -> recomputed Parity, each `fdatasync`-ed. Also
   increments M-SMART's `mount_count`/`clean_unmount_flag`,
   best-effort (ADR-008).

`ctx.close()` isn't one of the four named stages, same reasoning as
before: without it, `destroy_flush` never runs. Unlike the C version,
forgetting to call it doesn't leak the file descriptor either way —
`OwnedFd`'s `Drop` impl closes it regardless.

## What changed from the C version, concretely

- **Syscalls**: `rustix` (ADR-012) instead of hand-written assembly.
  Its direct-syscall backend still does the same "no libc" thing the
  assembly did, just safely wrapped. The only `unsafe` block left in
  the entire engine is the one ioctl (`BLKGETSIZE64`) rustix doesn't
  have a named wrapper for yet.
- **Checksums**: official `blake3` crate (automatic SIMD, was
  deliberately software-only in C), `crc32c` crate (hardware-
  accelerated via SSE4.2 with runtime CPUID, was hand-written
  software-only in C), `xxhash-rust` (was hand-written in C). All
  three fix limitations the C engine's README explicitly flagged as
  known gaps.
- **Type safety concretely used, not just claimed**: `ByteOffset` is
  a newtype, not a bare `u64` — mixing it up with an anchor slot
  index (the exact class of bug ADR-010 documents in the C port,
  `function_table_anchor_lba` holding a slot index while every other
  `real_lba`-style field holds a byte offset) is now a compile error.
  `ctx.close()` consumes `self`, so using `ctx` afterward is a compile
  error too, not a runtime use-after-close bug.
- **What the compiler did *not* catch**: while porting `check()`, the
  computed pointer-checksum-algorithm-used value was wired into a
  local variable and never assigned to the result struct — silently
  compiled clean, no warning, because it's a logic gap, not a type or
  borrow error. Only the integration test caught it. This is the same
  bug class (a value computed but never wired to its output field)
  found in the *C* version's `pointer_checksum_algo_used` field during
  that engine's own testing — direct, repeated evidence for ADR-011's
  point that compile-time guarantees and specification/logic
  correctness are different axes, not a substitute for testing.
- **A real borrow-checker catch**: the first compile attempt rejected
  `read_raw(&ctx, ..., &mut ctx.superblock)` — simultaneous immutable
  (whole-struct, for the fd) and mutable (one field) borrows. Fixed by
  reading into a local buffer first. This one *was* a compiler catch,
  not a test catch — the kind of aliasing mistake the C version had
  no equivalent guard against.

## Known limitations (unchanged from the C version unless noted above)

- `misaligned_action=copy` bounces through a 512-byte-aligned heap
  buffer; some 4Kn devices may want 4096-byte alignment instead — not
  currently queried via `BLKSSZGET`/`BLKPBSZGET`.
- Endianness: all multi-byte fields assume little-endian on disk,
  safe for the x86-64-only V1 target.
- `xxhash-rust`'s license (BSL-1.0) is a different permissive family
  than the MIT/Apache-2.0 used elsewhere in this dependency tree —
  compatible with BSD-3-Clause, just noted explicitly rather than
  assumed.
