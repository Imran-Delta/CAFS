# CAFS Roadmap — What's Settled, What Isn't

"Settled" = usable as-is for implementation. "Open" = needs a real decision before code gets written against it.

## I/O Engine

**Settled** (fs.info v19 + config.fs v19):
- Anchor region format: Superblock, Config Snapshot (TOML, confirmed permanent), Meta/User Pointer Table, Function Table, Parity Block — full byte layout, checksums, backup/recovery for all four real structures.
- Anchor LBA discovery + Superblock/Config confirmation and reconstruction on mount: Recovery Order (fs.info §14) has three explicit tracks by failure domain; In-Place Content Update protocol (§13.3) covers the common non-relocating case.
- WAL structure: fixed 48-byte entries, ANCHOR_DIFF via a separate 4KB block, `anchor_id` enumerated, sizing guidance stated.
- Config application inputs: config.fs now has everything the engine was already assuming existed (the `[io]` section, the two missing `[mount_hints]` fields).

**Open:**
- **io-engine-data.txt itself.** Everything above is the on-disk *format*. The engine's own spec — how it walks that format on read/write, its process structure, retry/error behavior — is still either the stale Aug-8 doc on disk or the newer draft from this chat. Blocks writing the actual read/write path. Needs: confirm which, or that it's being written fresh.
- **Data-block / B-tree node checksum storage.** fs.info only specifies anchor-region checksums. Nowhere specifies where a per-block checksum lives for ordinary file data, or resolves whether xxhash64 output is really truncated to 4 bytes for storage. Blocks "checksum" as a spec-able item.
- **Block size approach** — see below. Affects the read/write path directly.
- **"Reports to the above process."** Config application is I/O reads config → applies it → tells the process above what was chosen. That process isn't named yet — depends on the C-process architecture below.

## Allocator

**Settled:**
- Independent-knobs model; tiebreak = most-specific-condition-wins, with an explicit config fallback for the one case (Delayed Allocation) where two conditions share no variable.
- Concurrency fixed for the whole mount (closes the untracked-blocks race), informed by a persisted Service Data structure (Function Table entry 5) instead of a single live sample.
- Concurrency's coverage gaps closed (SSD+Q>16, NVMe 4–16 both land somewhere now).
- Live reconfiguration for the other six knobs has real precedent (atomic swap + generation counter, DeltaFS-style) — off by default, available as an option.

**Open:**
- **The actual v19 allocator.md file.** Everything above is agreed in conversation; not yet written into a document.
- **Live-reconfiguration edge cases** — queue depth limits, behavior if a client keeps writing through a stuck drain, timeout/abort. The mechanism is sound; the edges aren't specified.

## C-process architecture (new, undefined)

Introduced this session for the first time: multiple C processes, the oldest holding the critical handlers, newer ones not. Nothing — not fs.info, not io-engine-data.txt, not this chat before now — specifies:
- How many processes, and what "critical" concretely contains (WAL writes? checksum verification? Is SMART telemetry non-critical by definition?).
- How "oldest" is determined and what happens on its crash — promotion, read-only mount, or a designated primary independent of age?
- The IPC mechanism between processes — this is also what the SMART Handler protocol and "reports to the process above" both depend on.

This is what I'd put first, of everything here — three other open items can't be fully specified until it exists. It's also almost certainly the "which code" ambiguity behind Make-Test.x: without a settled process architecture, there isn't yet a single answer to which binary the test harness is compiling and running against.

## Python SMART Handler

**Settled:** on-disk format (fs.info §11 — 144-byte table, M-SMART/B-SMART split) and the write path's shape (I/O Engine → SMART Buffer → SMART Handler → B-SMART → WAL → M-SMART, §11.2).

**Open:** the actual protocol between the C I/O Engine and the Python handler — what the "SMART Buffer" concretely is (shared memory? pipe? socket?), message format, backpressure if the Python side lags. Depends on the C-process architecture decision.

## "A few other handlers"

Not inventoried anywhere yet. config.fs has runtime sections that read like they'd need their own handler (`[defrag]`, `[compaction]`, `[dedup]` scanning) — not guessing which of these get their own process versus running inline in the I/O Engine, since you flagged this as uncertain yourself. Worth a short list before this is more specific than "exists."

## Recommended order

1. C-process architecture — everything else depends on it directly or gets easier once it exists.
2. Block size approach and the data-block checksum gap — both block I/O Engine code specifically, independent of the process question.
3. Write the actual v19 io-engine-data.txt and allocator.md documents, now that the underlying decisions exist.
4. SMART Handler protocol, the other handlers — once the process architecture makes them answerable.

---

## Block size: your two options, plus a third worth weighing

**A — uniform block_size, per-file flags** (what `[optimization]` already does). No new structure. Doesn't actually vary block size — flags tune allocation/readahead, every block stays the same physical size. Not really a new toggle so much as the existing one under a new name.

**B — true per-block size, a Block Map in the Meta/User Table, updated every few seconds.** Delivers the real ZFS-style benefit — small files get small blocks, large sequential files get large ones. The open question: "updated every few seconds" is fine for telemetry (SMART-style — losing a few seconds on crash is harmless) but risky if the Block Map determines *how to interpret data already on disk*. A crash between writing a block and flushing its Block Map entry risks the next read using the wrong size for that extent. Needs to be either WAL-logged and atomic with the write it describes, or explicitly redesigned as advisory with a checksum-boundary cross-check — not left as "periodically flushed" the way SMART counters safely are.

**C — variable extent length instead of variable block size** (not one of your two, worth considering as a third): CAFS already has extent-based allocation (`max_extents_per_file`, the `extent` optimization flag). A large sequential file already gets most of the benefit "variable block size" is chasing — one metadata entry covering a long contiguous run — without changing the fundamental block unit or needing a Block Map at all. Closer to what ext4/XFS/Btrfs actually do, and more proven for mixed general-purpose workloads than ZFS's per-dataset recordsize (which shines more where a whole dataset shares one I/O pattern — VM images, databases — than a consumer machine's mixed file types). Doesn't help the many-tiny-files case the way B would — that's the one problem extent length can't solve.

Not picking one today — flagging as one open item, since it changes what "checksum" and "read/write" mean for the I/O Engine regardless of which wins.
