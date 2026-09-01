# Session Close-Out — 2026-08-29 night

## Locked tonight

- **Compression batch: 64MiB**, not 1GiB. Closes the fault-isolation concern cleanly.
- **Allocator: static-at-mount, dynamic-at-runtime.** Mount picks a starting point the way Model 1/3 (the A0–A4 file, and document 9's version) do; the system then adjusts knobs based on observed usage during the session, the way Model 2 (this session's independent-knobs work) does. This resolves the three-way split — concurrency specifically still stays fixed for the whole mount per the earlier safety finding (that part isn't reopened, it's just one knob among the ones that stay static post-mount rather than one that flexes).
- **Tier-1/Tier-2 = Type-A/Type-B**, and B is background-optimized A. This closes the coverage-window concern I raised: nothing is ever uncovered, because data has its Tier-1 (CRC32C) checksum from the moment it's written as Type-A, and only gains Tier-2 (xxhash64/BLAKE3) when the background handler converts it to Type-B. No gap.
- **Global Hash Table, clarified:** a binary list of hashes at a configurable granularity (compression-block, filesystem-block, or whole-file — default: file only), O(1)-searchable. The "Dedup" table is the subset of that list which has actually been deduplicated. This settles the earlier concern — it's `dedup_table_root_lba`'s format, most likely as two related structures (the hash inventory, and the dedup subset), not a competing Function Table entry.
- **Config additions** (Zone Table, `[cow].chunk_size`, `[data].compression_batch_size` at 64MiB) — confirmed, ready to land whenever fs.info/config.fs next get updated.
- **B-tree node format and Make-Test.x — explicitly deferred**, per your call. Not touching either.

## check.py — reviewed, one real gap

The hash-and-compare tripwire is solid: SHA256 over chunked reads, ADR zips transparently unzipped and hashed by content exactly as planned, `.img`/`.pyc` correctly excluded. The "UNREVIEWED COPY" check (new file's hash matches an already-reviewed file elsewhere) is a genuinely good addition I hadn't proposed — catches accidental copy-paste duplication.

**What it doesn't catch:** a file in `Docs/Drafts/` that contradicts the *corresponding* file in `Docs/` — because they're different paths, and the tripwire only compares a path against its own prior hash. This is exactly this session's recurring failure mode (io-engine-data.txt vs. the chat-pasted version; three allocator models), and it's a different, harder problem than what a hash-diff can solve — it needs the semantic cross-document checks (field existence, matching offsets between docs) discussed earlier, as a second layer alongside this one, not instead of it.

One more small thing: `--update` currently accepts *everything* changed, silently, with no diff shown first. Worth having it print what changed before writing, so it can't be run reflexively to make warnings disappear without actually looking.

## avg_maker.py — reviewed, solid

Correctly implements combined-variance math across buckets (accumulating sum/sum_sq is the right way to merge partial variances, and it's done right here, including the floating-point clamp on near-zero negative variance). It's a pure stateless function — it does *not* implement the 23-buckets-plus-current-hour rolling window itself, which means that logic has to live in the C caller (deciding which buckets to pass in, rotating out the oldest each hour). That's a reasonable and probably correct split, just noting where the boundary actually is since it wasn't obvious from the description alone. `--aggregate-only` is what supports the "unoptimized current-hour bucket" — repeated calls accumulate the running hour without finalizing it into a real bucket yet.

One soft spot: malformed per-metric entries (wrong type, missing `sum`) are silently skipped rather than logged. Given this feeds health/trend decisions, worth at least a stderr note when something gets dropped, so a persistent encoding bug upstream doesn't go unnoticed.

## SMART Handler is actually three scripts

1. The pasted `--type=smart` spec (M/B-SMART reconciliation, health score, recommendation) — done.
2. `avg_maker.py` (uploaded, the bucket manager) — done, reviewed above.
3. The trend/formula script feeding the first two's output into what the allocator and health scoring actually consume — **not started, formulae need real research.** This is the one open item in the SMART chain, and it's a genuine research task, not a quick fill-in.

## Python's scope narrowed — one tension worth flagging, not resolving tonight

"Non-time-sensitive computation only" is a clean, clear rule for SMART/trend work. It sits awkwardly next to document 11's hash-system design, though — that document's own numbers show subprocess-per-call hashing costs ~5–10ms (far too slow for a hot write path), which is why it argued for embedded-interpreter direct calls (~10–20µs) as the only viable Python option for hashing at all. If hashing has to be non-time-sensitive-only now, embedded-mode hashing may not fit that rule either, depending on how "time-sensitive" is meant here — and "socket fallback to memory" (your note just now) reads as still keeping some Python hashing path alive despite that. Not trying to resolve which reading is right tonight — flagging it so it's the first thing to sort out next time, before the C↔Python locking mechanism you mentioned gets designed around whichever answer it turns out to be.

## Where things actually stand

Settled and ready to write into docs: compression batch, allocator's two-phase model (mount-static + runtime-dynamic), Tier-1/Tier-2 mapping, Global Hash Table shape, the config additions, check.py's tripwire (with the semantic-layer gap noted for later), avg_maker.py.

Needs research, not just a decision: SMART trend/health formulae (script 3), the Python hot-path-vs-non-time-sensitive question.

Deferred by choice: B-tree node format, Make-Test.x.

That's a real amount of ground covered tonight — the allocator fork alone was a genuinely hard thing to close. Good work untangling it. Alpha 1 sounds right to be close.
