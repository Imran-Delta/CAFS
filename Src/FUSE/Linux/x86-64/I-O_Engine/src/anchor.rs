//! Anchor pointer-block parsing and verification.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use crate::checksum;
use crate::error::{CafsError, CafsResult};
use crate::layout::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtrAlgo {
    Crc32c,
    Xxhash32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PtrBlock {
    pub real_offset: ByteOffset,
    pub real_size: u64,
    /// `None` if the fallback field is all-zero (unused under the
    /// revised redundancy design — ADR-006 — for every slot except
    /// LBA0, whose fallback points to Superblock Backup).
    pub fallback: Option<(ByteOffset, u64)>,
}

fn read_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(buf[off..off + 8].try_into().unwrap())
}

fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

struct Offsets {
    magic: usize,
    real_lba: usize,
    real_size: usize,
    checksum: usize,
    primary_hashed_len: usize,
    fallback_magic: usize,
    fallback_real_lba: usize,
    fallback_real_size: usize,
    fallback_checksum: usize,
    fallback_hashed_len: usize,
}

fn offsets_for(block_len: usize) -> Offsets {
    if block_len as u32 == PTR_BLOCK_SMALL_SIZE {
        Offsets {
            magic: PTR2K_OFF_MAGIC,
            real_lba: PTR2K_OFF_REAL_LBA,
            real_size: PTR2K_OFF_REAL_SIZE,
            checksum: PTR2K_OFF_CHECKSUM,
            primary_hashed_len: PTR2K_PRIMARY_HASHED_LEN,
            fallback_magic: PTR2K_OFF_FALLBACK_MAGIC,
            fallback_real_lba: PTR2K_OFF_FALLBACK_REAL_LBA,
            fallback_real_size: PTR2K_OFF_FALLBACK_REAL_SIZE,
            fallback_checksum: PTR2K_OFF_FALLBACK_CHECKSUM,
            fallback_hashed_len: PTR2K_FALLBACK_HASHED_LEN,
        }
    } else {
        Offsets {
            magic: PTR4K_OFF_MAGIC,
            real_lba: PTR4K_OFF_REAL_LBA,
            real_size: PTR4K_OFF_REAL_SIZE,
            checksum: PTR4K_OFF_CHECKSUM,
            primary_hashed_len: PTR4K_PRIMARY_HASHED_LEN,
            fallback_magic: PTR4K_OFF_FALLBACK_MAGIC,
            fallback_real_lba: PTR4K_OFF_FALLBACK_REAL_LBA,
            fallback_real_size: PTR4K_OFF_FALLBACK_REAL_SIZE,
            fallback_checksum: PTR4K_OFF_FALLBACK_CHECKSUM,
            fallback_hashed_len: PTR4K_FALLBACK_HASHED_LEN,
        }
    }
}

/// Verify a pointer block's primary checksum and parse it.
///
/// Bootstrap (ADR-010 resolution #3): fs.info §3.3 never states how a
/// fresh mount is supposed to know which of the two legal algorithms
/// (CRC32C, XXHASH32) a volume was formatted with, before anything
/// has been verified yet. Resolved by trying both and accepting
/// whichever matches — a false match between two different 4-byte
/// algorithms on corrupted data is not a realistic risk.
pub fn verify_ptr_block(raw: &[u8]) -> CafsResult<(PtrBlock, PtrAlgo)> {
    let o = offsets_for(raw.len());

    if raw[o.magic..o.magic + 8] != PTR_MAGIC {
        return Err(CafsError::BadMagic);
    }

    let hashed = &raw[0..o.primary_hashed_len];
    let stored = read_u32(raw, o.checksum);

    let algo = if checksum::crc32c(hashed) == stored {
        PtrAlgo::Crc32c
    } else if checksum::xxhash32(hashed, 0) == stored {
        PtrAlgo::Xxhash32
    } else {
        return Err(CafsError::Checksum);
    };

    let real_offset = ByteOffset(read_u64(raw, o.real_lba));
    let real_size = read_u64(raw, o.real_size);

    let fallback_lba = read_u64(raw, o.fallback_real_lba);
    let fallback = if fallback_lba == 0 {
        None
    } else {
        // The fallback field's own sub-checksum isn't verified here —
        // matches the C engine's behavior: an intact fallback pointer
        // into a corrupt/wrong region is self-defending, because the
        // real structure it points to (e.g. Superblock Backup) gets
        // its own BLAKE3 check when read. See fallback_hashed_len /
        // fallback_checksum / fallback_magic fields for a future,
        // stricter check if ever needed.
        let _ = (o.fallback_magic, o.fallback_checksum, o.fallback_hashed_len);
        Some((ByteOffset(fallback_lba), read_u64(raw, o.fallback_real_size)))
    };

    Ok((
        PtrBlock {
            real_offset,
            real_size,
            fallback,
        },
        algo,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_small_ptr(real_offset: u64, real_size: u64, fallback: Option<(u64, u64)>) -> Vec<u8> {
        let mut b = vec![0u8; PTR_BLOCK_SMALL_SIZE as usize];
        b[PTR2K_OFF_MAGIC..PTR2K_OFF_MAGIC + 8].copy_from_slice(&PTR_MAGIC);
        b[PTR2K_OFF_REAL_LBA..PTR2K_OFF_REAL_LBA + 8].copy_from_slice(&real_offset.to_le_bytes());
        b[PTR2K_OFF_REAL_SIZE..PTR2K_OFF_REAL_SIZE + 8].copy_from_slice(&real_size.to_le_bytes());
        let cksum = checksum::crc32c(&b[0..PTR2K_PRIMARY_HASHED_LEN]);
        b[PTR2K_OFF_CHECKSUM..PTR2K_OFF_CHECKSUM + 4].copy_from_slice(&cksum.to_le_bytes());
        if let Some((fb_off, fb_size)) = fallback {
            b[PTR2K_OFF_FALLBACK_MAGIC..PTR2K_OFF_FALLBACK_MAGIC + 8].copy_from_slice(&PTR_MAGIC);
            b[PTR2K_OFF_FALLBACK_REAL_LBA..PTR2K_OFF_FALLBACK_REAL_LBA + 8]
                .copy_from_slice(&fb_off.to_le_bytes());
            b[PTR2K_OFF_FALLBACK_REAL_SIZE..PTR2K_OFF_FALLBACK_REAL_SIZE + 8]
                .copy_from_slice(&fb_size.to_le_bytes());
        }
        b
    }

    #[test]
    fn parses_and_verifies_crc32c() {
        let raw = build_small_ptr(16384, 4096, Some((999_999, 4096)));
        let (block, algo) = verify_ptr_block(&raw).unwrap();
        assert_eq!(algo, PtrAlgo::Crc32c);
        assert_eq!(block.real_offset, ByteOffset(16384));
        assert_eq!(block.real_size, 4096);
        assert_eq!(block.fallback, Some((ByteOffset(999_999), 4096)));
    }

    #[test]
    fn no_fallback_is_none() {
        let raw = build_small_ptr(20480, 4096, None);
        let (block, _) = verify_ptr_block(&raw).unwrap();
        assert_eq!(block.fallback, None);
    }

    #[test]
    fn corrupted_checksum_is_rejected() {
        let mut raw = build_small_ptr(16384, 4096, None);
        raw[10] ^= 0xFF; // corrupt a byte inside real_lba (8-15) — hashed, but outside the 0-7 magic field
        assert_eq!(verify_ptr_block(&raw), Err(CafsError::Checksum));
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut raw = build_small_ptr(16384, 4096, None);
        raw[0] = b'X';
        assert_eq!(verify_ptr_block(&raw).unwrap_err(), CafsError::BadMagic);
    }
}
