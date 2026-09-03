/*
 * cafs_io.h — CAFS I/O Engine, public API.
 *
 * Scope (V1): Linux x86-64, kernel 6.12+. Raw anchor-region mount
 * lifecycle (mount -> check -> remount -> confirm) plus generic
 * block read/write. No allocator, no filesystem semantics (no Zone
 * Table, B-tree, dedup, WAL replay) — all of that is delegated to
 * layers above this library, unchanged from that scoping decision.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#ifndef CAFS_IO_H
#define CAFS_IO_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    CAFS_OK = 0,
    CAFS_ERR_OPEN,          /* device open failed */
    CAFS_ERR_IO,             /* a read/write syscall failed */
    CAFS_ERR_TOO_SMALL,      /* device smaller than the anchor region needs */
    CAFS_ERR_BAD_MAGIC,      /* a structure's magic didn't match */
    CAFS_ERR_CHECKSUM,       /* a structure's checksum didn't match */
    CAFS_ERR_NOMEM,
    CAFS_ERR_READONLY,       /* write attempted against a readonly mount */
    CAFS_ERR_MISALIGNED,     /* unaligned I/O and misaligned_action=reject */
    CAFS_ERR_NOT_MOUNTED,    /* called out of order, e.g. confirm before mount */
    CAFS_ERR_INVALID_ARG,
    CAFS_ERR_UNSUPPORTED,    /* e.g. O_DIRECT unavailable and direct_io_required */
} cafs_status_t;

/* Opaque mount context. */
typedef struct cafs_ctx cafs_ctx_t;

/*
 * Per-mount runtime options. These map to config.fs's [mount_hints]
 * and [io] sections (both explicitly RUNTIME ONLY / NOT IN the Config
 * Snapshot per fs.info §5) — this library does not parse a host-side
 * config.fs file itself; the caller resolves args + local config into
 * this struct before calling cafs_remount().
 */
typedef enum { CAFS_MISALIGNED_COPY = 0, CAFS_MISALIGNED_REJECT = 1 } cafs_misaligned_action_t;

typedef struct {
    int readonly;              /* [mount_hints].readonly */
    int direct_io;              /* [mount_hints].direct_io */
    int direct_io_required;     /* [mount_hints].direct_io_required */
    int sync_open;               /* [mount_hints].sync_open */
    cafs_misaligned_action_t misaligned_action; /* [io].misaligned_action */
    int destroy_flush;           /* [io].destroy_flush */
} cafs_mount_opts_t;

/* Crash-critical fields read out of the on-disk Config Snapshot's TOML
 * text (fs.info §5). Extracted by a minimal targeted key scanner, not
 * a general TOML parser — see README.md. Informational for V1: this
 * engine doesn't act on data_checksum_algo/verify_data itself since it
 * never touches data blocks. */
typedef struct {
    uint32_t block_size;               /* [physical].block_size */
    uint32_t pointer_checksum_algo_id;  /* [physical].pointer_checksum_algo, resolved to §3.3 ID */
    uint32_t data_checksum_algo_id;     /* [integrity].data_checksum_algo, resolved to §16 ID */
    int verify_data;                     /* [integrity].verify_data */
    int fields_found;                    /* bitmask, bit per field above actually found in the snapshot text */
} cafs_config_snapshot_t;

#define CAFS_CFG_FOUND_BLOCK_SIZE      0x1
#define CAFS_CFG_FOUND_POINTER_ALGO    0x2
#define CAFS_CFG_FOUND_DATA_ALGO       0x4
#define CAFS_CFG_FOUND_VERIFY_DATA     0x8

/*
 * On-disk SMART is real and in scope for V1: mount() reads M-SMART
 * (via the Function Table), confirm() writes mount_count and
 * clean_unmount_flag to it directly — a direct write, not the full
 * V-SMART->Buffer->Handler->B-SMART->WAL->M-SMART reconciliation
 * pipeline, which stays the Python SMART Handler's job untouched.
 * Separately, this struct is pure in-RAM telemetry for this engine's
 * own read_block/write_block calls — it is never written to disk.
 */
typedef struct {
    uint64_t total_reads;
    uint64_t total_writes;
    uint32_t total_read_errors;
    uint32_t total_write_errors;
    uint32_t total_checksum_mismatch;
} cafs_smart_counters_t;

/*
 * check() result. PoC scope: only the Superblock gates overall_ok and
 * is eligible for auto-repair. Config Snapshot, Parity, Function
 * Table, and SMART are read best-effort by mount() but their
 * checksums are not verified or repaired here — "ignore all anchor
 * block errors except Superblock," a deliberate, temporary scope cut
 * to get a working read/write proof of concept first.
 */
typedef struct {
    int superblock_ok;        /* true after any repair below, if any, succeeded */
    int superblock_repaired;   /* true if the primary Superblock checksum failed and a good backup was copied over it */
    int overall_ok;             /* == superblock_ok in V1 */
    uint32_t pointer_checksum_algo_used; /* which algo (CRC32C/XXHASH32) verified LBA0's pointer block — see README's bootstrap note */
} cafs_check_result_t;

/*
 * mount() — opens the device and reads the anchor blocks: LBA0-2 and
 * LBA4 pointer blocks (parsed, LBA0's checksum verified since it's
 * the prerequisite for trusting where Superblock/Superblock-Backup
 * live — a doubly-corrupt LBA0 pointer block, primary and fallback
 * both bad, is a fatal CAFS_ERR_CHECKSUM here, out of scope for
 * auto-repair), then the real Superblock, Superblock Backup, Config
 * Snapshot, Parity Block, and Function Table (raw reads, best-effort
 * beyond Superblock — see cafs_check_result_t's scope note). If the
 * Function Table yields a usable smart_main_lba, M-SMART is read too
 * (best-effort). No writes. LBA3 (Meta/User Pointer Table) is not
 * read — everything it points to is delegated upward, out of scope
 * for this library.
 *
 * want_write: 0 opens O_RDONLY, nonzero opens O_RDWR (still no writes
 * happen until confirm()).
 */
cafs_status_t cafs_mount(const char *device_path, int want_write, cafs_ctx_t **out_ctx);

/* check() — verifies the Superblock's checksum. On mismatch, tries
 * Superblock Backup (reached via LBA0's fallback pointer, read at
 * mount()); if the backup's checksum is good, copies it over the
 * primary Superblock in place, fsyncs, logs the event to stderr, and
 * reports superblock_repaired=1 — "simple and foreseeable," per the
 * locked scope, so it doesn't just report, it fixes. If the backup is
 * also bad, reports superblock_ok=0 without crashing; the caller
 * decides what to do next. All other anchor structures are ignored in
 * V1 (see cafs_check_result_t). Safe to call multiple times. */
cafs_status_t cafs_check(cafs_ctx_t *ctx, cafs_check_result_t *out_result);

/* remount() — resolves the effective runtime configuration: reads
 * crash-critical fields out of the Config Snapshot already in memory
 * (out_snapshot may be NULL if the caller doesn't need them), then
 * applies opts on top for the fields this engine's own read/write
 * path consumes (readonly, direct_io, sync_open, misaligned_action,
 * destroy_flush). May close and reopen the device fd if direct_io/
 * sync_open change what flags it needs. Still no anchor writes. */
cafs_status_t cafs_remount(cafs_ctx_t *ctx, const cafs_mount_opts_t *opts,
                            cafs_config_snapshot_t *out_snapshot);

/*
 * confirm() — the only stage that writes beyond check()'s Superblock
 * self-repair. Sets Superblock last_mount_time = now, recomputes its
 * BLAKE3-128 checksum, and commits it via fs.info §13.3 (In-Place
 * Content Update): primary Superblock, then Superblock Backup (via
 * LBA0's fallback field), then the primary Parity Block (XOR of the
 * new Superblock and the cached Config Snapshot) — each write
 * followed by fdatasync(). If a usable M-SMART location was found at
 * mount(), also increments mount_count and sets clean_unmount_flag=0
 * on M-SMART directly (not through the full SMART Handler pipeline),
 * best-effort — a missing/unavailable SMART table doesn't fail the
 * call. There is no Backup Parity Block to keep in sync — Parity has
 * no backup under the revised design (see cafs_io_layout.h). Fails
 * with CAFS_ERR_READONLY if the mount is readonly.
 */
cafs_status_t cafs_confirm(cafs_ctx_t *ctx);

/* Generic block I/O. lba/size are raw byte offset/length (see
 * cafs_io_layout.h's addressing-unit note). Increments the in-RAM
 * SMART counters. Honors misaligned_action from the last
 * cafs_remount() (defaults to CAFS_MISALIGNED_COPY before the first
 * remount). */
cafs_status_t cafs_read_block(cafs_ctx_t *ctx, uint64_t lba, uint32_t size, void *buf);
cafs_status_t cafs_write_block(cafs_ctx_t *ctx, uint64_t lba, uint32_t size, const void *buf);

/* Not part of the originally-named four-stage flow — added because
 * confirm() only ever marks a mount as started; without a
 * counterpart, destroy_flush and a graceful fd close never happen.
 * Flushes (if destroy_flush is set), closes the device fd, frees ctx.
 * ctx is invalid after this call regardless of the return value. */
cafs_status_t cafs_close(cafs_ctx_t *ctx);

void cafs_get_smart_counters(const cafs_ctx_t *ctx, cafs_smart_counters_t *out);
const char *cafs_strerror(cafs_status_t status);

#ifdef __cplusplus
}
#endif

#endif /* CAFS_IO_H */
