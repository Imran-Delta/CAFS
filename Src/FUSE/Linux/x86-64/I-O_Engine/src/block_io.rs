//! Raw device read/write (looping over rustix's pread/pwrite to
//! handle short reads/writes and EINTR), and the public
//! `read_block`/`write_block` methods with alignment handling and
//! in-RAM SMART counters.
//!
//! Alignment: "aligned" means offset and size are both multiples of
//! 512 bytes — the universal minimum sector size, satisfied by every
//! fixed anchor offset this engine itself uses, and a safe floor for
//! O_DIRECT regardless of the underlying device's actual logical
//! sector size. `misaligned_action=copy` bounces an unaligned request
//! through a correctly-sized, 512-aligned heap buffer; `=reject`
//! fails it outright.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use rustix::io::Errno;

use crate::error::{CafsError, CafsResult};
use crate::{ByteOffset, CafsCtx, MisalignedAction};

const ALIGN: u64 = 512;

pub(crate) fn read_raw(ctx: &CafsCtx, offset: ByteOffset, buf: &mut [u8]) -> CafsResult<()> {
    let mut got = 0usize;
    while got < buf.len() {
        match rustix::io::pread(&ctx.fd, &mut buf[got..], offset.0 + got as u64) {
            Ok(0) => return Err(CafsError::Io), // unexpected EOF against a fixed-size structure
            Ok(n) => got += n,
            Err(Errno::INTR) => continue,
            Err(_) => return Err(CafsError::Io),
        }
    }
    Ok(())
}

pub(crate) fn write_raw(ctx: &CafsCtx, offset: ByteOffset, buf: &[u8]) -> CafsResult<()> {
    if !ctx.want_write {
        return Err(CafsError::Readonly);
    }
    let mut sent = 0usize;
    while sent < buf.len() {
        match rustix::io::pwrite(&ctx.fd, &buf[sent..], offset.0 + sent as u64) {
            Ok(n) => sent += n,
            Err(Errno::INTR) => continue,
            Err(_) => return Err(CafsError::Io),
        }
    }
    Ok(())
}

fn is_aligned(offset: ByteOffset, len: usize) -> bool {
    offset.0.is_multiple_of(ALIGN) && (len as u64).is_multiple_of(ALIGN)
}

impl CafsCtx {
    pub fn read_block(&mut self, offset: ByteOffset, buf: &mut [u8]) -> CafsResult<()> {
        if buf.is_empty() {
            return Err(CafsError::InvalidArg);
        }

        let result = if is_aligned(offset, buf.len()) {
            read_raw(self, offset, buf)
        } else if self.opts.misaligned_action == MisalignedAction::Reject {
            Err(CafsError::Misaligned)
        } else {
            let aligned_off = ByteOffset((offset.0 / ALIGN) * ALIGN);
            let end = offset.0 + buf.len() as u64;
            let aligned_end = end.div_ceil(ALIGN) * ALIGN;
            let mut bounce = vec![0u8; (aligned_end - aligned_off.0) as usize];
            read_raw(self, aligned_off, &mut bounce).map(|_| {
                let start = (offset.0 - aligned_off.0) as usize;
                buf.copy_from_slice(&bounce[start..start + buf.len()]);
            })
        };

        self.smart.total_reads += 1;
        if result.is_err() {
            self.smart.total_read_errors += 1;
        }
        result
    }

    pub fn write_block(&mut self, offset: ByteOffset, buf: &[u8]) -> CafsResult<()> {
        if buf.is_empty() {
            return Err(CafsError::InvalidArg);
        }
        if self.opts.readonly {
            return Err(CafsError::Readonly);
        }

        let result = if is_aligned(offset, buf.len()) {
            write_raw(self, offset, buf)
        } else if self.opts.misaligned_action == MisalignedAction::Reject {
            Err(CafsError::Misaligned)
        } else {
            // Bounce through an aligned buffer: read-modify-write,
            // since the aligned range extends beyond what the caller
            // supplied and those extra bytes must not be clobbered.
            let aligned_off = ByteOffset((offset.0 / ALIGN) * ALIGN);
            let end = offset.0 + buf.len() as u64;
            let aligned_end = end.div_ceil(ALIGN) * ALIGN;
            let mut bounce = vec![0u8; (aligned_end - aligned_off.0) as usize];
            read_raw(self, aligned_off, &mut bounce).and_then(|_| {
                let start = (offset.0 - aligned_off.0) as usize;
                bounce[start..start + buf.len()].copy_from_slice(buf);
                write_raw(self, aligned_off, &bounce)
            })
        };

        self.smart.total_writes += 1;
        if result.is_err() {
            self.smart.total_write_errors += 1;
        }
        result
    }
}
