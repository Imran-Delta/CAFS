# CAFS Repo Structure

```
cafs/
├── Docs/
│   ├── ./                        fs.info, config.fs, allocator.md, io-engine.md
│   ├── ADR/                      one file per decision, current stable cycle
│   │   └── (older cycles as .zip, same directory — check.py unzips these transparently, so archiving is storage-only and never blind to automation)
│   └── Drafts/                   raw DeepSeek/Claude output lands here first
│
├── Src/
│   ├── (shared code lives loose here — Python, anything Kernel and FUSE both use)
│   ├── Kernel/
│   │   ├── Windows/
│   │   └── Linux/
│   └── FUSE/
│       ├── Windows/               WinFsp-based — the closest Windows equivalent to libfuse
│       └── Linux/                 libfuse-based
│
├── Tools/
│   ├── check.py                   the consistency-check script
│   ├── .reviewed-hashes           one hash per Specs file, updated when a human reviews a change — this is what lets check.py catch "this file changed without a matching ADR"
│   └── Make-Test.x
│
├── .github/
│   └── workflows/
│       └── consistency.yml        runs check.py on push/PR, ubuntu-latest
│
├── Makefile                       top-level, dispatches into Src/Kernel or Src/FUSE
├── README.md
├── LICENSE                        BSD-3
├── Git_Structure.md               Repo Structure Summary
└── .gitignore                     excludes generated .img test images — see Make-Test.x below
```

## Docs/

**./** — the last canonical flush, as you put it. Only content that's been reviewed and confirmed lives here. This is what check.py validates for internal and cross-document consistency, and what any new AI session should be primed with — never a chat transcript, never a draft.

**ADR/** — the decision log. One file per decision, never edited; a changed decision is a new file that says which one it supersedes. This is the direct fix for the specific failure this session hit twice: a design explicitly rejected by an audit (the variable-length WAL entry, the live-switching concurrency model) resurfacing later as if it were new, because nothing recorded that it had already been decided against. Zipping older cycles keeps the browsable folder small without losing that record — check.py unzips on demand, so a current decision can still cite and supersede something from three stable releases ago without a gap.

**Drafts/** — where DeepSeek/Claude output lands before it's trusted. This is the piece the original structure was missing: without it, there's nowhere for in-progress work to be checked *before* it's treated as canon, which is exactly how io-engine-data.txt and the allocator file ended up out of sync with what was actually being discussed. check.py should run against Drafts/ too — a draft that contradicts Specs/ fails loud there, before a PR ever tries to merge it in.

## Src/

Shared code sitting loose directly under `Src/` (rather than its own `Common/` subfolder) mirrors the flat, everything's-a-peer style the Linux kernel itself uses in `fs/`. Worth a second look once it grows — the Python SMART Handler code, shared checksum/serialization routines, and anything else genuinely used by both Kernel/ and FUSE/ builds will all pile up at the same level as two subfolders that are otherwise clearly scoped. A `Src/Common/` split costs nothing now and keeps that from getting messy later — flagging it as an option, not a correction, since the flat layout is a reasonable, precedented choice on its own.

Kernel/ and FUSE/ both getting Windows/Linux subfolders is consistent — FUSE proper is Linux (and macOS, unlisted since it's not a target), and WinFsp is the Windows analogue close enough to slot into the same shape. Kernel/ stays a placeholder per the README (FUSE is the recommended, actually-developed path; kernel-level is future work, not current).

Where does the SMART Handler go, concretely? `Src/` (shared), not `Src/FUSE/` — it's a userspace Python process talking to the I/O Engine over IPC, and that boundary should look the same whether the engine underneath is FUSE or, eventually, kernel-resident. This is one of the things that becomes fully answerable once the C-process architecture from the roadmap is settled — right now it's a reasonable placement, not a locked one.

## Tools/

**check.py** validates Specs/ (and Drafts/, per above) against itself and against `.reviewed-hashes`.

**.reviewed-hashes** is new — it's the concrete home for the content-hash tripwire from the CI discussion. A plain list of `filename: hash` pairs, updated by whoever reviews a Specs/ change alongside the ADR that justifies it. check.py recomputes and compares; a mismatch with no corresponding new ADR is exactly the signal that would have caught this session's io-engine-data.txt discovery on the first push, not 30 turns into a manual audit.

**Make-Test.x** compiles and runs against a generated `.img` file, not a committed one — hence `.gitignore` excluding them. Sizing logic (up to 70% of available space on GitHub, up to 16GB on a PC, both overridable by args) needs the runner disk-space caveat from earlier folded in: `ubuntu-latest` gives ~14GB free by default, so "70% of given space" is ~9.8GB there unless a cleanup step runs first to reclaim more.

## Root

**Makefile** at the top, dispatching into `Src/Kernel/` or `Src/FUSE/`, is the direct Linux-kernel-inspired choice you named — one entry point, the actual build logic lives closer to what it's building.

**.gitignore** — flagging since it wasn't in the original list: needs to exclude Make-Test.x's generated `.img` files at minimum, plus whatever build artifacts Src/'s compiles produce.
