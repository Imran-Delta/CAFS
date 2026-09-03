/*
 * confirm.c — cafs_confirm() and cafs_close().
 *
 * confirm() writes, in order, each followed by fdatasync(): (1) the
 * primary Superblock with last_mount_time refreshed and its BLAKE3-128
 * checksum recomputed; (2) the same content to Superblock Backup (via
 * LBA0's fallback pointer), if one exists; (3) the recomputed primary
 * Parity Block, XOR(new Superblock, cached Config Snapshot); (4) if a
 * usable M-SMART location was found at mount(), mount_count++ and
 * clean_unmount_flag=0 on it directly. Only the primary Superblock
 * write (1) is fatal to the call; (2)-(4) failing is logged and
 * non-fatal, consistent with "ignore all anchor block errors except
 * Superblock" for this PoC. There is no Backup Parity Block to keep
 * in sync — the revised design (see cafs_io_layout.h) drops
 * end-of-device redundancy for everything except Superblock and
 * SMART, Parity included.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "cafs_io_internal.h"
#include "syscalls.h"

cafs_status_t cafs_confirm(cafs_ctx_t *ctx)
{
    if (!ctx) return CAFS_ERR_INVALID_ARG;
    if (!ctx->anchor_loaded) return CAFS_ERR_NOT_MOUNTED;
    if (ctx->opts.readonly || !ctx->want_write) return CAFS_ERR_READONLY;

    pthread_mutex_lock(&ctx->lock);

    int64_t now = (int64_t)time(NULL);
    memcpy(ctx->superblock + SB_OFF_LAST_MOUNT_TIME, &now, 8);

    uint8_t sb_checksum[16];
    cafs_blake3_128(ctx->superblock, SB_HASHED_LEN, sb_checksum);
    memcpy(ctx->superblock + SB_OFF_CHECKSUM, sb_checksum, 16);

    cafs_status_t st = cafs_write_raw(ctx, ctx->lba0.real_lba, CAFS_REAL_BLOCK_4K, ctx->superblock);
    if (st != CAFS_OK) {
        pthread_mutex_unlock(&ctx->lock);
        return st;
    }
    cafs_sys_fdatasync(ctx->fd);

    if (ctx->lba0.fallback_real_lba != 0) {
        if (cafs_write_raw(ctx, ctx->lba0.fallback_real_lba, CAFS_REAL_BLOCK_4K, ctx->superblock) == CAFS_OK) {
            cafs_sys_fdatasync(ctx->fd);
            memcpy(ctx->superblock_backup, ctx->superblock, CAFS_REAL_BLOCK_4K);
            ctx->superblock_backup_valid = 1;
        } else {
            fprintf(stderr, "cafs: confirm: primary Superblock updated but the backup "
                             "write failed (non-fatal, primary is authoritative)\n");
        }
    }

    if (ctx->lba2.real_lba != 0) {
        uint8_t new_parity[CAFS_REAL_BLOCK_4K];
        for (size_t i = 0; i < CAFS_REAL_BLOCK_4K; i++)
            new_parity[i] = ctx->superblock[i] ^ ctx->config_snapshot[i];
        if (cafs_write_raw(ctx, ctx->lba2.real_lba, CAFS_REAL_BLOCK_4K, new_parity) == CAFS_OK) {
            cafs_sys_fdatasync(ctx->fd);
            memcpy(ctx->parity, new_parity, CAFS_REAL_BLOCK_4K);
        } else {
            fprintf(stderr, "cafs: confirm: Parity Block recompute write failed (non-fatal)\n");
        }
    }

    if (ctx->smart_main_lba != 0) {
        uint8_t smart_buf[CAFS_REAL_BLOCK_4K];
        memcpy(smart_buf, ctx->smart_main, CAFS_REAL_BLOCK_4K);

        uint32_t magic;
        memcpy(&magic, smart_buf + SMART_OFF_MAGIC, 4);

        uint32_t mount_count;
        if (magic == SMART_MAGIC) {
            memcpy(&mount_count, smart_buf + SMART_OFF_MOUNT_COUNT, 4);
            mount_count++;
        } else {
            /* Uninitialized/garbage M-SMART slot: start fresh rather
             * than fail — matches "won't crash" and this PoC's
             * best-effort treatment of everything but Superblock. */
            memset(smart_buf, 0, sizeof(smart_buf));
            uint32_t magic_val = SMART_MAGIC;
            memcpy(smart_buf + SMART_OFF_MAGIC, &magic_val, 4);
            uint32_t ver = 1;
            memcpy(smart_buf + SMART_OFF_VERSION, &ver, 4);
            uint64_t seq = 1;
            memcpy(smart_buf + SMART_OFF_SEQUENCE, &seq, 8);
            mount_count = 1;
        }
        memcpy(smart_buf + SMART_OFF_MOUNT_COUNT, &mount_count, 4);
        smart_buf[SMART_OFF_CLEAN_UNMOUNT_FLAG] = 0;

        uint8_t smart_checksum[16];
        cafs_blake3_128(smart_buf, SMART_HASHED_LEN, smart_checksum);
        memcpy(smart_buf + SMART_OFF_CHECKSUM, smart_checksum, 16);

        if (cafs_write_raw(ctx, ctx->smart_main_lba, CAFS_REAL_BLOCK_4K, smart_buf) == CAFS_OK) {
            cafs_sys_fdatasync(ctx->fd);
            memcpy(ctx->smart_main, smart_buf, CAFS_REAL_BLOCK_4K);
            ctx->smart_main_valid = 1;
        } else {
            fprintf(stderr, "cafs: confirm: M-SMART update write failed (non-fatal)\n");
        }
    }

    pthread_mutex_unlock(&ctx->lock);
    return CAFS_OK;
}

/* Not part of the originally-named four-stage flow — see cafs_io.h's
 * doc comment: without a counterpart, destroy_flush and a graceful fd
 * close never happen. */
cafs_status_t cafs_close(cafs_ctx_t *ctx)
{
    if (!ctx) return CAFS_ERR_INVALID_ARG;
    if (ctx->is_open) {
        if (ctx->opts.destroy_flush && ctx->want_write)
            cafs_sys_fdatasync(ctx->fd);
        cafs_sys_close(ctx->fd);
        ctx->is_open = 0;
    }
    pthread_mutex_destroy(&ctx->lock);
    free(ctx);
    return CAFS_OK;
}
