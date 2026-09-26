# CAFS I/O Engine — Research Pass & To-Do List
**Date:** 2026-09-19
**Scope:** Verification of every ADR/spec claim relied on this session, plus new gaps implied by this session's own decisions but not yet discussed. Numbered for section-by-section discussion — reference by number. Resolved items keep their number and are marked RESOLVED with the date, rather than being deleted, so the record of what changed stays intact.

---

## Part 1 — Verification results

**1. ADR supersession sweep: clean.** ADR-009 → superseded by ADR-011 (Rust) — already the working assumption throughout. ADR-013 (Hash Table origin) → amended same-session by ADR-023 — already treated as current authority, confirmed consistent.

**2. `append_only`'s status chain.** ADR-026 flagged it as not ready to be a simple bitmask bit; ADR-030 (Snapshots) resolves it back to plain POSIX write-position meaning, with the retention behavior it was originally meant for moved to snapshots instead. The 9-hint/1-structural/1-new(dedup) = 11 count already given is correct; this just closes the loop on why.

**3. `config.fs [optimization]`'s `# Flags:` comment is stale** — ADR-026's own consequences flag it directly. Superseded in scope by #34 (the deeper recategorization), but the mechanical fix is still real and not yet done.

---

## Part 2 — New gaps, found by cross-referencing ADRs

**4. Raw-as-dedup-source — RESOLVED, 2026-09-19.** Strict exclusion: no CoW means off dedup entirely, source or target, no exception. ADR-027's original text already said this; confirmed it stands, unmodified.

**5. Snapshots of non-CoW (`raw`) files — direction set, mechanism leaning, two questions open.** CAFS will support this via an explicit opt-in (unlike BTRFS), toggle, default off. Leaning lazy: first write after the snapshot triggers a one-time CoW break for the touched blocks only, same as any ordinary post-snapshot write already does — no new mechanism. Confirmed independently twice during filesystem research: bcachefs's `nocow` does exactly this on a snapshot/reflink reference, and the generic `NO_DATA_COW` inode flag (BTRFS's family) is defined the same way — "no CoW while refcount is 1." Open: (a) per-file opt-in vs. volume-level `[snapshot]` policy — the "per-file where possible" principle (#36) argues per-file; (b) eager-copy alternative is simpler but breaks ADR-030's near-zero-creation-cost claim, not the current lean.

**6. "Metadata" in the 4th hash-table toggle mode was never defined.** ADR-029 leaves content-addressable metadata dedup open, names filenames as the plausible candidate. The "per-LBA + metadata" mode assumes an answer here but never pinned one down — filenames specifically, or broader (ACL blobs, `unix_mode`)? Needed before that mode is buildable.

**7. Identifier slot reuse-vs-retire (ADR-029's own open item) looks like the same shape as this session's permit design.** Whether a freed numeric identifier can be reissued is unresolved in ADR-029. This session's permit table solved the identical problem for `grant_id` (reusable, safe because the table — not the bare ID — is authoritative). Worth checking whether the same mechanism applies directly, and whether ADR-029's "recently-zeroed queue" is meant to be that table already — the Function Table's `recent_zero_queue_lba` field exists, with a provisional entry format (`tag`+`value`+`timestamp`, not yet locked), confirming the field's real but not confirming the reuse mechanism itself.

**8. Zone Table's per-block tracking has no byte layout, and two Accepted ADRs already depend on it existing.** ADR-021: zone-level aggregate only, "exact per-zone field widths deferred to Job 3." ADR-022/ADR-029 both cite finer per-block tracking as already real — not a contradiction (ADR-022 confirms it exists), just unspecified at the byte level. Not core I/O Engine scope, but `heat_caches()` may want zone free-space summaries for fast `statfs`, so adjacent enough to note.

---

## Part 3 — New gaps, found by reasoning through this session's own decisions

**9.** Permit table size never given a number — proposed "a bit above WAL's ~2 concurrent grants," no actual slot count.

**10.** Permit validation failure has no defined behavior — needs a `CafsError` variant/`CA-` code and a retry-or-fail decision.

**11.** `CafsCtx`'s concurrency story stops at the counters — `heat_caches()`'s loaded tables (Dedup, Hash) need their own lock per table once mntproc is concurrent; permits only protect physical LBA ranges, not in-memory structures.

**12.** Misaligned write + permit interaction unverified — does one permit need to cover both the internal read and internal write of the bounce-buffer path, or is there a window between them?

**13.** Checksum-mismatch handling on an ordinary (non-drain) read never walked through — the CI's own test summary references an existing "corruption/auto-repair path" worth checking before deciding whether the real design keeps it, extends it, or simplifies to error-up-to-mntproc.

**14.** `heat_caches()` at scale is unbounded as discussed — fine for a modest volume, no stated ceiling for a very large one.

**15.** mkfs's real scope, now that Cache SMART/Dedup/Hash Table all have (or will have) real addresses — does the real formatting tool need to initialize these regions, or can mount-time code treat unformatted as empty and lazily initialize?

---

## Part 4 — I/O and SMART on-disk

**16. Cache SMART / SMART flush pipeline — mechanism fully understood, 2026-09-19.** M-SMART and B-SMART are never modified in place. Cache SMART holds the raw, unverified combination of `{device SMART stats + current V-SMART RAM counters}`, written after every check cycle, *before* reconciliation — it's `reconcile_smart_tables`'s input, not an authoritative value (the function already tolerates it being absent or checksum-failed, falling back to main/backup). Because M/B-SMART can go hours between updates, Cache SMART is the only current picture available in the meantime — that's a real job, which means it does need protection against being read mid-write, just not WAL-level protection (staleness is fine, torn reads aren't). Landed on: same atomic-pointer-swap treatment as B-SMART (write new, flip, old discarded) rather than a bare in-place overwrite — cheap enough given the update frequency. B-SMART updates this way directly; M-SMART goes through a WAL entry first for the extra margin on the primary copy. "WAL and Cache SMART are both scratch type" reads as a shared allocation-purpose *tag* (filesystem-internal, not user data), not a shared grant *mechanism* — their actual needs (large/long-lived/must-not-be-lost vs. small/single-writer/loss-tolerant) are different enough that WAL's full exclusive-grant machinery doesn't fit Cache SMART. Function Table already has `smart_main_lba`/`smart_backup_lba`/`scratch_lba` as separate pointer fields — placement being "random" (mkfs/allocator-chosen, not fixed) is already how every pointer field in this format works, nothing new needed there.

**17.** WAL's inner format under ADR-046 still open — does the old block-header/prev-next chaining still apply within one chunk, or is drain a pure linear scan?

**18. `scratch_lba` vs. `temp_cache_lba` — RESOLVED, 2026-09-19, and "scratch" now has a precise, unified definition.** `temp_cache_lba` → routed to RAM, not disk at all — originally conceived as RAM-like (heavily rewritten), then correctly recognized as exactly the wrong access pattern for SSD write endurance/amplification. Sidesteps the problem by not persisting it. Open sub-question: does the Function Table field get removed outright, or left reserved? `scratch_lba` is Cache SMART's own pointer (see #16) — a fixed, directly-overwritten small block, not a generalized grant mechanism shared with WAL.
  What "scratch" actually means, now confirmed by three independent examples: allocated space holding **provisional** data — not yet part of the filesystem's committed, referenced state — that only becomes real once a pointer-flip/promotion step completes. WAL chunks (pending operations, not yet applied), Cache SMART (a candidate value, not yet promoted to B/M-SMART), and relocation's own copy-first staging (#43 — a candidate new location, not yet the file's official extent) are all the same pattern, not three unrelated things sharing a name.

**19.** ~~Raw-as-dedup-source config key name~~ — closed. #4 resolved to strict exclusion, no config gate to name.

**20.** Extent-limit mechanism — see #35 (XFS precedent) and #44 (timestamps change the row-growth math too); still open which way this goes.

**21.** `flag_group` data file — format and location, explicitly deferred for later.

**22.** Hash-table toggle naming/tokens need redoing — earlier guess (`off/file/lba/all_lba`) was based on a wrong reading of the 4th mode (it's per-LBA + metadata, not "ignore per-file opt-out").

**23.** Hash Table hash algorithm — SHA-256 vs. reusing BLAKE3-256 — asked, never actually answered.

**24.** `[hash_table]` has no bucket-count/overflow-depth config knob, unlike `[dedup_table]`.

**25.** Superblock `flags` field (offset 56–59) — undocumented, no bits assigned. Candidate future use: #40's remount-pending-freeze trigger, if that gets picked up.

**26. `format_version` validation — RESOLVED, 2026-09-19.** `check()` compares the Superblock's stored value against `layout::FORMAT_VERSION` at mount; any mismatch (older or newer) refuses the mount for V1. No backward-compat parsing — out of scope until there's a real second format version to support.

**27.** Permission-delegation layering (POSIX check → allocator permit → I/O Engine execution) — proposed in conversation, not yet written down anywhere real.

**28.** Allocator-vs-master ownership of the permission cases the kernel can't handle — explicitly left as "needs discussion."

**29.** TRIM relay — never re-audited against every new way a block becomes free this session added (WAL drain/shrink, dedup-redirect freeing the losing copy, raw-group interactions).

**30.** I/O Engine's `CA-` range — 5-digit width agreed, actual starting block/prefix never picked.

**31.** WAL chunk sizing correction (`max(512MiB, block_size)`, MiB not MB) — said in conversation, not yet folded into ADR-046's actual text.

**32.** ADR-056 itself — everything locked this session (allocator-as-sole-lock-holder, permit shape/table, WAL exclusive-grant model, generalized to any scratch-type grant per #16/#18, mount resequencing, `confirm()`'s new report shape) still needs writing up as the real ADR.

---

## Part 5 — Flags, filesystem research, and file metadata

**33. `append_only`'s BTRFS precedent doesn't actually exist — checked directly.** BTRFS supports only `i` (immutable), `D`, `m`, `S`, `V`, no-dump — explicitly "no other attributes are supported." Append-only is a generic ext2-family `chattr` attribute BTRFS never implements. The real BTRFS-precedented (and XFS/ext4-precedented, per #35) flag CAFS doesn't have is `immutable` — stricter than append-only, blocks all writes/deletes/renames. No "delete-only" attribute found anywhere. Open: add `immutable` as a 12th hint flag?

**34. `[optimization]`'s flat hint bitmask needs recategorizing by which component enforces each flag (supersedes #3's narrower scope).**
  - Allocator-consulted (placement/layout): `sequential`, `random`, `raw`, `journal` (probable), `extent`/`btree` (already separate, structural)
  - I/O-Engine-enforced (on the read/write call itself): `sync`, `append_only`
  - Policy/indexing subsystem: `hash`, `dedup`
  - Unclear, needs checking rather than guessing: `read`, `write` — plausibly mntproc/page-cache territory, not allocator or raw I/O.
  Not a format change — one 32-bit field either way. About documenting which code looks at which bit.

**35. Cross-filesystem research, first pass (BTRFS/XFS/ZFS).**
  - **XFS's `fsxattr` struct is the real-world precedent for extent-limit, and it argues for row growth over a blob-trailer.** A small fixed set of per-file `u32` fields (`fsx_extsize`, `fsx_nextents`, `fsx_projid`, `fsx_cowextsize`) sit directly alongside the flags bitmask — no xattr detour. Revises the earlier blob-trailer lean toward growing the Identifier row XFS-style, *if* extent-limit stays a small, fixed set of numeric fields rather than open-ended (see #44, timestamps compete for the same row-growth budget).
  - XFS also confirms `immutable` as a broad, multi-filesystem standard (ext4/XFS/BTRFS all implement it identically).
  - **ZFS's dedup/compression/sync/etc. are per-dataset only, never per-file** — no per-file equivalent exists; the real workaround is more datasets. Concrete contrast for #36: CAFS is deliberately going finer-grained than ZFS offers for the same feature set.

**36. "Almost everything that can be per-file should be per-file" — design principle, stated 2026-09-19.** Applies to policy/behavior knobs (argues for real per-file granularity on extent-limit, and for #5's opt-in being per-file). Doesn't apply to physical format decisions that are genuinely volume-wide by nature — concurrency/locking model (ADR-041, hardware-derived), `format_version`, WAL chunk size, hash algorithm choice. "Per-file" isn't meaningful for these; worth keeping the boundary explicit so the principle doesn't over-extend.

**37. Is fs.info the right document for end users? Opinion: no.** It's a byte-offset format spec — right for a developer or hex-editor debugging, wrong for "should I turn on hash-table mode." `config.fs`'s comments are closer but too terse for real tradeoff/interaction explanation. Recommend a genuinely separate user-facing document, organized by "what does this do / when do I want it," with `flag_group` (#21) likely belonging there too rather than standing alone. Requirement added, 2026-09-19: every option's entry needs to cover HDD and SSD implications specifically, not just a flat description — #47's ECC feature is the clearest example of why (identical toggle, opposite verdict depending on media), but write amplification/endurance considerations show up throughout this whole pass (`temp_cache`, WAL rotation, zone ECC) and shouldn't be scattered — this document is where they get stated consistently, per option, in one place.

**38. F2FS and bcachefs, second pass.**
  - **bcachefs already ships the exact two-tier model in #36** — options tagged `format,mount,runtime` (volume-only) vs. `...,inode` (also per-file/per-dir via `setattr`). Real, shipped precedent for the volume-default-with-per-file-override split.
  - **bcachefs's `nocow` already answers #5** — see #5, now the primary citation.
  - **F2FS contributes genuinely new concepts:** `pin_file` (GC/relocation exemption specifically, not absolute — still GC'd past a failure threshold, precedent for protected flags needing an escape hatch), `atomic_file` (all-or-nothing multi-write transactions, nothing like this discussed in CAFS), `inline_data` (very small files live in the inode, no extent at all).
  Not covered: legacy/niche (ReiserFS, JFS, HFS+) — low value, skip unless wanted.

**39. NTFS and APFS.**
  - **Both have real, mainstream per-file encryption** (NTFS: EFS; APFS: separate key per file for data, another for metadata). CAFS has discussed none — largest feature gap this research pass found.
  - **Per-file compression is universal** (NTFS, F2FS, generic inode-flags family) — CAFS had zero compression *flag* discussion before #41 corrected the record on the policy side; the per-file toggle itself still isn't decided.
  - **Third independent confirmation of conditional-no-CoW** — generic `NO_DATA_COW`: "no CoW while refcount is 1," same shape as bcachefs's `nocow`+snapshot behavior. Convergent across two unrelated filesystem families now.
  - **APFS's `clonefileat(2)`** — explicit, user-invoked, instant per-file/per-dir clone, CoW-shared until modified. A third primitive alongside dedup (automatic) and snapshot (whole-volume) that CAFS doesn't have an equivalent of. Not urgent, worth a decision eventually.
  - Lower priority, also present: `PREALLOC` (narrower than `raw`), `NO_ATIME`, `DIR_SYNC`, `NO_DATA_SUM` (per-file checksum opt-out).

**40. Remount-triggered immutable-snapshot idea (Fedora Kinoite-inspired) — RESOLVED as out of I/O Engine scope, 2026-09-19.** Core insight held up under checking: a CoW snapshot already *is* squashfs+overlay's semantics natively, no second filesystem type needed — verified Kinoite's real mechanism (OSTree deployment-swap-at-boot) is the same "takes effect at next mount" shape. Scope resolved cleanly without touching ADR-030: tag directories `immutable` plus a marker to scope the *freeze*, keep the snapshot itself whole-volume. Correctly identified as namespace/directory territory, not I/O Engine. If picked up later: trigger storage for "pending freeze, apply next remount" is a candidate use for the undocumented Superblock `flags` field (#25).

**41. Compression — corrected, 2026-09-19.** Real design exists (ADR-025, Accepted, `[data]` config fields: 128KiB default chunk, 1GiB opt-in ceiling, floor tied to `block_size`) — earlier claim of "zero discussion" was wrong. Settles policy, not mechanics. Two real gaps, both I/O Engine scope:
  - Extent-list entry (`start_lba`+`length`, 16 bytes) needs a physical/logical length pair for compressed extents — stated as required by ADR-025, byte layout never worked out (grow every extent, or a discriminated variable shape?).
  - Whether compressed output pads to a block-size boundary or packs dense across boundaries is undecided — the dense option means every compressed access needs the same sub-block handling `misaligned_action` already does for ordinary unaligned I/O, just constantly instead of occasionally. Directly affects how much of the existing misaligned-I/O path compression ends up depending on.

---

## Part 6 — Superblock detail, and follow-ons from that pass

**42. `namespace_structure_id` — explained, 2026-09-19.** A future-proofing selector (ADR-034, following ADR-033's pattern): one legal value for V1 (the hash-keyed, per-directory B-tree ADR-034 defines), reserved so a later alternative namespace structure wouldn't break volumes formatted before it existed. Worth knowing: ADR-034's own consequences call its Superblock offset (84) "proposed, not yet explicitly re-confirmed as final" — `layout.rs` already carries it at 84 as settled. Doesn't break anything today (nothing reads it), but worth actually locking.

**43. Relocation, and the multi-journal architecture — corrected, broadened, and now largely resolved, 2026-09-19.**
  **Corrected**: the relocation log is not a permanent, every-I/O-consults-it indirection layer — that was my misreading. It's a crash-recovery journal for the relocator's own in-progress operations: before a move, an intent is logged; if power is lost mid-move, the relocator reads the log on next mount and knows what state that move was in, rather than the filesystem needing to become globally indirected.
  **Entry format, as described**: file hash, old logical LBA, old physical LBA, new destination, plus ECC data (error-*correcting*, not just detecting — a stronger, self-healing layer beyond the plain checksums used elsewhere in the format). Makes sense specifically because this structure is low-volume (capped at 1000 intents) — the same ECC overhead would be a real cost if applied to the main WAL's high-frequency entries, so this is a deliberate, scoped tradeoff, not a general pattern to carry elsewhere by default.
  **Reference-update timing — RESOLVED.** Copy data to the new location first, WAL-protected; only then flip whatever pointer/extent-list entry refers to it, old copy discarded. Same atomic-pointer-swap shape already established for B-SMART (#16). No deferred background collapse pass needed.
  **Where the copy stages — this is scratch's third confirmed use case (see #18).**
  **`abort_on_log_corruption` — RESOLVED as a three-value setting.** `false` / `true` / `on_ucerr` — standard correctable/uncorrectable framing. ECC (above) is the first line of defense for small corruption; the `data_checksum_algo_id`/Hash Table content-check is the fallback for what ECC can't fix; `on_ucerr` only aborts if both of those are exhausted.

**45. WAL vs. "journal" as a concept — opinion given, 2026-09-19, not yet decided.** Was an early, undiscussed assumption that the two were the same thing; explicitly reopened. Weighed both:
  - **One unified journal** (filesystem/relocation/adaptive all in one physical stream, one entry format): simplest implementation, and total ordering across every subsystem's entries falls out for free during recovery, no cross-stream reconciliation needed. Costs: the entry format has to compromise between relocation's rich, ECC-protected needs and the filesystem journal's compact, high-throughput needs (either bloats the common case or forces variable-length entries); every subsystem's activity contends for the same append point; a single point of failure for crash recovery across the whole system.
  - **Separate journals per subsystem** (current organic direction — `relocation_log_head_lba`/`tail_lba` already exist distinct from `wal_lba`): each format tailored to its actual need, no cross-subsystem contention, a problem in one doesn't jeopardize the others' recoverability. Main cost in the abstract — cross-journal recovery ordering — is substantially defused here specifically, because the allocator already holds exclusive access to whatever's being relocated for the operation's whole duration (this session's own locking model), so the dangerous interleaving case that ordering would need to solve for mostly doesn't arise.
  **Opinion: separate physical streams and entry formats, but one shared underlying mechanism** — head/tail rotation, checksum/ECC verification, and recovery-scan logic built once as a reusable primitive, with the filesystem journal, relocation journal, and any future journal each being their own instance of it rather than either a single merged stream or independently reimplemented from scratch. Gets the implementation-efficiency case for unification without the format-compromise and contention costs, and matches the direction the format's already grown in rather than working against it.

**46. Should unified-vs-separate journaling itself be a config option? Opinion: no, keep it fixed — same category as concurrency model, not a staged "defer past V1" question.** Scope correction: this session isn't V1-constrained (see below), so the right framing isn't "lock it for now, reconsider later" — it's a permanent architectural choice or it isn't. The shared-primitive architecture in #45 means the *data layout* could plausibly support either shape — a generic journal registry entry is small and uniform, and "how many instances exist" isn't fundamentally different from how many WAL chunks exist, which is already dynamic. The real cost is recovery-code risk: code that only ever handles N fixed, known, separate streams is simpler and more thoroughly testable than code that has to correctly handle either a merged or split layout depending on configuration. Recovery is the worst place in the system to carry branching for a choice that isn't itself safety-critical. Lean: fixed as separate, permanently, same footing as the concurrency model — not something to revisit post-launch either.

**47. Zone-level ECC for user data — corrected: HDD-only, and it's not a worse tradeoff on SSD, it's essentially no benefit at all.** Two independent reasons, not one:
  - **Write amplification** (already flagged) — real cost on any media, worse on SSD specifically for endurance.
  - **The actual protection doesn't exist on SSD.** An SSD's FTL owns real physical placement — wear-leveling constantly moves data, and the filesystem has no visibility into or control over which physical NAND die any given logical block actually lands on. A stripe's data and its parity could end up co-located on the exact same physical die with no way for CAFS to know or prevent it — if that die fails, both the data and the thing meant to recover it are lost together, in the same event. The redundancy has no guaranteed independence from the failure domain, which is the entire premise RAID-style parity depends on. HDDs don't have this problem — LBA-to-physical mapping is direct enough (modulo the rare remapped bad sector) that stripe members and their parity can genuinely land in different physical locations, giving real failure independence. SSDs also already have their own internal, hardware-level ECC (often LDPC-based, well beyond what software could reasonably do) protecting against the bit-flip-level errors this would otherwise help with — so even setting the co-location problem aside, there's little left for a software layer to usefully add there.
  Conclusion: HDD-only feature, config-gated — and the config should probably refuse to enable it on a volume identified as SSD/NVMe, rather than silently allowing a no-benefit, all-cost option.

---

## Part 7 — Session close-out, 2026-09-19

**Session ended on the session limit** mid-discussion of five items requested in one message: encryption (research), timestamps (discuss), extent-limit (elaborate), hash algorithm (discuss — "what's the difference?"), 4th hash-table mode (needed more context). Encryption research (fscrypt) was one search in before the cutoff. All five now have a real answer or elaboration in `cafs-job4-research-2026-09-19.md` and `cafs-concerns-opinions-2026-09-19.md` — none were left fully unaddressed, but none of the resulting recommendations are confirmed decisions yet either.

**Confirmed in the final exchange, now formalized as ADRs** (see `cafs-adrs-job4-2026-09-19.json`):
- Snapshot-of-raw opt-in is per-file (#5) → ADR-063.
- `immutable` is a flag (#33) → ADR-064.
- Zone-level ECC is opt-in (#47) → ADR-065, HDD-only per the corrected reasoning.
- Relocation/resize safety is non-negotiable, independent of Parted's own upstream timeline → folded into ADR-061's rationale.
- Journals stay split, not unified (#45/46) → ADR-062. Supporting analogy given: unifying journals for its own sake is the same failure mode as the Windows Registry versus Linux's per-application config file convention — a shared *location registry* (Function Table) is fine and already how this works; a shared *data store* is the part worth avoiding.

**Also formalized this session** (already resolved earlier in the conversation, written up properly now): concurrency execution model and the permit system (ADR-056), scratch's unified definition (ADR-057), the SMART flush pipeline's actual mechanics (ADR-058), the mount sequence (ADR-059), and the two Superblock corrections (ADR-060).

**Not resolved, not in the ADRs — see the concerns file for the actual reasoning on each:** permit table size (#9), permit validation failure behavior (#10), `CafsCtx` concurrency beyond counters (#11), misaligned-write+permit interaction (#12), checksum-mismatch handling (#13), `heat_caches()` at scale (#14), mkfs's real scope (#15), identifier slot reuse (#7), `temp_cache_lba`'s field disposition, permission-delegation layering (#27), allocator-vs-master permission ownership (#28), TRIM relay re-audit (#29), `CA-` range starting number (#30), extent-limit's final byte layout (#20, now bundled with timestamps — see the concerns file for the actual combined math), the 4th hash-table mode's real scope (#6, concrete framing given, not decided), hash algorithm choice (#23, recommendation given, not confirmed), encryption's scope and mechanism (#39, research done, nothing designed yet), and which timestamp fields to actually add (#44).

**Handoff:** this session's edited working copies of `fs.info`, `config.fs`, and the I/O Engine's `layout.rs` (format_version corrections, `locking_model` removal, the namespace_structure_id constant, and others made over the course of this session) are included as files alongside this one — apply them to the real repo copies, since they were only ever edited in this session's own container.
