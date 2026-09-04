//! `CafsCtx::check` — verifies the Superblock's BLAKE3-128 checksum.
//! On mismatch: tries Superblock Backup (via LBA0's fallback
//! pointer); if that's good, copies it over the primary in place
//! (when the mount is writable) and logs the repair. All other
//! anchor structures are neither checked nor repaired in V1
//! (ADR-007).
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use crate::block_io::{read_raw, write_raw};
use crate::checksum;
use crate::error::CafsResult;
use crate::layout::*;
use crate::{CafsCtx, CheckResult};

fn superblock_checksum_ok(sb: &[u8; REAL_BLOCK_4K as usize]) -> bool {
    let computed = checksum::blake3_128(&sb[0..SB_HASHED_LEN]);
    computed == sb[SB_OFF_CHECKSUM..SB_OFF_CHECKSUM + SB_CHECKSUM_LEN]
}

impl CafsCtx {
    pub fn check(&mut self) -> CafsResult<CheckResult> {
        let result = CheckResult {
            pointer_checksum_algo_used: Some(self.lba0_algo),
            ..Default::default()
        };

        if superblock_checksum_ok(&self.superblock) {
            result.superblock_ok = true;
            return Ok(result);
        }

        // Primary is bad — try the backup.
        let Some((backup_offset, _size)) = self.lba0.fallback else {
            eprintln!("cafs: primary Superblock checksum mismatch, and no backup pointer is set — unrecoverable");
            return Ok(result); // superblock_ok stays false
        };

        let mut backup = [0u8; REAL_BLOCK_4K as usize];
        if read_raw(self, backup_offset, &mut backup).is_err() || !superblock_checksum_ok(&backup) {
            eprintln!(
                "cafs: primary Superblock checksum mismatch, and Superblock Backup at offset {} is also bad or unreadable — unrecoverable",
                backup_offset.0
            );
            return Ok(result);
        }

        // Backup is good. Adopt it in memory regardless of write
        // access, so the rest of this session sees correct data even
        // on a readonly mount.
        self.superblock = backup;
        result.superblock_ok = true;

        if self.want_write {
            write_raw(self, self.lba0.real_offset, &self.superblock)?;
            eprintln!(
                "cafs: repaired primary Superblock at offset {} from Superblock Backup at offset {}",
                self.lba0.real_offset.0, backup_offset.0
            );
            result.superblock_repaired = true;
        } else {
            eprintln!(
                "cafs: primary Superblock checksum mismatch — valid backup found, but mount is readonly, so the fix was not written back"
            );
        }

        Ok(result)
    }
}
