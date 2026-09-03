/*
 * mkfs_cafs.c — CAFS V1 format tool.
 *
 * Without this there was no way to produce a device image the engine
 * could actually mount, so this exists purely to make cafs_mount() /
 * cafs_check() / cafs_remount() / cafs_confirm() testable end to end.
 * It is deliberately minimal: writes exactly what the V1 engine reads
 * and acts on, nothing from the fuller fs.info spec that this
 * library's PoC scope ignores (Meta/User Table gets a zeroed
 * placeholder so the image is structurally complete, but its content
 * is never meaningful since nothing reads it).
 *
 * Device layout written (byte offsets):
 *
 *   0      LBA0 ptr block  (2048B) -> Superblock @ 16384
 *                                     fallback   -> Superblock Backup @ size-4096
 *   2048   LBA1 ptr block  (2048B) -> Config Snapshot @ 20480
 *   4096   LBA2 ptr block  (2048B) -> Parity Block @ 24576
 *   6144   LBA3 ptr block  (2048B) -> Meta/User Table placeholder @ 28672 (unread by the engine)
 *   8192   LBA4 ptr block  (4096B) -> Function Table @ 32768
 *   12288  LBA5 ptr block  (4096B) -> byte-for-byte copy of LBA4 (fs.info §2)
 *   16384  Superblock                (4096B)
 *   20480  Config Snapshot           (4096B)
 *   24576  Parity Block              (4096B) = XOR(Superblock, Config Snapshot)
 *   28672  Meta/User Table           (4096B, zeroed placeholder)
 *   32768  Function Table            (4096B slot, 80 bytes meaningful)
 *   36864  M-SMART table             (4096B slot, 144 bytes meaningful)
 *   ...    free space (no data path in V1)
 *   size-8192  B-SMART table         (4096B slot, 144 bytes meaningful)
 *   size-4096  Superblock Backup     (4096B, identical to primary at format time)
 *
 * No end-of-device mirror of LBA0-5, no Config-Snapshot/Function-
 * Table/Meta-Table backups — per the revised design, redundancy
 * exists only for Superblock and SMART (see cafs_io_layout.h).
 *
 * Uses the same raw syscalls as the engine (syscalls_x86_64.S) for
 * all device I/O; ordinary libc stdio is used only for CLI messages,
 * which aren't device I/O.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include "cafs_io_layout.h"
#include "checksum.h"
#include "syscalls.h"

#define OFF_SUPERBLOCK        16384ull
#define OFF_CONFIG_SNAPSHOT   20480ull
#define OFF_PARITY            24576ull
#define OFF_META_PLACEHOLDER  28672ull
#define OFF_FUNCTION_TABLE    32768ull
#define OFF_SMART_MAIN        36864ull
#define FRONT_REGION_SIZE     40960ull
#define END_REGION_SIZE       8192ull  /* B-SMART + Superblock Backup, 4096 each */
#define MIN_DEVICE_SIZE       (1024ull * 1024ull)
#define DEFAULT_DEVICE_SIZE   (4ull * 1024ull * 1024ull)

static void die(const char *msg)
{
    fprintf(stderr, "mkfs.cafs: %s\n", msg);
    exit(1);
}

static void write_at(int fd, uint64_t off, const void *buf, size_t len)
{
    const uint8_t *p = (const uint8_t *)buf;
    size_t sent = 0;
    while (sent < len) {
        int64_t r = cafs_sys_pwrite64(fd, p + sent, len - sent, (int64_t)(off + sent));
        if (r < 0) die("write failed (target too small, or permission denied)");
        sent += (size_t)r;
    }
}

static void build_ptr_block(uint8_t *block, uint32_t block_len,
                             uint64_t real_lba, uint64_t real_size,
                             uint64_t fb_real_lba, uint64_t fb_real_size)
{
    memset(block, 0, block_len);
    memcpy(block + PTR2K_OFF_MAGIC, CAFS_PTR_MAGIC, 8); /* offset 0, same for both sizes */
    memcpy(block + PTR2K_OFF_REAL_LBA, &real_lba, 8);
    memcpy(block + PTR2K_OFF_REAL_SIZE, &real_size, 8);

    uint32_t primary_hashed_len = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                                       ? PTR2K_PRIMARY_HASHED_LEN : PTR4K_PRIMARY_HASHED_LEN;
    uint32_t checksum_off = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                                 ? PTR2K_OFF_CHECKSUM : PTR4K_OFF_CHECKSUM;
    uint32_t cksum = cafs_crc32c(block, primary_hashed_len);
    memcpy(block + checksum_off, &cksum, 4);

    if (fb_real_lba != 0) {
        uint32_t off_fb_magic = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                                     ? PTR2K_OFF_FALLBACK_MAGIC : PTR4K_OFF_FALLBACK_MAGIC;
        uint32_t off_fb_lba = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                                   ? PTR2K_OFF_FALLBACK_REAL_LBA : PTR4K_OFF_FALLBACK_REAL_LBA;
        uint32_t off_fb_size = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                                    ? PTR2K_OFF_FALLBACK_REAL_SIZE : PTR4K_OFF_FALLBACK_REAL_SIZE;
        uint32_t off_fb_cksum = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                                     ? PTR2K_OFF_FALLBACK_CHECKSUM : PTR4K_OFF_FALLBACK_CHECKSUM;
        uint32_t fb_hashed_len = (block_len == CAFS_PTR_BLOCK_SMALL_SIZE)
                                      ? PTR2K_FALLBACK_HASHED_LEN : PTR4K_FALLBACK_HASHED_LEN;

        memcpy(block + off_fb_magic, CAFS_PTR_MAGIC, 8);
        memcpy(block + off_fb_lba, &fb_real_lba, 8);
        memcpy(block + off_fb_size, &fb_real_size, 8);
        uint32_t fbcksum = cafs_crc32c(block + off_fb_magic, fb_hashed_len);
        memcpy(block + off_fb_cksum, &fbcksum, 4);
    }
}

static void build_superblock(uint8_t *sb)
{
    memset(sb, 0, CAFS_REAL_BLOCK_4K);
    uint32_t magic = CAFS_SUPERBLOCK_MAGIC;
    memcpy(sb + SB_OFF_MAGIC, &magic, 4);
    uint32_t fmtver = CAFS_FORMAT_VERSION;
    memcpy(sb + SB_OFF_FORMAT_VERSION, &fmtver, 4);
    for (int i = 0; i < 16; i++) sb[SB_OFF_VOLUME_UUID + i] = (uint8_t)(0xA0 + i); /* deterministic test UUID */
    uint64_t cfggen = 1;
    memcpy(sb + SB_OFF_CONFIG_GENERATION, &cfggen, 8);
    uint64_t ft_anchor_slot = 4; /* the LBA4 SLOT INDEX, not a byte offset — see mount.c */
    memcpy(sb + SB_OFF_FUNCTION_TABLE_ANCHOR_LBA, &ft_anchor_slot, 8);
    uint64_t zero64 = 0;
    memcpy(sb + SB_OFF_WAL_SEQUENCE, &zero64, 8);
    memcpy(sb + SB_OFF_LAST_MOUNT_TIME, &zero64, 8); /* never mounted yet; confirm() sets this */
    uint32_t flags = 0;
    memcpy(sb + SB_OFF_FLAGS, &flags, 4);
    uint32_t data_algo = CAFS_DATA_ALGO_BLAKE3;
    memcpy(sb + SB_OFF_DATA_CHECKSUM_ALGO, &data_algo, 4);
    uint32_t anchor_algo = CAFS_ANCHOR_ALGO_BLAKE3_128;
    memcpy(sb + SB_OFF_ANCHOR_CHECKSUM_ALGO, &anchor_algo, 4);
    uint32_t ptr_algo = CAFS_PTR_ALGO_CRC32C;
    memcpy(sb + SB_OFF_POINTER_CHECKSUM_ALGO, &ptr_algo, 4);
    memcpy(sb + SB_OFF_LAST_APPLIED_WAL_SEQ, &zero64, 8);
    uint32_t root_hash_algo = 0;
    memcpy(sb + SB_OFF_ROOT_HASH_ALGO, &root_hash_algo, 4);

    uint8_t cksum[16];
    cafs_blake3_128(sb, SB_HASHED_LEN, cksum);
    memcpy(sb + SB_OFF_CHECKSUM, cksum, 16);
}

static void build_config_snapshot(uint8_t *cs)
{
    memset(cs, 0, CAFS_REAL_BLOCK_4K);
    static const char toml[] =
        "[identity]\n"
        "name = \"test-volume\"\n"
        "\n"
        "[physical]\n"
        "block_size = 4096\n"
        "pointer_checksum_algo = \"crc32c\"\n"
        "\n"
        "[integrity]\n"
        "data_checksum_algo = \"blake3\"\n"
        "verify_data = true\n";
    size_t len = sizeof(toml) - 1;
    if (len >= CFGSNAP_HASHED_LEN) die("internal: config snapshot template too large");
    memcpy(cs, toml, len);

    uint8_t cksum[16];
    cafs_blake3_128(cs, CFGSNAP_HASHED_LEN, cksum);
    memcpy(cs + CFGSNAP_OFF_CHECKSUM, cksum, 16);
}

static void build_parity(uint8_t *parity, const uint8_t *sb, const uint8_t *cs)
{
    for (size_t i = 0; i < CAFS_REAL_BLOCK_4K; i++)
        parity[i] = sb[i] ^ cs[i];
}

static void build_function_table(uint8_t *ft, uint64_t smart_main_lba, uint64_t smart_backup_lba)
{
    memset(ft, 0, CAFS_REAL_BLOCK_4K);
    uint64_t wal_lba = 0, scratch = 0, tempcache = 0;
    memcpy(ft + FT_OFF_WAL_LBA, &wal_lba, 8);
    memcpy(ft + FT_OFF_SMART_MAIN_LBA, &smart_main_lba, 8);
    memcpy(ft + FT_OFF_SMART_BACKUP_LBA, &smart_backup_lba, 8);
    memcpy(ft + FT_OFF_SCRATCH_LBA, &scratch, 8);
    memcpy(ft + FT_OFF_TEMP_CACHE_LBA, &tempcache, 8);

    uint8_t cksum[16];
    cafs_blake3_128(ft, FT_HASHED_LEN, cksum);
    memcpy(ft + FT_OFF_CHECKSUM, cksum, 16);
}

static void build_smart(uint8_t *sm)
{
    memset(sm, 0, CAFS_REAL_BLOCK_4K);
    uint32_t magic = SMART_MAGIC;
    memcpy(sm + SMART_OFF_MAGIC, &magic, 4);
    uint32_t ver = 1;
    memcpy(sm + SMART_OFF_VERSION, &ver, 4);
    uint64_t seq = 1;
    memcpy(sm + SMART_OFF_SEQUENCE, &seq, 8);
    /* all counters start at 0, already zeroed by memset */
    sm[SMART_OFF_CLEAN_UNMOUNT_FLAG] = 1; /* freshly formatted = clean */

    uint8_t cksum[16];
    cafs_blake3_128(sm, SMART_HASHED_LEN, cksum);
    memcpy(sm + SMART_OFF_CHECKSUM, cksum, 16);
}

int main(int argc, char **argv)
{
    if (argc < 2 || argc > 3) {
        fprintf(stderr, "usage: mkfs_cafs <path> [size_bytes]\n"
                         "  Creates/truncates <path> as a regular file of the given size\n"
                         "  (default %llu bytes) and writes a fresh CAFS V1 anchor region.\n",
                (unsigned long long)DEFAULT_DEVICE_SIZE);
        return 1;
    }
    const char *path = argv[1];
    uint64_t size = (argc == 3) ? strtoull(argv[2], NULL, 10) : DEFAULT_DEVICE_SIZE;
    if (size < MIN_DEVICE_SIZE) die("size too small (minimum 1 MiB)");
    if (size < FRONT_REGION_SIZE + END_REGION_SIZE + 4096)
        die("size too small to fit the front and end regions without overlap");

    int64_t fd = cafs_sys_openat(CAFS_AT_FDCWD, path, O_RDWR | O_CREAT | O_TRUNC, 0644);
    if (fd < 0) die("could not create/open target path");

    { uint8_t z = 0; write_at((int)fd, size - 1, &z, 1); } /* extend to full size */

    uint64_t sb_backup_off = size - 4096;
    uint64_t bsmart_off = size - 8192;

    uint8_t sb[CAFS_REAL_BLOCK_4K], cs[CAFS_REAL_BLOCK_4K], parity[CAFS_REAL_BLOCK_4K];
    uint8_t meta[CAFS_REAL_BLOCK_4K], ft[CAFS_REAL_BLOCK_4K], smart[CAFS_REAL_BLOCK_4K];

    build_superblock(sb);
    build_config_snapshot(cs);
    build_parity(parity, sb, cs);
    memset(meta, 0, sizeof(meta)); /* placeholder; never read by the V1 engine */
    build_function_table(ft, OFF_SMART_MAIN, bsmart_off);
    build_smart(smart);

    uint8_t lba0[CAFS_PTR_BLOCK_SMALL_SIZE], lba1[CAFS_PTR_BLOCK_SMALL_SIZE];
    uint8_t lba2[CAFS_PTR_BLOCK_SMALL_SIZE], lba3[CAFS_PTR_BLOCK_SMALL_SIZE];
    uint8_t lba4[CAFS_PTR_BLOCK_LARGE_SIZE], lba5[CAFS_PTR_BLOCK_LARGE_SIZE];

    build_ptr_block(lba0, CAFS_PTR_BLOCK_SMALL_SIZE, OFF_SUPERBLOCK, CAFS_REAL_BLOCK_4K,
                     sb_backup_off, CAFS_REAL_BLOCK_4K);
    build_ptr_block(lba1, CAFS_PTR_BLOCK_SMALL_SIZE, OFF_CONFIG_SNAPSHOT, CAFS_REAL_BLOCK_4K, 0, 0);
    build_ptr_block(lba2, CAFS_PTR_BLOCK_SMALL_SIZE, OFF_PARITY, CAFS_REAL_BLOCK_4K, 0, 0);
    build_ptr_block(lba3, CAFS_PTR_BLOCK_SMALL_SIZE, OFF_META_PLACEHOLDER, CAFS_REAL_BLOCK_4K, 0, 0);
    build_ptr_block(lba4, CAFS_PTR_BLOCK_LARGE_SIZE, OFF_FUNCTION_TABLE, FT_MIN_REAL_SIZE, 0, 0);
    memcpy(lba5, lba4, CAFS_PTR_BLOCK_LARGE_SIZE); /* byte-for-byte copy, fs.info §2 */

    write_at((int)fd, CAFS_LBA0_OFFSET, lba0, sizeof(lba0));
    write_at((int)fd, CAFS_LBA1_OFFSET, lba1, sizeof(lba1));
    write_at((int)fd, CAFS_LBA2_OFFSET, lba2, sizeof(lba2));
    write_at((int)fd, CAFS_LBA3_OFFSET, lba3, sizeof(lba3));
    write_at((int)fd, CAFS_LBA4_OFFSET, lba4, sizeof(lba4));
    write_at((int)fd, CAFS_LBA5_OFFSET, lba5, sizeof(lba5));

    write_at((int)fd, OFF_SUPERBLOCK, sb, sizeof(sb));
    write_at((int)fd, OFF_CONFIG_SNAPSHOT, cs, sizeof(cs));
    write_at((int)fd, OFF_PARITY, parity, sizeof(parity));
    write_at((int)fd, OFF_META_PLACEHOLDER, meta, sizeof(meta));
    write_at((int)fd, OFF_FUNCTION_TABLE, ft, sizeof(ft));
    write_at((int)fd, OFF_SMART_MAIN, smart, sizeof(smart));

    write_at((int)fd, sb_backup_off, sb, sizeof(sb));   /* Superblock Backup */
    write_at((int)fd, bsmart_off, smart, sizeof(smart)); /* B-SMART */

    cafs_sys_fdatasync((int)fd);
    cafs_sys_close((int)fd);

    fprintf(stdout, "mkfs.cafs: formatted %s (%llu bytes)\n", path, (unsigned long long)size);
    return 0;
}
