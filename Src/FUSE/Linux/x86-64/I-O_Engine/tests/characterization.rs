//! Phase 1 — behaviour pinned from the engine as uploaded (format 4)
//! that STILL holds after the Device seam, tail-mirror and bypass
//! changes. Tests whose behaviour intentionally flipped were removed
//! and re-expressed in `anchors.rs` / `bypass.rs`; see the commit
//! message of "tests: ..." for the list.

mod common;
use common::*;

use cafs_io::fault::FaultDevice;
use cafs_io::{CafsCtx, CafsError, MountOpts};

const SZ: u64 = 1024 * 1024;

#[test]
fn char_remount_readonly_to_readwrite_is_unsupported() {
    let (_d, mut c) = mount_image(formatted_image(SZ), false).unwrap();
    let o = MountOpts { readonly: false, ..Default::default() };
    assert_eq!(c.remount(&o).map(|_| ()), Err(CafsError::Unsupported));
}

#[test]
fn char_sync_open_remount_reports_success() {
    // The engine claims success...
    let (_d, mut c) = mount_image(formatted_image(SZ), true).unwrap();
    let o = MountOpts { sync_open: true, ..Default::default() };
    assert!(c.remount(&o).is_ok());
}

#[test]
fn kernel_fact_o_sync_cannot_be_set_through_f_setfl() {
    // ...but the kernel ignores O_SYNC via F_SETFL (fcntl(2)), so a
    // FileDevice remount with sync_open does nothing. Environment fact;
    // run it on your own host too.
    use rustix::fs::{fcntl_getfl, fcntl_setfl, open, Mode, OFlags};
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("k.img");
    std::fs::write(&p, formatted_image(SZ)).unwrap();
    let fd = open(p.to_str().unwrap(), OFlags::RDWR, Mode::empty()).unwrap();
    let mut fl = fcntl_getfl(&fd).unwrap();
    fl.set(OFlags::SYNC, true);
    fcntl_setfl(&fd, fl).unwrap();
    assert!(!fcntl_getfl(&fd).unwrap().contains(OFlags::SYNC));
}

#[test]
fn char_unreadable_superblock_primary_fails_mount_instead_of_using_the_backup() {
    // A checksum failure in the Superblock is repaired from the
    // backup; an I/O error reading it is not.
    let dev = FaultDevice::from_image(formatted_image(SZ));
    dev.add_bad_read_range(SB_OFF, 4096);
    let r = CafsCtx::mount_device(Box::new(dev), false);
    assert!(matches!(r, Err(CafsError::Io)));
}
