/*
 * checksum.h — CRC32C, XXHASH32, BLAKE3-128. All three are software
 * implementations, not hardware-accelerated (no SSE4.2 CRC32
 * instruction, no SIMD BLAKE3). Deliberate for V1: anchor-region
 * checksums are computed rarely (once per check()/confirm() call, a
 * few KB of data), not on a hot path, so a straightforward, easy-to-
 * audit software path was chosen over a CPUID-dispatch subsystem.
 * Flagged in README.md as a known, low-priority optimization opening.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#ifndef CAFS_CHECKSUM_H
#define CAFS_CHECKSUM_H

#include <stdint.h>
#include <stddef.h>

/* CRC32C (Castagnoli), reflected polynomial 0x82F63B78. Used for
 * pointer-block checksums (fs.info §3.3, ID 1, the default). */
uint32_t cafs_crc32c(const void *data, size_t len);

/* xxHash32. Used for pointer-block checksums (fs.info §3.3, ID 2,
 * the non-default option). */
uint32_t cafs_xxhash32(const void *data, size_t len, uint32_t seed);

/* BLAKE3, truncated to a 16-byte (128-bit) output via BLAKE3's native
 * variable-length finalize. Used for all real-structure anchor
 * checksums (fs.info §16: anchor_checksum_algo_id must be 1). */
void cafs_blake3_128(const void *data, size_t len, uint8_t out[16]);

/* Runs known test vectors for all three algorithms above. Returns 1
 * if every vector matches, 0 on the first mismatch (in which case a
 * message has been written to stderr identifying which one failed). */
int cafs_checksum_selftest(void);

#endif /* CAFS_CHECKSUM_H */
