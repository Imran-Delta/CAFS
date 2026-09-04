//! `CafsCtx::mount` — opens the device and reads the anchor region:
//! LBA0-2/4 pointer blocks, then the real Superblock (raw), and
//! best-effort Config Snapshot / Function Table / M-SMART. No
//! Superblock checksum verification here (that's `check()`'s job)
//! and no writes. LBA3 (Meta/User Pointer Table) is not read —
//! delegated upward, out of scope for this library.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use rustix::fs::{self, Mode, OFlags};

use crate::anchor::verify_ptr_block;
use crate::block_io::read_raw;
use crate::error::{CafsError, CafsResult};
use crate::layout::*;
use crate::{CafsCtx, MountOpts, SmartCounters};

const BLKGETSIZE64: u32 = 0x8008_1272;

fn device_size(fd: &rustix::fd::OwnedFd) -> CafsResult<u64> {
    // std::fs::File::metadata().len() would also work for a regular
    // file, but silently returns a *wrong* number on an actual block
    // device (it reports 0 or the backing inode's size, not device
    // capacity) — BLKGETSIZE64 is the correct call for a device.
    // rustix has no named wrapper for it yet; this is the one place
    // in the engine that still needs `unsafe`, isolated and
    // documented, per rustix's own generic-ioctl pattern.
    let result: Result<u64, rustix::io::Errno> = unsafe {
        let ctl = rustix::ioctl::Getter::<BLKGETSIZE64, u64>::new();
        rustix::ioctl::ioctl(fd, ctl)
    };
    match result {
        Ok(v) => Ok(v),
        // ENOTTY: not a block device (e.g. a regular file test
        // image) — fall back to the file's actual length.
        Err(rustix::io::Errno::NOTTY) => {
            let stat = fs::fstat(fd).map_err(CafsError::from)?;
            Ok(stat.st_size as u64)
        }
        Err(e) => Err(CafsError::from(e)),
    }
}

impl CafsCtx {
    pub fn mount(path: &str, want_write: bool) -> CafsResult<Self> {
        let flags = if want_write { OFlags::RDWR } else { OFlags::RDONLY };
        let fd = fs::open(path, flags, Mode::empty()).map_err(|_| CafsError::Open)?;

        let size = device_size(&fd)?;
        if size < ANCHOR_REGION_BYTES {
            return Err(CafsError::TooSmall);
        }

        let mut ctx = CafsCtx {
            fd,
            want_write,
            device_size: size,
            lba0: crate::anchor::PtrBlock {
                real_offset: ByteOffset(0),
                real_size: 0,
                fallback: None,
            },
            lba0_algo: crate::anchor::PtrAlgo::Crc32c, // overwritten below before any read happens
            parity_offset: ByteOffset(0),
            superblock: [0u8; REAL_BLOCK_4K as usize],
            config_snapshot: None,
            function_table: Vec::new(),
            smart_main: None,
            opts: MountOpts::default(),
            smart: SmartCounters::default(),
        };

        // LBA0 is the trust root: without a verified pointer to the
        // Superblock, nothing else can proceed. This is the one
        // pointer-block checksum failure that's fatal to mount().
        let mut lba0_raw = vec![0u8; PTR_BLOCK_SMALL_SIZE as usize];
        read_raw(&ctx, LBA0_OFFSET, &mut lba0_raw)?;
        let (lba0, algo) = verify_ptr_block(&lba0_raw)?;
        ctx.lba0 = lba0.clone();
        ctx.lba0_algo = algo;

        let mut sb = [0u8; REAL_BLOCK_4K as usize];
        read_raw(&ctx, lba0.real_offset, &mut sb)?;
        ctx.superblock = sb;

        // Everything past here is best-effort: a failed read just
        // leaves the corresponding field unpopulated. It never
        // aborts the mount (ADR-007: Superblock-only scope for V1).
        let _ = Self::try_read_lba2_parity_offset(&mut ctx);
        let _ = Self::try_read_lba1_config_snapshot(&mut ctx);
        let function_table_ok = Self::try_read_lba4_function_table(&mut ctx).is_ok();
        if function_table_ok {
            let _ = Self::try_read_smart_main(&mut ctx);
        }

        Ok(ctx)
    }

    fn try_read_lba2_parity_offset(ctx: &mut CafsCtx) -> CafsResult<()> {
        let mut raw = vec![0u8; PTR_BLOCK_SMALL_SIZE as usize];
        read_raw(ctx, LBA2_OFFSET, &mut raw)?;
        let (lba2, _algo) = verify_ptr_block(&raw)?;
        ctx.parity_offset = lba2.real_offset;
        Ok(())
    }

    fn try_read_lba1_config_snapshot(ctx: &mut CafsCtx) -> CafsResult<()> {
        let mut raw = vec![0u8; PTR_BLOCK_SMALL_SIZE as usize];
        read_raw(ctx, LBA1_OFFSET, &mut raw)?;
        let (lba1, _algo) = verify_ptr_block(&raw)?;
        let mut cfg = [0u8; REAL_BLOCK_4K as usize];
        read_raw(ctx, lba1.real_offset, &mut cfg)?;
        ctx.config_snapshot = Some(cfg);
        Ok(())
    }

    fn try_read_lba4_function_table(ctx: &mut CafsCtx) -> CafsResult<()> {
        let mut raw = vec![0u8; PTR_BLOCK_LARGE_SIZE as usize];
        read_raw(ctx, LBA4_OFFSET, &mut raw)?;
        let (lba4, _algo) = verify_ptr_block(&raw)?;
        if lba4.real_size < FT_MIN_REAL_SIZE {
            return Err(CafsError::TooSmall);
        }
        let mut ft = vec![0u8; lba4.real_size as usize];
        read_raw(ctx, lba4.real_offset, &mut ft)?;
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
