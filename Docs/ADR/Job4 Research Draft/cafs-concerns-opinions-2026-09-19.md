# CAFS — Concerns, Opinions, and Elaborations Not Yet Confirmed
**Date:** 2026-09-19
**Purpose:** Everything in this file is my own judgment, flagged risk, or a recommendation — not a decision. The ADRs file only contains what was actually confirmed this session; this is where the surrounding reasoning, open judgment calls, and things needing your explicit sign-off live instead, so the two don't get mixed.

---

## Elaborations requested but not delivered before the session ended

### Extent-limit — the actual row-growth math, now that timestamps compete for the same budget

Row is 48 bytes today: `filename_blob_offset`(8) + `extent_list_offset`(8) + `extent_count`(4) + `unix_mode+uid+gid`(12) + `acl_blob_offset`(8) + `identifier_refcount`(4) + reserved(4) = 48.

`optimization_flags` (4 bytes) reuses the existing reserved slot — no growth there, already accounted for. `extent_limit` as its own field, XFS-style, is new growth: recommend `u32` (4 bytes) to match XFS's own field widths for the analogous `fsx_nextents`/`fsx_extsize`, rather than introducing an inconsistent narrower type. Timestamps (mtime + ctime, `s64` seconds + `u32` nanoseconds each, 12 bytes apiece) are 24 more bytes; add `btime` and it's 36.

Total new bytes: 28 (extent_limit + mtime/ctime only) to 40 (also btime). Row grows from 48 to 76–88 bytes — not a small tweak anymore once both are counted together, which changes the direct-indexing formula (`row_offset = identifier_table_lba + identifier × 48` → × whatever the new size is) and means this is a real, one-time breaking format change: either a migration path for existing v22 volumes, or accepting that only volumes formatted under the new row size get these fields, with old ones needing a reformat. Worth deciding as one combined resize, not two separate small ones, given both are now on the table at the same time. Also worth asking before locking a number: given the row is being resized anyway, is this the moment to also reserve a little headroom for a future per-file encryption nonce (typically 12–24 bytes depending on cipher), or would that be premature given encryption's own design isn't decided yet? Leaning toward not pre-reserving for something undesigned, but flagging the question rather than deciding it myself.

### 4th hash-table mode ("per-LBA + metadata") — concrete framing, since more context was asked for

The plain per-LBA mode already covers file content. The open question is what "metadata" adds on top. Two real candidates, with a real value difference between them worth knowing before picking:

- **Filenames** — ADR-029's own suggested candidate (content-addressable, position-independent, unlike B-tree nodes). Honest concern: filenames are typically a handful to a couple hundred bytes. The space actually saved by deduping repeated filename *strings* specifically seems marginal next to what file-content dedup already saves — worth questioning whether this is actually worth the mode's own existence, or whether it was assumed as the obvious answer mostly because it's the obvious *content-addressable* thing sitting right next to file content in the design.
- **ACL blobs** — not proposed anywhere so far, but a stronger case: ACL blobs can be considerably larger than a filename, and in a lot of real deployments (an org with a shared permission scheme, a directory tree with inherited/default ACLs) a huge number of files carry the *exact same* ACL blob content. Deduping that has a much clearer space-saving story than deduping filenames does.

My read: if "metadata" was meant loosely rather than specifically as "filenames," ACL blobs are the more compelling target to actually build. Worth clarifying which was originally meant, since it changes whether this mode is worth its own complexity at all.

---

## Flagged risks and recommendations not yet confirmed

- **Permit table size** — no number given yet; "a bit above WAL's ~2 concurrent grants" is a shape, not a size. Needs an actual figure, not something to invent unilaterally.
- **Permit validation failure** — needs a `CafsError` variant and `CA-` code; not named yet, and no decision on whether the caller retries or the operation just fails.
- **`CafsCtx` concurrency beyond the counters** — once `heat_caches()`'s loaded Dedup/Hash tables are shared across concurrent FUSE worker threads, they need their own protection, separate from the permit mechanism (which only covers physical LBA ranges, not in-memory structures). Recommend a lock per cached table. Not confirmed.
- **Misaligned write + permit interaction** — recommend one permit covering both the internal read and internal write of the bounce-buffer path as a single unit, rather than two separate permit checks with a gap between them. Not confirmed.
- **Checksum-mismatch handling on an ordinary read** — the existing PoC test suite already references a "corruption/auto-repair path." Recommend actually reading what that does before designing something new on top of an assumption about it.
- **`heat_caches()` at scale** — no stated ceiling. A very large Dedup/Hash Table loaded wholesale into RAM at every mount could become a real problem; a modest volume won't notice. Recommend a size threshold above which heat-up becomes partial/lazy rather than whole-table, but this hasn't been sized or agreed.
- **mkfs's real scope** — recommend lazy-initialization (mount-time code treats an unformatted Cache SMART/Dedup/Hash Table region as empty rather than requiring mkfs to pre-populate them) — simpler, and consistent with drain already being "stateless but healing." Not confirmed.
- **Identifier slot reuse** — proposed reusing the exact permit-table pattern (table-owned authoritative state, ID as a bare index) for ADR-029's still-open identifier reuse-vs-retire question, since it's structurally the same problem. Plausible, not yet confirmed as actually the same shape rather than superficially similar.
- **`temp_cache_lba`'s Function Table slot** — retired per ADR-057, but whether the field is removed outright or left reserved for something else wasn't decided. Lean: remove outright — a reserved field with no assigned purpose is worse documentation than no field.
- **`namespace_structure_id` offset finalized at 84** — folded into ADR-060 as a confirmation, but that was my call when compiling the ADRs, not something explicitly re-confirmed by you this session. Flagging so it doesn't quietly become "decided" without a real look.
- **Permission-delegation layering** (POSIX check → allocator permit → I/O Engine execution) — proposed in conversation across several turns, never actually written up as its own real decision or ADR.
- **Allocator-vs-master ownership of the permission cases the kernel can't handle** — you left this as "needs discussion" explicitly; I have no strong opinion to offer yet, genuinely open.
- **TRIM relay re-audit** — never actually performed, only flagged as needed, against every new way a block becomes free this session added (WAL drain/shrink, dedup-redirect freeing a losing copy, raw-group interactions).
- **`CA-` error range's actual starting block** — 5-digit width agreed; no starting number chosen. 4xxxx was floated early on, never confirmed once the range widened to 5 digits.
- **Encryption's actual scope for CAFS** — fscrypt research done (see the research file), but nothing about CAFS's own mechanism, key storage, or per-file field layout has been decided yet. This is the single largest remaining open area.
- **Timestamps — which fields** — mtime and ctime seem essential; btime is worth it given CAFS has no legacy reason to omit it, but that's my lean, not your decision yet. atime specifically is worth deciding as an optional flag rather than an unconditional field, given the write-on-every-read cost already flagged elsewhere this session for SSD endurance.
- **Hash algorithm (BLAKE3-256 vs. SHA-256)** — recommendation given in the research file; not yet confirmed as the actual decision.

## One meta-note on this session's own scope

You reframed partway through from "V1 of the I/O Engine" to full-feature design, because the questions this pass raised were too foundational to answer inside a V1-only frame. Worth keeping in mind for whoever picks this up next: several of the ADRs this session produced (concurrency model, multi-journal architecture, scratch's definition) are load-bearing for *everything* built on top of them — I'd treat those as needing to be genuinely solid before real code gets written against them, rather than provisional.
