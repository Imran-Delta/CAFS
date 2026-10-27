//! CAFS I/O Engine (V1), Rust rewrite — see ADR-011/012.
//!
//! Scope unchanged from the retired C engine: raw anchor-region mount
//! lifecycle (mount -> check -> remount -> confirm) plus generic
//! block read/write. No allocator, no filesystem semantics.
//!
//! PoC additions (2026-10-27): all device access goes through the
//! [`Device`] trait; anchors are read from both the head and the
//! end-of-device mirror; [`MountMode::Bypass`] opens a device for raw
//! reads/writes without reading or trusting any anchor.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

pub mod anchor;
mod checksum;
pub mod device;
pub mod fault;
pub mod layout;
pub mod safety;

mod block_io;
mod check;
mod confirm;
mod mount;
mod remount;

pub use anchor::{PtrAlgo, Side, SlotReport};
pub use device::{Device, FileDevice};
pub use error::{CafsError, CafsResult};
pub use layout::ByteOffset;

mod error;

use layout::{REAL_BLOCK_4K, SMART_TABLE_SIZE};

/// How a device is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MountMode {
    /// Read and verify the anchors; `check`/`confirm` available.
    #[default]
    Full,
    /// Open the device only. No anchor is read or trusted; `check`
    /// and `confirm` return [`CafsError::Bypassed`]. `read_block`,
    /// `write_block`, `sync`, `scan_anchors` and `remount` work, so
    /// the raw disk can be inspected or modified (including a damaged
    /// one that `Full` would refuse to mount).
    Bypass,
}

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

/// `check()` result. The Superblock is verified and repaired from its
/// backup; each of the six anchor slots is verified at both ends of
/// the device and a damaged end is rebuilt from the intact one.
#[derive(Debug, Clone, Copy, Default)]
pub struct CheckResult {
    pub superblock_ok: bool,
    pub superblock_repaired: bool,
    pub pointer_checksum_algo_used: Option<PtrAlgo>,
    /// Slots where exactly one end failed verification.
    pub anchor_slots_degraded: u8,
    /// Of those, how many were rewritten from the intact end.
    pub anchor_slots_repaired: u8,
    /// Slots where both ends failed.
    pub anchor_slots_lost: u8,
    /// Slots where both ends verify but point at different places.
    pub anchor_disagreements: u8,
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

/// A mounted CAFS device. Not `Clone`/`Copy` — owns the device.
pub struct CafsCtx {
    pub(crate) dev: Box<dyn Device>,
    pub(crate) want_write: bool,
    pub(crate) mode: MountMode,
    #[allow(dead_code)]
    pub(crate) device_size: u64,

    pub(crate) ptr_algo: PtrAlgo,
    pub(crate) anchors: Vec<SlotReport>,
    pub(crate) sb_primary: layout::ByteOffset,
    pub(crate) sb_backup: Option<layout::ByteOffset>,
    pub(crate) parity_offset: layout::ByteOffset,

    pub(crate) superblock: [u8; REAL_BLOCK_4K as usize],
    pub(crate) config_snapshot: Option<[u8; REAL_BLOCK_4K as usize]>,
    pub(crate) function_table: Vec<u8>,
    pub(crate) smart_main: Option<([u8; SMART_TABLE_SIZE], layout::ByteOffset)>,

    pub(crate) opts: MountOpts,
    pub(crate) smart: SmartCounters,
}

impl CafsCtx {
    pub fn smart_counters(&self) -> SmartCounters {
        self.smart
    }

    pub fn mode(&self) -> MountMode {
        self.mode
    }

    pub fn device_size(&self) -> u64 {
        self.dev.size()
    }

    /// Anchor slots as read at mount (empty in bypass mode; call
    /// [`CafsCtx::scan_anchors`] to read them on demand).
    pub fn anchors(&self) -> &[SlotReport] {
        &self.anchors
    }

    /// Durability barrier on the device (`fdatasync`).
    pub fn sync(&mut self) -> CafsResult<()> {
        self.dev.sync()
    }

    /// Not one of the originally-named four stages — added because
    /// `confirm()` only ever marks a mount as started; without a
    /// counterpart, `destroy_flush` never runs. Consumes `self`: the
    /// borrow checker makes using `ctx` after `close()` a compile
    /// error, not a runtime bug — a class of mistake the C version's
    /// `cafs_close()` had no way to prevent.
    pub fn close(self) -> CafsResult<()> {
        let mut this = self;
        if this.opts.destroy_flush && this.want_write {
            this.dev.sync()?;
        }
        // The device (and its fd, for FileDevice) closes on drop —
        // no leak even if a caller never calls close() at all.
        Ok(())
    }
}
