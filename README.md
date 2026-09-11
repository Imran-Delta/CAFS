# Configurable Adaptive FileSystem (CAFS)

A (Linux Kernel and) ZFS-inspired, config-first filesystem for consumer drives.

> [!WARNING]
> Alpha. Not production-ready.

> [!NOTE]
> Kernel driver is a placeholder. Isn't a priority for the time being.

> [!IMPORTANT]
> No software can fix a failing drive. CAFS can only try to delay data loss.

## What it is

CAFS aims to bring customizability and adaptability to consumer drives: health-aware block allocation driven by SMART data, two-tier deduplication, and an anchor/recovery design that only duplicates what actually matters. The configuration surface is the point — most design decisions are exposed as parameters rather than baked in.

Currently at PoC stage: the I/O Engine builds and passes tests. Everything else is unbuilt.

## Two builds

| Build | IPC | Guardrails | For |
|---|---|---|---|
| Release | Per-interface, CAFS-controlled | On | Normal use |
| Custom | One standard messaging format | Off | See below |

**Custom** exists for people who want to plug CAFS into their own system, swap the allocator or any other module, or drive it over a standard messaging format instead of the per-interface IPC that Release uses internally. It ships as source only — anyone building Custom is already compiling.

## What works right now

- **PoC I/O Engine** — Rust, at `Src/FUSE/Linux/x86-64/I-O_Engine/`. Anchor mount lifecycle, generic block read/write. `cargo test` passes.
- **`mkfs_cafs`** — format tool, built as a bin target inside the engine crate.
- **`smart_handler.py`** — shared SMART telemetry processor at `Src/`.

Not built yet: allocator, FUSE request layer, dedup table, WAL.

## Repo layout

```text
cafs/
├── Docs/
│   ├── fs.info              on-disk format spec — canonical
│   ├── config.fs            config format + crash-criticality
│   ├── ADR/                 one decision per file, never edited
│   ├── Research/            informs decisions, isn't one
│   └── Drafts/              AI output before it's trusted — not canon
├── Src/
│   ├── smart_handler.py     shared: Kernel + FUSE
│   ├── FUSE/
│   │   ├── Linux/x86-64/I-O_Engine/   Rust PoC — the only buildable component
│   │   └── Windows/                   placeholder
│   └── Kernel/              placeholder
├── Tools/                   check.py, test helpers
├── .github/                 CI pipeline
├── LICENSE
├── LICENSES/                BSD-3-Clause, GPL-2.0
├── Makefile                 provisional
└── README.md
```

Critical files:

· Docs/fs.info — on-disk format, byte-for-byte. Canonical.
· Docs/config.fs — config file format and crash-criticality per section.
· LICENSE / LICENSES/ — BSD-3-Clause repo-wide; GPL-2.0 for Src/Kernel/ only.

Repo conventions and the full layout rationale: cafs-repo-structure-2026-09-03.md.

Building

The only buildable component today is the I/O Engine:

```bash
cd Src/FUSE/Linux/x86-64/I-O_Engine
cargo build
cargo test
```

The root Makefile is provisional and may not work.

A workspace root is planned so cargo build works from the repo root, but it isn't set up yet.

Releases & versioning

```text
Phase Tag GitHub flag Artifacts
Alpha none — none
Beta v0.1.0-beta.dev1 pre-release yes
RC v0.1.0-rc1 pre-release yes
Release v0.1.0 latest yes
```

Each tagged release includes:

```text
source.tar.gz             Release + Custom source. Build it yourself.
installer_linux.sh        Bootstrap. Downloads portable.zip.
installer_win_x86.exe     Self-contained Windows installer.
installer_win_amd64.exe   Self-contained Windows installer.
portable.zip              Offline bundle: Release binaries + docs.
```

Custom ships as source only, inside source.tar.gz.

<details>
<summary>AI disclosure</summary>

AI is used to generate code from the specs in Docs/. Alpha builds may not be fully reviewed. Beta and release builds are checked before tagging.

Bug reports and advice welcome.

</details>

License

BSD-3-Clause repo-wide. GPL-2.0 for Src/Kernel/ only. See LICENSE and LICENSES/.