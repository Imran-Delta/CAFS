# CAFS — Project Summary (2026-09-13)

Written to ground a fresh chat session with no prior context. If you're
that session: read this fully before touching code or specs. The
canonical, currently-accurate documents are `fs.info`, `config.fs`,
`Docs/allocator.md`, `Docs/io_engine.md`, `Docs/error_id.md`, and
everything under `Docs/ADR/` — this file is a map to them, not a
replacement. This revision supersedes the 2026-09-04 summary; that
one was already six ADRs and two format versions behind by the time
this one was written, so don't trust anything in it that conflicts
with this document.

## What CAFS is

A configurable, adaptive, copy-on-write filesystem — "the configuration
stat maxed out" is the project's own stated positioning. FUSE-based
for V1 on both platforms (WinFsp on Windows, libfuse on Linux); a full
Windows kernel-mode driver is an explicit post-V1 PoC, not a V1
deliverable — Windows does get real kernel-mode code in V1, but scoped
narrowly to a helper driver (TRIM relay), not the filesystem itself.
Distinguishing design choices: health-aware block allocation (SMART-
and workload-derived signals feed the allocator continuously, not just
at mount), two-tier deduplication (whole-file, falling back to
block-level "CoW Extreme"), a from-scratch anchor/recovery design, and
an allocator that hot-swaps six of its seven knobs live, independently,
based on those signals, with the seventh (concurrency) fixed at mount.

Owner: Imran Bin Gifary (System Delta). License: BSD-3-Clause repo-
wide, GPL-2.0 for `Src/Kernel/` only. See `LICENSE` and `LICENSES/`
(existence still unconfirmed as of this summary — check before
assuming, same open item as the last revision).

## Current build status

**Built and tested**: the I/O Engine core, in Rust, at
`Src/FUSE/Linux/x86-64/I-O_Engine/`. Anchor mount lifecycle (mount ->
check -> remount -> confirm) plus generic block read/write, 10/10
tests passing. `smart_handler.py` — three modes (`--type=smart`,
`--type=avg`, `--type=trends`), plus a new Signals layer inside
`--type=trends` (Workload Bias, R_write, Health Composite, Pending
Trajectory, Write-Size CV, the hard-floor monotonic flag) —
implemented and smoke-tested this session, additive alongside the
pre-existing internal-error-counter z-score mechanism, not a
replacement for it.

**Not yet built**: `mntproc` proper (the FUSE request-handling layer —
`fuser` crate chosen, not integrated), master-process supervision,
`tasker.rs` (FS Tasker + Routine Tasker), `defrag.rs`/`balance.rs`/
`scrub.rs`/`store_mgr.rs`, `allocator.rs`/`allocation_chs.rs`, WAL,
`syscalls.rs`'s I/O-counter instrumentation, the real `smartctl`
integration (no shell-out exists anywhere yet — `smart_handler.py`'s
Signals layer currently runs against placeholder payload fields for
both per-operation I/O data and real drive SMART reads), and
`Src/Kernel/Windows/helper.rs`. Architecture for all of the above is
decided (`Docs/allocator.md`, `Docs/io_engine.md`) — this is a list of
what's spec'd but not coded, not what's undecided.

**Retired**: unchanged from the last revision — the C+asm I/O Engine
predecessor, superseded by Rust (ADR-011/012).

## Repo conventions

Unchanged from the last revision (ADR system, shared-code-one-level-up
rule, `Docs/Research/`, `Docs/Drafts/`, CI shape) — see
`Docs/cafs-repo-structure-2026-09-13.md` for what moved this session
specifically (`allocator.rs` and the housekeeping handlers relocating
to shared `Src/` under that same rule, applied to files that didn't
exist at the last revision).

## On-disk format

`fs.info` v22, `config.fs` v22, on-disk format version 7 — bumped this
session from v21/format 6. Two real format changes: the B-tree
branching factor fixed to 254 (was 255, overlapped the checksum
trailer when a node was full — a real conflict, found and fixed this
session, not the ext4-precedent question it started as), and a new
`UNWRITTEN` flag (top bit of an extent's `length` field) for `raw`
files' full-allocation semantics. Everything else decided this session
— the allocator's whole design, SMART's Signals layer, TRIM, the
handler architecture — is behavioral/architectural, not a byte-layout
change, and lives in `Docs/allocator.md` / `Docs/io_engine.md` /
`smart_handler.py` itself rather than in `fs.info`.

## ADR index

001–033 unchanged from the last revision (see that summary or
`Docs/ADR/` directly). New this cycle:

| # | Decision |
|---|---|
| 034 | B-tree: hash-keyed, per-directory, Identifier Table as the cross-directory path |
| 035 | Zone Table's second responsibility: integrity signal aggregation |
| 036 | Scheduling/preference system: four distinct shapes, not one unified type |
| 037 | Hash Daemon: dynamic interval via timeout-based socket wait |
| 038 | SMART Handler: user-configurable vs. allocator-critical cadence split |
| 039 | Module organization: function files, wrapper files, shared-folder promotion |

**Not yet written as formal ADRs**, despite being decided this
session — flagged so they don't get lost before someone does write
them: the allocator's daemon/library dual-mode architecture, the
per-knob independent hot-swap mechanism, the hard-floor override
design, the B-tree/extent format fixes above, the error-identifier
scheme. All are documented in `Docs/allocator.md`, `Docs/error_id.md`,
and this session's decisions log, just not yet in ADR form.

## Known open items, carried forward and new

Unchanged from the last revision: `LICENSES/` existence, the
`ADR-006_pseudo_draft.md` numbering-collision check.

**Resolved since the last revision** (removing from "open," not
repeating here — see `Docs/allocator.md` for the actual resolutions):
ADR-013's permission-duplication corollary (Identifier Table now
exists and carries permissions in the critical tier), allocator
process architecture (both daemon and library, not one), balance
mode's status (superseded by the full seven-knob model).

**New, substantive, and unresolved** — this session's own
open-questions file (`cafs-session-open-questions-2026-09-13.md`, kept
alongside this summary) is the authoritative, itemized list. Highest-
level items only, here: the three mystery pointer fields
(`fsck_scratch_lba`, `scratch_lba`, `temp_cache_lba` — no described
purpose anywhere), hard-floor response scope (global vs. zone-level),
the grant-revocation mechanism for exclusive allocator permits
(requirement locked, mechanism deferred to implementation), and a
substantial list of numeric thresholds (EWMA constants, signal
warn-at values, the consensus-window agreement range) that are
reasoned placeholder shapes, not validated against real data.

## Where to look for more

- On-disk format, byte-for-byte: `fs.info` (v22, format version 7).
- Config file format: `config.fs` (v22, format version 7) — note the
  substantially extended `[allocator]` section this session.
- Allocator design in full — architecture, concurrency, presets, the
  seven knobs, hot-swap mechanism, hard floor, CoW/dedup/ENOSPC
  boundary behavior: `Docs/allocator.md`.
- I/O Engine constraints (small — wasn't discussed in depth this
  session): `Docs/io_engine.md`.
- Error identifier scheme: `Docs/error_id.md`; the actual growing
  catalog is top-level `errors.md`, not under `Docs/`.
- `smart_handler.py` itself — the Signals layer implementation, with
  inline rationale comments at each formula.
- This session's full decision log and open-questions list:
  `cafs-session-decisions-2026-09-13.md` /
  `cafs-session-open-questions-2026-09-13.md`.
- Repo layout, current: `Docs/cafs-repo-structure-2026-09-13.md`.
