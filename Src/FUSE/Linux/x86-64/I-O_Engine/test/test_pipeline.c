/*
 * test_pipeline.c — exercises the built library against a real image
 * produced by mkfs_cafs, rather than reasoning about the format
 * statically. Run via `make test`.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../cafs_io.h"
#include "../cafs_io_layout.h"
#include "../checksum.h"

static int failures = 0;

#define CHECK(cond, msg) do { \
    if (cond) { printf("  PASS: %s\n", msg); } \
    else { printf("  FAIL: %s\n", msg); failures++; } \
} while (0)

static const char *IMG = "test/cafs_test.img";
static const uint64_t IMG_SIZE = 4ull * 1024 * 1024;

static void run_mkfs(void)
{
    char cmd[256];
    snprintf(cmd, sizeof(cmd), "./mkfs_cafs %s %llu >/tmp/mkfs.out 2>&1", IMG, (unsigned long long)IMG_SIZE);
    int rc = system(cmd);
    CHECK(rc == 0, "mkfs_cafs exits 0");
}

static void corrupt_byte(uint64_t offset)
{
    FILE *f = fopen(IMG, "r+b");
    if (!f) { printf("  FAIL: could not open image to corrupt it\n"); failures++; return; }
    fseek(f, (long)offset, SEEK_SET);
    int c = fgetc(f);
    fseek(f, (long)offset, SEEK_SET);
    fputc(c ^ 0xFF, f);
    fclose(f);
}

static int read_byte(uint64_t offset)
{
    FILE *f = fopen(IMG, "rb");
    if (!f) return -1;
    fseek(f, (long)offset, SEEK_SET);
    int c = fgetc(f);
    fclose(f);
    return c;
}

int main(void)
{
    printf("== checksum self-test ==\n");
    CHECK(cafs_checksum_selftest(), "CRC32C / XXHASH32 / BLAKE3-128 known vectors");

    printf("== mkfs ==\n");
    run_mkfs();

    printf("== fresh mount / check / remount / confirm ==\n");
    cafs_ctx_t *ctx = NULL;
    cafs_status_t st = cafs_mount(IMG, 1, &ctx);
    CHECK(st == CAFS_OK, "mount succeeds on a freshly formatted image");

    cafs_check_result_t chk;
    st = cafs_check(ctx, &chk);
    CHECK(st == CAFS_OK, "check() returns CAFS_OK");
    CHECK(chk.superblock_ok == 1, "check(): superblock_ok on a clean image");
    CHECK(chk.superblock_repaired == 0, "check(): no repair needed on a clean image");
    CHECK(chk.pointer_checksum_algo_used == CAFS_PTR_ALGO_CRC32C, "check(): bootstrap picked CRC32C (the format-time default)");

    cafs_mount_opts_t opts = {0};
    opts.readonly = 0;
    opts.misaligned_action = CAFS_MISALIGNED_COPY;
    opts.destroy_flush = 1;
    cafs_config_snapshot_t snap;
    st = cafs_remount(ctx, &opts, &snap);
    CHECK(st == CAFS_OK, "remount() returns CAFS_OK");
    CHECK(snap.fields_found == (CAFS_CFG_FOUND_BLOCK_SIZE | CAFS_CFG_FOUND_POINTER_ALGO |
                                 CAFS_CFG_FOUND_DATA_ALGO | CAFS_CFG_FOUND_VERIFY_DATA),
          "remount(): key scanner found all four expected fields");
    CHECK(snap.block_size == 4096, "remount(): block_size == 4096");
    CHECK(snap.pointer_checksum_algo_id == CAFS_PTR_ALGO_CRC32C, "remount(): pointer_checksum_algo == crc32c");
    CHECK(snap.data_checksum_algo_id == CAFS_DATA_ALGO_BLAKE3, "remount(): data_checksum_algo == blake3");
    CHECK(snap.verify_data == 1, "remount(): verify_data == true");

    st = cafs_confirm(ctx);
    CHECK(st == CAFS_OK, "confirm() returns CAFS_OK");

    printf("== raw block read/write proof of concept ==\n");
    uint8_t wbuf[512], rbuf[512];
    memset(wbuf, 0xAB, sizeof(wbuf));
    /* Free space well past everything mkfs_cafs wrote in the front region. */
    uint64_t scratch_off = 512ull * 1024;
    st = cafs_write_block(ctx, scratch_off, sizeof(wbuf), wbuf);
    CHECK(st == CAFS_OK, "write_block() into free space succeeds");
    st = cafs_read_block(ctx, scratch_off, sizeof(rbuf), rbuf);
    CHECK(st == CAFS_OK, "read_block() back succeeds");
    CHECK(memcmp(wbuf, rbuf, sizeof(wbuf)) == 0, "read_block() returns exactly what write_block() wrote");

    cafs_smart_counters_t sc;
    cafs_get_smart_counters(ctx, &sc);
    CHECK(sc.total_reads >= 1 && sc.total_writes >= 1, "in-RAM SMART counters incremented");

    cafs_close(ctx);
    ctx = NULL;

    printf("== re-mount after confirm(): last_mount_time persisted, checksum still valid ==\n");
    st = cafs_mount(IMG, 0, &ctx);
    CHECK(st == CAFS_OK, "re-mount (readonly) succeeds");
    st = cafs_check(ctx, &chk);
    CHECK(st == CAFS_OK && chk.superblock_ok == 1 && chk.superblock_repaired == 0,
          "re-check() still passes after confirm()'s Superblock rewrite");
    cafs_close(ctx);
    ctx = NULL;

    printf("== corruption + auto-repair ==\n");
    /* Primary Superblock lives at byte offset 16384; corrupt a byte
     * well inside its hashed range (0-4079) so the checksum breaks. */
    corrupt_byte(16384 + 100);

    st = cafs_mount(IMG, 1, &ctx);
    CHECK(st == CAFS_OK, "mount still succeeds with a corrupted primary Superblock");
    st = cafs_check(ctx, &chk);
    CHECK(st == CAFS_OK, "check() returns CAFS_OK even on a corrupted Superblock (no crash)");
    CHECK(chk.superblock_ok == 1, "check(): superblock_ok after repair");
    CHECK(chk.superblock_repaired == 1, "check(): superblock_repaired == 1");
    cafs_close(ctx);
    ctx = NULL;

    int fixed_byte = read_byte(16384 + 100);
    int backup_byte = read_byte((long)(IMG_SIZE - 4096) + 100);
    CHECK(fixed_byte >= 0 && fixed_byte == backup_byte,
          "primary Superblock byte 100 was actually rewritten to match the backup on disk");

    printf("== re-check after repair: no repair needed the second time ==\n");
    st = cafs_mount(IMG, 1, &ctx);
    CHECK(st == CAFS_OK, "mount succeeds after repair was persisted");
    st = cafs_check(ctx, &chk);
    CHECK(st == CAFS_OK && chk.superblock_ok == 1 && chk.superblock_repaired == 0,
          "check(): primary now clean, no repair triggered");
    cafs_close(ctx);
    ctx = NULL;

    printf("\n%s: %d failure(s)\n", failures == 0 ? "RESULT: ALL PASS" : "RESULT: FAILURES", failures);
    return failures == 0 ? 0 : 1;
}
