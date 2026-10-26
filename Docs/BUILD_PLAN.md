# CAFS — PoC build plan (brick by brick)

**Status:** CURRENT
**Updated:** 2026-10-25
**Decided:** 2026-10-25 — build the I/O Engine brick by brick with practical testing;
discuss each design decision when the brick that needs it comes up.
**Companions:** `DECISIONS-FORMAT-8.md` (decisions), `OPEN_ITEMS.md` (open list),
`io_engine.md` (engine rules), `fs.info` (format).

---

## 1. Ground rules

1. **One brick = code + tests + spec note + open-items update, in one commit.** If the code
   forces a format decision, `fs.info` and `DECISIONS-FORMAT-8.md` change in that same
   commit. Code never becomes the spec silently.
2. **Tests are the executable spec.** Every on-disk structure gets a golden byte image and
   offset/size assertions. A layout change must change a test on purpose.
3. **Fault injection from the start.** "Write, read back, matches" passes on a healthy
   image and says nothing about torn writes, reordering or double allocation. No brick is
   done until it survives its fault matrix (section 4).
4. **The audit runs after every state-mutating test** (section 5).
5. **Format may change** (PoC is not Beta). A change bumps the format note in
   `DECISIONS-FORMAT-8.md`; test volumes are disposable, so re-run mkfs.
6. **Tests run locally first** (`cargo test --release`). GitHub CI (`data-check.yml`
   Stage 3) comes later. Keep tests deterministic and fast: seeded RNG, no wall clock, no
   sleeps, so they move into CI unchanged. Stage 2 runs `clippy -D warnings`, so new code
   should be clippy-clean from the first commit.
7. **Each brick names the questions it will answer** (the `OPEN_ITEMS` IDs). When a test
   answers one, move the item to section F with the evidence.

## 2. Starting point

The crate at `Src/FUSE/Linux/x86-64/I-O_Engine` already has `anchor`, `block_io`, `check`,
`checksum`, `confirm`, `error`, `layout`, `mount`, `remount`, a `mkfs_cafs` binary, and
`tests/pipeline.rs` (mkfs -> mount -> check -> remount -> confirm, plus corruption and
auto-repair on a real `.img`). **Brick 0** is to baseline it:

- `cargo test --release` green; record the test count and runtime.
- Read `block_io.rs`'s public API and reconcile it with `io_engine.md` (OPEN_ITEMS B11).
- Settle the unit of an LBA (B12) before any new offsets are written.
- Decide where shared format code lives (B7): move `layout`, `anchor`, `checksum`,
  `error` to a shared top-level crate, with `mkfs_cafs` reusing it.

## 3. Bricks

| # | Scope | Fault matrix / tests | Answers |
|---|---|---|---|
| 0 | Baseline, reconcile docs with code, shared-crate move | Existing pipeline tests unchanged and green | B7, B11, B12 |
| 1 | **Device layer:** a block-device trait (read-at, write-at, flush, size, sector sizes) with file-backed, raw-device and fault-injecting implementations; alignment and direct-I/O rules | Read/write round-trips at aligned and misaligned offsets; short I/O; sector-size query and the trust rule (match, mismatch, override) | G2 sector trust, A5 |
| 2 | **Anchors under faults:** whole-4096-unit write primitive; pointer replace with generation + alt LBA; Function Table ring; `format_version` and feature-bit refusal | Tear every anchor write at every sector boundary; crash after each of the three pointer-replace writes; recovery must yield the old or the new image, never garbage | A1, A2, A15, A16, G2 |
| 3 | **Format codec:** Superblock, pointer blocks, Function Table, Meta/User Table, Identifier row (256 B) with golden images, crc32c, self-identifier | Golden-image tests; flip each byte and confirm detection; reserved bytes round-trip as zero | A8, A9, A10, A13 |
| 4 | **WAL:** sealed groups, epoch, backward chunk chain, CHECKPOINT order, rediscovery scan | Crash at every write boundary; replay twice equals replay once; stale-tail resurrection test; torn tail group; lost list; hint stale in both directions | A6, A7 |
| 5 | **Zone Table + allocator pass-through** (bump allocation, per-block refcount) + the **audit tool** | Double-allocation attempts; refcount undercount injected on purpose must be caught by the audit; free blocks never referenced | C5, C4 |
| 6 | **Rows + extents + data path:** identifier allocate/retire, extent lists, 16 B checksum trailer, write and read file data | Crash mid-write; misdirected write (lands on another LBA); bit rot; the block-40 scenario written as a test | A8, D2 |
| 7 | **Namespace B-tree** (254 slots) minimal: create, lookup, readdir, unlink; WAL integration | Crash during split; collision resolution; unlink with open handle | D1, C6 |
| 8 | **FUSE read/write** on top; remount; `check`; power-fail soak with random crash points | Long seeded run, audit after each crash and recovery | — |
| 9 | Then add allocator logic one piece at a time (zones, weights, hints, SMART bias), CoW redirect, dedup, TRIM backend (`BLKDISCARD`, `reserve_percent`) | Each addition re-runs the whole matrix | D8, D15 |

**Out of scope for these bricks:** Windows glue and WinFsp, the kernel helper, tree mode,
snapshot reads, hard links (all post-PoC; see `DECISIONS-FORMAT-8.md` sections 2 and 4).

## 4. FaultDevice (brick 1) — what it must do

Wraps any block device and records every write and flush. Fault modes:

- **Crash-prefix:** keep the first N writes, drop the rest.
- **Torn write:** apply only the first k sectors (or bytes) of a 4096-byte unit.
- **Reorder:** reorder unflushed writes inside a flush window.
- **Lost flush:** acknowledge a flush that did not make data durable.
- **Misdirected write:** a write lands at a different LBA.
- **Bit flip / short I/O** on reads.

**Crash-point enumeration:** run an operation once recording the write log; for each
prefix and each torn variant of the last write, build a "crash image", run recovery, then
run the audit. Seeded and exhaustive for small operations, sampled for long ones.

## 5. Audit invariants

| ID | Invariant |
|---|---|
| I1 | `refcount(block)` equals the number of references (extent entries + snapshot holds + dedup) |
| I2 | A block referenced by two live files has refcount >= 2 |
| I3 | Free blocks are never referenced |
| I4 | Every row's crc32c and self-identifier are valid; reserved bytes are zero |
| I5 | Replaying any WAL prefix twice equals replaying it once |
| I6 | Anchors: the surviving state is exactly the old or the new image |
| I7 | B-tree: keys sorted and unique, node arity within bounds, every leaf value names a live identifier |
| I8 | Identifiers unique; retired identifiers not referenced |
| I9 | `nlink` is 1 for files in the PoC; no directory entry points at a retired identifier |

## 6. Resume checklist (start of the next session)

1. Upload the crate: `Src/FUSE/Linux/x86-64/I-O_Engine` (`Cargo.toml`, `src/`, `tests/`).
2. Upload the current `Docs/`: `fs.info`, `config.fs`, `allocator.md`, `io_engine.md`,
   `DECISIONS-FORMAT-8.md`, `OPEN_ITEMS.md`, `BUILD_PLAN.md`.
3. Brick 0, in order: baseline `cargo test`; read `block_io.rs`; answer B12 (LBA unit).
4. Then brick 1 (device trait + FaultDevice).

**Tooling note:** Claude's sandbox has cargo 1.75 (Ubuntu package), so bricks can be built
and their tests run there before you pull them. CI uses a newer stable toolchain; avoid
features newer than 1.75 unless you decide the MSRV is higher.
