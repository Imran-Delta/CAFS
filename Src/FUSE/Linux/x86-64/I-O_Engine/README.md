# CAFS I/O Engine — V1 (Linux x86-64, kernel 6.12+)

Base I/O Engine library plus a format tool. No allocator, no filesystem
semantics (no Zone Table, B-tree, dedup, WAL replay) — those are
delegated to layers above this library. FUSE-only. BSD-3-Clause
(kernel-side CAFS code elsewhere in the repo stays GPLv2 — unrelated to
this directory).

## Build

```
make          # builds libcafs_io.a and mkfs_cafs
make test     # also builds and runs test/test_pipeline
```

Requires gcc, binutils (`as`/`ar`), a Linux x86-64 host. No other
dependencies beyond the vendored BLAKE3 files under `../../../../../vendor/blake3`.

## Try it by hand

```
./mkfs_cafs /tmp/cafs.img            # 4 MiB test image, default size
```

Then link a small program against `libcafs_io.a` (`-I. -Lsomewhere
-lcafs_io -lpthread`) and call `cafs_mount("/tmp/cafs.img", 1, &ctx)`,
`cafs_check(ctx, &result)`, `cafs_remount(ctx, &opts, &snapshot)`,
`cafs_confirm(ctx)`, `cafs_close(ctx)` — see `cafs_io.h` for the full
API and `test/test_pipeline.c` for a worked example, including the
corruption/auto-repair path.

## Pipeline (as locked)

1. **mount** — opens the device, reads the anchor blocks (LBA0-2/4
   pointer blocks, Superblock, Superblock Backup, Config Snapshot,
   Parity, Function Table, M-SMART if the Function Table yields a
   usable pointer). LBA0's own pointer-block checksum is verified here
   (everything else hangs off trusting it); everything past that is
   best-effort — a failed read just leaves a `*_valid` flag clear, it
   never aborts the mount. No writes.
2. **check** — verifies the Superblock's BLAKE3-128 checksum. On
   mismatch: tries Superblock Backup; if that's good, copies it over
   the primary in place (if the mount is writable), fsyncs, and logs
   the repair to stderr — "simple and foreseeable," so it fixes rather
   than just reports. If both are bad, reports failure without
   crashing. All other anchor structures are **not** checked or
   repaired in V1 — see "Scope cut" below.
3. **remount** — extracts the handful of crash-critical fields this
   engine needs from the Config Snapshot's TOML (a minimal targeted
   key scanner, not a general TOML parser — see "Config parsing"
   below), then applies caller-supplied `cafs_mount_opts_t` on top for
   the runtime-only knobs (`readonly`, `direct_io`, `sync_open`,
   `misaligned_action`, `destroy_flush`). Reopens the fd if the
   effective flags changed. Callable multiple times — that's the
   point: mount read-only (safe mode) → remount read-write to let
   `check()` persist a repair → do required writes → remount again
   with the final on-disk config. This works because every anchor
   structure is fixed at 4KB regardless of the filesystem's configured
   block size (e.g. 8KiB) — anchor I/O never depends on which
   block_size ends up in effect.
4. **confirm** — the only stage that writes unconditionally: sets
   Superblock `last_mount_time = now`, recomputes its BLAKE3-128
   checksum, and commits it (primary Superblock → Superblock Backup →
   recomputed Parity Block, each `fdatasync()`-ed). If a usable
   M-SMART location was found at mount, also increments `mount_count`
   and sets `clean_unmount_flag = 0` directly on it (not the full
   V-SMART→Buffer→Handler→B-SMART→WAL→M-SMART pipeline — that stays
   the Python SMART Handler's job). Fails on a readonly mount.

`cafs_close()` isn't one of the four named stages — added because
`confirm()` only ever marks a mount as started; without a counterpart,
`destroy_flush` and a graceful fd close never happen.

## Revised anchor design (supersedes fs.info §12 for this component)

End-of-device redundancy is kept **only** for Superblock and SMART
(M-SMART/B-SMART). Config Snapshot, Parity, Function Table, and
Meta/User Table have no backup copy — this removes both the write
amplification and the torn-write surface of keeping several backup
structures in sync on every anchor update. Consequences, precisely:

- Superblock: still has a real backup (via LBA0's fallback pointer),
  still auto-repaired by `check()`.
- Config Snapshot: no direct backup, but stays indirectly
  reconstructable via `Parity XOR Superblock` — not implemented in V1
  (Parity itself isn't verified/repaired, see scope cut below), but
  the data to do so exists on disk.
- Function Table, Meta/User Table: **zero** redundancy. A checksum
  failure on either is unrecoverable data loss for that structure.
  Consistent with the project's existing "won't die easily" floor
  (§7 of `fs.info`'s checksum-failure behavior): losing the Function
  Table costs WAL/SMART-discovery, not the ability to mount at all,
  since `check()` only gates on the Superblock.
- There is no "Backup Parity Block" concept at all — Parity has
  exactly one copy.

This should get its own ADR on the main repo.

## Scope cut: "ignore all anchor block errors except Superblock"

Explicit, temporary, stated instruction for this PoC — the goal was a
working read/write proof of concept first. `mount()` reads Config
Snapshot / Parity / Function Table / M-SMART best-effort; nothing
verifies their checksums or acts on a mismatch. `cafs_check_result_t`
only reports on the Superblock. Extending `check()` to the other
anchor structures (verify-only for Function Table/Meta Table, since
they have no backup to repair from; XOR-reconstruct for Config
Snapshot against Parity) is future work, not done here.

## Config parsing

The Config Snapshot is machine-generated, comment/blank-line-stripped
TOML (`fs.info` §5) — regular enough that a general TOML parser wasn't
worth building for four fields. `remount()` does a targeted
section+key scan for exactly `[physical].block_size`,
`[physical].pointer_checksum_algo`, `[integrity].data_checksum_algo`,
`[integrity].verify_data`. Runtime-only knobs (`[io]`/`[mount_hints]`
— explicitly *not* in the Config Snapshot per §5) come from the
caller-supplied `cafs_mount_opts_t`; this library never reads a
host-side `config.fs` file itself.

## Resolved spec gaps

`fs.info` left several things unstated that this implementation had to
pick a concrete answer for. Recorded here so a fuller spec pass can
confirm or override each one — see inline comments at point of use for
the full reasoning:

- **LBA addressing unit**: never stated. Resolved as raw byte offset
  from device start, applied uniformly to `real_lba`/`fallback_real_lba`
  too (`cafs_io_layout.h`).
- **Pointer block magic** `0xCAFSPTR`: not valid hex (P/T/R aren't hex
  digits). Resolved as the literal ASCII bytes `"CAFSPTR\0"`, compared
  byte-for-byte.
- **Pointer-block checksum algorithm bootstrap**: §3.3 says the
  algorithm is "configurable at format time" but never says how a
  fresh mount discovers *which* one before it's read anything
  trustworthy. Resolved as try-CRC32C-then-XXHASH32 and accept
  whichever matches (`anchor_io.c`).
- **`function_table_anchor_lba` = 4**: this is the anchor *slot index*
  (0-5), not a byte offset — inconsistent with every other `real_lba`/
  `fallback_real_lba` field in the format, which are byte offsets.
  `mkfs_cafs` writes literal `4`; nothing currently reads this field
  back to cross-check it (out of scope, matches the Superblock-only
  check policy).

## Licensing

This directory (and `mkfs_cafs.c`) is BSD-3-Clause, matching the
repo's `LICENSE`. `../../../../../vendor/blake3/` is a vendored, unmodified
copy of the official BLAKE3 reference implementation
(BLAKE3-team/BLAKE3, portable build only — SIMD/AVX/SSE files were not
pulled in, see the Makefile's `BLAKE3_DEFS`), dual-licensed CC0-1.0 /
Apache-2.0 by its authors; `vendor/blake3/LICENSE_A2` is the Apache-2.0
text as fetched from upstream. This is a disclosed exception to "the
entire repo is BSD," the same way the kernel driver is a disclosed
GPLv2 exception. CRC32C and XXHASH32 are hand-written directly in
`checksum.c` (both are small, well-known, publicly-specified
algorithms; XXHASH32's known-vector self-test in
`cafs_checksum_selftest()` is the correctness backstop) — no
vendoring, no license question there.

## Known limitations (beyond the deliberate scope cut above)

- Checksums are software-only, no SSE4.2/hardware CRC32C or SIMD
  BLAKE3. Not a hot path (a few KB, once per `check()`/`confirm()`
  call), so correctness-first was chosen over a CPUID-dispatch
  subsystem. Straightforward to add later without changing the public
  API.
- `misaligned_action=copy` bounces through a 512-byte-aligned heap
  buffer; O_DIRECT alignment requirements on some 4Kn devices may
  need 4096-byte alignment instead of 512 — not currently queried via
  `BLKSSZGET`/`BLKPBSZGET`.
- Endianness: all multi-byte fields assume little-endian on disk, safe
  for the x86-64-only V1 target.
