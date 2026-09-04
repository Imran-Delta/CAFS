//! Checksum wrappers. All three algorithms are now hardware-accelerated
//! where the underlying crate supports it (CRC32C: SSE4.2 with runtime
//! CPUID; BLAKE3: SIMD with runtime CPU feature detection) — an
//! upgrade over the retired C engine, which was deliberately
//! software-only for simplicity (see ADR-011).
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

/// CRC32C (Castagnoli). Pointer-block checksums, fs.info §3.3 ID 1
/// (the default).
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c::crc32c(data)
}

/// xxHash32. Pointer-block checksums, fs.info §3.3 ID 2.
pub fn xxhash32(data: &[u8], seed: u32) -> u32 {
    xxhash_rust::xxh32::xxh32(data, seed)
}

/// BLAKE3, truncated to 16 bytes (128 bits) via BLAKE3's native
/// variable-length output. Real-structure anchor checksums, fs.info
/// §16 (anchor_checksum_algo_id must be 1).
pub fn blake3_128(data: &[u8]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(data);
    let mut out = [0u8; 16];
    hasher.finalize_xof().fill(&mut out);
    out
}

/// Same known vectors used to verify the retired C engine's hand-
/// written CRC32C/XXH32 and vendored BLAKE3 — kept identical so this
/// rewrite is checked against the same ground truth, not a new one.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32c_check_value() {
        // Standard CRC32C (Castagnoli) check value, RFC 3720 (iSCSI).
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    }

    #[test]
    fn xxhash32_empty_vector() {
        assert_eq!(xxhash32(b"", 0), 0x02CC_5D05);
    }

    #[test]
    fn blake3_128_empty_vector() {
        // Verified against reference `b3sum` output for zero-length
        // input, truncated to 16 bytes.
        let expect: [u8; 16] = [
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49,
        ];
        assert_eq!(blake3_128(b""), expect);
    }
}
