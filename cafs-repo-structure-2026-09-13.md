# CAFS Repo Structure — 2026-09-13

Diff against the 2026-09-03 revision. Structural reasoning from that document (shared-code placement rule, Docs/ subfolder purposes, license layout) still holds and isn't repeated here except where it changed.

```
cafs/
├── Docs/
│   ├── ./                        fs.info, config.fs, allocator.md, io_engine.md, error_id.md
│   ├── ADR/                      one file per decision (ADR-001..039, up from 001..012)
│   ├── Research/                 unchanged
│   └── Drafts/                   unchanged
│
├── Src/
│   ├── smart_handler.py          unchanged in location; internals extended (Signals layer, see below)
│   ├── tasker.rs                 NEW — FS Tasker (on-demand, non-blocking scripts called during I/O) + Routine Tasker (scheduler: decides when Normal/Urgent/Check run)
│   ├── defrag.rs                 NEW — on-demand only on SSD, automatic+config on HDD
│   ├── balance.rs                NEW
│   ├── scrub.rs                  NEW — full A-to-Z pass, this is what "Check" (Routine Tasker's third priority) actually is
│   ├── store_mgr.rs              NEW — "storage expander" / "the fast scrubber," reclaims dead identifier/data-block references
│   ├── allocator.rs               NEW — core allocation logic, media-agnostic, exposes a shared trait
│   ├── allocation_chs.rs          NEW — knob decision policy ("conf hot swap"), separate file from allocator.rs on purpose (see Docs/allocator.md Section 5)
│   ├── Kernel/
│   │   ├── Windows/
│   │   │   └── helper.rs         NEW — placeholder for all Windows kernel-mode helper drivers (currently just the TRIM relay; Docs/io_engine.md)
│   │   └── Linux/                 unchanged, still a placeholder
│   └── FUSE/
│       ├── Windows/                WinFsp-based, still a placeholder — unchanged
│       └── Linux/
│           └── x86-64/
│               ├── I-O_Engine/     mntproc (confirmed Rust — an earlier ADR/docstring said "C," corrected this session) + syscalls.rs (NEW — I/O counter bumping, Docs/io_engine.md)
│               └── Allocator/      REMOVED as a nested folder — see note below
│
├── Tools/                          unchanged — check.py, .reviewed-hashes, Make-Test.x
│
├── .github/workflows/data-check.yml   unchanged
│
├── errors.md                       NEW — top-level, canonical error catalog (Docs/error_id.md describes the scheme; this is the actual populated list)
├── LICENSE / LICENSES/             unchanged
├── README.md                       unchanged location; content additions expected (TRIM/Windows compromise note, "configuration maxed out" positioning — both mentioned this session, not yet drafted here)
├── Git_Structure.md
└── .gitignore
```

## What actually moved, and why

**`Allocator/` is no longer nested under `Src/FUSE/Linux/x86-64/`.** The prior structure placed it there because V1 was scoped to Linux/FUSE only and the allocator's process-architecture question was still open. Both have changed: the allocator now needs to serve both the Linux FUSE path and, eventually, the Windows WinFsp path (daemon mode specifically has no reason to be Linux-only), and the process-architecture question resolved to "both daemon and library, real, gated by config" rather than picking one. Per the shared-code rule already established in the prior revision of this document — anything used by more than one sibling consumer moves up to their common parent, not root — `allocator.rs` and `allocation_chs.rs` move to `Src/`, alongside `smart_handler.py`, rather than staying nested under one platform's path.

**`tasker.rs`, `defrag.rs`, `balance.rs`, `scrub.rs`, `store_mgr.rs` land at `Src/` for the same reason**, not under either platform folder — housekeeping/maintenance logic isn't platform-specific the way `I-O_Engine/`'s actual syscall handling is.

**`I-O_Engine/` stays exactly where it was**, gains `syscalls.rs` as a new named file inside it (the I/O counter instrumentation point, Docs/io_engine.md) — this one genuinely is platform-specific, unlike the above.

**`Src/Kernel/Windows/` gains its first real content**: `helper.rs`, a placeholder file for kernel-mode helper drivers. `Src/Kernel/Linux/` and `Src/FUSE/Windows/` remain untouched placeholders — this session's kernel-mode work was Windows-specific (the TRIM relay problem has no Linux equivalent, since Linux's `FITRIM` is directly callable from a mounted filesystem without a kernel-mode helper).

**`errors.md` is new at repo root**, not under `Docs/`. `Docs/error_id.md` describes the naming scheme and platform-surfacing rules; `errors.md` is the actual, growing catalog — root-level because it's referenced by both the Docs/ specs and anything user-facing (README, platform Event Log/dmesg output), not naturally "owned" by one side over the other.

## ADR count

Twelve at the prior revision, thirty-nine now — the B-tree byte-layout resolution, Zone Table's second responsibility, the scheduling-shape decision, Hash Daemon's interval model, and the SMART Handler cadence split (ADR-034 through 039) account for the six most recent; the rest predate this document's last revision and were already reflected there under a different total.

## Still open, not a structure question

Where `allocator.rs`'s daemon-mode binary actually gets built/shipped from (a separate crate under `Src/`, or a build-time target of the same crate as the library form) isn't settled — this document reflects source-file location, not build/packaging shape, which is a separate decision not yet made.
