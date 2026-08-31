# CAFS Configuration Tradeoffs — Space / Compatibility / Speed

Compatibility reads "—" for most rows below because these are internal format choices with no external interop target — CAFS isn't aiming to be readable by ext4 or NTFS. The compatibility cost that actually matters here is format-version stability (can old data still be read after a change) and cross-platform behavior (Windows vs. Linux, since both have kernel-driver folders even with FUSE as the recommended path) — noted specifically where a knob touches either.

## Data checksum algorithm

| Option | Space | Speed |
|---|---|---|
| none | 0 overhead | Fastest; zero corruption detection — against "won't die easily," best reserved for scratch/temp data only |
| crc32c | 4B/checksummed unit | Fastest of the real options — hardware-accelerated (SSE4.2 x86, CRC32 ARMv8) |
| xxhash64 | 4–8B (storage width still an open item — see roadmap) | Fast, software-only, no hardware dependency |
| blake3 | 16B if ever used at data-block granularity (currently anchor-only) | Slowest of the four, but hardware-accelerated (AVX2/AVX512/NEON) and cryptographic-strength |

## Compression

| Option | Space | Speed |
|---|---|---|
| none | 0 savings | Fastest — no CPU spent |
| lz4 | Modest, ~2:1 typical | Very fast both directions, minimal CPU tax |
| zstd | Better, ~2.5–3:1+, tunable via level | Slower to compress (especially at high levels); decompression stays fast. Good for cold/archival data, poor for hot active files at high levels |

## Dedup

| Setting | Space | Speed |
|---|---|---|
| Off (current default) | 0 savings, 0 overhead | No write-path cost |
| On | Large savings on genuinely duplicate-heavy data (VM images, backups); *negative* on already-unique or pre-compressed data — the RAM table and Dedup Table cost space regardless of hit rate | Real write-path cost: hash + lookup before every write commits. `skip_dedup_for_raw` mitigates for pre-compressed/raw data specifically |

## Block size approach

| Option | Space | Speed | Implementation risk |
|---|---|---|---|
| A — uniform + flags | No change from today | No new cost | None — already exists |
| B — true variable + Block Map | Best — every file type gets a right-sized block | Block Map lookup added to the access path | Highest — the crash-safety question in the roadmap |
| C — variable extent length | Good for large sequential files; doesn't help many-small-files | Minimal added cost | Low — reuses existing extent machinery |

## Concurrency mode (mount-time fixed)

| Option | Space | Speed |
|---|---|---|
| Global Lock | Negligible | Simplest; becomes the bottleneck under real concurrent access |
| Per-Zone Lock | Negligible | Better parallelism; more lock objects to maintain |
| Per-CPU Lock-Free | Negligible | Best throughput on hardware that can feed it (high queue depth); wasted complexity if forced onto hardware that can't — why it's metrics-gated rather than user-picked directly |

## Direct I/O / sync_open

| Setting | Space | Speed | Compatibility |
|---|---|---|---|
| direct_io on | 0 | Helps large sequential I/O (no double-buffering); can hurt small random I/O (loses page-cache benefit) | Not guaranteed available on every storage/OS combo — `direct_io_required` decides fail-loud vs. fall back |
| sync_open on | 0 | Every write blocks until physically committed — much slower, but closes the `sync_interval_seconds` crash-loss window entirely | — |

## Integrity extras (verify_data, verify_on_move, root hash)

| Setting | Space | Speed |
|---|---|---|
| Off | 0 | No extra pass |
| On | Small (one stored hash) | Real cost — an extra read-and-verify pass on every write or relocation, in exchange for catching silent corruption immediately instead of at next read |

## COW vs. in-place

| Option | Space | Speed |
|---|---|---|
| In-place | Most space-efficient instantaneously | Fastest for overwrite-heavy workloads; a crash mid-write risks the only copy |
| COW | Transiently more (old + new coexist until reclaim) — enables snapshotting | Avoids read-modify-write for small-append patterns; adds allocation bookkeeping per write |
