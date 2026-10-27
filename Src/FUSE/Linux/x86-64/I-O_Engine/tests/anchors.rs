//! Head + end-of-device anchor pointer blocks (fs.info §2, §3, §12,
//! §14 Track A) as the engine now uses them.

mod common;
use common::*;

use cafs_io::anchor::build_ptr_block;
use cafs_io::fault::FaultDevice;
use cafs_io::layout::*;
use cafs_io::{CafsCtx, CafsError, MountOpts, PtrAlgo, Side};

const SZ: u64 = 1024 * 1024;

fn img() -> Vec<u8> {
    formatted_image(SZ)
}
fn zero(dev: &FaultDevice, off: u64, n: usize) {
    dev.poke(off, &vec![0u8; n]);
}
fn mount_rw(dev: &FaultDevice) -> CafsCtx {
    CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap()
}

#[test]
fn fresh_volume_has_twelve_good_pointer_blocks_and_the_mirror_matches() {
    let image = img();
    let (_d, c) = mount_image(image.clone(), false).unwrap();
    for r in c.anchors() {
        assert!(r.head_ok() && r.tail_ok() && !r.disagree(), "slot {}", r.slot);
        assert_eq!(r.used(), Some(Side::Head));
    }
    // Slots 1-5: tail is a byte-for-byte copy of the head.
    for slot in 1..6u8 {
        let n = slot_block_size(slot) as usize;
        let (h, t) = (head_slot_offset(slot).0 as usize, tail_slot_offset(slot, SZ).0 as usize);
        assert_eq!(image[h..h + n], image[t..t + n], "slot {slot}");
    }
    // Slot 0: tail swaps primary and fallback.
    let (primary, backup) = c.anchors()[0].sb_pair().unwrap();
    assert_eq!(primary.0, SB_OFF);
    assert_eq!(backup.unwrap(), sb_backup_offset(SZ));
}

#[test]
fn lba0_checksum_flip_mounts_from_the_tail() {
    let dev = FaultDevice::from_image(img());
    dev.flip_byte(24);
    let mut c = mount_rw(&dev);
    assert_eq!(c.anchors()[0].used(), Some(Side::Tail));
    assert!(c.check().unwrap().superblock_ok);
}

#[test]
fn first_sector_zeroed_mounts_and_check_rebuilds_the_heads_byte_for_byte() {
    let original = img();
    let dev = FaultDevice::from_image(original.clone());
    zero(&dev, 0, 4096); // LBA0 + LBA1 heads
    let mut c = mount_rw(&dev);
    assert_eq!(c.anchors()[1].used(), Some(Side::Tail));
    let r = c.check().unwrap();
    assert_eq!((r.anchor_slots_degraded, r.anchor_slots_repaired, r.anchor_slots_lost), (2, 2, 0));
    assert_eq!(dev.durable_image()[0..4096], original[0..4096]);
    assert!(c.anchors().iter().all(|r| r.head_ok() && r.tail_ok()));
}

#[test]
fn unreadable_head_sector_mounts_from_the_tail() {
    let dev = FaultDevice::from_image(img());
    dev.add_bad_read_range(0, 4096);
    let mut c = mount_rw(&dev);
    let r = c.check().unwrap();
    assert_eq!(r.anchor_slots_degraded, 2);
    assert!(r.superblock_ok);
    // Rewriting cannot cure a sector that still cannot be read.
    assert!(!c.anchors()[0].head_ok());
}

#[test]
fn both_ends_of_lba0_bad_is_unmountable() {
    let dev = FaultDevice::from_image(img());
    dev.flip_byte(24);
    dev.flip_byte(tail_slot_offset(0, SZ).0 + 24);
    assert!(matches!(CafsCtx::mount_device(Box::new(dev), false), Err(CafsError::Checksum)));
}

#[test]
fn both_ends_of_a_non_root_slot_bad_is_reported_lost_but_mount_succeeds() {
    let dev = FaultDevice::from_image(img());
    dev.flip_byte(head_slot_offset(2).0 + 24);
    dev.flip_byte(tail_slot_offset(2, SZ).0 + 24);
    let mut c = mount_rw(&dev);
    assert_eq!(c.check().unwrap().anchor_slots_lost, 1);
}

#[test]
fn damaged_head_fallback_is_detected_and_the_head_rebuilt() {
    let original = img();
    let dev = FaultDevice::from_image(original.clone());
    dev.flip_byte(1048); // head LBA0 fallback_checksum
    let mut c = mount_rw(&dev);
    assert!(c.anchors()[0].head_ok() && !c.anchors()[0].head_good());
    let r = c.check().unwrap();
    assert_eq!((r.anchor_slots_degraded, r.anchor_slots_repaired), (1, 1));
    assert_eq!(dev.durable_image()[0..2048], original[0..2048]);
}

#[test]
fn damaged_fallback_pointer_plus_bad_superblock_now_recovers_via_the_tail() {
    // As uploaded this combination was unrecoverable.
    let dev = FaultDevice::from_image(img());
    dev.flip_byte(1034); // head LBA0 fallback_real_lba
    dev.flip_byte(SB_OFF + 100);
    let mut c = mount_rw(&dev);
    let r = c.check().unwrap();
    assert!(r.superblock_ok && r.superblock_repaired);
}

#[test]
fn readonly_mount_reports_damage_but_writes_nothing() {
    let dev = FaultDevice::from_image(img());
    zero(&dev, 0, 4096);
    let mut c = CafsCtx::mount_device(Box::new(dev.clone()), false).unwrap();
    let r = c.check().unwrap();
    assert_eq!((r.anchor_slots_degraded, r.anchor_slots_repaired), (2, 0));
    assert_eq!(dev.counters().writes, 0);
}

#[test]
fn software_readonly_remount_blocks_repairs() {
    // remount.rs documents that check()'s repair path honours
    // opts.readonly; as uploaded it did not.
    let dev = FaultDevice::from_image(img());
    dev.flip_byte(SB_OFF + 100);
    zero(&dev, 0, 4096);
    let mut c = mount_rw(&dev);
    c.remount(&MountOpts { readonly: true, ..Default::default() }).unwrap();
    let r = c.check().unwrap();
    assert!(r.superblock_ok && !r.superblock_repaired);
    assert_eq!(r.anchor_slots_repaired, 0);
    assert_eq!(dev.counters().writes, 0);
}

#[test]
fn ends_that_verify_but_disagree_are_reported_and_head_wins() {
    let dev = FaultDevice::from_image(img());
    let other = build_ptr_block(PTR_BLOCK_SMALL_SIZE, PtrAlgo::Crc32c, 8 * 4096, 4096, None);
    dev.poke(head_slot_offset(1).0, &other);
    let mut c = mount_rw(&dev);
    assert_eq!(c.anchors()[1].used(), Some(Side::Head));
    let r = c.check().unwrap();
    assert_eq!((r.anchor_disagreements, r.anchor_slots_degraded), (1, 0));
}

#[test]
fn grown_device_orphans_the_tail_and_check_rebuilds_it_at_the_new_end() {
    // The Superblock has no device-size field (fs.info §4), so a
    // device that grew outside CAFS cannot be told apart from damage.
    let mut grown = img();
    grown.resize(2 * SZ as usize, 0);
    let dev = FaultDevice::from_image(grown);
    let mut c = mount_rw(&dev);
    assert!(c.anchors().iter().all(|r| r.head_ok() && !r.tail_ok()));
    let r = c.check().unwrap();
    assert_eq!((r.anchor_slots_degraded, r.anchor_slots_repaired), (6, 6));
    assert!(c.anchors().iter().all(|r| r.head_ok() && r.tail_ok()));
    // Real structures stay where they were; pointers are authoritative.
    assert_eq!(c.anchors()[0].sb_pair().unwrap().1.unwrap(), sb_backup_offset(SZ));
    assert_ne!(sb_backup_offset(SZ), sb_backup_offset(2 * SZ));
}

#[test]
fn known_hazard_shrunk_device_check_overwrites_live_structures_with_a_new_tail() {
    // Same blindness in the other direction: after a shrink the
    // "end of device" lies inside what used to be the real tail
    // region, and check() writes a mirror over whatever is there —
    // here the Superblock Backup. PINS A HAZARD, not a desired result.
    let original = img();
    let mut shrunk = original.clone();
    shrunk.truncate(SZ as usize - 16384);
    let old_backup = sb_backup_offset(SZ).0 as usize;
    assert_eq!(shrunk[old_backup..old_backup + 4096], original[SB_OFF as usize..SB_OFF as usize + 4096]);
    let dev = FaultDevice::from_image(shrunk);
    let mut c = mount_rw(&dev);
    c.check().unwrap();
    let after = dev.durable_image();
    assert_ne!(after[old_backup..old_backup + 4096], original[SB_OFF as usize..SB_OFF as usize + 4096]);
}
