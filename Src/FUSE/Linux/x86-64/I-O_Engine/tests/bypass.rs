//! MountMode::Bypass — raw engine I/O without mount, for inspecting
//! and modifying a disk (including one Full mount refuses).

mod common;
use common::*;

use cafs_io::{ByteOffset, CafsCtx, CafsError, MountMode, MountOpts};

const SZ: u64 = 1024 * 1024;

fn file_image(name: &str) -> (tempfile::TempDir, String) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join(name);
    std::fs::write(&p, formatted_image(SZ)).unwrap();
    let s = p.to_str().unwrap().to_string();
    (d, s)
}

#[test]
fn bypass_opens_a_volume_that_full_mount_refuses_and_modifies_it() {
    let (_d, p) = file_image("a.img");
    // Destroy both anchor regions completely.
    std::fs::write(&p, {
        let mut v = std::fs::read(&p).unwrap();
        v[..16384].fill(0);
        let n = v.len();
        v[n - 16384..].fill(0);
        v
    })
    .unwrap();
    assert!(CafsCtx::mount(&p, true).is_err());

    let mut c = CafsCtx::mount_with(&p, true, MountMode::Bypass).unwrap();
    assert!(c.anchors().is_empty());
    let mut b = [0xAAu8; 8];
    c.read_block(ByteOffset(0), &mut b).unwrap();
    assert_eq!(b, [0u8; 8]);
    c.write_block(ByteOffset(4096), b"RAWWRITE").unwrap();
    c.sync().unwrap();
    drop(c);
    let on_disk = std::fs::read(&p).unwrap();
    assert_eq!(&on_disk[4096..4104], b"RAWWRITE");
}

#[test]
fn bypass_refuses_check_and_confirm_but_allows_scan_and_remount() {
    let (_d, p) = file_image("a.img");
    let mut c = CafsCtx::mount_with(&p, true, MountMode::Bypass).unwrap();
    assert!(matches!(c.check(), Err(CafsError::Bypassed)));
    assert!(matches!(c.confirm(), Err(CafsError::Bypassed)));
    assert_eq!(c.scan_anchors().len(), 6);
    c.remount(&MountOpts::default()).unwrap();
}

#[test]
fn every_device_access_is_bounds_checked_and_never_grows_the_image() {
    let (_d, p) = file_image("a.img");
    let mut c = CafsCtx::mount_with(&p, true, MountMode::Bypass).unwrap();
    let mut b = [0u8; 16];
    assert_eq!(c.read_block(ByteOffset(SZ - 8), &mut b), Err(CafsError::OutOfRange));
    assert_eq!(c.write_block(ByteOffset(SZ - 8), &b), Err(CafsError::OutOfRange));
    assert_eq!(c.write_block(ByteOffset(u64::MAX - 4), &b), Err(CafsError::OutOfRange));
    drop(c);
    assert_eq!(std::fs::metadata(&p).unwrap().len(), SZ);
}

#[test]
fn bypass_readonly_open_rejects_writes() {
    let (_d, p) = file_image("a.img");
    let mut c = CafsCtx::mount_with(&p, false, MountMode::Bypass).unwrap();
    assert_eq!(c.write_block(ByteOffset(0), &[1]), Err(CafsError::Readonly));
}
