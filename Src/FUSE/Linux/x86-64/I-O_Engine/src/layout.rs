//! On-disk anchor-region layout, fs.info v19 (format version 4), as
//! revised by ADR-006 (Superblock+SMART-only redundancy) and resolved
//! per ADR-010 (addressing unit, pointer magic, checksum bootstrap,
//! anchor slot index). Ported from the C engine's `cafs_io_layout.h`;
//! every constant below matches it exactly. Not part of the public
//! API surface (`pub(crate)` throughout).
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

/// A raw byte offset from the start of the device. Distinct from
/// [`AnchorSlot`] specifically to make the bug ADR-010 documents in
/// the C port impossible to reintroduce silently: Superblock's
/// `function_table_anchor_lba` field holds a slot *index* (0-5), not
/// a byte offset, unlike every other `real_lba`-style field in the
/// format. In the C version this was just a `uint64_t` either way —
/// nothing stopped the two meanings from being mixed up at a call
/// site. Here, mixing them is a compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteOffset(pub u64);

impl ByteOffset {
    pub const fn add(self, n: u64) -> ByteOffset {
        ByteOffset(self.0 + n)
    }
}

impl std::ops::Add<u64> for ByteOffset {
    type Output = ByteOffset;
    fn add(self, rhs: u64) -> ByteOffset {
        ByteOffset(self.0 + rhs)
    }
}

impl std::ops::Sub<u64> for ByteOffset {
    type Output = ByteOffset;
    fn sub(self, rhs: u64) -> ByteOffset {
        ByteOffset(self.0 - rhs)
    }
}

/// One of the six anchor pointer-block slots (0-5). Never a byte
/// offset — see [`ByteOffset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorSlot(pub u8);

pub const FORMAT_VERSION: u32 = 4;

// ---- Anchor slot byte offsets (ADR-010 resolution #1: LBA = raw
// byte offset from device start, applied uniformly). ----
pub const LBA0_OFFSET: ByteOffset = ByteOffset(0); // Superblock ptr block   (2048)
pub const LBA1_OFFSET: ByteOffset = ByteOffset(2048); // Config Snapshot ptr (2048)
pub const LBA2_OFFSET: ByteOffset = ByteOffset(4096); // Parity Block ptr    (2048)
pub const LBA3_OFFSET: ByteOffset = ByteOffset(6144); // Meta/User ptr       (2048) — not read (delegated upward)
pub const LBA4_OFFSET: ByteOffset = ByteOffset(8192); // Function Table ptr  (4096)
pub const LBA5_OFFSET: ByteOffset = ByteOffset(12288); // copy of LBA4       (4096) — not read in V1
pub const ANCHOR_REGION_BYTES: u64 = 16384;

pub const PTR_BLOCK_SMALL_SIZE: u32 = 2048; // LBA0-3
pub const PTR_BLOCK_LARGE_SIZE: u32 = 4096; // LBA4-5

/// Fixed real-structure size (Superblock/Config Snapshot/Parity —
/// fs.info states these explicitly as "Size: 4KB"). Function Table's
/// size is never hardcoded — always taken from its pointer block's
/// `real_size` field.
pub const REAL_BLOCK_4K: u32 = 4096;

/// ADR-010 resolution #2: fs.info writes `0xCAFSPTR`, not valid hex
/// (P/T/R aren't hex digits). Resolved as the literal 8-byte ASCII
/// string, compared byte-for-byte.
pub const PTR_MAGIC: [u8; 8] = *b"CAFSPTR\0";

pub const SUPERBLOCK_MAGIC: u32 = 0x5355_5042; // "SUPB"

// fs.info §3.3 pointer-block checksum algorithm IDs.
pub const PTR_ALGO_CRC32C: u32 = 1;
pub const PTR_ALGO_XXHASH32: u32 = 2;

// fs.info §16: anchor_checksum_algo_id must be exactly 1 for format v4.
pub const ANCHOR_ALGO_BLAKE3_128: u32 = 1;

// fs.info §16 data_checksum_algo_id (informational only — this engine
// never touches data blocks).
pub const DATA_ALGO_NONE: u32 = 0;
pub const DATA_ALGO_CRC32C: u32 = 1;
pub const DATA_ALGO_XXHASH64: u32 = 2;
pub const DATA_ALGO_BLAKE3: u32 = 3;

// ---- §3.1 pointer block (LBA0-3, 2048 bytes) field offsets ----
pub const PTR2K_OFF_MAGIC: usize = 0;
pub const PTR2K_OFF_REAL_LBA: usize = 8;
pub const PTR2K_OFF_REAL_SIZE: usize = 16;
pub const PTR2K_OFF_CHECKSUM: usize = 24;
pub const PTR2K_PRIMARY_HASHED_LEN: usize = 24; // bytes 0-23
pub const PTR2K_OFF_FALLBACK_MAGIC: usize = 1024;
pub const PTR2K_OFF_FALLBACK_REAL_LBA: usize = 1032;
pub const PTR2K_OFF_FALLBACK_REAL_SIZE: usize = 1040;
pub const PTR2K_OFF_FALLBACK_CHECKSUM: usize = 1048;
pub const PTR2K_FALLBACK_HASHED_LEN: usize = 24; // bytes 1024-1047

// ---- §3.2 pointer block (LBA4-5, 4096 bytes) field offsets ----
pub const PTR4K_OFF_MAGIC: usize = 0;
pub const PTR4K_OFF_REAL_LBA: usize = 8;
pub const PTR4K_OFF_REAL_SIZE: usize = 16;
pub const PTR4K_OFF_CHECKSUM: usize = 24;
pub const PTR4K_PRIMARY_HASHED_LEN: usize = 24;
pub const PTR4K_OFF_FALLBACK_MAGIC: usize = 2048;
pub const PTR4K_OFF_FALLBACK_REAL_LBA: usize = 2056;
pub const PTR4K_OFF_FALLBACK_REAL_SIZE: usize = 2064;
pub const PTR4K_OFF_FALLBACK_CHECKSUM: usize = 2072;
pub const PTR4K_FALLBACK_HASHED_LEN: usize = 24;

/// Only LBA0's fallback field is meaningful under the revised
/// redundancy design (ADR-006): fallback -> Superblock Backup.
/// LBA1/2/3's fallback fields are unused/zero — Config Snapshot,
/// Parity, and Meta/User Table have no backup on this design.
pub const _FALLBACK_NOTE: () = ();

// ---- §4 Superblock (4096 bytes) field offsets ----
pub const SB_OFF_MAGIC: usize = 0;
pub const SB_OFF_FORMAT_VERSION: usize = 4;
pub const SB_OFF_VOLUME_UUID: usize = 8;
pub const SB_OFF_CONFIG_GENERATION: usize = 24;
/// Holds the anchor **slot index** (always 4), not a byte offset —
/// see [`AnchorSlot`] and ADR-010 resolution #4.
pub const SB_OFF_FUNCTION_TABLE_ANCHOR_SLOT: usize = 32;
pub const SB_OFF_WAL_SEQUENCE: usize = 40;
pub const SB_OFF_LAST_MOUNT_TIME: usize = 48;
pub const SB_OFF_FLAGS: usize = 56;
pub const SB_OFF_DATA_CHECKSUM_ALGO: usize = 60;
pub const SB_OFF_ANCHOR_CHECKSUM_ALGO: usize = 64;
pub const SB_OFF_POINTER_CHECKSUM_ALGO: usize = 68;
pub const SB_OFF_LAST_APPLIED_WAL_SEQ: usize = 72;
pub const SB_OFF_ROOT_HASH_ALGO: usize = 80;
pub const SB_OFF_CHECKSUM: usize = 4080;
pub const SB_CHECKSUM_LEN: usize = 16;
pub const SB_HASHED_LEN: usize = 4080; // bytes 0-4079

// ---- §5 Config Snapshot (4096 bytes) ----
pub const CFGSNAP_OFF_CHECKSUM: usize = 4080;
pub const CFGSNAP_CHECKSUM_LEN: usize = 16;
pub const CFGSNAP_HASHED_LEN: usize = 4080;

// §6 Parity Block: no header, no checksum. Verified by recomputing
// XOR(Superblock, Config Snapshot) and comparing — not implemented in
// V1 (ADR-007: Superblock-only check/repair scope).

// ---- §8 Function Table field offsets (no fixed total size, no
// magic field — checksum is the only integrity signal available) ----
pub const FT_OFF_WAL_LBA: usize = 0;
pub const FT_OFF_SMART_MAIN_LBA: usize = 8;
pub const FT_OFF_SMART_BACKUP_LBA: usize = 16;
pub const FT_OFF_SCRATCH_LBA: usize = 24;
pub const FT_OFF_TEMP_CACHE_LBA: usize = 32;
pub const FT_OFF_CHECKSUM: usize = 64;
pub const FT_CHECKSUM_LEN: usize = 16;
pub const FT_HASHED_LEN: usize = 64; // bytes 0-63
pub const FT_MIN_REAL_SIZE: u64 = 80;

// ---- §11.1 SMART table (144 bytes total) ----
pub const SMART_TABLE_SIZE: usize = 144;
pub const SMART_MAGIC: u32 = 0x534D_5254; // "SMRT"
pub const SMART_OFF_MAGIC: usize = 0;
pub const SMART_OFF_VERSION: usize = 4;
pub const SMART_OFF_SEQUENCE: usize = 8;
pub const SMART_OFF_TOTAL_READS: usize = 16;
pub const SMART_OFF_TOTAL_WRITES: usize = 24;
pub const SMART_OFF_TOTAL_READ_ERRORS: usize = 32;
pub const SMART_OFF_TOTAL_WRITE_ERRORS: usize = 36;
pub const SMART_OFF_TOTAL_CKSUM_MISMATCH: usize = 40;
pub const SMART_OFF_TOTAL_HOST_RELOC: usize = 44;
pub const SMART_OFF_LAST_ACCESS_LBA: usize = 52;
pub const SMART_OFF_LAST_ACCESS_TIME: usize = 60;
pub const SMART_OFF_MOUNT_COUNT: usize = 68;
pub const SMART_OFF_CLEAN_UNMOUNT_FLAG: usize = 72;
pub const SMART_OFF_CHECKSUM: usize = 128;
pub const SMART_CHECKSUM_LEN: usize = 16;
pub const SMART_HASHED_LEN: usize = 128; // bytes 0-127
