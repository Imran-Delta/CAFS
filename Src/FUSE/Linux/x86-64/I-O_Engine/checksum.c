/*
 * checksum.c — CRC32C, XXHASH32, and a self-test harness for both
 * plus BLAKE3-128 (implementation of the latter in blake3_wrap.c,
 * over the vendored blake3 library).
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include <string.h>
#include <stdio.h>
#include "checksum.h"

/* ---- CRC32C (Castagnoli) ----
 * Bit-at-a-time, no lookup table: avoids any shared mutable state
 * (no static table to lazily initialize, no threading concern), at
 * the cost of speed that doesn't matter off the hot path. */
uint32_t cafs_crc32c(const void *data, size_t len)
{
    const uint8_t *buf = (const uint8_t *)data;
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < len; i++) {
        crc ^= buf[i];
        for (int k = 0; k < 8; k++) {
            uint32_t mask = (uint32_t)(-(int32_t)(crc & 1u));
            crc = (crc >> 1) ^ (0x82F63B78u & mask);
        }
    }
    return crc ^ 0xFFFFFFFFu;
}

/* ---- xxHash32 (public algorithm, hand-implemented; see selftest) ---- */
#define XXH_PRIME32_1 2654435761U
#define XXH_PRIME32_2 2246822519U
#define XXH_PRIME32_3 3266489917U
#define XXH_PRIME32_4 668265263U
#define XXH_PRIME32_5 374761393U

static inline uint32_t xxh_rotl32(uint32_t x, int r)
{
    return (x << r) | (x >> (32 - r));
}

static inline uint32_t xxh_read32le(const uint8_t *p)
{
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static inline uint32_t xxh_round(uint32_t acc, uint32_t input)
{
    acc += input * XXH_PRIME32_2;
    acc = xxh_rotl32(acc, 13);
    acc *= XXH_PRIME32_1;
    return acc;
}

uint32_t cafs_xxhash32(const void *data, size_t len, uint32_t seed)
{
    const uint8_t *p = (const uint8_t *)data;
    const uint8_t *const end = p + len;
    uint32_t h32;

    if (len >= 16) {
        const uint8_t *const limit = end - 16;
        uint32_t v1 = seed + XXH_PRIME32_1 + XXH_PRIME32_2;
        uint32_t v2 = seed + XXH_PRIME32_2;
        uint32_t v3 = seed + 0u;
        uint32_t v4 = seed - XXH_PRIME32_1;
        do {
            v1 = xxh_round(v1, xxh_read32le(p)); p += 4;
            v2 = xxh_round(v2, xxh_read32le(p)); p += 4;
            v3 = xxh_round(v3, xxh_read32le(p)); p += 4;
            v4 = xxh_round(v4, xxh_read32le(p)); p += 4;
        } while (p <= limit);
        h32 = xxh_rotl32(v1, 1) + xxh_rotl32(v2, 7) +
              xxh_rotl32(v3, 12) + xxh_rotl32(v4, 18);
    } else {
        h32 = seed + XXH_PRIME32_5;
    }

    h32 += (uint32_t)len;

    while (p + 4 <= end) {
        h32 += xxh_read32le(p) * XXH_PRIME32_3;
        h32 = xxh_rotl32(h32, 17) * XXH_PRIME32_4;
        p += 4;
    }

    while (p < end) {
        h32 += (uint32_t)(*p) * XXH_PRIME32_5;
        h32 = xxh_rotl32(h32, 11) * XXH_PRIME32_1;
        p++;
    }

    h32 ^= h32 >> 15;
    h32 *= XXH_PRIME32_2;
    h32 ^= h32 >> 13;
    h32 *= XXH_PRIME32_3;
    h32 ^= h32 >> 16;

    return h32;
}

int cafs_checksum_selftest(void)
{
    /* CRC32C("123456789") = 0xE3069283 — the standard CRC32C check
     * value (Castagnoli), used across implementations (e.g. RFC 3720
     * iSCSI). */
    {
        uint32_t v = cafs_crc32c("123456789", 9);
        if (v != 0xE3069283u) {
            fprintf(stderr, "cafs_checksum_selftest: CRC32C mismatch: got %08x want e3069283\n", v);
            return 0;
        }
    }
    /* XXH32(empty input, seed=0) = 0x02CC5D05 — the canonical
     * empty-input xxHash32 vector. */
    {
        uint32_t v = cafs_xxhash32(NULL, 0, 0);
        if (v != 0x02CC5D05u) {
            fprintf(stderr, "cafs_checksum_selftest: XXH32 mismatch: got %08x want 02cc5d05\n", v);
            return 0;
        }
    }
    /* BLAKE3-128 of empty input. Verified against the reference
     * `b3sum` output for a zero-length input, truncated to 16 bytes:
     * af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f232... */
    {
        static const uint8_t expect[16] = {
            0xaf,0x13,0x49,0xb9,0xf5,0xf9,0xa1,0xa6,
            0xa0,0x40,0x4d,0xea,0x36,0xdc,0xc9,0x49
        };
        uint8_t out[16];
        cafs_blake3_128("", 0, out);
        if (memcmp(out, expect, 16) != 0) {
            fprintf(stderr, "cafs_checksum_selftest: BLAKE3-128 mismatch on empty input\n");
            return 0;
        }
    }
    return 1;
}
