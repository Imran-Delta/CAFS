# CAFS Repo Structure

```
cafs/
├── Docs/
│   ├── ./                        fs.info, config.fs, allocator.md, io-engine.md
│   ├── ADR/                      one file per decision, current stable cycle (ADR-001..012)
│   ├── Research/                 extensive-research deliverables (allocator formulae, crate surveys, etc.) — informational, not decisions; an ADR may cite one
│   └── Drafts/                   raw DeepSeek/Claude output lands here first
│
├── Src/
│   ├── (shared code lives loose here — Python, and anything else genuinely used by both Kernel/ and FUSE/ builds)
│   ├── smart_handler.py          shared: userspace Python, talks to whichever engine is under it (FUSE today, kernel-resident eventually) over IPC
│   ├── Kernel/
│   │   ├── Windows/
│   │   └── Linux/
│   └── FUSE/
│       ├── Windows/               WinFsp-based — the closest Windows equivalent to libfuse
│       └── Linux/                 libfuse-based
│           └── x86-64/            V1 target: 64-bit Linux, kernel 6.12+. x86-64 holds the parent/shared code for this platform.
│               ├── I-O_Engine/    Rust (ADR-011/012). Anchor mount lifecycle + generic block I/O. No allocator, no filesystem semantics.
│               └── Allocator/     not yet built — process-architecture question still open, see Docs/ADR once decided
│
├── Tools/
│   ├── check.py                   the consistency-check script
│   ├── .reviewed-hashes           one hash per Specs file, updated when a human reviews a change
│   └── Make-Test.x
│
├── .github/
│   └── workflows/
│       └── data-check.yml         3-stage pipeline: syntax/integrity -> static analysis -> engine build+test on a real .img
│
├── LICENSE                        dual: BSD-3-Clause (default, whole repo) / GPL-2.0 (Src/Kernel/ only) — see LICENSES/
├── LICENSES/
│   ├── BSD-3-Clause.txt
│   └── GPL-2.0.txt
├── README.md
├── Git_Structure.md               this file
└── .gitignore
```

## Shared resources: one folder above their consumers

Formalizing what was already true for `smart_handler.py` (shared between Kernel/ and FUSE/, so it sits at `Src/`, not inside either): **anything shared between two or more sibling components lives one directory above them, not at the repo root.** A dependency shared between `Src/Kernel/` and `Src/FUSE/` belongs at `Src/`, the same way `Src/FUSE/Linux/x86-64/I-O_Engine/` and `Src/FUSE/Linux/x86-64/Allocator/` sharing something would put it at `Src/FUSE/Linux/x86-64/`, not at `Src/` or repo root. Repo-root is reserved for things genuinely global to the whole project (LICENSE, README, the top-level Makefile, CI config) — not a default dumping ground for "shared" in a looser sense.

Concretely, this is why the I/O Engine's earlier `vendor/blake3/` sat at repo root, and why that placement is now moot rather than corrected: the Rust rewrite (ADR-011) replaced the vendored C BLAKE3 source with a normal Cargo dependency, so there's no vendored code left to place at all. If a future component vendors something *shared* (e.g. Kernel/ and FUSE/ both needing the same C library), it goes at `Src/vendor/`, per this rule — not root.

## Docs/

**./** — the last canonical flush. Only reviewed, confirmed content lives here; what any new AI session should be primed with, never a chat transcript, never a draft.

**ADR/** — the decision log. One file per decision, never edited; a changed decision is a new file that says which one it supersedes. Twelve decisions recorded so far, all from the I/O Engine's build: anchor redundancy scope, V1 check/repair scope, the confirm() write path, four resolved fs.info ambiguities, and — most recently — the language switch to Rust (ADR-011) and its syscall foundation (ADR-012), superseding the original Assembly+C choice (ADR-009) once the right category of crate (safe synchronous syscall wrappers, not io_uring) was actually evaluated.

**Research/** — new. Extensive-research deliverables that inform a decision without being one themselves — e.g. the allocator formulae/algorithms survey (ZFS metaslab weighting, Btrfs's dual-indexed free space tree, WAFL's write allocator). An ADR can and should cite a Research/ file as its rationale; the research file itself never gets superseded the way an ADR does, since it's not a decision — it just stops being cited when something later invalidates it.

**Drafts/** — where DeepSeek/Claude output lands before it's trusted. Still the fix for the specific failure this project hit early: `io-engine-data.txt` sitting here, self-labeled "LOCKED," while actually being three weeks stale against `fs.info`'s anchor redesign — exactly the gap this folder exists to catch before something drafted gets treated as canon.

## Src/

Flat, shared code at `Src/` mirrors the Linux kernel's own `fs/` style, now stated as an explicit rule rather than just an observed pattern (see "Shared resources" above).

`Src/FUSE/Linux/x86-64/` is new since the last revision of this document — added because V1 is explicitly scoped to one platform (64-bit Linux, kernel 6.12+), and `x86-64/` is where that platform's parent/shared code lives, with `I-O_Engine/` and `Allocator/` as siblings underneath it. Windows (WinFsp) and Kernel/ stay placeholders per the README — FUSE is the recommended, actually-developed path.

Where the process-architecture question lands (master/mntproc split, allocator-as-library vs. allocator-as-process) will likely determine whether `Allocator/` ends up as a library crate the I/O Engine links against, or a separate binary — still open, tracked for the next ADR round.

## Tools/

Unchanged from the prior revision — **check.py**, **.reviewed-hashes**, **Make-Test.x** — see that revision's notes; nothing here was touched by the Rust migration.

## Root

**LICENSE** now points at a **LICENSES/** folder (`BSD-3-Clause`, `GPL-2.0`) rather than inlining terms — cleaner for tooling (SPDX-style references, GitHub's license detection) that expects a canonical license file per license, not one combined document. *Note: this structure document assumes those two files exist under LICENSES/; they weren't part of this revision's inputs and should be added if they aren't already there — the top-level LICENSE file already references them, so a missing LICENSES/ folder is currently a dangling reference.*

**.github/workflows/data-check.yml** replaces the placeholder `consistency.yml` name from the prior revision with what's actually implemented: three gated jobs (syntax/integrity -> static analysis -> engine build+test), the last of which now runs `cargo build`/`cargo test` against a real `.img`, not `make`.
