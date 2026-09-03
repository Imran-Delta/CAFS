/*
 * mount.c — cafs_mount(). Opens the device and reads the anchor
 * blocks: LBA0-2 and LBA4 pointer blocks, then Superblock, Superblock
 * Backup (via LBA0's fallback), Config Snapshot, Parity, Function
 * Table, and (if the Function Table yields one) M-SMART. Read only.
 *
 * LBA0's own pointer-block checksum is verified here because it's
 * the prerequisite for trusting where Superblock/Superblock-Backup
 * even are — if it's bad (primary AND fallback both fail), mount()
 * fails outright; that's pointer-block-level corruption (fs.info
 * Track A territory) and is out of scope for this PoC's "simple,
 * foreseeable" repair. Everything past that point is read
 * best-effort: a failed read leaves the corresponding ctx buffer
 * zeroed and a *_valid flag clear, never aborts the mount. Superblock
 * CONTENT verification/repair is check()'s job, not mount()'s.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <fcntl.h>
#include <errno.h>
#include <string.h>
#include <stdlib.h>
#include "cafs_io_internal.h"
#include "syscalls.h"

/* Raw kernel `struct stat` (x86-64), not glibc's — see syscalls_x86_64.S. */
#define KSTAT_SIZE       144
#define KSTAT_OFF_MODE   24
#define KSTAT_OFF_SIZE   48
#define KSTAT_S_IFMT     0170000u
#define KSTAT_S_IFBLK    0060000u

#define CAFS_BLKGETSIZE64 0x80081272ul

static cafs_status_t get_device_size(int fd, uint64_t *out_size)
{
    uint8_t stbuf[KSTAT_SIZE];
    int64_t r = cafs_sys_fstat(fd, stbuf);
    if (r < 0) return CAFS_ERR_IO;

    uint32_t mode;
    memcpy(&mode, stbuf + KSTAT_OFF_MODE, 4);

    if ((mode & KSTAT_S_IFMT) == KSTAT_S_IFBLK) {
        uint64_t sz = 0;
        int64_t ir = cafs_sys_ioctl(fd, CAFS_BLKGETSIZE64, &sz);
        if (ir < 0) return CAFS_ERR_IO;
        *out_size = sz;
    } else {
        int64_t sz;
        memcpy(&sz, stbuf + KSTAT_OFF_SIZE, 8);
        if (sz < 0) return CAFS_ERR_IO;
        *out_size = (uint64_t)sz;
    }
    return CAFS_OK;
}

/* Reads min(real_size, buf_cap) bytes at real_lba into buf (zeroed
 * first). Never fails the caller — returns the status separately so
 * mount() can record best-effort availability without aborting. */
static cafs_status_t read_real_capped(cafs_ctx_t *ctx, uint64_t real_lba,
                                       uint64_t real_size, uint8_t *buf,
                                       size_t buf_cap, size_t *out_len)
{
    memset(buf, 0, buf_cap);
    if (real_lba == 0 || real_size == 0) { *out_len = 0; return CAFS_ERR_IO; }
    size_t want = (real_size < buf_cap) ? (size_t)real_size : buf_cap;
    if (real_lba + want > ctx->device_size) { *out_len = 0; return CAFS_ERR_TOO_SMALL; }
    cafs_status_t st = cafs_read_raw(ctx, real_lba, (uint32_t)want, buf);
    *out_len = (st == CAFS_OK) ? want : 0;
    return st;
}

cafs_status_t cafs_mount(const char *device_path, int want_write, cafs_ctx_t **out_ctx)
{
    if (!device_path || !out_ctx) return CAFS_ERR_INVALID_ARG;

    cafs_ctx_t *ctx = (cafs_ctx_t *)calloc(1, sizeof(cafs_ctx_t));
    if (!ctx) return CAFS_ERR_NOMEM;

    int flags = want_write ? O_RDWR : O_RDONLY;
    int64_t fd = cafs_sys_openat(CAFS_AT_FDCWD, device_path, flags, 0);
    if (fd < 0) { free(ctx); return CAFS_ERR_OPEN; }
    ctx->fd = (int)fd;
    ctx->is_open = 1;
    ctx->want_write = want_write;
    strncpy(ctx->device_path, device_path, sizeof(ctx->device_path) - 1);

    cafs_status_t st = get_device_size(ctx->fd, &ctx->device_size);
    if (st != CAFS_OK) { cafs_sys_close(ctx->fd); free(ctx); return st; }
    if (ctx->device_size < CAFS_ANCHOR_REGION_BYTES) {
        cafs_sys_close(ctx->fd); free(ctx); return CAFS_ERR_TOO_SMALL;
    }

    /* LBA0: must verify — everything else hangs off trusting this. */
    uint8_t lba0_raw[CAFS_PTR_BLOCK_SMALL_SIZE];
    st = cafs_read_raw(ctx, CAFS_LBA0_OFFSET, CAFS_PTR_BLOCK_SMALL_SIZE, lba0_raw);
    if (st != CAFS_OK) { cafs_sys_close(ctx->fd); free(ctx); return st; }
    int algo_used = 0;
    if (!cafs_verify_ptr_block(lba0_raw, CAFS_PTR_BLOCK_SMALL_SIZE,
                                CAFS_PTR_ALGO_CRC32C, &ctx->lba0, &algo_used)) {
        cafs_sys_close(ctx->fd); free(ctx); return CAFS_ERR_CHECKSUM;
    }
    ctx->lba0_pointer_algo = (uint32_t)algo_used;

    /* LBA1/LBA2/LBA4: parsed best-effort, not gated (V1 PoC scope). */
    uint8_t buf2k[CAFS_PTR_BLOCK_SMALL_SIZE];
    if (cafs_read_raw(ctx, CAFS_LBA1_OFFSET, CAFS_PTR_BLOCK_SMALL_SIZE, buf2k) == CAFS_OK)
        cafs_parse_ptr_block(buf2k, CAFS_PTR_BLOCK_SMALL_SIZE, &ctx->lba1);
    if (cafs_read_raw(ctx, CAFS_LBA2_OFFSET, CAFS_PTR_BLOCK_SMALL_SIZE, buf2k) == CAFS_OK)
        cafs_parse_ptr_block(buf2k, CAFS_PTR_BLOCK_SMALL_SIZE, &ctx->lba2);
    uint8_t buf4k[CAFS_PTR_BLOCK_LARGE_SIZE];
    if (cafs_read_raw(ctx, CAFS_LBA4_OFFSET, CAFS_PTR_BLOCK_LARGE_SIZE, buf4k) == CAFS_OK)
        cafs_parse_ptr_block(buf4k, CAFS_PTR_BLOCK_LARGE_SIZE, &ctx->lba4);

    size_t got;
    read_real_capped(ctx, ctx->lba0.real_lba, ctx->lba0.real_size,
                      ctx->superblock, sizeof(ctx->superblock), &got);
    ctx->superblock_backup_valid =
        (read_real_capped(ctx, ctx->lba0.fallback_real_lba, ctx->lba0.fallback_real_size,
                           ctx->superblock_backup, sizeof(ctx->superblock_backup), &got) == CAFS_OK);
    read_real_capped(ctx, ctx->lba1.real_lba, ctx->lba1.real_size,
                      ctx->config_snapshot, sizeof(ctx->config_snapshot), &got);
    read_real_capped(ctx, ctx->lba2.real_lba, ctx->lba2.real_size,
                      ctx->parity, sizeof(ctx->parity), &got);
    if (read_real_capped(ctx, ctx->lba4.real_lba, ctx->lba4.real_size,
                          ctx->function_table, sizeof(ctx->function_table), &got) == CAFS_OK
        && got >= FT_MIN_REAL_SIZE) {
        ctx->function_table_real_size = got;
        memcpy(&ctx->smart_main_lba, ctx->function_table + FT_OFF_SMART_MAIN_LBA, 8);
        memcpy(&ctx->smart_backup_lba, ctx->function_table + FT_OFF_SMART_BACKUP_LBA, 8);
        ctx->smart_main_valid =
            (read_real_capped(ctx, ctx->smart_main_lba, CAFS_REAL_BLOCK_4K,
                               ctx->smart_main, sizeof(ctx->smart_main), &got) == CAFS_OK);
    }

    /* Sane defaults until the caller calls remount(). */
    ctx->opts.readonly = !want_write;
    ctx->opts.misaligned_action = CAFS_MISALIGNED_COPY;
    ctx->opts.destroy_flush = 1;

    pthread_mutex_init(&ctx->lock, NULL);
    ctx->anchor_loaded = 1;
    *out_ctx = ctx;
    return CAFS_OK;
}
