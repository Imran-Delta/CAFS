/*
 * cafs_io_layout.h — on-disk anchor-region layout, fs.info v19 (format
 * version 4). Internal header, not installed, not part of the public API.
 *
 * Every constant below cites the fs.info section it comes from. Where
 * fs.info is silent or self-contradictory, the resolution taken is
 * recorded in a comment at the point of use — see README.md for the full,
 * consolidated list ("Resolved Gaps").
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#ifndef CAFS_IO_LAYOUT_H
#define CAFS_IO_LAYOUT_H

#include <stdint.h>
#include <stddef.h>

#define CAFS_FORMAT_VERSION 4u

/*
 * Addressing unit: fs.info never states what unit "LBA" is in for the
 * anchor region — §2's table just lists LBA 0..5 against byte sizes
 * (2048/2048/2048/2048/4096/4096) with no stated multiplier. Resolved
 * here as: LBA = raw byte offset from the start of the device, and
 * successive anchor LBAs sit back-to-back at the cumulative sum of the
 * preceding sizes. Applied uniformly to real_lba/real_size inside
 * pointer blocks too (Section 3.1/3.2), so the whole engine speaks one
 * consistent unit: bytes.
 */
#define CAFS_LBA0_OFFSET   0ull      /* Superblock pointer block   (2048) */
#define CAFS_LBA1_OFFSET   2048ull   /* Config Snapshot ptr block  (2048) */
#define CAFS_LBA2_OFFSET   4096ull   /* Parity Block ptr block     (2048) */
#define CAFS_LBA3_OFFSET   6144ull   /* Meta/User Table ptr block  (2048) — not read by this engine (§7, delegated upward) */
#define CAFS_LBA4_OFFSET   8192ull   /* Function Table ptr block   (4096) */
#define CAFS_LBA5_OFFSET   12288ull  /* copy of LBA4 ptr block     (4096) — not read in V1 */
#define CAFS_ANCHOR_REGION_BYTES 16384ull /* sum of the six sizes above */

#define CAFS_PTR_BLOCK_SMALL_SIZE 2048u  /* LBA0-3 */
#define CAFS_PTR_BLOCK_LARGE_SIZE 4096u  /* LBA4-5 */

/* Fixed real-structure sizes. fs.info states these explicitly as "Size:
 * 4KB" for Superblock (§4), Config Snapshot (§5), and Parity Block (§6).
 * Function Table (§8) has no stated fixed size — its size is instead
 * always taken from the pointing pointer block's real_size field, never
 * hardcoded, since that field exists precisely to make the real
 * structure self-describing. */
#define CAFS_REAL_BLOCK_4K 4096u

/*
 * Pointer block magic (§3.1/3.2): fs.info writes `0xCAFSPTR`, which is
 * not valid hex — P/T/R are not hex digits. Resolved as the literal
 * 8-byte ASCII string "CAFSPTR\0", compared byte-for-byte rather than
 * parsed as an integer. (Contrast with the Superblock magic
 * 0x53555042 and WAL magic 0x57414C42, both of which ARE valid hex and
 * decode cleanly to ASCII "SUPB"/"WALB" — only the pointer-block magic
 * has this problem.)
 */
static const uint8_t CAFS_PTR_MAGIC[8] = {'C','A','F','S','P','T','R','\0'};

#define CAFS_SUPERBLOCK_MAGIC 0x53555042u /* "SUPB", §4 */

/* §3.3 — pointer block checksum algorithm IDs. */
#define CAFS_PTR_ALGO_CRC32C  1u
#define CAFS_PTR_ALGO_XXHASH32 2u

/* §16 — anchor_checksum_algo_id must be exactly 1 (BLAKE3-128) for
 * format version 4; no other value is valid for real structures. */
#define CAFS_ANCHOR_ALGO_BLAKE3_128 1u

/* §16 — data_checksum_algo_id (parsed from Config Snapshot for
 * informational purposes only; this engine does not touch data
 * blocks). */
#define CAFS_DATA_ALGO_NONE    0u
#define CAFS_DATA_ALGO_CRC32C  1u
#define CAFS_DATA_ALGO_XXHASH64 2u
#define CAFS_DATA_ALGO_BLAKE3  3u

/* ---- §3.1 pointer block (LBA0-3, 2048 bytes) field offsets ---- */
#define PTR2K_OFF_MAGIC            0
#define PTR2K_OFF_REAL_LBA         8
#define PTR2K_OFF_REAL_SIZE        16
#define PTR2K_OFF_CHECKSUM         24
#define PTR2K_CHECKSUM_LEN         4
#define PTR2K_PRIMARY_HASHED_LEN   24  /* bytes 0-23 */
#define PTR2K_OFF_FALLBACK_MAGIC       1024
#define PTR2K_OFF_FALLBACK_REAL_LBA    1032
#define PTR2K_OFF_FALLBACK_REAL_SIZE   1040
#define PTR2K_OFF_FALLBACK_CHECKSUM    1048
#define PTR2K_FALLBACK_HASHED_LEN      24 /* bytes 1024-1047 */

/* ---- §3.2 pointer block (LBA4-5, 4096 bytes) field offsets ---- */
#define PTR4K_OFF_MAGIC            0
#define PTR4K_OFF_REAL_LBA         8
#define PTR4K_OFF_REAL_SIZE        16
#define PTR4K_OFF_CHECKSUM         24
#define PTR4K_PRIMARY_HASHED_LEN   24 /* bytes 0-23 */
#define PTR4K_OFF_FALLBACK_MAGIC       2048
#define PTR4K_OFF_FALLBACK_REAL_LBA    2056
#define PTR4K_OFF_FALLBACK_REAL_SIZE   2064
#define PTR4K_OFF_FALLBACK_CHECKSUM    2072
#define PTR4K_FALLBACK_HASHED_LEN      24 /* bytes 2048-2071 */

/*
 * Fallback targets in the PRIMARY (front-of-device) pointer blocks —
 * revised design, superseding fs.info §12's end-of-device mirror for
 * everything except Superblock and SMART: redundancy is deliberately
 * dropped for Config Snapshot, Parity, Meta/User Table, and Function
 * Table (removes write amplification and the torn-write surface of
 * keeping several backup structures in sync; Config Snapshot remains
 * indirectly reconstructable via Parity XOR against Superblock even
 * with no direct backup — Function Table and Meta/User Table have
 * none). Only LBA0's fallback field is meaningful (-> Superblock
 * Backup); LBA1/2/3's fallback fields are unused/zero. SMART's
 * backup (B-SMART) isn't reached via any LBA0-5 fallback field at all
 * — it's smart_backup_lba, a plain field inside the Function Table
 * (§8), independent of this pointer-block mechanism.
 */

/* ---- §4 Superblock (4096 bytes) field offsets ---- */
#define SB_OFF_MAGIC                0
#define SB_OFF_FORMAT_VERSION       4
#define SB_OFF_VOLUME_UUID          8
#define SB_OFF_CONFIG_GENERATION    24
#define SB_OFF_FUNCTION_TABLE_ANCHOR_LBA 32
#define SB_OFF_WAL_SEQUENCE         40
#define SB_OFF_LAST_MOUNT_TIME      48
#define SB_OFF_FLAGS                56
#define SB_OFF_DATA_CHECKSUM_ALGO   60
#define SB_OFF_ANCHOR_CHECKSUM_ALGO 64
#define SB_OFF_POINTER_CHECKSUM_ALGO 68
#define SB_OFF_LAST_APPLIED_WAL_SEQ 72
#define SB_OFF_ROOT_HASH_ALGO       80
#define SB_OFF_CHECKSUM             4080
#define SB_CHECKSUM_LEN             16
#define SB_HASHED_LEN               4080 /* bytes 0-4079 */

/* ---- §5 Config Snapshot (4096 bytes) ---- */
#define CFGSNAP_OFF_CHECKSUM 4080
#define CFGSNAP_CHECKSUM_LEN 16
#define CFGSNAP_HASHED_LEN   4080

/* ---- §6 Parity Block (4096 bytes): no header, no checksum. Verified
 * by recomputing XOR(Superblock, Config Snapshot) and comparing. ---- */

/* ---- §8 Function Table field offsets (no fixed total size, no magic
 * field — checksum is the only integrity signal available) ---- */
#define FT_OFF_WAL_LBA         0
#define FT_OFF_SMART_MAIN_LBA  8
#define FT_OFF_SMART_BACKUP_LBA 16
#define FT_OFF_SCRATCH_LBA     24
#define FT_OFF_TEMP_CACHE_LBA  32
#define FT_OFF_CHECKSUM        64
#define FT_CHECKSUM_LEN        16
#define FT_HASHED_LEN          64 /* bytes 0-63 */
#define FT_MIN_REAL_SIZE       80 /* must be at least this many bytes */

/* ---- §11.1 SMART table (144 bytes: 0-127 data, 128-143 checksum).
 * In scope for V1 now: mount() reads M-SMART via the Function Table's
 * smart_main_lba, confirm() writes mount_count/clean_unmount_flag to
 * it directly (bypassing the full Buffer->Handler->B-SMART->WAL->
 * M-SMART reconciliation, which remains the Python SMART Handler's
 * job). Per the "ignore all anchor errors except Superblock" PoC
 * scope, M-SMART's own checksum is not verified/repaired here — it's
 * read/written best-effort. ---- */
#define SMART_OFF_MAGIC              0
#define SMART_MAGIC                  0x534D5254u /* "SMRT" */
#define SMART_OFF_VERSION            4
#define SMART_OFF_SEQUENCE           8
#define SMART_OFF_TOTAL_READS        16
#define SMART_OFF_TOTAL_WRITES       24
#define SMART_OFF_TOTAL_READ_ERRORS  32
#define SMART_OFF_TOTAL_WRITE_ERRORS 36
#define SMART_OFF_TOTAL_CHECKSUM_MISMATCH 40
#define SMART_OFF_TOTAL_HOST_RELOCATIONS 44
#define SMART_OFF_LAST_ACCESS_LBA    52
#define SMART_OFF_LAST_ACCESS_TIMESTAMP 60
#define SMART_OFF_MOUNT_COUNT        68
#define SMART_OFF_CLEAN_UNMOUNT_FLAG 72
#define SMART_OFF_CHECKSUM           128
#define SMART_CHECKSUM_LEN           16
#define SMART_HASHED_LEN             128 /* bytes 0-127 */
#define SMART_STRUCT_LEN             144 /* 0-143 total */

#endif /* CAFS_IO_LAYOUT_H */
