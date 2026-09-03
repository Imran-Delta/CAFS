/*
 * cafs_io_internal.h — internal ctx struct, shared by mount.c/check.c/
 * remount.c/confirm.c/block_io.c. Not installed, not part of the
 * public API.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#ifndef CAFS_IO_INTERNAL_H
#define CAFS_IO_INTERNAL_H

#include <pthread.h>
#include "cafs_io.h"
#include "cafs_io_layout.h"

typedef struct {
    uint8_t magic[8];
    uint64_t real_lba;
    uint64_t real_size;
    uint8_t fallback_magic[8];
    uint64_t fallback_real_lba;
    uint64_t fallback_real_size;
} cafs_ptr_block_t;

struct cafs_ctx {
    int fd;
    int is_open;
    int want_write;             /* requested at mount() */
    uint64_t device_size;        /* bytes, from ioctl BLKGETSIZE64 or fstat */
    char device_path[256];        /* cached so remount() can reopen with new flags */

    /* Raw anchor reads captured at mount(), reused by check()/confirm()
     * so neither has to re-read the front of the device from scratch. */
    cafs_ptr_block_t lba0, lba1, lba2, lba4;
    uint32_t lba0_pointer_algo; /* which of CRC32C/XXHASH32 actually verified LBA0 at mount() — see check.c */
    uint8_t superblock[CAFS_REAL_BLOCK_4K];
    uint8_t superblock_backup[CAFS_REAL_BLOCK_4K];
    int superblock_backup_valid;   /* LBA0 fallback pointed somewhere and the read succeeded */
    uint8_t config_snapshot[CAFS_REAL_BLOCK_4K];
    uint8_t parity[CAFS_REAL_BLOCK_4K];
    uint8_t function_table[CAFS_REAL_BLOCK_4K]; /* sized to the larger fixed buffer; only function_table_real_size bytes are valid */
    uint64_t function_table_real_size;
    uint64_t smart_main_lba, smart_main_size;
    uint64_t smart_backup_lba, smart_backup_size;
    uint8_t smart_main[CAFS_REAL_BLOCK_4K];
    int smart_main_valid;          /* Function Table gave a usable smart_main_lba and the read succeeded */
    int anchor_loaded;           /* mount() completed successfully */

    /* Effective runtime options, set by remount(); sane defaults apply
     * before the first remount() call. */
    cafs_mount_opts_t opts;
    int remounted;
    cafs_config_snapshot_t cfg;   /* last config parsed by remount() */

    /* Per-session RAM-only counters (cafs_read_block/write_block call
     * counts). Distinct from smart_main[] above: that's the real
     * on-disk M-SMART table (mount_count/clean_unmount_flag, written
     * by confirm()); these are never flushed to it in V1 — persisting
     * them would mean building the full V-SMART->Handler->B-SMART->
     * WAL->M-SMART reconciliation flow, still unresolved per
     * roadmap.md and out of scope here. */
    cafs_smart_counters_t smart;

    pthread_mutex_t lock;
};

/* internal helpers shared across .c files */
uint32_t cafs_crc32c(const void *data, size_t len);
uint32_t cafs_xxhash32(const void *data, size_t len, uint32_t seed);
void cafs_blake3_128(const void *data, size_t len, uint8_t out[16]);

cafs_status_t cafs_read_raw(cafs_ctx_t *ctx, uint64_t offset, uint32_t size, void *buf);
cafs_status_t cafs_write_raw(cafs_ctx_t *ctx, uint64_t offset, uint32_t size, const void *buf);

int cafs_verify_ptr_block(const uint8_t *block, uint32_t block_len,
                           uint32_t algo_id, cafs_ptr_block_t *out_parsed,
                           int *out_which_algo_matched);
void cafs_parse_ptr_block(const uint8_t *block, uint32_t block_len, cafs_ptr_block_t *out);

int cafs_verify_blake3_region(const uint8_t *block, size_t hashed_len,
                               size_t checksum_off);

#endif /* CAFS_IO_INTERNAL_H */
