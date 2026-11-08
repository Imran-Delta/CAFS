# Configurable Adaptive FileSystem (CAFS)

CAFS is a config-first filesystem inspired by the Linux kernel and ZFS, intended for consumer drives. The project prioritizes configurability and aims to keep custom builds flexible while standardizing inter-process communication.

> **Warning:** This is a pre-alpha proof of concept, not production-ready. The on-disk format may still change. Do not store data you cannot afford to lose on a CAFS volume.

> **Important:** No software can fix a failing drive. CAFS can only try to delay data loss.

## Project status

The Rust I/O Engine is the current proof of concept: it builds and passes tests, with anchor mount lifecycle and generic block read/write support. The `mkfs_cafs` format tool and shared SMART telemetry processor are also present. The allocator, FUSE request layer, deduplication table, and WAL are not implemented. The kernel driver is a placeholder and is not a current priority.

The proof of concept focuses on the syscall and hardware layer; filesystem logic is planned for later stages.

## Builds

| Build | IPC | Guardrails | Intended use |
|:--|:--|:--|:--|
| Release | Per-interface, CAFS-controlled | On | Normal use |
| Custom | One standard messaging format | Off | Integrating CAFS into another system or replacing modules |

Custom builds are source-only and intended for people who want to integrate CAFS into their own system, change components such as the allocator, or use a standard messaging format.

## Build the I/O Engine

The only buildable component today is the Linux x86-64 I/O Engine:

```bash
cd Src/FUSE/Linux/x86-64/I-O_Engine
cargo build
cargo test
```

A workspace root is planned, but `cargo build` is not yet configured to work from the repository root.

## Documentation notes

The format and design documents evolve alongside the project. Check the source repository and individual document status for the latest context. See the [README](https://github.com/Imran-Delta/CAFS#readme) for repository layout, releases, licensing, and additional project notes.
