//! `CafsCtx::remount` — extracts the crash-critical fields this
//! engine needs from the Config Snapshot's TOML (a minimal targeted
//! key scanner, not a general TOML parser — the snapshot is machine-
//! generated, comment/blank-line-stripped text, regular enough that a
//! full parser wasn't worth building for four fields), then applies
//! caller-supplied [`MountOpts`] on top for the runtime-only knobs.
//! Callable multiple times: mount read-only (safe mode) -> remount
//! read-write so `check()` can persist a repair -> remount again with
//! the final on-disk config. Works because every anchor structure is
//! fixed at 4KB regardless of the filesystem's configured block size.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use rustix::fs::{self, OFlags};

use crate::error::{CafsError, CafsResult};
use crate::layout::{PTR_ALGO_CRC32C, PTR_ALGO_XXHASH32};
use crate::{CafsCtx, ConfigSnapshot, MountOpts};

fn scan_config_snapshot(raw: &[u8]) -> ConfigSnapshot {
    let text = String::from_utf8_lossy(raw);
    let mut snap = ConfigSnapshot::default();
    let mut section = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = inner.to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');

        match (section.as_str(), key) {
            ("physical", "block_size") => snap.block_size = value.parse().ok(),
            ("physical", "pointer_checksum_algo") => {
                snap.pointer_checksum_algo_id = match value {
                    "crc32c" => Some(PTR_ALGO_CRC32C),
                    "xxhash32" => Some(PTR_ALGO_XXHASH32),
                    _ => None,
                };
            }
            ("integrity", "data_checksum_algo") => {
                snap.data_checksum_algo_id = match value {
                    "none" => Some(crate::layout::DATA_ALGO_NONE),
                    "crc32c" => Some(crate::layout::DATA_ALGO_CRC32C),
                    "xxhash64" => Some(crate::layout::DATA_ALGO_XXHASH64),
                    "blake3" => Some(crate::layout::DATA_ALGO_BLAKE3),
                    _ => None,
                };
            }
            ("integrity", "verify_data") => snap.verify_data = value.parse().ok(),
            _ => {}
        }
    }

    snap
}

impl CafsCtx {
    pub fn remount(&mut self, opts: &MountOpts) -> CafsResult<ConfigSnapshot> {
        let snapshot = match &self.config_snapshot {
            Some(raw) => scan_config_snapshot(raw),
            None => ConfigSnapshot::default(),
        };

        if opts.readonly && self.want_write {
            // Downgrading write -> readonly in-process isn't
            // meaningful to enforce at the fd level (the fd stays
            // O_RDWR), so this is enforced in software: write_block/
            // confirm/check's repair path all check `opts.readonly`
            // before writing. want_write (fd-level) only ever
            // reflects what mount() originally opened with.
        } else if !opts.readonly && !self.want_write {
            // Caller wants to escalate to read-write, but the fd was
            // opened read-only at mount() — this can't be fixed with
            // fcntl (O_ACCMODE isn't changeable in place on Linux).
            // The caller needs a fresh cafs_mount(path, true, ...).
            return Err(CafsError::Unsupported);
        }

        let mut flags = fs::fcntl_getfl(&self.fd)?;
        flags.set(OFlags::DIRECT, opts.direct_io);
        flags.set(OFlags::SYNC, opts.sync_open);
        fs::fcntl_setfl(&self.fd, flags)?;

        if opts.direct_io && opts.direct_io_required {
            let now = fs::fcntl_getfl(&self.fd)?;
            if !now.contains(OFlags::DIRECT) {
                return Err(CafsError::Unsupported);
            }
        }

        self.opts = *opts;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_all_four_fields() {
        let toml = b"[identity]\nname = \"test-volume\"\n\n[physical]\nblock_size = 4096\npointer_checksum_algo = \"crc32c\"\n\n[integrity]\ndata_checksum_algo = \"blake3\"\nverify_data = true\n";
        let snap = scan_config_snapshot(toml);
        assert_eq!(snap.block_size, Some(4096));
        assert_eq!(snap.pointer_checksum_algo_id, Some(PTR_ALGO_CRC32C));
        assert_eq!(snap.data_checksum_algo_id, Some(crate::layout::DATA_ALGO_BLAKE3));
        assert_eq!(snap.verify_data, Some(true));
    }

    #[test]
    fn ignores_unrelated_sections() {
        let toml = b"[mount_hints]\nreadonly = false\n\n[physical]\nblock_size = 8192\n";
        let snap = scan_config_snapshot(toml);
        assert_eq!(snap.block_size, Some(8192));
        assert_eq!(snap.verify_data, None);
    }
}
