/*
 * remount.c — cafs_remount(). Extracts the handful of crash-critical
 * fields this engine cares about from the Config Snapshot's TOML text
 * (already in ctx->config_snapshot from mount()) via a minimal,
 * targeted key scanner — not a general TOML parser. The Config
 * Snapshot is machine-generated, comment/blank-line-stripped text
 * (fs.info §5), so this is sufficient: track the current `[section]`,
 * match `key = value` lines only inside the sections we care about.
 *
 * Merges opts on top for the runtime-only knobs (readonly, direct_io,
 * sync_open, misaligned_action, destroy_flush — fs.info §5 explicitly
 * excludes these from the Config Snapshot). If the effective
 * direct_io/sync_open flags differ from how the fd is currently open,
 * closes and reopens it. Callable multiple times (that's the point —
 * see cafs_io.h's doc comment on the safe-mode-then-fix-then-final-
 * config workflow).
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include "cafs_io_internal.h"
#include "syscalls.h"

static int is_ws(char c) { return c == ' ' || c == '\t' || c == '\r'; }

static int is_section_line(const char *line, size_t linelen)
{
    size_t i = 0;
    while (i < linelen && is_ws(line[i])) i++;
    return (i < linelen && line[i] == '[');
}

static int key_match(const char *line, size_t linelen, const char *key,
                      const char **val, size_t *vallen)
{
    size_t i = 0;
    while (i < linelen && is_ws(line[i])) i++;
    size_t klen = strlen(key);
    if (linelen < i + klen) return 0;
    if (memcmp(line + i, key, klen) != 0) return 0;
    i += klen;
    while (i < linelen && is_ws(line[i])) i++;
    if (i >= linelen || line[i] != '=') return 0;
    i++;
    while (i < linelen && is_ws(line[i])) i++;
    size_t vstart = i, vend = linelen;
    for (size_t j = vstart; j < linelen; j++) {
        if (line[j] == '#') { vend = j; break; }
    }
    while (vend > vstart && is_ws(line[vend - 1])) vend--;
    if (vend > vstart && line[vstart] == '"' && line[vend - 1] == '"' && vend - vstart >= 2) {
        vstart++; vend--; /* strip surrounding quotes */
    }
    *val = line + vstart;
    *vallen = vend - vstart;
    return 1;
}

static uint32_t parse_uint(const char *val, size_t vallen)
{
    uint32_t n = 0;
    for (size_t i = 0; i < vallen && val[i] >= '0' && val[i] <= '9'; i++)
        n = n * 10 + (uint32_t)(val[i] - '0');
    return n;
}

static int strval_eq(const char *val, size_t vallen, const char *s)
{
    size_t slen = strlen(s);
    return vallen == slen && memcmp(val, s, slen) == 0;
}

static void scan_config_snapshot(const uint8_t *bytes, size_t cap, cafs_config_snapshot_t *out)
{
    memset(out, 0, sizeof(*out));
    size_t len = 0;
    while (len < cap && bytes[len] != '\0') len++; /* stop at the zero-padding */

    char section[64] = {0};
    size_t pos = 0;
    const char *text = (const char *)bytes;

    while (pos < len) {
        size_t eol = pos;
        while (eol < len && text[eol] != '\n') eol++;
        size_t linelen = eol - pos;
        const char *line = text + pos;

        if (is_section_line(line, linelen)) {
            size_t i = 0;
            while (i < linelen && is_ws(line[i])) i++;
            i++; /* skip '[' */
            size_t start = i;
            while (i < linelen && line[i] != ']') i++;
            size_t namelen = (i < linelen) ? (i - start) : 0;
            if (namelen >= sizeof(section)) namelen = sizeof(section) - 1;
            memcpy(section, line + start, namelen);
            section[namelen] = '\0';
        } else if (section[0] != '\0') {
            const char *val; size_t vallen;
            if (strcmp(section, "physical") == 0) {
                if (key_match(line, linelen, "block_size", &val, &vallen)) {
                    out->block_size = parse_uint(val, vallen);
                    out->fields_found |= CAFS_CFG_FOUND_BLOCK_SIZE;
                } else if (key_match(line, linelen, "pointer_checksum_algo", &val, &vallen)) {
                    if (strval_eq(val, vallen, "crc32c")) out->pointer_checksum_algo_id = CAFS_PTR_ALGO_CRC32C;
                    else if (strval_eq(val, vallen, "xxhash32")) out->pointer_checksum_algo_id = CAFS_PTR_ALGO_XXHASH32;
                    out->fields_found |= CAFS_CFG_FOUND_POINTER_ALGO;
                }
            } else if (strcmp(section, "integrity") == 0) {
                if (key_match(line, linelen, "data_checksum_algo", &val, &vallen)) {
                    if (strval_eq(val, vallen, "none")) out->data_checksum_algo_id = CAFS_DATA_ALGO_NONE;
                    else if (strval_eq(val, vallen, "crc32c")) out->data_checksum_algo_id = CAFS_DATA_ALGO_CRC32C;
                    else if (strval_eq(val, vallen, "xxhash64")) out->data_checksum_algo_id = CAFS_DATA_ALGO_XXHASH64;
                    else if (strval_eq(val, vallen, "blake3")) out->data_checksum_algo_id = CAFS_DATA_ALGO_BLAKE3;
                    out->fields_found |= CAFS_CFG_FOUND_DATA_ALGO;
                } else if (key_match(line, linelen, "verify_data", &val, &vallen)) {
                    out->verify_data = strval_eq(val, vallen, "true");
                    out->fields_found |= CAFS_CFG_FOUND_VERIFY_DATA;
                }
            }
        }
        pos = eol + 1;
    }
}

cafs_status_t cafs_remount(cafs_ctx_t *ctx, const cafs_mount_opts_t *opts,
                            cafs_config_snapshot_t *out_snapshot)
{
    if (!ctx) return CAFS_ERR_INVALID_ARG;
    if (!ctx->anchor_loaded) return CAFS_ERR_NOT_MOUNTED;

    pthread_mutex_lock(&ctx->lock);

    scan_config_snapshot(ctx->config_snapshot, sizeof(ctx->config_snapshot), &ctx->cfg);
    if (out_snapshot) *out_snapshot = ctx->cfg;

    cafs_mount_opts_t prev = ctx->opts;
    if (opts) ctx->opts = *opts;
    /* readonly can't be lifted by opts alone if the fd itself was
     * opened O_RDONLY at mount() — reopening below handles that. */

    int want_direct = ctx->opts.direct_io;
    int want_sync = ctx->opts.sync_open;
    int want_write_now = !ctx->opts.readonly;

    int need_reopen = (want_write_now != ctx->want_write) ||
                       (want_direct != prev.direct_io) ||
                       (want_sync != prev.sync_open);

    cafs_status_t st = CAFS_OK;
    if (need_reopen) {
        int flags = want_write_now ? O_RDWR : O_RDONLY;
        if (want_direct) flags |= O_DIRECT;
        if (want_sync) flags |= O_SYNC;

        int64_t newfd = cafs_sys_openat(CAFS_AT_FDCWD, ctx->device_path, flags, 0);
        if (newfd < 0) {
            if (ctx->opts.direct_io_required) {
                st = CAFS_ERR_UNSUPPORTED;
            } else {
                /* Fall back to the flags we already had; direct_io
                 * wasn't required, so this is a soft failure. */
                ctx->opts.direct_io = prev.direct_io;
                ctx->opts.sync_open = prev.sync_open;
                st = CAFS_OK;
            }
        } else {
            cafs_sys_close(ctx->fd);
            ctx->fd = (int)newfd;
            ctx->want_write = want_write_now;
        }
    }

    ctx->remounted = 1;
    pthread_mutex_unlock(&ctx->lock);
    return st;
}
