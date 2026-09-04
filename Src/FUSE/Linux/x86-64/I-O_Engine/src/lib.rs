//! CAFS I/O Engine (V1), Rust rewrite — see ADR-011/012.
//!
//! Scope unchanged from the retired C engine: raw anchor-region mount
//! lifecycle (mount -> check -> remount -> confirm) plus generic
//! block read/write. No allocator, no filesystem semantics.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

mod anchor;
mod checksum;
pub mod layout;

mod block_io;
mod check;
mod confirm;
mod mount;
mod remount;

pub use error::{CafsError, CafsResult};
pub use layout::ByteOffset;
pub use anchor::PtrAlgo;

mod error;

use anchor::PtrBlock;
use layout::{REAL_BLOCK_4K, SMART_TABLE_SIZE};
use rustix::fd::OwnedFd;

/// Per-mount runtime options — maps to config.fs's `[mount_hints]`/
/// `[io]` sections (both explicitly runtime-only, not in the Config
/// Snapshot per fs.info §5). This engine never reads a host-side
/// config.fs file itself; the caller resolves args + local config
/// into this struct before calling [`CafsCtx::remount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MisalignedAction {
    #[default]
    Copy,
    Reject,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MountOpts {
    pub readonly: bool,
    pub direct_io: bool,
    pub direct_io_required: bool,
    pub sync_open: bool,
    pub misaligned_action: MisalignedAction,
    pub destroy_flush: bool,
}

/// Crash-critical fields read out of the Config Snapshot's TOML text.
/// Extracted by a minimal targeted key scanner, not a general TOML
/// parser (see `remount.rs`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfigSnapshot {
    pub block_size: Option<u32>,
    pub pointer_checksum_algo_id: Option<u32>,
    pub data_checksum_algo_id: Option<u32>,
    pub verify_data: Option<bool>,
}

/// `check()` result: Superblock only, per ADR-007. All other anchor
/// structures are read best-effort at mount() but neither verified
/// nor repaired here.
#[derive(Debug, Clone, Copy, Default)]
pub struct CheckResult {
    pub superblock_ok: bool,
    pub superblock_repaired: bool,
    pub pointer_checksum_algo_used: Option<PtrAlgo>,
}

/// In-RAM-only per-session telemetry — mirrors the on-disk SMART
/// table's counter shape (fs.info §11.1) so a future full SMART
/// Handler integration is a drop-in. `confirm()` writes mount_count/
/// clean_unmount_flag directly to M-SMART (ADR-008); it does not
/// flush these counters anywhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct SmartCounters {
    pub total_reads: u64,
    pub total_writes: u64,
    pub total_read_errors: u32,
    pub total_write_errors: u32,
}

/// A mounted CAFS device. Not `Clone`/`Copy` — owns the device file
/// descriptor.
pub struct CafsCtx {
    pub(crate) fd: OwnedFd,
    pub(crate) want_write: bool,
    #[allow(dead_code)]
    pub(crate) device_size: u64,

    pub(crate) lba0: PtrBlock,
    pub(crate) lba0_algo: crate::anchor::PtrAlgo,
    pub(crate) parity_offset: ByteOffset,

    pub(crate) superblock: [u8; REAL_BLOCK_4K as usize],
    pub(crate) config_snapshot: Option<[u8; REAL_BLOCK_4K as usize]>,
    pub(crate) function_table: Vec<u8>,
    pub(crate) smart_main: Option<([u8; SMART_TABLE_SIZE], ByteOffset)>,

    pub(crate) opts: MountOpts,
    pub(crate) smart: SmartCounters,
}

impl CafsCtx {
    pub fn smart_counters(&self) -> SmartCounters {
        self.smart
    }

    /// Not one of the originally-named four stages — added because
    /// `confirm()` only ever marks a mount as started; without a
    /// counterpart, `destroy_flush` never runs. Consumes `self`: the
    /// borrow checker makes using `ctx` after `close()` a compile
    /// error, not a runtime bug — a class of mistake the C version's
    /// `cafs_close()` had no way to prevent.
    pub fn close(self) -> CafsResult<()> {
        if self.opts.destroy_flush && self.want_write {
            rustix::fs::fsync(&self.fd)?;
        }
        // fd closes automatically on drop (OwnedFd RAII) — no leak
        // even if a caller never calls close() at all.
        Ok(())
    }
}
