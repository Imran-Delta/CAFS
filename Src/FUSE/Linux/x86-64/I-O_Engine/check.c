/*
 * check.c — cafs_check(). Verifies the Superblock's BLAKE3-128
 * checksum. On mismatch, tries Superblock Backup (read at mount() via
 * LBA0's fallback pointer); if the backup checks out, copies it over
 * the primary Superblock in place, fsyncs, and logs the repair to
 * stderr — "simple and foreseeable," per the locked V1 scope, so this
 * fixes rather than just reports. All other anchor structures are out
 * of scope for V1 ("ignore all anchor block errors except
 * Superblock") — read best-effort by mount(), not verified here.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <stdio.h>
#include <string.h>
#include "cafs_io_internal.h"
#include "syscalls.h"

cafs_status_t cafs_check(cafs_ctx_t *ctx, cafs_check_result_t *out_result)
{
    if (!ctx || !out_result) return CAFS_ERR_INVALID_ARG;
    if (!ctx->anchor_loaded) return CAFS_ERR_NOT_MOUNTED;

    memset(out_result, 0, sizeof(*out_result));
    out_result->pointer_checksum_algo_used = ctx->lba0_pointer_algo;

    pthread_mutex_lock(&ctx->lock);

    int primary_ok = cafs_verify_blake3_region(ctx->superblock, SB_HASHED_LEN, SB_OFF_CHECKSUM);

    if (primary_ok) {
        out_result->superblock_ok = 1;
    } else if (ctx->superblock_backup_valid &&
               cafs_verify_blake3_region(ctx->superblock_backup, SB_HASHED_LEN, SB_OFF_CHECKSUM)) {
        fprintf(stderr,
                "cafs: primary Superblock checksum mismatch at offset %llu; "
                "repairing from Superblock Backup at offset %llu\n",
                (unsigned long long)ctx->lba0.real_lba,
                (unsigned long long)ctx->lba0.fallback_real_lba);

        memcpy(ctx->superblock, ctx->superblock_backup, sizeof(ctx->superblock));

        if (ctx->want_write) {
            cafs_status_t wst = cafs_write_raw(ctx, ctx->lba0.real_lba,
                                                CAFS_REAL_BLOCK_4K, ctx->superblock);
            if (wst == CAFS_OK)
                cafs_sys_fdatasync(ctx->fd);
            else
                fprintf(stderr, "cafs: repair copy succeeded in memory but the write-back "
                                 "to the primary Superblock failed (mount is still usable "
                                 "this session; the repair did not persist)\n");
        } else {
            fprintf(stderr, "cafs: mount is read-only; repair applied in memory only, "
                             "not written back\n");
        }

        out_result->superblock_ok = 1;
        out_result->superblock_repaired = 1;
    } else {
        fprintf(stderr, "cafs: primary Superblock checksum mismatch and no usable backup; "
                         "reporting failure, not crashing\n");
        out_result->superblock_ok = 0;
    }

    out_result->overall_ok = out_result->superblock_ok;

    pthread_mutex_unlock(&ctx->lock);
    return CAFS_OK;
}
