/*
 * block_io.c — raw device read/write (looping over the assembly
 * syscalls to handle short reads/writes and EINTR), and the public
 * cafs_read_block()/cafs_write_block() API with alignment handling
 * and in-RAM SMART counters.
 *
 * Alignment: "aligned" means offset and size are both multiples of
 * 512 bytes — the universal minimum sector size, satisfied by every
 * fixed anchor offset this engine itself uses (0/2048/4096/6144/
 * 8192/12288 are all 512-aligned), and a safe floor for O_DIRECT
 * regardless of the underlying device's actual logical sector size.
 * misaligned_action=copy bounces an unaligned request through a
 * correctly-sized, 512-aligned heap buffer; =reject fails it outright.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include "cafs_io_internal.h"
#include "checksum.h"
#include "syscalls.h"

#define CAFS_ALIGN 512u

cafs_status_t cafs_read_raw(cafs_ctx_t *ctx, uint64_t offset, uint32_t size, void *buf)
{
    if (!ctx || !ctx->is_open) return CAFS_ERR_NOT_MOUNTED;
    uint8_t *p = (uint8_t *)buf;
    uint64_t got = 0;
    while (got < size) {
        int64_t r = cafs_sys_pread64(ctx->fd, p + got, size - got, (int64_t)(offset + got));
        if (r < 0) {
            if (r == -EINTR) continue;
            return CAFS_ERR_IO;
        }
        if (r == 0) return CAFS_ERR_IO; /* unexpected EOF against a fixed-size structure */
        got += (uint64_t)r;
    }
    return CAFS_OK;
}

cafs_status_t cafs_write_raw(cafs_ctx_t *ctx, uint64_t offset, uint32_t size, const void *buf)
{
    if (!ctx || !ctx->is_open) return CAFS_ERR_NOT_MOUNTED;
    if (!ctx->want_write) return CAFS_ERR_READONLY;
    const uint8_t *p = (const uint8_t *)buf;
    uint64_t sent = 0;
    while (sent < size) {
        int64_t r = cafs_sys_pwrite64(ctx->fd, p + sent, size - sent, (int64_t)(offset + sent));
        if (r < 0) {
            if (r == -EINTR) continue;
            return CAFS_ERR_IO;
        }
        sent += (uint64_t)r;
    }
    return CAFS_OK;
}

static int is_aligned(uint64_t offset, uint32_t size)
{
    return (offset % CAFS_ALIGN == 0) && (size % CAFS_ALIGN == 0);
}

cafs_status_t cafs_read_block(cafs_ctx_t *ctx, uint64_t lba, uint32_t size, void *buf)
{
    if (!ctx || !buf || size == 0) return CAFS_ERR_INVALID_ARG;
    pthread_mutex_lock(&ctx->lock);

    cafs_status_t st;
    if (is_aligned(lba, size)) {
        st = cafs_read_raw(ctx, lba, size, buf);
    } else if (ctx->opts.misaligned_action == CAFS_MISALIGNED_REJECT) {
        st = CAFS_ERR_MISALIGNED;
    } else {
        uint64_t aligned_off = (lba / CAFS_ALIGN) * CAFS_ALIGN;
        uint64_t end = lba + size;
        uint64_t aligned_end = ((end + CAFS_ALIGN - 1) / CAFS_ALIGN) * CAFS_ALIGN;
        uint32_t aligned_len = (uint32_t)(aligned_end - aligned_off);
        void *bounce = malloc(aligned_len);
        if (!bounce) {
            st = CAFS_ERR_NOMEM;
        } else {
            st = cafs_read_raw(ctx, aligned_off, aligned_len, bounce);
            if (st == CAFS_OK)
                memcpy(buf, (uint8_t *)bounce + (lba - aligned_off), size);
            free(bounce);
        }
    }

    ctx->smart.total_reads++;
    if (st != CAFS_OK) ctx->smart.total_read_errors++;
    pthread_mutex_unlock(&ctx->lock);
    return st;
}

cafs_status_t cafs_write_block(cafs_ctx_t *ctx, uint64_t lba, uint32_t size, const void *buf)
{
    if (!ctx || !buf || size == 0) return CAFS_ERR_INVALID_ARG;
    if (ctx->opts.readonly) return CAFS_ERR_READONLY;
    pthread_mutex_lock(&ctx->lock);

    cafs_status_t st;
    if (is_aligned(lba, size)) {
        st = cafs_write_raw(ctx, lba, size, buf);
    } else if (ctx->opts.misaligned_action == CAFS_MISALIGNED_REJECT) {
        st = CAFS_ERR_MISALIGNED;
    } else {
        /* Bounce through an aligned buffer: read-modify-write, since
         * the aligned range extends beyond what the caller supplied
         * and those extra bytes must not be clobbered. */
        uint64_t aligned_off = (lba / CAFS_ALIGN) * CAFS_ALIGN;
        uint64_t end = lba + size;
        uint64_t aligned_end = ((end + CAFS_ALIGN - 1) / CAFS_ALIGN) * CAFS_ALIGN;
        uint32_t aligned_len = (uint32_t)(aligned_end - aligned_off);
        void *bounce = malloc(aligned_len);
        if (!bounce) {
            st = CAFS_ERR_NOMEM;
        } else {
            st = cafs_read_raw(ctx, aligned_off, aligned_len, bounce);
            if (st == CAFS_OK) {
                memcpy((uint8_t *)bounce + (lba - aligned_off), buf, size);
                st = cafs_write_raw(ctx, aligned_off, aligned_len, bounce);
            }
            free(bounce);
        }
    }

    ctx->smart.total_writes++;
    if (st != CAFS_OK) ctx->smart.total_write_errors++;
    pthread_mutex_unlock(&ctx->lock);
    return st;
}

void cafs_get_smart_counters(const cafs_ctx_t *ctx, cafs_smart_counters_t *out)
{
    if (!ctx || !out) return;
    /* Snapshot read; not worth taking the lock for a diagnostic read
     * of a handful of counters the caller can't act atomically on
     * anyway. */
    *out = ctx->smart;
}
