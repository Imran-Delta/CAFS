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

    // The fallback sub-block has its own magic and checksum (§3.1/3.2,
    // same algorithm as the primary). A fallback that fails either is
    // treated as absent: acting on a damaged pointer cannot be
    // 'self-defending' once a verified copy exists at the other end of
    // the device, and it hid that copy from check().
    let fallback_lba = read_u64(raw, o.fallback_real_lba);
    let fallback = if fallback_lba == 0 {
        None
    } else {
        let f_hashed = &raw[o.fallback_magic..o.fallback_magic + o.fallback_hashed_len];
        let f_stored = read_u32(raw, o.fallback_checksum);
        let f_sum = match algo {
            PtrAlgo::Crc32c => checksum::crc32c(f_hashed),
            PtrAlgo::Xxhash32 => checksum::xxhash32(f_hashed, 0),
        };
        let magic_ok = raw[o.fallback_magic..o.fallback_magic + 8] == PTR_MAGIC;
        if magic_ok && f_sum == f_stored {
            Some((ByteOffset(fallback_lba), read_u64(raw, o.fallback_real_size)))
        } else {
            None
        }
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

/// Which end of the device a pointer block was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Head,
    Tail,
}

/// One end of an anchor slot: verified pointer block + the checksum
/// algorithm that verified it, or why it failed.
pub type PtrSide = Result<(PtrBlock, PtrAlgo), CafsError>;

/// Both copies of one anchor slot (head at the start of the device,
/// tail at the end — fs.info §2, §12) and how each verified.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotReport {
    pub slot: u8,
    pub head: PtrSide,
    pub tail: PtrSide,
}

impl SlotReport {
    pub fn head_ok(&self) -> bool {
        self.head.is_ok()
    }
    pub fn tail_ok(&self) -> bool {
        self.tail.is_ok()
    }
    /// Head wins when both verify (it is the primary, §13.1).
    pub fn used(&self) -> Option<Side> {
        if self.head_ok() {
            Some(Side::Head)
        } else if self.tail_ok() {
            Some(Side::Tail)
        } else {
            None
        }
    }
    pub fn chosen(&self) -> Option<&(PtrBlock, PtrAlgo)> {
        match self.used()? {
            Side::Head => self.head.as_ref().ok(),
            Side::Tail => self.tail.as_ref().ok(),
        }
    }
    /// Slot 0 only: (Superblock, Superblock Backup) as seen from the
    /// side in use. The tail copy swaps primary and fallback (§12).
    pub fn sb_pair(&self) -> Option<(ByteOffset, Option<ByteOffset>)> {
        let head = self.head.as_ref().ok().map(|(b, _)| b);
        let tail = self.tail.as_ref().ok().map(|(b, _)| b);
        // Primary: the head's real pointer, else the tail's fallback.
        let primary = head
            .map(|b| b.real_offset)
            .or_else(|| tail.and_then(|b| b.fallback.map(|f| f.0)))?;
        // Backup: the head's (verified) fallback, else the tail's real pointer.
        let backup = head
            .and_then(|b| b.fallback.map(|f| f.0))
            .or_else(|| tail.map(|b| b.real_offset));
        Some((primary, backup))
    }
    /// Verifies AND carries everything this end is supposed to carry
    /// (slot 0 must have a verified fallback).
    fn complete(&self, side: &PtrSide) -> bool {
        match side {
            Ok((b, _)) => self.slot != 0 || b.fallback.is_some(),
            Err(_) => false,
        }
    }
    pub fn head_good(&self) -> bool {
        self.complete(&self.head)
    }
    pub fn tail_good(&self) -> bool {
        self.complete(&self.tail)
    }
    /// Both sides verify but point at different places.
    pub fn disagree(&self) -> bool {
        let (Ok((h, _)), Ok((t, _))) = (&self.head, &self.tail) else {
            return false;
        };
        if self.slot == 0 {
            (Some(h.real_offset), h.fallback.map(|f| f.0)) != (t.fallback.map(|f| f.0), Some(t.real_offset))
        } else {
            h.real_offset != t.real_offset || h.real_size != t.real_size
        }
    }
}

/// Build a pointer block (2048 B for LBA0-3, 4096 B for LBA4-5).
/// The fallback sub-block is only written when `fallback` is `Some`.
pub fn build_ptr_block(block_len: u32, algo: PtrAlgo, real_offset: u64, real_size: u64, fallback: Option<(u64, u64)>) -> Vec<u8> {
    let o = offsets_for(block_len as usize);
    let sum = |data: &[u8]| match algo {
        PtrAlgo::Crc32c => checksum::crc32c(data),
        PtrAlgo::Xxhash32 => checksum::xxhash32(data, 0),
    };
    let mut b = vec![0u8; block_len as usize];
    b[o.magic..o.magic + 8].copy_from_slice(&PTR_MAGIC);
    b[o.real_lba..o.real_lba + 8].copy_from_slice(&real_offset.to_le_bytes());
    b[o.real_size..o.real_size + 8].copy_from_slice(&real_size.to_le_bytes());
    let c = sum(&b[0..o.primary_hashed_len]);
    b[o.checksum..o.checksum + 4].copy_from_slice(&c.to_le_bytes());
    if let Some((fb_off, fb_size)) = fallback {
        b[o.fallback_magic..o.fallback_magic + 8].copy_from_slice(&PTR_MAGIC);
        b[o.fallback_real_lba..o.fallback_real_lba + 8].copy_from_slice(&fb_off.to_le_bytes());
        b[o.fallback_real_size..o.fallback_real_size + 8].copy_from_slice(&fb_size.to_le_bytes());
        let fc = sum(&b[o.fallback_magic..o.fallback_magic + o.fallback_hashed_len]);
        b[o.fallback_checksum..o.fallback_checksum + 4].copy_from_slice(&fc.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_small_ptr(real_offset: u64, real_size: u64, fallback: Option<(u64, u64)>) -> Vec<u8> {
        build_ptr_block(PTR_BLOCK_SMALL_SIZE, PtrAlgo::Crc32c, real_offset, real_size, fallback)
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
