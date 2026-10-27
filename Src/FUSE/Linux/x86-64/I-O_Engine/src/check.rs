//! `CafsCtx::check` — two stages, in this order:
//!
//! 1. Anchor pointer blocks: all six slots are verified at both ends
//!    of the device (fs.info §2, §12). A slot with exactly one bad end
//!    is rebuilt from the intact end (§14 Track A) when the mount is
//!    writable; a slot with both ends bad is reported as lost.
//! 2. Superblock: verifies its BLAKE3-128 checksum. On mismatch, tries
//!    the Superblock Backup; if that is good, copies it over the
//!    primary (when writable) and syncs.
//!
//! The Config Snapshot, Parity Block, Function Table and Meta/User
//! table are neither checked nor repaired (ADR-007).
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use crate::anchor::{build_ptr_block, Side, SlotReport};
use crate::block_io::{read_raw, write_raw};
use crate::checksum;
use crate::error::{CafsError, CafsResult};
use crate::layout::*;
use crate::{CafsCtx, CheckResult, MountMode};

fn superblock_checksum_ok(sb: &[u8; REAL_BLOCK_4K as usize]) -> bool {
    let computed = checksum::blake3_128(&sb[0..SB_HASHED_LEN]);
    computed == sb[SB_OFF_CHECKSUM..SB_OFF_CHECKSUM + SB_CHECKSUM_LEN]
}

impl CafsCtx {
    fn can_write(&self) -> bool {
        self.want_write && !self.opts.readonly
    }

    pub fn check(&mut self) -> CafsResult<CheckResult> {
        if self.mode == MountMode::Bypass {
            return Err(CafsError::Bypassed);
        }
        let mut result = CheckResult {
            pointer_checksum_algo_used: Some(self.ptr_algo),
            ..Default::default()
        };
        self.check_pointer_blocks(&mut result);
        self.check_superblock(&mut result)?;
        Ok(result)
    }

    /// Rebuild the failed end of `r` from its intact end. `None` if
    /// the intact end lacks what is needed (slot 0 needs both
    /// Superblock offsets to write the tail copy).
    fn rebuild_ptr_block(r: &SlotReport, bad: Side) -> Option<Vec<u8>> {
        let (good, algo) = r.chosen().or(r.head.as_ref().ok()).or(r.tail.as_ref().ok())?;
        let len = slot_block_size(r.slot);
        if r.slot == 0 {
            let (primary, backup) = r.sb_pair()?;
            let backup = backup?;
            let sz = REAL_BLOCK_4K as u64;
            return Some(match bad {
                Side::Head => build_ptr_block(len, *algo, primary.0, sz, Some((backup.0, sz))),
                Side::Tail => build_ptr_block(len, *algo, backup.0, sz, Some((primary.0, sz))),
            });
        }
        Some(build_ptr_block(
            len,
            *algo,
            good.real_offset.0,
            good.real_size,
            good.fallback.map(|(o, s)| (o.0, s)),
        ))
    }

    fn check_pointer_blocks(&mut self, result: &mut CheckResult) {
        let size = self.dev.size();
        let can_write = self.can_write();
        let mut repaired = 0u8;

        for r in self.anchors.clone() {
            if r.disagree() {
                result.anchor_disagreements += 1;
                eprintln!("cafs: anchor slot {} verifies at both ends but they point at different places; head is used", r.slot);
            }
            let (hg, tg) = (r.head_good(), r.tail_good());
            if hg && tg {
                continue;
            }
            // Slot 0 can be recovered from partial information at
            // both ends as long as both Superblock offsets are known.
            let recoverable = if r.slot == 0 {
                matches!(r.sb_pair(), Some((_, Some(_))))
            } else {
                r.head_ok() || r.tail_ok()
            };
            if !recoverable {
                result.anchor_slots_lost += 1;
                eprintln!("cafs: anchor slot {} cannot be recovered from either end of the device", r.slot);
                continue;
            }
            result.anchor_slots_degraded += 1;
            for bad in [Side::Head, Side::Tail] {
                if (bad == Side::Head && hg) || (bad == Side::Tail && tg) {
                    continue;
                }
                if !can_write {
                    eprintln!("cafs: anchor slot {} bad at the {:?} end; recoverable, but the mount is readonly, so not rewritten", r.slot, bad);
                    continue;
                }
                let Some(raw) = Self::rebuild_ptr_block(&r, bad) else {
                    eprintln!("cafs: anchor slot {} bad at the {:?} end and cannot be rebuilt", r.slot, bad);
                    continue;
                };
                let off = match bad {
                    Side::Head => head_slot_offset(r.slot),
                    Side::Tail => tail_slot_offset(r.slot, size),
                };
                match write_raw(self, off, &raw).and_then(|_| self.dev.sync()) {
                    Ok(()) => {
                        repaired += 1;
                        eprintln!("cafs: rebuilt anchor slot {} at the {:?} end (offset {}) from the intact end", r.slot, bad, off.0);
                    }
                    Err(e) => eprintln!("cafs: failed to rewrite anchor slot {} at the {:?} end: {e}", r.slot, bad),
                }
            }
        }

        // `anchor_slots_repaired` counts rewritten pointer blocks (ends), not slots.
        result.anchor_slots_repaired = repaired;
        if repaired > 0 {
            self.anchors = self.scan_anchors();
        }
    }

    fn check_superblock(&mut self, result: &mut CheckResult) -> CafsResult<()> {
        if superblock_checksum_ok(&self.superblock) {
            result.superblock_ok = true;
            return Ok(());
        }

        // Primary is bad — try the backup.
        let Some(backup_offset) = self.sb_backup else {
            eprintln!("cafs: primary Superblock checksum mismatch, and no backup pointer is set — unrecoverable");
            return Ok(()); // superblock_ok stays false
        };

        let mut backup = [0u8; REAL_BLOCK_4K as usize];
        if read_raw(self, backup_offset, &mut backup).is_err() || !superblock_checksum_ok(&backup) {
            eprintln!(
                "cafs: primary Superblock checksum mismatch, and Superblock Backup at offset {} is also bad or unreadable — unrecoverable",
                backup_offset.0
            );
            return Ok(());
        }

        // Backup is good. Adopt it in memory regardless of write
        // access, so the rest of this session sees correct data even
        // on a readonly mount.
        self.superblock = backup;
        result.superblock_ok = true;

        if self.can_write() {
            let (sb, at) = (self.superblock, self.sb_primary);
            write_raw(self, at, &sb)?;
            self.dev.sync()?;
            eprintln!(
                "cafs: repaired primary Superblock at offset {} from Superblock Backup at offset {}",
                at.0, backup_offset.0
            );
            result.superblock_repaired = true;
        } else {
            eprintln!(
                "cafs: primary Superblock checksum mismatch — valid backup found, but mount is readonly, so the fix was not written back"
            );
        }
        Ok(())
    }
}
