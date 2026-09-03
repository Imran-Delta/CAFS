/*
 * anchor_io.c — pointer-block parse/verify and generic BLAKE3-128
 * region verification, shared by mount.c/check.c/confirm.c.
 *
 * Endianness: all multi-byte fields are read via memcpy into native
 * uint64_t/uint32_t, assumed little-endian on disk. Safe for a V1
 * that targets x86-64 exclusively (itself little-endian); revisit if
 * a big-endian host is ever in scope.
 *
 * Pointer-block checksum bootstrap: fs.info §3.3 says the algorithm
 * is "configurable at format time" but never says where a fresh mount
 * discovers that choice before it has read anything it can trust
 * (Config Snapshot's own pointer block is subject to the same
 * problem). Resolved here as: try CRC32C (the documented default)
 * first, then XXHASH32 — the only two legal IDs (§3.3) — and accept
 * whichever matches. The chance of a corrupted block coincidentally
 * matching one of two independent 4-byte checksums is negligible.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <string.h>
#include "cafs_io_internal.h"
#include "checksum.h"

void cafs_parse_ptr_block(const uint8_t *block, uint32_t block_len, cafs_ptr_block_t *out)
{
    uint32_t off_fb_magic, off_fb_lba, off_fb_size;

    memset(out, 0, sizeof(*out));
    memcpy(out->magic, block + PTR2K_OFF_MAGIC, 8); /* offset 0 is the same for both sizes */
    memcpy(&out->real_lba, block + PTR2K_OFF_REAL_LBA, 8);
    memcpy(&out->real_size, block + PTR2K_OFF_REAL_SIZE, 8);

    if (block_len == CAFS_PTR_BLOCK_SMALL_SIZE) {
        off_fb_magic = PTR2K_OFF_FALLBACK_MAGIC;
        off_fb_lba   = PTR2K_OFF_FALLBACK_REAL_LBA;
        off_fb_size  = PTR2K_OFF_FALLBACK_REAL_SIZE;
    } else {
        off_fb_magic = PTR4K_OFF_FALLBACK_MAGIC;
        off_fb_lba   = PTR4K_OFF_FALLBACK_REAL_LBA;
        off_fb_size  = PTR4K_OFF_FALLBACK_REAL_SIZE;
    }
    memcpy(out->fallback_magic, block + off_fb_magic, 8);
    memcpy(&out->fallback_real_lba, block + off_fb_lba, 8);
    memcpy(&out->fallback_real_size, block + off_fb_size, 8);
}

int cafs_verify_ptr_block(const uint8_t *block, uint32_t block_len,
                           uint32_t algo_id_hint, cafs_ptr_block_t *out_parsed,
                           int *out_which_algo_matched)
{
    (void)algo_id_hint; /* reserved: V1 always tries both, see file header */
    cafs_parse_ptr_block(block, block_len, out_parsed);
    if (memcmp(out_parsed->magic, CAFS_PTR_MAGIC, 8) != 0)
        return 0;

    uint32_t hashed_len = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                               ? PTR2K_PRIMARY_HASHED_LEN : PTR4K_PRIMARY_HASHED_LEN;
    uint32_t checksum_off = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                                 ? PTR2K_OFF_CHECKSUM : PTR4K_OFF_CHECKSUM;

    uint32_t stored;
    memcpy(&stored, block + checksum_off, 4);

    if (cafs_crc32c(block, hashed_len) == stored) {
        if (out_which_algo_matched) *out_which_algo_matched = (int)CAFS_PTR_ALGO_CRC32C;
        return 1;
    }
    if (cafs_xxhash32(block, hashed_len, 0) == stored) {
        if (out_which_algo_matched) *out_which_algo_matched = (int)CAFS_PTR_ALGO_XXHASH32;
        return 1;
    }
    return 0;
}

int cafs_verify_blake3_region(const uint8_t *block, size_t hashed_len, size_t checksum_off)
{
    uint8_t computed[16];
    cafs_blake3_128(block, hashed_len, computed);
    return memcmp(computed, block + checksum_off, 16) == 0;
}
