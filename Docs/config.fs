# =================================================================
# CAFS CONFIGURATION PARAMETERS (v21 – format version 6)
# =================================================================
# This file contains all user- and administrator-facing settings.
# For on-disk layout, binary structures, and atomic protocols,
# see fs.info. The Config Snapshot in LBA1 is this file, verbatim,
# minus [mount_hints] — see fs.info Section 5.
#
# CHANGES FROM v20 (format_version 5 -> 6 — B-tree byte layout
# resolved, Superblock gains a namespace-structure selector field.
# See ADR-034 through ADR-039 and JOB3-OVERVIEW):
#   - [smart]: new section. report_interval_seconds governs ONLY the
#     user-facing --type=smart health report; --type=avg/--type=trends
#     feed the allocator directly and are not exposed here (ADR-038)
#   - [scrub]: new section. full_drive_scan_preference, default
#     "monthly" — piggyback checking (on blocks balance/defrag already
#     touch) is always on and not a setting; this governs only the
#     dedicated full-volume pass on top of that (ADR-035)
#   - [hash_daemon]: new section. min/max interval bounds only — the
#     live value is runtime state, not a config field (ADR-037)
#   - Scheduling is deliberately NOT one unified preference type: see
#     ADR-036 for why tier-enum / numeric-seconds / dynamic-runtime /
#     idle-triggered are kept as four separate shapes
#
# CHANGES FROM v19 (format_version 4 -> 5 — new critical-tier
# structures added: Identifier Table, Snapshot Table, per-block
# refcount in place of the old free/used bit. See ADR-018, 021,
# 029, 030 and STRUCTURE-OVERVIEW):
#   - [cow]: clarified as a whole-partition toggle (ADR-031),
#     replacing the previously undocumented default_mode integer
#   - new [hash_table] section: the ADR-013/023 speed-index cache
#     (off / file / lba), distinct from [dedup]/[dedup_table] below,
#     which govern the authoritative critical-tier structure
#   - new [snapshot] section (ADR-030): toggle, tiered retention,
#     space cap
#   - [data]: added compression_chunk_size / compression_chunk_max
#     / compression_chunk_min (ADR-025); raw files are never
#     eligible regardless of this section's settings (ADR-027)
#   - [dedup]: removed skip_dedup_for_raw — this was a runtime bool
#     implying raw+dedup could be enabled together. It can't: raw is
#     a composite flag that structurally excludes dedup participation
#     (ADR-027), not a separate togglable interaction
#   - [optimization]: flags comment corrected to include sync and
#     append_only, which examples already used but the declared list
#     omitted; flags documented as two independent categories
#     (structural / hint) rather than one list, per ADR-026
#
# CHANGES FROM v14:
#   - format_version: 3 -> 4 (matches fs.info Superblock)
#   - [dedup].enabled: true -> false (matches fs.info's stated default)
#   - [dedup].algo is now crash-critical (see section below); the rest
#     of [dedup] stays runtime-only
#   - [buffer].block_size -> [buffer].granularity (was colliding with
#     [physical].block_size in name only, not value)
#   - [journal] removed; the WAL is the journal (journal_lock_behavior
#     already lived in [recovery], unaffected)
#   - [integrity].anchor_checksum_algo comment corrected: it protects
#     the real structures (Superblock/Config Snapshot/Meta Table/
#     Function Table), not the LBA0/2/4/5 pointer blocks, which use
#     [physical].pointer_checksum_algo instead (new field, see below)
#   - [physical]: added pointer_checksum_algo (fs.info Section 3.3)
#   - [mount_hints]: added sync_open, direct_io_required (already
#     assumed to exist by the I/O engine spec; they didn't)
#   - new [io] section: destroy_flush, misaligned_action (same reason)
#   - new [allocator] section: explicit defaults for the two
#     allocator-doc conflicts that don't resolve by specificity alone
#
# NOTES ON CONFIGURATION PERSISTENCE:
# =================================================================
# 1. config.fs is the authoritative source for ALL settings.
# 2. Mount options (--workers, --locking-model, etc.) OVERRIDE
#    config.fs for the current mount only.
# 3. Mount options are NEVER written back to config.fs.
# 4. To permanently change a setting:
#    a. Edit config.fs
#    b. Run cafs.sync-config
#    c. Remount
# 5. [mount_hints] is EXCLUDED from LBA2 (crash survival) and
#    EXCLUDED from config_self_checksum computation.
# 6. config_self_checksum is recomputed by cafs.sync-config over
#    the canonical TOML payload (excluding [mount_hints] and itself).
# =================================================================

# =================================================================
# SECTION: IDENTITY (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[identity]
format_version = 6
volume_uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
uuid_sequence = 0
config_generation = 0

# hex-encoded 64-bit checksum; computed by cafs.sync-config over the canonical TOML
# with this field temporarily set to "" (empty string) and [mount_hints] removed.
config_self_checksum = "0000000000000000"

volume_label = "MyVolume"
volume_label_strict = false

# =================================================================
# SECTION: PHYSICAL (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[physical]
block_size = 4096
zone_size = "1GB"
page_buffer_size = "4MB"
max_flush_ms = 1000

# Checksum algorithm for the LBA0-5 pointer blocks (fs.info Section 3.3).
# Set at format time; changing after mkfs requires a reformat.
# Options: "crc32c" (default), "xxhash32"
pointer_checksum_algo = "crc32c"

# =================================================================
# SECTION: INTEGRITY (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[integrity]
# Checksum algorithm used for user data and B-tree nodes.
# Options: "none", "crc32c", "xxhash64", "blake3"
data_checksum_algo = "xxhash64"

# Anchor checksum algorithm for the real structures: Superblock,
# Config Snapshot, Meta/User Pointer Table, Function Table.
# (NOT the LBA0/2/4/5 pointer blocks themselves - those use
# [physical].pointer_checksum_algo instead.)
# WARNING: This is an immutable format parameter. Changing it after mkfs is forbidden.
# The only valid value is "blake3-128" (16-byte output).
anchor_checksum_algo = "blake3-128"

# Root hash algorithm for the B-tree root.
root_hash_algo = "blake3"

verify_data = true
flush_behavior = "relaxed"

# =================================================================
# SECTION: NAMESPACE (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[namespace]
case_sensitive = true
permission_model = "both"   # "unix", "windows", "both"
max_filename_bytes = 255
min_filename_bytes = 1
directory_lookup_depth = 1

# =================================================================
# SECTION: BTREE (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[btree]
max_entries_hot = 100
max_entries_cold = 125
min_fill_percent = 40

# =================================================================
# SECTION: DATA (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[data]
compression_algo = "zstd"      # "none", "zstd", "lz4"
compression_level = 3
min_compression_ratio = 0.95

# Compression chunk: amount of raw data fed to the compressor at once.
# Distinct from [physical].block_size (physical geometry) and from a
# storage extent (one file's contiguous run). See ADR-025.
compression_chunk_size = "128KB"   # default, matches BTRFS precedent
compression_chunk_max = "1GB"      # opt-in ceiling; read-amplification
                                    # cost rises sharply above the default
compression_chunk_min = 0          # 0 = floor to [physical].block_size

max_extents_per_file = 1024
max_file_size = 0              # 0 = unlimited (no enforced boundary)
preallocation_hint = "never"
preallocation_min_size = 0
raw_mode_priority = true

# =================================================================
# SECTION: COW (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[cow]
# Whole-partition toggle (ADR-031). Not BTRFS's default appeal here —
# an optional feature, off by default. Safe to change during usage:
# this sets the default for newly-created files/extents only, never
# retroactive (ADR-032). Disabling this also disables [snapshot] below
# — snapshots have no mechanism to retain anything without CoW.
enabled = false

# Independent of the toggle above: a block whose refcount is currently
# above 1 (shared, via dedup or a snapshot hold) is always force-CoW'd
# on write, regardless of this setting. Not optional — writing a
# shared block in place would corrupt every other reference to it.

# =================================================================
# SECTION: WAL (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[wal]
region_size = "512MB"
sync_interval_seconds = 2
sync_on_every_write = false

# =================================================================
# SECTION: WRITE CACHE (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[write_cache]
mode = "flush_then_clean"      # "flush_then_clean", "writethrough"
dirty_threshold = "256MB"
max_age_seconds = 60

# =================================================================
# SECTION: IDLE DETECTION (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[idle_detection]
threshold_seconds = 2
sample_window_seconds = 5      # NOW CRASH-SAFE (v3)

# I/O pressure classification (averaged over sample_window_seconds)
light_io_threshold_mbps = 5    # above this -> LIGHT load
heavy_io_threshold_mbps = 50   # above this -> HEAVY load

# =================================================================
# SECTION: BACKGROUND (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[background]
priority_order = ["dedup", "defrag", "balance"]
idle_trigger_seconds = 10

# Background I/O bandwidth cap.
# Default 5 MB/s (~1% of SATA 3 (6 Gbps) bandwidth). Set to 0 to disable.
max_io_speed_mbps = 5          # NOW CRASH-SAFE (v3)

scan_interval_seconds = 300    # NOW CRASH-SAFE (v3)

# Load-aware scaling: multipliers applied to scan_interval_seconds
light_load_interval_multiplier = 3.0    # e.g., 300s -> 900s (15 min)
heavy_load_interval_multiplier = 20.0   # e.g., 300s -> 6000s (100 min)

# =================================================================
# SECTION: PANIC MODE (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[panic_mode]
enabled = true
free_space_threshold_percent = 10
fragmentation_threshold_percent = 70
check_interval_seconds = 60
aggressive_io_speed_mbps = 20
aggressive_extent_threshold = 16
auto_disable_free_threshold_percent = 15
auto_disable_fragmentation_threshold_percent = 50
include_dedup_in_panic = false
verbose_logging = true
cooldown_seconds = 120         # 2 minutes; prevents rapid toggling

# =================================================================
# SECTION: SMART (RUNTIME ONLY – NOT IN LBA2)
# =================================================================
# Governs only the human-facing health report (smart_handler.py
# --type=smart). --type=avg (hourly aggregator) and --type=trends
# (the allocator's health/degradation signal) feed the allocator
# directly and are NOT exposed here — their cadence is internal to
# the filesystem's own cycle, not a user setting (ADR-038).
# =================================================================

[smart]
# Ceiling: must stay <= 3600. Tied to --type=avg's hourly bucket
# boundary (ADR-003) — a longer interval risks reading a health
# score computed against a bucket that already rolled over.
report_interval_seconds = 3600

# =================================================================
# SECTION: SCRUB (RUNTIME ONLY – NOT IN LBA2)
# =================================================================
# Piggyback checking (verifying the checksum trailer on blocks
# balance/defrag already touch) is always on and not a setting here
# — it costs nothing beyond CPU on I/O that was happening anyway.
# This section governs only the dedicated full-volume pass on top
# of that, which exists specifically to cover cold data the
# piggyback approach would never reach (ADR-035).
# =================================================================

[scrub]
# off / hourly / daily / weekly / monthly / yearly — shared tier
# type, same as [snapshot]'s retention tiers (ADR-036).
full_drive_scan_preference = "monthly"

# =================================================================
# SECTION: HASH DAEMON (RUNTIME ONLY – NOT IN LBA2)
# =================================================================
# Bounds only — the live interval is runtime state, floating
# between these based on load and priority-elevation requests, not
# a fixed value read from this file (ADR-037).
# =================================================================

[hash_daemon]
hash_daemon_min_interval_seconds = 0.25
hash_daemon_max_interval_seconds = 15.0

# =================================================================
# SECTION: DEDUP (MOSTLY RUNTIME - algo is CRASH-CRITICAL, see below)
# =================================================================
# Governs the authoritative, critical-tier Dedup Table (ADR-014,
# bucket layout in [dedup_table] below). For the non-critical
# speed-index cache over this table, see [hash_table].
# raw files never participate — this is structural (ADR-027), not a
# separate toggle here (the old skip_dedup_for_raw bool implied it
# could be turned off independently, which isn't actually possible).
# =================================================================

[dedup]
enabled = false                        # RUNTIME ONLY (not in LBA2)
algo = "blake3"                        # CRASH-CRITICAL (in LBA2) - immutable, fs.info Section 5
min_file_size_for_dedup = "64KB"       # RUNTIME ONLY (not in LBA2)
scan_interval_seconds = 3600           # RUNTIME ONLY (not in LBA2)
max_ram_table_mb = 512                 # RUNTIME ONLY (not in LBA2)

# =================================================================
# SECTION: HASH TABLE (RUNTIME ONLY – NOT IN LBA2)
# =================================================================
# Non-critical speed-index cache (ADR-013, redesigned ADR-023).
# Existence-check only, over the authoritative [dedup] table above.
# Losing it costs lookup speed, never correctness — the fallback is
# walking the Dedup Table directly.
# =================================================================

[hash_table]
# "off" / "file" (whole-file hash) / "lba" (per-block hash).
# Mutually exclusive: "lba" mode's detection coverage is a strict
# superset of "file" mode's, so running both is never useful.
mode = "off"

# Fixed at 256 bits, unfolded. Unlike a corruption checksum (where a
# collision between two unrelated blocks is harmless, since it's
# never compared cross-block), a collision here risks treating two
# different pieces of data as share-candidates — correctness-adjacent,
# not tolerable, so this width is never reduced. Not configurable.
# width_bits = 256   (informational — not a real toggle, see ADR-023)

# =================================================================
# SECTION: DEFRAG (RUNTIME ONLY – NOT IN LBA2)
# =================================================================

[defrag]
enabled = true
extent_threshold = 64
scan_interval_seconds = 300
balance_fragmentation_threshold = 30
balance_interval_hours = 24

# =================================================================
# SECTION: HASH CACHE (RUNTIME ONLY – NOT IN LBA2)
# =================================================================

[hash_cache]
ttl_days = 7
hit_threshold = 7
scan_interval_seconds = 300

# =================================================================
# SECTION: COLD STORAGE (RUNTIME ONLY – NOT IN LBA2)
# =================================================================

[cold_storage]
atime_threshold_days = 7
hit_threshold = 7
migration_interval_seconds = 3600

# =================================================================
# SECTION: RECOVERY (CRASH-CRITICAL + RUNTIME)
# =================================================================

[recovery]
journal_lock_behavior = "block"           # "block", "warn"
journal_fallback_to_scan = true
migration_staging_size = "10GB"           # CRASH-CRITICAL (in LBA2)
abort_on_error = false
uuid_sync_tool = "cafs.sync-uuid"
auto_remount_on_irrecoverable = false     # RUNTIME ONLY (not in LBA2)

# =================================================================
# SECTION: MIGRATION (RUNTIME ONLY – NOT IN LBA2)
# =================================================================

[migration]
chunk_size = "1MB"
repack_slack = true
max_allowed_time_warning = 3600
drive_flip_enabled = false

# =================================================================
# SECTION: COMPACTION (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[compaction]
enabled = false
target_density = 0.84
min_tail_free = "10GB"
verify_on_move = true
checkpoint_interval_blocks = 1000

# =================================================================
# SECTION: RELOCATION LOG (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[relocation_log]
max_intents = 1000
abort_on_log_corruption = false

# =================================================================
# SECTION: SNAPSHOT (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================
# Whole-volume only for V1 — no subvolume concept (ADR-030). Requires
# [cow].enabled = true; there is nothing for a snapshot to retain
# without it. Layered policy, validated against real-world precedent
# (Snapper) rather than a single knob — all four axes are independent.
# =================================================================

[snapshot]
enabled = false

# Automatic schedule, tiered retention count per tier (0 = disabled
# for that tier). Pruned oldest-first within each tier.
timeline_enabled = false
limit_hourly = 0
limit_daily = 0
limit_weekly = 0
limit_monthly = 0
limit_yearly = 0

# Space-based cap, independent of count/age. Fraction of the volume
# snapshots may consume in total before oldest-first pruning kicks
# in regardless of the timeline limits above. Reads free-space state
# from the Zone Table, not separate accounting.
space_limit_fraction = 0.5

# Policy when a volume-wide snapshot encounters a raw file (ADR-027),
# which cannot be protected (no CoW, nothing for the snapshot to
# retain). "skip" silently omits it (dangerous default — a later
# restore may assume protection that was never there), "warn" skips
# and logs, "block" refuses to take the snapshot at all.
raw_file_policy = "warn"     # OPEN — not yet decided, provisional

# Manual, on-demand snapshots are always available when enabled = true,
# regardless of timeline_enabled — no separate flag needed for this.

# =================================================================
# SECTION: DEDUP TABLE (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[dedup_table]
primary_bucket_count = 32768
max_overflow_chain_depth = 16
auto_rebuild_on_corruption = true

# =================================================================
# SECTION: BAD BLOCK (CRASH-CRITICAL – STORED IN LBA2)
# =================================================================

[bad_block]
# Behavior when a checksum mismatch is detected on a read operation:
# "true":  Attempt to read the raw block, allocate a new block, copy data,
#          update pointers, and log the relocation.
# "false": Remount as read-only and log the error.
relocation_on_checksum_fail = true

# If true: when a block is physically unreadable (media error), remount RO.
# If false: treat the block as a sparse hole (zero-filled) and continue R/W.
#           Use with extreme caution – this can lead to silent data corruption.
force_readonly_on_unrecoverable = true

# =================================================================
# SECTION: FUSE (RUNTIME ONLY – NOT IN LBA2)
# =================================================================

[fuse]
default_options = [
    "-o", "allow_other",
    "-o", "auto_unmount",
    "-o", "max_read=131072"
]

# =================================================================
# SECTION: MOUNT HINTS (RUNTIME ONLY – NOT IN LBA2)
# =================================================================
# These fields are read by the FUSE driver at mount time and used
# as defaults. They are overridden by command-line options.
#
# WARNING: Changing these does NOT require cafs.sync-config.
# They do NOT survive a crash (they are not in LBA2).
# They are EXCLUDED from config_self_checksum computation.
# =================================================================

[mount_hints]
# Number of FUSE worker threads (0 = auto = CPU count)
workers = 0

# Locking model: "zone", "inode", "global"
locking_model = "zone"

# Reduced functions mode: disables dedup, compression, defrag, balance, compaction
reduced_functions = false

# Use O_DIRECT for user data (bypasses page cache)
direct_io = false

# If true, fail the mount when O_DIRECT can't be opened instead of
# silently retrying without it
direct_io_required = false

# Use O_SYNC on the underlying device fd
sync_open = false

# Mount read-only
readonly = false

# Zone-level write locking (only relevant if locking_model = "zone")
zone_locking = true

# =================================================================
# SECTION: IO (RUNTIME ONLY – NOT IN LBA2)
# =================================================================
# Low-level I/O engine behavior. Not crash-critical: these affect how
# writes are issued, not what's persisted.
# =================================================================

[io]
# Flush pending writes on clean unmount/destroy before closing the device fd
destroy_flush = true

# What to do with an I/O request that isn't block_size-aligned:
# "copy" (bounce through an aligned buffer) or "reject" (return an error)
misaligned_action = "copy"

# =================================================================
# SECTION: BUFFER POOL CONFIGURATION (RUNTIME ONLY – NOT IN LBA2)
# =================================================================
# Excluded from config_self_checksum. Override via mount options.
# =================================================================

[buffer]
# Block size profile: "smol", "big", "auto"
profile = "auto"

# Manual overrides (if set, overrides profile defaults)
granularity = 0              # 0 = use profile default (or 4096)
metadata_pool_mb = 0        # 0 = use profile default
data_pool_mb = 0            # 0 = use profile default
scratch_pool_mb = 0         # 0 = use profile default

# Hot hash cache (RAM accelerator for frequently accessed directories)
hot_cache_enabled = true
hot_cache_max_entries = 10000
hot_cache_promotion_hits = 3

# B+ tree block cache
btree_cache_size_mb = 256

# WAL checkpoint (both time and size – whichever hits first)
wal_max_size_mb = 64
wal_checkpoint_interval_seconds = 5

# =================================================================
# SECTION: ALLOCATOR (RUNTIME ONLY – NOT IN LBA2)
# =================================================================
# Defaults for the hybridization engine (see the allocator spec).
# General rule: when more than one mutually-exclusive scoring
# condition matches, the more specific one wins (its trigger range is
# a strict subset of the other's). Where two matching conditions
# share no variable and neither is a subset of the other, specificity
# has no defined answer - this section is the explicit override for
# those cases; an unset value falls back to the stated default.
# =================================================================

[allocator]
# Delayed Allocation's On path for SSD was originally scored as a
# coin-flip (0.5) with no defined threshold. Explicit default now.
delayed_allocation_on_ssd_default = true

# Delayed Allocation On (HDD, high fragmentation, no shared variable
# with Off) and Off (low free space) can both trigger at once.
delayed_allocation_conflict_default = "off"   # "on" or "off"

# =================================================================
# SECTION: PER-FILE / PER-DIRECTORY OPTIMIZATION FLAGS
# =================================================================
# Excluded from config_self_checksum. Flags are stored in inodes.
# =================================================================

[optimization]
# Two independent categories (ADR-026), not one flat list — the
# previous "Flags:" comment omitted sync/append_only despite examples
# below already using both.
#
# Structural (mutually exclusive, picks the on-disk storage shape,
# ADR-018/019/020): extent, btree
#
# Hint (freely combinable, runtime behavior only, no shape change):
# sequential, random, journal, read, write, raw, hash, sync,
# append_only
#
# "hash" is a hint, not structural — it's the per-file/per-directory
# participation switch for [hash_table] above, which is why it
# legitimately combines with either extent or btree in the examples
# below rather than being a third structural option.
#
# "raw" is a composite (ADR-027): setting it forces no-CoW, no-hash,
# not-compression-eligible, and full upfront allocation as one bundle
# — it is not an independently combinable peer of the other hints,
# and the invalid combinations (raw+hash, raw+compression, raw+
# snapshot-protection) can't be expressed rather than needing
# validation against them.
#
# "append_only" is the honest, narrow POSIX meaning only (write-
# position gate) — the retention behavior originally wanted under
# this name is provided by [snapshot] above instead (ADR-030).
dir_default_flags = "hash|extent"

# Default flags for new files
file_default_flags = "extent|sequential"

# Per-directory override: directory path -> flags
[optimization.dir_overrides]
# "/mnt/games" = "hash|btree|read"
# "/mnt/logs" = "extent|journal|write"
# "/mnt/photos" = "hash|read"
# "/mnt/db" = "btree|random"

# Per-file override: file path -> flags
[optimization.file_overrides]
# "/mnt/db/data.db" = "btree|random|sync"
# "/mnt/logs/app.log" = "extent|journal|write|append_only"

# =================================================================
# END OF CONFIGURATION
# =================================================================