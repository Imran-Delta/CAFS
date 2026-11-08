# Configurable Adaptive FileSystem (CAFS)

This is a config-first filesystem inspired by the Linux Kernel and ZFS and is for consumer drives.

> [!WARNING]
> Proof of concept (pre-Alpha). Not production-ready. The on-disk format can still change, so don't store data you can't lose on a CAFS volume.

> [!NOTE]
> Kernel driver is a placeholder. Isn't a priority for the time being.

> [!IMPORTANT]
> No software can fix a failing drive. CAFS can only try to delay data loss.

## What it is

The goal of this filesystem is simple, bring the configurability stat to its highest. Seriously- that's the only goal I have for this. That's why kernel inspired. That thing can be used anywhere. Currently the newest thing I'm adding to this that I haven't seen before is using S.M.A.R.T data (and on-disk smart) to dictate how the filesystem works. So a bad drive or slow drive isn't pushed towards failing. It also will have selective deuplication where you can choose to deduplicate user data only or user + meta data. The configurability is the main point of this filesystem, user facing has safety.

But for the custom build you will have absolute freedom, I will try my best to keep everything for custom builds to as little hardcoded constants as possible and standardize the IPC so you can pick and choose.

Currently at PoC stage: the I/O Engine builds and passes tests. Everything else is unbuilt.

### How the pieces fit (planned)

Green exists today; dashed is planned. Everything that touches the device goes through the I/O Engine.

```mermaid
flowchart TB
    CAFS[CAFS Process Start] --> MASTER
    CAFS --> FS
    FS["FUSE (Linux) / WinFsp (Windows)"] --> MNT["mntproc: request handler"]
    MASTER["Master: supervises"] --> MNT
    MASTER --> HK["Housekeeping children: defrag, balance, scrub, SMART"]
    MNT --> ALLOC["Allocator: library or daemon, chosen at mount"]
    MNT --> IO["I/O Engine"]
    ALLOC --> IO
    HK --> IO
    IO --> DEV[("Block device")]
    classDef built fill:#d4f8d4,stroke:#2a7a2a,color:#111
    classDef planned fill:#f4f4f4,stroke:#888,stroke-dasharray: 4 3,color:#111
    class IO built
    class FS,MNT,MASTER,HK,ALLOC planned
```

## Two builds

| Build | IPC | Guardrails | For |
|:---:|:---:|:---:|:---:|
| Release | Per-interface, CAFS-controlled | On | Normal use |
| Custom | One standard messaging format | Off | See below |

> **Custom** exists for people who want to plug CAFS into their own system, swap the allocator or any other module, or drive it over a standard messaging format instead of the per-interface IPC that Release uses internally. It ships as source only — anyone building Custom is already compiling.

## What works right now

- [X] **PoC I/O Engine** — Rust, at [`Src/FUSE/Linux/x86-64/I-O_Engine/`](Src/FUSE/Linux/x86-64/I-O_Engine/). Anchor mount lifecycle, generic block read/write. `cargo test` passes.
- [X] **`mkfs_cafs`** — format tool, built as a bin target inside the engine crate.
- [X] **`smart_handler.py`** — shared SMART telemetry processor at [`Src/`](Src/).
- [] allocator
- [] FUSE request layer
- [] dedup table
- [] WAL
Out of scope for the proof of concept:
- [] hard links
- [] snapshot reads

> [!NOTE]
> The proof of concepts only deal with the syscall and hardware layer.
> Logic is tested in alpha not PoC.

### Where the design stands (OUTDATED)

- [`Docs/fs.info`](Docs/fs.info) — on-disk format, byte-for-byte. Canonical.
- [`Docs/DECISIONS-FORMAT-8.md`](Docs/DECISIONS-FORMAT-8.md) — decisions for the current format revision.
- [`Docs/OPEN_ITEMS.md`](Docs/OPEN_ITEMS.md) — everything still undecided.
- [`Docs/BUILD_PLAN.md`](Docs/BUILD_PLAN.md) — how the I/O Engine is being built and tested, brick by brick.

## Repo layout (Docs is a bit messy I will fix it)

```text
cafs/
├── Docs/
│   ├── fs.info                 on-disk format spec — canonical
│   ├── config.fs               config format + crash-criticality
│   ├── allocator.md            allocator design
│   ├── io_engine.md            I/O Engine rules
│   ├── error_id.md             error identifiers
│   ├── DECISIONS-FORMAT-8.md   decisions for the current format revision
│   ├── OPEN_ITEMS.md           everything still undecided
│   ├── BUILD_PLAN.md           brick-by-brick build and test plan
│   ├── ADR/                    decision log: never deleted, only superseded
│   └── Research/               informs decisions, isn't one
├── Src/
│   ├── smart_handler.py        shared: Kernel + FUSE
│   ├── FUSE/
│   │   ├── Linux/x86-64/I-O_Engine/   Rust PoC — the only buildable component
│   │   └── Windows/                   placeholder
│   └── Kernel/                 placeholder (GPL-2.0-only)
│       ├── Linux/
│       └── Windows/
├── Tools/                      check.py, test helpers
├── .github/                    CI pipeline
├── LICENSE
├── LICENSES/                   BSD-3-Clause, GPL-2.0
└── README.md
```

Critical files (OUTDATED)

- [`Docs/fs.info`](Docs/fs.info) — on-disk format, byte-for-byte. Canonical.
- [`Docs/config.fs`](Docs/config.fs) — config file format and crash-criticality per section.
- [`LICENSE`](LICENSE) / [`LICENSES/`](LICENSES/) — BSD-3-Clause repo-wide; GPL-2.0-only for `Src/Kernel/` only.

### OUTDATED Repo conventions and the full layout rationale: `cafs-repo-structure-2026-09-03.md`.

## Building

The only buildable component today is the I/O Engine:

```bash
cd Src/FUSE/Linux/x86-64/I-O_Engine
cargo build
cargo test
```

A workspace root is planned so `cargo build` works from the repo root, but it isn't set up yet.

## Releases & versioning

| Phase | Tag | GitHub flag | Artifacts |
|---|---|---|---|
| Alpha | none | — | none |
| Beta | `v0.1.0-beta.dev1` | pre-release | yes |
| RC | `v0.1.0-rc1` | pre-release | yes |
| Release | `v0.1.0` | latest | yes |

Each tagged release includes:

| Artifact | What it is |
|---|---|
| `source.tar.gz` | Release + Custom source. Build it yourself. |
| `installer_linux.sh` | Bootstrap. Downloads `portable.zip`. |
| `installer_win_x86.exe` | Self-contained Windows installer. |
| `installer_win_amd64.exe` | Self-contained Windows installer. |
| `portable.zip` | Offline bundle: Release binaries + docs. |

Custom ships as source only, inside `source.tar.gz`.

<details>
<summary>AI disclosure</summary>

AI is used to generate code from the specs in Docs/. Alpha builds may not be fully reviewed. Beta and release builds are checked before tagging.

Bug reports and advice welcome.

</details>

## License (OUTDATED)

BSD-3-Clause repo-wide. GPL-2.0-only for `Src/Kernel/` only. See [`LICENSE`](LICENSE) and [`LICENSES/`](LICENSES/).

Kernel code builds as separate binaries; userspace never links against it. The optional Windows kernel helper (a planned TRIM relay) will not be code-signed, so loading it will need Test Mode or a Secure Boot change; CAFS works without it.

### Third-party notices

On Windows, CAFS uses **WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos** ([github.com/winfsp/winfsp](https://github.com/winfsp/winfsp)), under its GPLv3 free/open-source exception. A closed-source Windows build of CAFS would need its own WinFsp commercial licence.
