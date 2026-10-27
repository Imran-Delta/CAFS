//! `CafsCtx::confirm` — marks the mount as started in M-SMART:
//! `mount_count += 1`, `clean_unmount_flag = 0`, `last_mount_time =
//! now`; recomputes the table's BLAKE3-128, writes it, syncs. Best-
//! effort, never fatal (ADR-008): a missing/unwritable SMART table does
//! not fail the call.
//!
//! TEMPORARY (2026-10-27): `last_mount_time` is no longer written to
//! the Superblock. The Superblock, its backup and the Parity Block
//! therefore stay untouched on mount (fs.info §13.3 does not run).
//! `last_mount_time` lives in M-SMART at `SMART_OFF_LAST_MOUNT_TIME`
//! (reserved space in §11.1) until the decision is revisited.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::time::{SystemTime, UNIX_EPOCH};

use crate::block_io::write_raw;
use crate::checksum;
use crate::error::{CafsError, CafsResult};
use crate::layout::*;
use crate::{CafsCtx, MountMode};

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl CafsCtx {
    pub fn confirm(&mut self) -> CafsResult<()> {
        if self.mode == MountMode::Bypass {
            return Err(CafsError::Bypassed);
        }
        if self.opts.readonly || !self.want_write {
            return Err(CafsError::Readonly);
        }

        let Some((mut smart, offset)) = self.smart_main else {
            return Ok(());
        };
        let mount_count = u32::from_le_bytes(
            smart[SMART_OFF_MOUNT_COUNT..SMART_OFF_MOUNT_COUNT + 4].try_into().unwrap(),
        );
        smart[SMART_OFF_MOUNT_COUNT..SMART_OFF_MOUNT_COUNT + 4]
            .copy_from_slice(&mount_count.wrapping_add(1).to_le_bytes());
        smart[SMART_OFF_CLEAN_UNMOUNT_FLAG] = 0;
        smart[SMART_OFF_LAST_MOUNT_TIME..SMART_OFF_LAST_MOUNT_TIME + 8]
            .copy_from_slice(&now_unix().to_le_bytes());
        let sc = checksum::blake3_128(&smart[0..SMART_HASHED_LEN]);
        smart[SMART_OFF_CHECKSUM..SMART_OFF_CHECKSUM + SMART_CHECKSUM_LEN].copy_from_slice(&sc);

        if let Err(e) = write_raw(self, offset, &smart) {
            eprintln!("cafs: confirm(): failed to update M-SMART at offset {}: {e}", offset.0);
        } else {
            if let Err(e) = self.dev.sync() {
                eprintln!("cafs: confirm(): fdatasync after M-SMART write failed: {e}");
            }
            self.smart_main = Some((smart, offset));
        }
        Ok(())
    }
}
