# CAFS — Project Summary (2026-09-04)

Written to ground a fresh chat session with no prior context. If you're
that session: read this fully before touching code or specs. The
canonical, currently-accurate documents are `fs.info`, `config.fs`,
and everything under `Docs/ADR/` — this file is a map to them, not a
replacement for them.

## What CAFS is

A configurable, adaptive, copy-on-write filesystem. FUSE-based for
V1 (kernel driver is a stated non-goal for now, both Kernel/Linux and
Kernel/Windows are placeholders). Distinguishing design choices:
health-aware block allocation (SMART-derived filesystem statistics
feed the Allocator, independent of whether `smartctl` is even
present), two-tier deduplication (whole-file, falling back to
block-level "CoW Extreme"), and a from-scratch anchor/recovery design
(Superblock + SMART redundancy, everything else deliberately
non-redundant to cut write amplification).

Owner: Imran Bin Gifary (System Delta). License: BSD-3-Clause repo-
wide, GPL-2.0 for `Src/Kernel/` only (unavoidable, kernel-side). See
`LICENSE` and `LICENSES/` (the latter is referenced but wasn't
confirmed to exist as of this summary — check before assuming).

## Current build status

**Built and tested**: the I/O Engine, V1, in Rust, at
`Src/FUSE/Linux/x86-64/I-O_Engine/`. Anchor mount lifecycle (mount ->
check -> remount -> confirm) plus generic block read/write. No
allocator, no filesystem semantics — deliberately scoped that way.
`cargo test` passes 10/10 (unit tests + one full integration test
covering mkfs -> mount -> check -> remount -> confirm, raw block I/O,
re-mount-after-confirm persistence, and Superblock corruption +
auto-repair). Comes with its own format tool, `mkfs_cafs` — there was
no way to test the engine without one.

**Not yet built**: the Allocator (architecture decided — see ADR-015/
017 — balance mode is the next concrete implementation target), the
FUSE request-handling layer proper (Module 1 in the original
module-survey doc — `fuser` crate, MIT, chosen as the reference
library, not yet integrated), master-process supervision code, the
three speed-index tables (ADR-013), the real dedup table (ADR-014),
WAL, and the `flock()`-based singleton locking (ADR-016).

**Retired**: an earlier C + hand-written x86-64 assembly version of
the I/O Engine, superseded by the Rust rewrite (ADR-011/012). If its
files are still present anywhere under `Src/FUSE/Linux/x86-64/
I-O_Engine/` (`*.c`, `*.h`, `*.S`, `Makefile`, `test/`) or a root-level
`vendor/blake3/`, they're stale and should be deleted — BLAKE3 is a
normal Cargo dependency now, nothing is vendored.

## Repo conventions

- **ADR system**: one file per decision, in `Docs/ADR/`, never edited.
  A changed decision is a new, higher-numbered file that says which
  one it supersedes.
- **Shared resources sit one directory above their consumers**,
  recursively — not at the repo root by default. `smart_handler.py`
  is shared between `Kernel/` and `FUSE/`, so it lives at `Src/`. If
  `Allocator/`'s performance-mode code diverges from its shared
  baseline, the diverging code gets its own subfolder
  (`Allocator/Performance/`) while the shared baseline stays at
  `Allocator/` — same relationship, one level down instead of two.
- **`Docs/Research/`**: extensive-research deliverables that inform a
  decision without being one — cited by ADRs, never superseded the
  way an ADR is (they just stop being cited if something invalidates
  them).
- **`Docs/Drafts/`**: where AI-session output lands before it's
  trusted. Exists because of a real early failure mode: a draft
  (`io-engine-data.txt`) sat self-labeled "LOCKED" while being weeks
  stale against the actual anchor-layout spec. Nothing in `Drafts/`
  should be treated as current without checking it against `Docs/`.
- **CI**: `.github/workflows/data-check.yml`, three gated jobs —
  syntax/integrity (dependency-free) -> static analysis (linting,
  conditional per language present) -> engine build+test (`cargo
  test` against a real `.img`, on GitHub Actions).

## ADR index

| # | Decision |
|---|---|
| 001-005 | SMART table hierarchy (V/Cache/B/M-SMART), flush order, bucket statistics, related early decisions — see files for detail |
| 006 | Anchor redundancy scoped to Superblock + SMART only; Config Snapshot/Parity/Function Table/Meta Table have no backup |
| 007 | V1 `check()`/repair scope is Superblock-only |
| 008 | `confirm()` writes Superblock + M-SMART directly, bypassing the full ADR-002 flush pipeline |
| 009 | *(superseded by 011)* Original Assembly+C toolchain choice |
| 010 | Four resolved `fs.info` ambiguities (LBA addressing unit, pointer-block magic, checksum-algorithm bootstrap, anchor slot index vs. byte offset) |
| 011 | I/O Engine rewritten in Rust, supersedes 009 |
| 012 | Syscall foundation: `rustix` (over `nix`) |
| 013 | Three non-critical speed-index tables (hash-exists / filename / LBA+permissions), Function-Table-referenced, with critical-structure fallback |
| 014 | Two-tier dedup: file-level primary (critical, Meta/User Table), LBA-level "CoW Extreme" fallback |
| 015 | Process architecture: master/mntproc separate supervised processes; each housekeeping task (defrag/balance/python) its own supervised child, not inline in master; failures WAL-revertible, never fsck-triggering |
| 016 | Partition singleton locking: `flock()` keyed by Superblock `volume_uuid`, not PID comparison |
| 017 | Allocator mode toggle (balance/performance/safety), independent interfaces per mode, balance mode built first |

## Known open items, carried forward

- **`LICENSES/BSD-3-Clause.txt` and `LICENSES/GPL-2.0.txt`** are
  referenced by the top-level `LICENSE` file but weren't confirmed to
  exist — dangling reference until checked.
- **ADR-013's permission-duplication corollary** (permissions must
  also live in the authoritative critical metadata structure, not
  only in the non-critical LBA table) is a stated requirement, not
  yet verified against an actual inode-equivalent design — there
  isn't one built yet to check it against.
- **ADR-015's WAL-revertibility requirement** for housekeeping-task
  partial work needs checking per task once defrag/balance are
  designed — not yet verified either can actually be cleanly
  WAL-reverted mid-operation.
- **`fs.info` itself** still contains the ambiguities ADR-010 patched
  at the engine level (byte-offset addressing unit, pointer magic,
  checksum bootstrap, slot-index field). ADR-010 says this explicitly:
  it should get folded back into `fs.info` directly at some point,
  closing that ADR out as "merged into the spec" rather than left as
  a permanent side-patch.
- **`Docs/ADR/ADR-006_pseudo_draft.md`** was seen once, early, sitting
  in `Docs/Drafts/` — never read, may or may not still exist, may or
  may not conflict with the numbering used here (this summary's
  ADR-006 is a different, later, unrelated decision — the anchor
  redundancy one). Check for a collision before assuming the numbering
  in this summary is uncontested.
- **Allocator process architecture** is decided at the "master
  supervises mntproc + per-task children, safety mode uses IPC,
  performance mode doesn't have to" level — not yet decided at the
  level of an actual wire format/API for any mode. Balance mode is
  next.

## Where to look for more

- On-disk format, byte-for-byte: `fs.info` (v19, format version 4).
- Config file format and section-by-section crash-criticality:
  `config.fs`.
- I/O Engine source, README, and inline rationale comments (every
  resolved ambiguity is cited at its point of use, not just in the
  ADR): `Src/FUSE/Linux/x86-64/I-O_Engine/`.
- Allocator algorithms/formulae survey (ZFS metaslab weighting,
  Btrfs's dual-indexed free-space tree, WAFL's write allocator,
  fragmentation metrics): `Docs/Research/cafs-allocator-research-*.md`.
- Repo layout and the shared-code convention in full:
  `Docs/cafs-repo-structure-*.md`.
