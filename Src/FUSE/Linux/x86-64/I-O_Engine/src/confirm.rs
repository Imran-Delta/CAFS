//! `CafsCtx::confirm` — sets Superblock `last_mount_time = now`,
//! recomputes its BLAKE3-128 checksum, and commits it via fs.info
//! §13.3 (In-Place Content Update): primary Superblock, then
//! Superblock Backup (via LBA0's fallback), then the primary Parity
//! Block (XOR of the new Superblock and the cached Config Snapshot).
//! Each write is followed by `fdatasync()`. There is no Backup Parity
//! Block to keep in sync — Parity has no backup under the revised
//! redundancy design (ADR-006).
//!
//! Also increments `mount_count` and sets `clean_unmount_flag = 0` on
//! M-SMART directly, best-effort (ADR-008) — a missing/unavailable
//! SMART table doesn't fail the call. Only the primary Superblock
//! write is fatal to `confirm()`.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::time::{SystemTime, UNIX_EPOCH};

use crate::block_io::write_raw;
use crate::checksum;
use crate::error::{CafsError, CafsResult};
use crate::layout::*;
use crate::CafsCtx;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl CafsCtx {
    pub fn confirm(&mut self) -> CafsResult<()> {
        if self.opts.readonly || !self.want_write {
            return Err(CafsError::Readonly);
        }

        // 1. Primary Superblock: last_mount_time = now, recompute
        // checksum, write, fsync.
        let now = now_unix();
        self.superblock[SB_OFF_LAST_MOUNT_TIME..SB_OFF_LAST_MOUNT_TIME + 8]
            .copy_from_slice(&now.to_le_bytes());
        let cksum = checksum::blake3_128(&self.superblock[0..SB_HASHED_LEN]);
        self.superblock[SB_OFF_CHECKSUM..SB_OFF_CHECKSUM + SB_CHECKSUM_LEN].copy_from_slice(&cksum);

        write_raw(self, self.lba0.real_offset, &self.superblock.clone())?;
        rustix::fs::fdatasync(&self.fd)?;

        // 2. Superblock Backup, same content — best-effort, logged
        // and non-fatal on failure (ADR-007/008 posture: only the
        // primary write is load-bearing for the call to succeed).
        if let Some((backup_offset, _)) = self.lba0.fallback {
            if let Err(e) = write_raw(self, backup_offset, &self.superblock.clone()) {
                eprintln!("cafs: confirm(): failed to update Superblock Backup at offset {}: {e}", backup_offset.0);
            } else if let Err(e) = rustix::fs::fdatasync(&self.fd) {
                eprintln!("cafs: confirm(): fdatasync after Superblock Backup write failed: {e}");
            }
        }

        // 2.5. Primary Parity Block = XOR(new Superblock, cached
        // Config Snapshot) — only possible if Config Snapshot was
        // successfully read at mount() (best-effort, ADR-007).
        if let Some(cfg) = &self.config_snapshot {
            let mut parity = [0u8; REAL_BLOCK_4K as usize];
            for i in 0..REAL_BLOCK_4K as usize {
                parity[i] = self.superblock[i] ^ cfg[i];
            }
            if let Err(e) = write_raw(self, self.parity_offset, &parity) {
                eprintln!("cafs: confirm(): failed to update Parity Block at offset {}: {e}", self.parity_offset.0);
            } else if let Err(e) = rustix::fs::fdatasync(&self.fd) {
                eprintln!("cafs: confirm(): fdatasync after Parity Block write failed: {e}");
            }
        }

        // M-SMART: best-effort, never fatal.
        if let Some((mut smart, offset)) = self.smart_main {
            let mount_count = u32::from_le_bytes(
                smart[SMART_OFF_MOUNT_COUNT..SMART_OFF_MOUNT_COUNT + 4].try_into().unwrap(),
            );
            smart[SMART_OFF_MOUNT_COUNT..SMART_OFF_MOUNT_COUNT + 4]
                .copy_from_slice(&mount_count.wrapping_add(1).to_le_bytes());
            smart[SMART_OFF_CLEAN_UNMOUNT_FLAG] = 0;
            let sc = checksum::blake3_128(&smart[0..SMART_HASHED_LEN]);
            smart[SMART_OFF_CHECKSUM..SMART_OFF_CHECKSUM + SMART_CHECKSUM_LEN].copy_from_slice(&sc);

            if let Err(e) = write_raw(self, offset, &smart) {
                eprintln!("cafs: confirm(): failed to update M-SMART at offset {}: {e}", offset.0);
            } else {
                if let Err(e) = rustix::fs::fdatasync(&self.fd) {
                    eprintln!("cafs: confirm(): fdatasync after M-SMART write failed: {e}");
                }
                self.smart_main = Some((smart, offset));
            }
        }

        Ok(())
    }
}
