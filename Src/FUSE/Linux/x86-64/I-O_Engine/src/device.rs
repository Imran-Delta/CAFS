//! Device seam: everything the engine does to storage goes through
//! this trait, so the same engine runs against a real file/block
//! device ([`FileDevice`]) or a fault-injecting in-memory one
//! ([`crate::fault::FaultDevice`]).
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use rustix::fd::OwnedFd;
use rustix::fs::{self, Mode, OFlags};
use rustix::io::Errno;

use crate::error::{CafsError, CafsResult};

/// Byte-addressed storage. No alignment or bounds policy lives here;
/// that is the engine's job. `read_at` fills `buf` completely or
/// fails; `write_at` writes all of `buf` or fails.
pub trait Device: Send {
    fn size(&self) -> u64;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> CafsResult<()>;
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> CafsResult<()>;
    /// Durability barrier (`fdatasync` on a real device).
    fn sync(&mut self) -> CafsResult<()>;
    /// Apply runtime I/O flags (`O_DIRECT`, `O_SYNC`). Semantics of a
    /// real device are whatever the kernel does; see `remount.rs`.
    fn apply_flags(&mut self, direct: bool, sync: bool, direct_required: bool) -> CafsResult<()>;
}

const BLKGETSIZE64: u32 = 0x8008_1272;

fn device_size(fd: &OwnedFd) -> CafsResult<u64> {
    // std::fs::File::metadata().len() would also work for a regular
    // file, but silently returns a *wrong* number on an actual block
    // device (it reports 0 or the backing inode's size, not device
    // capacity) — BLKGETSIZE64 is the correct call for a device.
    // rustix has no named wrapper for it yet; this is the one place
    // in the engine that still needs `unsafe`, isolated and
    // documented, per rustix's own generic-ioctl pattern.
    let result: Result<u64, Errno> = unsafe {
        let ctl = rustix::ioctl::Getter::<BLKGETSIZE64, u64>::new();
        rustix::ioctl::ioctl(fd, ctl)
    };
    match result {
        Ok(v) => Ok(v),
        // ENOTTY: not a block device (e.g. a regular file test
        // image) — fall back to the file's actual length.
        Err(Errno::NOTTY) => {
            let stat = fs::fstat(fd).map_err(CafsError::from)?;
            Ok(stat.st_size as u64)
        }
        Err(e) => Err(CafsError::from(e)),
    }
}

/// A regular file or block device opened through rustix.
pub struct FileDevice {
    fd: OwnedFd,
    size: u64,
}

impl FileDevice {
    pub fn open(path: &str, want_write: bool) -> CafsResult<Self> {
        let mut flags = if want_write { OFlags::RDWR } else { OFlags::RDONLY };
        // A block device opened for writing is claimed exclusively:
        // the kernel then refuses (EBUSY) if it is mounted, in use as
        // swap, or held by another driver. Not applied to regular
        // files, or to read-only opens (inspection of a live volume).
        let is_blk = fs::stat(path).is_ok_and(|st| fs::FileType::from_raw_mode(st.st_mode) == fs::FileType::BlockDevice);
        if want_write && is_blk {
            flags |= OFlags::EXCL;
        }
        let fd = fs::open(path, flags, Mode::empty()).map_err(|_| CafsError::Open)?;
        let size = device_size(&fd)?;
        Ok(FileDevice { fd, size })
    }
}

impl FileDevice {
    /// (logical, physical) sector size in bytes from the block-device
    /// ioctls; `None` for a regular file or if the kernel refuses.
    pub fn sector_sizes(&self) -> (Option<u32>, Option<u32>) {
        const BLKSSZGET: u32 = 0x1268;
        const BLKPBSZGET: u32 = 0x127B;
        // SAFETY: both ioctls write one C int through the pointer
        // rustix's Getter supplies; same pattern as BLKGETSIZE64 above.
        let logical: Result<i32, Errno> = unsafe { rustix::ioctl::ioctl(&self.fd, rustix::ioctl::Getter::<BLKSSZGET, i32>::new()) };
        let physical: Result<i32, Errno> = unsafe { rustix::ioctl::ioctl(&self.fd, rustix::ioctl::Getter::<BLKPBSZGET, i32>::new()) };
        (logical.ok().map(|v| v as u32), physical.ok().map(|v| v as u32))
    }
}

impl Device for FileDevice {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> CafsResult<()> {
        let mut got = 0usize;
        while got < buf.len() {
            match rustix::io::pread(&self.fd, &mut buf[got..], offset + got as u64) {
                Ok(0) => return Err(CafsError::Io), // unexpected EOF against a fixed-size structure
                Ok(n) => got += n,
                Err(Errno::INTR) => continue,
                Err(_) => return Err(CafsError::Io),
            }
        }
        Ok(())
    }

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> CafsResult<()> {
        let mut sent = 0usize;
        while sent < buf.len() {
            match rustix::io::pwrite(&self.fd, &buf[sent..], offset + sent as u64) {
                Ok(n) => sent += n,
                Err(Errno::INTR) => continue,
                Err(_) => return Err(CafsError::Io),
            }
        }
        Ok(())
    }

    fn sync(&mut self) -> CafsResult<()> {
        fs::fdatasync(&self.fd)?;
        Ok(())
    }

    fn apply_flags(&mut self, direct: bool, sync: bool, direct_required: bool) -> CafsResult<()> {
        let mut flags = fs::fcntl_getfl(&self.fd)?;
        flags.set(OFlags::DIRECT, direct);
        flags.set(OFlags::SYNC, sync);
        fs::fcntl_setfl(&self.fd, flags)?;

        if direct && direct_required {
            let now = fs::fcntl_getfl(&self.fd)?;
            if !now.contains(OFlags::DIRECT) {
                return Err(CafsError::Unsupported);
            }
        }
        Ok(())
    }
}
