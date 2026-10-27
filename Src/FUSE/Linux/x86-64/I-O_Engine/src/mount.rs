//! `CafsCtx::mount` — opens the device and reads the anchor region.
//! Each of LBA0-5 is read from the head AND from the end-of-device
//! mirror (fs.info §2, §12); a slot whose head copy fails verification
//! is served from the tail copy (§14 Track A, step 2). The real
//! Superblock is then read raw (checksum verification is `check()`'s
//! job), followed by best-effort Config Snapshot / Function Table /
//! M-SMART. No writes. LBA3 (Meta/User Pointer Table) is not
//! interpreted — delegated upward.
//!
//! `MountMode::Bypass` opens the device and stops there.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use crate::anchor::{verify_ptr_block, PtrAlgo, PtrBlock, SlotReport};
use crate::block_io::read_raw;
use crate::device::{Device, FileDevice};
use crate::error::{CafsError, CafsResult};
use crate::layout::*;
use crate::{CafsCtx, MountMode, MountOpts, SmartCounters};

fn read_ptr(ctx: &mut CafsCtx, off: ByteOffset, len: usize) -> CafsResult<(PtrBlock, PtrAlgo)> {
    let mut raw = vec![0u8; len];
    read_raw(ctx, off, &mut raw)?;
    verify_ptr_block(&raw)
}

impl CafsCtx {
    pub fn mount(path: &str, want_write: bool) -> CafsResult<Self> {
        Self::mount_with(path, want_write, MountMode::Full)
    }

    pub fn mount_with(path: &str, want_write: bool, mode: MountMode) -> CafsResult<Self> {
        let dev = FileDevice::open(path, want_write)?;
        Self::mount_device_with(Box::new(dev), want_write, mode)
    }

    /// Mount over any [`Device`] (a real file/block device, or a
    /// fault-injecting in-memory one).
    pub fn mount_device(dev: Box<dyn Device>, want_write: bool) -> CafsResult<Self> {
        Self::mount_device_with(dev, want_write, MountMode::Full)
    }

    pub fn mount_device_with(dev: Box<dyn Device>, want_write: bool, mode: MountMode) -> CafsResult<Self> {
        let size = dev.size();
        if size < 2 * ANCHOR_REGION_BYTES {
            return Err(CafsError::TooSmall);
        }

        let mut ctx = CafsCtx {
            dev,
            want_write,
            mode,
            device_size: size,
            ptr_algo: PtrAlgo::Crc32c, // overwritten below before any read of a real structure
            anchors: Vec::new(),
            sb_primary: ByteOffset(0),
            sb_backup: None,
            parity_offset: ByteOffset(0),
            superblock: [0u8; REAL_BLOCK_4K as usize],
            config_snapshot: None,
            function_table: Vec::new(),
            smart_main: None,
            opts: MountOpts::default(),
            smart: SmartCounters::default(),
        };

        if mode == MountMode::Bypass {
            return Ok(ctx);
        }

        ctx.anchors = ctx.scan_anchors();

        // LBA0 is the trust root: without a verified pointer to the
        // Superblock from EITHER end of the device, nothing else can
        // proceed. This is the one slot whose loss is fatal to mount().
        let Some((sb_primary, sb_backup)) = ctx.anchors[0].sb_pair() else {
            return Err(ctx.anchors[0].head.as_ref().err().copied().unwrap_or(CafsError::BadMagic));
        };
        ctx.ptr_algo = ctx.anchors[0].chosen().map(|c| c.1).unwrap_or(PtrAlgo::Crc32c);
        ctx.sb_primary = sb_primary;
        ctx.sb_backup = sb_backup;

        let mut sb = [0u8; REAL_BLOCK_4K as usize];
        read_raw(&mut ctx, sb_primary, &mut sb)?;
        ctx.superblock = sb;

        // Everything past here is best-effort: a failed read just
        // leaves the corresponding field unpopulated. It never
        // aborts the mount (ADR-007: Superblock-only scope for V1).
        let _ = Self::try_read_parity_offset(&mut ctx);
        let _ = Self::try_read_config_snapshot(&mut ctx);
        let function_table_ok = Self::try_read_function_table(&mut ctx).is_ok();
        if function_table_ok {
            let _ = Self::try_read_smart_main(&mut ctx);
        }

        Ok(ctx)
    }

    /// Read and verify all twelve pointer blocks (six slots x two
    /// ends). Works in any mount mode and never fails as a whole: a
    /// block that cannot be read or verified is reported per side.
    pub fn scan_anchors(&mut self) -> Vec<SlotReport> {
        let size = self.dev.size();
        let mut out = Vec::with_capacity(SLOT_COUNT);
        for slot in 0..SLOT_COUNT as u8 {
            let len = slot_block_size(slot) as usize;
            let head = read_ptr(self, head_slot_offset(slot), len);
            let tail = read_ptr(self, tail_slot_offset(slot, size), len);
            out.push(SlotReport { slot, head, tail });
        }
        out
    }

    fn try_read_parity_offset(ctx: &mut CafsCtx) -> CafsResult<()> {
        let (b, _) = ctx.anchors[2].chosen().ok_or(CafsError::Checksum)?.clone();
        ctx.parity_offset = b.real_offset;
        Ok(())
    }

    fn try_read_config_snapshot(ctx: &mut CafsCtx) -> CafsResult<()> {
        let (b, _) = ctx.anchors[1].chosen().ok_or(CafsError::Checksum)?.clone();
        let mut cfg = [0u8; REAL_BLOCK_4K as usize];
        read_raw(ctx, b.real_offset, &mut cfg)?;
        ctx.config_snapshot = Some(cfg);
        Ok(())
    }

    fn try_read_function_table(ctx: &mut CafsCtx) -> CafsResult<()> {
        let (b, _) = ctx.anchors[4].chosen().ok_or(CafsError::Checksum)?.clone();
        if b.real_size < FT_MIN_REAL_SIZE {
            return Err(CafsError::TooSmall);
        }
        let mut ft = vec![0u8; b.real_size as usize];
        read_raw(ctx, b.real_offset, &mut ft)?;
        ctx.function_table = ft;
        Ok(())
    }

    fn try_read_smart_main(ctx: &mut CafsCtx) -> CafsResult<()> {
        if ctx.function_table.len() < FT_MIN_REAL_SIZE as usize {
            return Err(CafsError::TooSmall);
        }
        let smart_lba = u64::from_le_bytes(
            ctx.function_table[FT_OFF_SMART_MAIN_LBA..FT_OFF_SMART_MAIN_LBA + 8]
                .try_into()
                .unwrap(),
        );
        if smart_lba == 0 {
            return Err(CafsError::InvalidArg);
        }
        let offset = ByteOffset(smart_lba);
        let mut buf = [0u8; SMART_TABLE_SIZE];
        read_raw(ctx, offset, &mut buf)?;
        ctx.smart_main = Some((buf, offset));
        Ok(())
    }
}
