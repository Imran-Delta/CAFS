/*
 * blake3_wrap.c — cafs_blake3_128() over the vendored BLAKE3
 * reference implementation (../../../vendor/blake3, portable build
 * only — see README.md and vendor/blake3/NOTICE for licensing).
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include "checksum.h"
#include "blake3.h"

void cafs_blake3_128(const void *data, size_t len, uint8_t out[16])
{
    blake3_hasher hasher;
    blake3_hasher_init(&hasher);
    blake3_hasher_update(&hasher, data, len);
    blake3_hasher_finalize(&hasher, out, 16);
}
