# CAFS Allocator

**Document version:** v1
**Companion to:** fs.info v22, config.fs v22
**Status:** Architecture and behavior locked; several numeric thresholds still placeholder (marked throughout). Implementation not yet written.

This document covers the allocator's architecture, concurrency model, configuration surface, and runtime behavior. It does not cover code — no Rust included, per the scope this was written under.

---

## 1. Process architecture: daemon and library, both real

The allocator is not committed to a single deployment shape. Both a separate-process (daemon) and in-process (library) form are supported, selected via `[allocator].mode` (Section 3) — not a single global architectural choice.

**Why both, briefly**: a daemon buys crash isolation and simpler concurrency reasoning (requests naturally serialize through IPC) at the cost of per-operation latency — a bare Unix-domain-socket round trip runs roughly 10–50+ microseconds versus tens of nanoseconds for an in-process lock. That tax is noise against an HDD seek (several milliseconds) but a real fraction of an NVMe operation's total cost (often sub-100-microsecond access latency), and FUSE already pays one unavoidable kernel↔userspace hop per operation regardless — a daemon adds a second local hop on top of that. No mainstream FUSE filesystem splits its allocator into a separate local process for this reason (SPDK deliberately avoids any process boundary on the storage hot path for the same reason, at the NVMe-performance end of the spectrum).

**Code shape**: core allocation logic lives in one plain struct/module — `allocate(request) -> response`, `free(request) -> response`, `update_knob(knob_id, value) -> ()` — agnostic of caller. A shared trait defines that contract. Two thin implementations satisfy it: one calls the core struct directly, in-process (library); one serializes over a socket, using the same message shape as the trait's own methods, not a bespoke protocol (daemon). Callers in mntproc hold "something implementing the trait" and never branch on which mode is active — that decision is made once, at mount, based on `mode`.

**Panic isolation, library mode**: Rust's `catch_unwind` at the allocator's call boundary catches a panic without taking mntproc down with it, at effectively zero cost on the non-panicking path. Not equivalent to OS-level process isolation, but covers the "one bad allocation call shouldn't kill the whole mount" case without paying IPC tax on every operation.

---

## 2. Concurrency

**Model**: per-zone/block/B-tree-leaf `RwLock` (Rust's `std::sync::RwLock` or `parking_lot`) — any number of concurrent readers, or one exclusive writer, never both. Two unrelated areas (e.g. `/home` and `/logs`, different zones) write concurrently with zero serialization between them.

**Fixed at mount.** The one knob explicitly excluded from live reconfiguration — an earlier design that allowed live-switching this specifically caused a real data race (two overlapping locking regimes, audited and rejected before this session began). Media type picks it at mount: Global lock for HDD (matches ntfs-3g's own verified approach — confirmed directly from its source that it never runs FUSE multithreaded, `fuse_loop()` never `fuse_loop_mt()` — a mature, real precedent for "serialize everything" as a legitimate choice, at real throughput cost), per-zone `RwLock` for SSD/NVMe (needed to actually get parallel throughput out of flash).

**Multi-zone operations** (a cross-directory rename is the concrete case): fixed global lock-acquisition order — always the numerically lower zone ID first, regardless of which is logically "source" or "destination" — holding both for the whole operation. Deadlock-free by construction (no cycle can form if every thread acquires in the same order) and preserves POSIX rename atomicity (no window where a lookup resolves from neither the old nor new path). A same-directory rename only touches one zone and needs none of this.

**SATA/NVMe**: no artificial CAFS-level concurrency cap on either. SATA's NCQ caps at a 32-command queue depth in the protocol itself; NVMe supports up to 65,535 queues of up to 65,536 entries — the ceiling in both cases is the hardware/kernel, not something CAFS needs to manage. Confirmed via this session's discussion, not a placeholder.

**Open**: whether "same file / same block / same leaf / same zone" are four genuinely separate lock domains an operation might need simultaneously, or collapse to one (zone-level) in practice — not resolved.

---

## 3. Modes and presets

**Four modes**: `performance`, `balanced`, `safety`, `custom`. A mode gates which presets are *eligible* at mount time — it does not fix runtime knob values and does not override live signal-driven swapping afterward. If exactly one preset survives a mode's filter, it's auto-selected; if more than one survives, `--type=trends`' values pick among what's left (exact tiebreak scope — health signals only, or the full Signals set including Workload Bias — not yet confirmed).

This is a deliberate two-stage design: mode narrows the *starting point*; the six hot-swappable knobs (Section 5) move independently afterward, driven purely by live signals, regardless of which mode chose the starting preset. A healthy drive in `safety` mode still reaches fast knob values once trends confirms it; a degrading drive in `performance` mode still gets the same protective swaps everyone else gets. Mode is not a permanent ceiling.

`safety` biases toward the daemon process model and more checking (Section 1); `performance`/`balanced` default to library. **Open**: `balanced`'s daemon-vs-library default specifically was never confirmed.

### 3.1 Preset table (A0–A5, legacy naming retained)

Hardware collapses to two real categories, not three — M.2 is a connector, not a protocol; an M.2 drive is NVMe or SATA underneath, with no distinct behavior profile of its own. SSD-SATA and SSD-NVMe are identical across every current knob (nothing in the present knob vocabulary distinguishes them — the thing that would, an NVMe-specific concurrency treatment, doesn't exist as a knob; see Section 2's SATA/NVMe note). Six presets, not nine:

| Knob | HDD-Perf | HDD-Balanced | HDD-Safety | SSD-Perf | SSD-Balanced | SSD-Safety |
|---|---|---|---|---|---|---|
| Concurrency | Global | Global | Global | Per-zone | Per-zone | Per-zone |
| Placement | Locality-First | Locality-First | Locality-First | Parallel-Spread | Parallel-Spread | Parallel-Spread |
| Preallocation | On(4) | On(4) | Off | On(4) | On(4) | Off |
| Delayed Alloc | On | On | Off | On | On | Off |
| Read-Ahead | 4MB | 256KB | 128KB | 4MB | 256KB | 128KB |
| Flush Interval | Long | Medium | Short | Long | Medium | Short |
| Bad-Block Reloc | Reactive | Reactive | Proactive | Reactive | Reactive | Proactive |

Concurrency and Placement don't actually vary by Performance/Balanced/Safety within a media type — they're hardware-driven, not preference-driven. Only the other five knobs respond to the mode axis.

A4/Emergency is **not** a seventh preset — it's the hard-floor override (Section 6), cross-cutting every preset rather than being one. A drive's original static-model design had no hysteresis here (permanently trapped in emergency once triggered); routing hard-floor recovery through a real remount rather than a live in-place switch both avoids the concurrency-overlap hazard (Section 2) and gives a natural exit once a later remount's fresh detection shows improved health.

---

## 4. Signals

Computed by `smart_handler.py --type=trends`, implemented and smoke-tested (see the file itself for the actual code). Summarized here for the allocator's side of the contract:

| Signal | Range/shape | Drives |
|---|---|---|
| `free_space_percent` | 0.0–1.0, Zone Table pass-through | Placement, Preallocation |
| `workload_bias` (B) | unbounded, recalibrated threshold 2.0 | Read-Ahead |
| `r_write` | 0.0–1.0, fixed-cap normalized | feeds `workload_bias` only |
| `health_composite` | 0.0–1.0, EWMA-smoothed, 5 inputs | Delayed Allocation, Flush Interval |
| `pending_trajectory` | EWMA of rate-of-change | Bad-Block Relocation |
| `write_size_cv` | coefficient of variation | Preallocation |
| `hard_floor_active` | monotonic boolean | overrides all six knobs at once (Section 6) |

`health_composite` combines three real ATA attributes (SMART 5, 197, 188) with two CAFS-native on-disk counters (`checksum_mismatch`, `host_relocations`, sourced from the already-reconciled on-disk SMART table) via `max()`, not average — one climbing input isn't diluted by four clean ones. SMART 187/198 never reach this scoring at all; they go straight to the hard floor (Section 6). All values are pre-normalized specifically so `allocation_chs.rs` (Section 5) never computes anything, only compares.

`smart_handler.py`'s payload inputs for per-operation I/O data (`io_placeholder`) and real `smartctl` reads (`smartctl_placeholder`) are currently placeholders — no `smartctl` shell-out and no `syscalls.rs` counter-population exist in the codebase yet. The Signals math itself is real and reviewable now independent of that wiring.

---

## 5. The seven knobs and the hot-swap mechanism

**Concurrency** is fixed at mount (Section 2) and is not part of what follows.

The other six — Placement, Preallocation, Delayed Allocation, Read-Ahead, Flush Interval, Bad-Block Relocation — are fully independent. Each has its own timer and its own generation-counter/`Arc` state; none of the six depend on any other's state, and none require an in-flight operation to pin a whole-config snapshot the way one global lock would — an operation reads a knob's current value at the exact point it needs that knob (Placement when picking a block, Read-Ahead when issuing a read), not once at operation start.

**Decision logic lives in `allocation_chs.rs`** ("conf hot swap"), a distinct file/module from the core allocator (`allocator.rs`) and separate again from knob *definitions*. This three-way split — core mechanics, decision policy, knob value definitions — means a third party can swap `chs.rs` for different policy over the same known behaviors, and can add new positions to an existing knob's enum, without touching core allocation code. It does **not** give arbitrary plugin extensibility for genuinely novel knob semantics — that would need the knob's *execution* (what "Sequential-Append" concretely does) to be trait-object dispatched, which it isn't; `allocator.rs`'s core logic still needs a matching arm for any new position.

**Per-knob timing** (config in `[allocator.timing]`):
- Poll cadence: 15 seconds, matching `--type=trends`' invocation interval.
- Consensus: 3 consecutive poll readings must agree within a soft range (not exact match) before a knob is eligible to change — exact width of "soft agreement" is a placeholder, flagged as needing real on-disk testing.
- Stabilization lock: 300 seconds *per knob*, independently — once a knob changes, that specific knob cannot change again for 5 minutes, with zero effect on any other knob's timer.

**Mechanism, RCU-style**: each knob holds its current value behind a reference-counted pointer (`Arc<KnobValue>`) plus an atomic generation number. An operation needing that knob clones the `Arc` at the point of use — cheap, just a refcount bump — and uses it for that read, unaffected by any swap that happens afterward. A swap builds a new value, wraps it in a fresh `Arc`, atomically replaces the pointer, bumps the generation. No stalling, no draining, no operation ever observes a torn value.

**Signal-to-knob mapping and decision rules**:

| Knob | Driven by | Rule |
|---|---|---|
| Placement | media type (mount constant) + `free_space_percent` | media default, overridden to Sequential-Append if free space < threshold regardless of media |
| Preallocation | `free_space_percent` + `write_size_cv` | Off if free space low, regardless of variance; else On(4) if low variance, Off if high |
| Delayed Allocation | `health_composite` | Off if degraded, On if healthy |
| Read-Ahead | `workload_bias` (+ `health_composite` override) | 4MB if B above threshold, 256KB baseline, 128KB if health degraded overrides either |
| Flush Interval | `health_composite` | Long/Medium/Short mapped to healthy/mid/degraded |
| Bad-Block Relocation | `pending_trajectory` | Proactive if trending up past threshold, else Reactive; **Off is believed manual-only, never a live-swap target — not fully confirmed** |

---

## 6. Hard floor

A single global, monotonic flag — `hard_floor_active: AtomicBool` — checked first by every knob's read, ahead of that knob's normal `Arc`-based logic. If set, the knob skips its pinned value and uses its hardwired emergency position instead (Section 6.1); if unset, normal independent-timer logic proceeds untouched. One flag, no coordination needed across the six knobs' independent generation counters — it short-circuits ahead of them rather than trying to force them to bump together.

**Trigger, ATA**: SMART 187 (Reported Uncorrectable Errors) or 198 (Offline Uncorrectable Sector Count), either non-zero. Both are literally "uncorrectable" — the drive's own firmware has already exhausted its retry attempts before reporting either, which is what makes this real signal rather than noise, not just a strict-sounding name.

**Trigger, NVMe**: `critical_warning` bit 2 (NVM subsystem reliability degraded) or bit 3 (media placed read-only). Bit 2 specifically matches VMware vSAN's own automatic-action trigger — real precedent, not an invented threshold.

**Write semantics**: single writer only — the real-`smartctl`-reading code path. Monotonic within a mount: once true, stays true for the rest of the session; clearing it is a hot-remount concern (Section 6.2), never a live reset. A reader checking the flag mid-write-cycle just sees the last published value, stale by at most a few seconds — never a false positive, given the flag only ever moves one direction per mount.

Never disregarded regardless of `[allocator]` configuration — this is the one override no preset or mode can turn off.

### 6.1 Emergency knob values

Reuse A4's original per-knob positions rather than inventing a separate bundle: Sequential-Append, Off, Off, 128KB for Placement/Preallocation/Delayed Allocation/Read-Ahead. Flush Interval = Short (minimizes the data-loss window, directly matching the point of the override). Bad-Block Relocation is a genuine open call, not yet made — Off (relocating adds load to an already-failing drive) versus Reactive (still try to save what's recoverable before it's too late) — logged as open, not decided by default.

### 6.2 Response protocol

Switch to the safest available mode; this requires a hot remount specifically for the Concurrency knob (the one thing that can't change live), unless configuration explicitly forbids remounting. **Must fully quiesce/drain in-flight I/O under the old concurrency regime before switching** — a live in-place flip without a drain step reintroduces the exact overlapping-epoch race that got live algorithm-switching rejected in an earlier design; a hot remount only avoids that risk if it actually waits.

Determine the affected general area (zone-level, via the Zone Table's existing health-signal-aggregation role) and warn or error the user that their hardware is degrading or has failed. **Open, unresolved**: whether the safety escalation itself is global (whole volume) or zone-scoped (quarantine just the affected zone); what determines warning versus error severity; and a harder, deferred problem — smartctl-derived signals are inherently whole-device, so "which zone is affected" may not always be answerable even in principle, since the failing physical sector might not correspond to any zone CAFS is currently tracking at all.

---

## 7. Boundary behavior: CoW, dedup, ENOSPC, exclusive grants

**The allocator is policy-aware, not a dumb LBA provider.** It talks directly with the I/O Engine — CoW redirection only works if the allocator itself knows a block is snapshot-shared and needs to be redirected rather than overwritten in place; the I/O Engine cannot make that call blind.

**Nothing bypasses the allocator except by explicit exclusive grant** (WAL is the existing case: the allocator hands over a raw-tagged region at mount and the grant-holder operates within it directly). Any process holding such a grant has exclusive access to that region. This requires a **revocation path**, not just a grant path — a grant with no corresponding revoke is a leak by construction. **Open, deferred to implementation**: the actual independent-verification mechanism (the I/O Engine must be able to invalidate a permit through a system separate from whoever issued it — requirement locked, "how" is not). One piece is settled: fsck invalidates stale grants as part of its existing crash-recovery scope — a crash-time grant check, not a new mechanism, just one more thing fsck verifies on its way through.

**Dedup**: detection (the hash lookup/match) is independent of the allocator — no mutation, no need for the lock discipline. *Registering* a match (the refcount bump on the Dedup Table / Zone Table) goes through the allocator, same as CoW's redirect — because that's a mutation, and concurrency-safe mutation only happens through the one place that owns the locking.

**ENOSPC**: rescoped from "the device is full" to "the allocator could not satisfy this specific attempt" — a file carrying `extent_limit=1` can hit it on a drive that's mostly empty, if no single run of contiguous space large enough exists. Per-attempt, not persistent state.

On failure to place normally, the allocator logs a warning-level signal (discoverable via system logs) and attempts a lightweight recovery: temporarily dropping the extent-limit *preference* (not the hard `extent_limit=1` requirement — see below) to make already-available fragmented space usable, without invoking defrag/balance/storage-expander or any other Routine Tasker task inline. This is deliberately not "run compaction synchronously" — that would block the calling write for an unbounded, potentially very long time on a large HDD, and directly contradicts defrag's own on-demand-only status on SSD. If the file whose block is being written carries an extent-limit tag, or if genuinely no space exists at all, ENOSPC is raised immediately with no relaxation attempt.

`extent_limit=1` is the mechanical form of "don't fragment me": automatically implies no-CoW (a CoW redirect elsewhere would itself violate the single-extent guarantee), and is a hard requirement, not a soft preference — the allocator is explicitly forbidden from silently degrading to fragmentation as a fallback for a tagged file. If it can't satisfy the requirement, it fails with a distinct error (Docs/error_id.md) rather than fragmenting anyway. This is the direct mechanism behind the Easy2Boot-style contiguous-layout use case flagged earlier in this project's history.

Identifier-slot exhaustion (a separate resource from data-block space — the Identifier Table can be full while the Zone Table has free blocks) is a **third**, distinct condition from generic ENOSPC — reclaiming data blocks does nothing for it. Own error code (Docs/error_id.md); reclaim path is the existing async identifier-retirement/reuse mechanism (Recently-Zeroed Queue), not something ENOSPC's lightweight relaxation touches.

**Open**: EDQUOT and EFBIG are not part of CAFS's design — no quota system and no independent max-file-size concept exist anywhere in this project; including them would be importing POSIX's error taxonomy without the underlying mechanism that would make either real.

---

## 8. Cross-cutting notes

- **Cross-mount state**: fresh mount is fresh mount — the I/O Engine checks config and on-disk data, verifies, remounts with the concurrency model the config/hardware detection calls for, then hands the allocator a starting preset. Knob state from the previous mount is not remembered. The allocator and I/O Engine share the same concurrency-model code, since the I/O Engine is the one that has to remount to apply it.
- **Multi-device pools**: explicitly out of scope for V1, not architecturally precluded — modular file boundaries specifically so multi-device support later is drop-in replacements rather than a rewrite. Drive pooling is treated as directly opposed to the safety goal for V1 ("it's not RAID").
- **IPC**: `smart_handler.py` → mntproc uses the existing stdin/stdout pipe, unchanged. mntproc → allocator reuses the daemon/library trait (Section 1) with `update_knob` as one more method on it — no new channel invented for either hop.
