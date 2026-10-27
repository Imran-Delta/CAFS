//! Guards for pointing destructive tooling (mkfs, the device test
//! suite) at a real block device. Defence in depth, in this order:
//!  1. kernel: `FileDevice` opens a block device for writing with
//!     `O_EXCL`, which the kernel refuses (EBUSY) while the device is
//!     mounted, in use as swap, or claimed by another driver;
//!  2. this module: refuse whole disks that have partitions, devices
//!     with holders (dm/md/LUKS on top), anything listed in
//!     `/proc/self/mountinfo` or `/proc/swaps`, and any device whose
//!     first 128 KiB carry a filesystem signature other than "empty",
//!     "swap" or "CAFS" — so a mistyped partition number cannot
//!     silently flatten an NTFS/ext4/btrfs volume.
//!
//! Linux only (sysfs, procfs).
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::fs;
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::device::FileDevice;
use crate::layout::PTR_MAGIC;

/// What a device's first 128 KiB look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signature {
    Empty,
    Swap,
    Cafs,
    Ntfs,
    Fat,
    Ext,
    Btrfs,
    Xfs,
    Luks,
    Other,
}

impl Signature {
    /// Signatures a destructive CAFS tool may overwrite without being
    /// told to wipe the device first.
    pub const OVERWRITABLE: [Signature; 3] = [Signature::Empty, Signature::Swap, Signature::Cafs];
}

pub fn detect_signature(b: &[u8]) -> Signature {
    let at = |off: usize, pat: &[u8]| b.len() >= off + pat.len() && &b[off..off + pat.len()] == pat;
    if at(0, &PTR_MAGIC) {
        Signature::Cafs
    } else if at(4086, b"SWAPSPACE2") || at(4086, b"SWAP-SPACE") {
        Signature::Swap
    } else if at(3, b"NTFS    ") {
        Signature::Ntfs
    } else if at(82, b"FAT32") || at(54, b"FAT16") || at(54, b"FAT12") {
        Signature::Fat
    } else if at(1080, &[0x53, 0xEF]) {
        Signature::Ext
    } else if at(65600, b"_BHRfS_M") {
        Signature::Btrfs
    } else if at(0, b"XFSB") {
        Signature::Xfs
    } else if at(0, b"LUKS\xba\xbe") {
        Signature::Luks
    } else if b.iter().all(|&x| x == 0) {
        Signature::Empty
    } else {
        Signature::Other
    }
}

#[derive(Debug, Clone)]
pub struct TargetInfo {
    pub path: PathBuf,
    pub kernel_name: String,
    pub size: u64,
    pub logical_sector: Option<u32>,
    pub physical_sector: Option<u32>,
    pub rotational: Option<bool>,
    pub is_partition: bool,
    pub signature: Signature,
}

fn rdev_of(path: &Path) -> Option<u64> {
    let m = fs::metadata(path).ok()?;
    m.file_type().is_block_device().then(|| m.rdev())
}

fn read_trim(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// Check that `path` is a block device that is safe to overwrite.
/// `Ok` carries what was learned about it; `Err` is a human-readable
/// reason to stop.
pub fn check_target(path: &str, allowed: &[Signature]) -> Result<TargetInfo, String> {
    let meta = fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if !meta.file_type().is_block_device() {
        return Err(format!("{path} is not a block device"));
    }
    let rdev = meta.rdev();
    let (maj, min) = (rustix::fs::major(rdev), rustix::fs::minor(rdev));
    let sys = fs::canonicalize(format!("/sys/dev/block/{maj}:{min}"))
        .map_err(|e| format!("cannot resolve {maj}:{min} in sysfs: {e}"))?;
    let kernel_name = sys.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let is_partition = sys.join("partition").exists();

    // Devices that belong to this target: itself, plus its partitions.
    let mut mine: Vec<(u32, u32)> = vec![(maj, min)];
    if !is_partition {
        let mut children = 0;
        if let Ok(rd) = fs::read_dir(&sys) {
            for e in rd.flatten() {
                if e.path().join("partition").exists() {
                    children += 1;
                    if let Some(d) = read_trim(e.path().join("dev")) {
                        if let Some((a, b)) = d.split_once(':') {
                            if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                                mine.push((a, b));
                            }
                        }
                    }
                }
            }
        }
        if children > 0 {
            return Err(format!("{path} is a whole disk with {children} partition(s); name a partition instead"));
        }
    }
    if let Ok(rd) = fs::read_dir(sys.join("holders")) {
        if rd.count() > 0 {
            return Err(format!("{path} has holders (device-mapper/md/LUKS on top of it)"));
        }
    }

    let is_mine = |p: &Path| rdev_of(p).is_some_and(|r| mine.contains(&(rustix::fs::major(r), rustix::fs::minor(r))));
    if let Ok(mi) = fs::read_to_string("/proc/self/mountinfo") {
        for line in mi.lines() {
            let mut halves = line.splitn(2, " - ");
            let (pre, post) = (halves.next().unwrap_or(""), halves.next().unwrap_or(""));
            let devno = pre.split_whitespace().nth(2).unwrap_or("");
            let source = post.split_whitespace().nth(1).unwrap_or("");
            let by_devno = devno
                .split_once(':')
                .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
                .is_some_and(|d| mine.contains(&d));
            if by_devno || is_mine(Path::new(source)) {
                return Err(format!("{path} (or a partition of it) is mounted: {}", line.split_whitespace().nth(4).unwrap_or("?")));
            }
        }
    }
    if let Ok(sw) = fs::read_to_string("/proc/swaps") {
        for line in sw.lines().skip(1) {
            if let Some(p) = line.split_whitespace().next() {
                if is_mine(Path::new(p)) {
                    return Err(format!("{path} is active swap ({p}); run `swapoff {p}` first"));
                }
            }
        }
    }

    let rotational = {
        let disk = if is_partition { sys.parent().map(Path::to_path_buf).unwrap_or(sys.clone()) } else { sys.clone() };
        read_trim(disk.join("queue/rotational")).map(|s| s == "1")
    };

    let dev = FileDevice::open(path, false).map_err(|e| format!("{path}: cannot open for reading: {e}"))?;
    let size = crate::device::Device::size(&dev);
    let (logical_sector, physical_sector) = dev.sector_sizes();

    let mut head = vec![0u8; 128 * 1024];
    let n = fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("{path}: cannot read signature: {e}"))?;
    let signature = detect_signature(&head[..n]);
    if !allowed.contains(&signature) {
        let what = if signature == Signature::Other {
            "data that is not empty, swap or CAFS (an unrecognised filesystem, LVM, leftovers...)".to_string()
        } else {
            format!("a {signature:?} signature")
        };
        return Err(format!(
            "{path} holds {what}; refusing to overwrite it. If you are sure it is expendable, clear its start yourself first: `dd if=/dev/zero of={path} bs=1M count=1 conv=fsync`"
        ));
    }

    Ok(TargetInfo { path: PathBuf::from(path), kernel_name, size, logical_sector, physical_sector, rotational, is_partition, signature })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut v = vec![0u8; 128 * 1024];
        f(&mut v);
        v
    }

    #[test]
    fn recognises_common_signatures() {
        assert_eq!(detect_signature(&page(|_| {})), Signature::Empty);
        assert_eq!(detect_signature(&page(|v| v[4086..4096].copy_from_slice(b"SWAPSPACE2"))), Signature::Swap);
        assert_eq!(detect_signature(&page(|v| v[3..11].copy_from_slice(b"NTFS    "))), Signature::Ntfs);
        assert_eq!(detect_signature(&page(|v| v[1080..1082].copy_from_slice(&[0x53, 0xEF]))), Signature::Ext);
        assert_eq!(detect_signature(&page(|v| v[65600..65608].copy_from_slice(b"_BHRfS_M"))), Signature::Btrfs);
        assert_eq!(detect_signature(&page(|v| v[0..8].copy_from_slice(&PTR_MAGIC))), Signature::Cafs);
        assert_eq!(detect_signature(&page(|v| v[500] = 1)), Signature::Other);
    }

    #[test]
    fn regular_files_and_missing_paths_are_refused() {
        assert!(check_target("/definitely/not/here", &Signature::OVERWRITABLE).is_err());
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, b"x").unwrap();
        assert!(check_target(p.to_str().unwrap(), &Signature::OVERWRITABLE).unwrap_err().contains("not a block device"));
    }
}
