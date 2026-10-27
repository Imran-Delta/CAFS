//! Adaptive suite: the same scenarios run on a disk image (default) or
//! on a real block device:
//!
//!   CAFS_TEST_DEVICE=/dev/sdXN CAFS_TEST_DEVICE_I_AGREE=/dev/sdXN \
//!       cargo test --test adaptive --test bench -- --test-threads=1 --nocapture
//!
//! Corruption is applied with raw bypass writes (real bytes on the
//! medium), recovery goes through the normal mount/check path, and the
//! result is verified by an O_DIRECT read where the platform allows.

mod common;
use common::*;

use cafs_io::layout::*;
use cafs_io::{ByteOffset, CafsError, MountMode, Side};

const IMG: u64 = 4 * 1024 * 1024;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

#[test]
fn lifecycle_format_mount_check_confirm() {
    let t = Target::acquire(IMG);
    t.format();
    println!("target: {}", t.describe());
    let sb_before = t.peek(SB_OFF, 4096);

    for cycle in 1..=2u32 {
        let mut c = t.mount(true).unwrap();
        assert_eq!(c.device_size(), t.size);
        assert!(c.anchors().iter().all(|r| r.head_ok() && r.tail_ok() && !r.disagree()));
        let r = c.check().unwrap();
        assert!(r.superblock_ok && r.anchor_slots_degraded == 0 && !r.superblock_repaired);
        c.confirm().unwrap();
        c.close().unwrap();

        let (smart, how) = t.peek_media(SMART_OFF, 4096);
        assert_eq!(u32_at(&smart, SMART_OFF_MOUNT_COUNT), cycle, "mount_count ({how})");
        assert!(u64_at(&smart, SMART_OFF_LAST_MOUNT_TIME) > 1_700_000_000, "last_mount_time ({how})");
        assert!(blake3_128_ok(&smart, SMART_HASHED_LEN), "M-SMART checksum ({how})");
    }
    assert_eq!(t.peek_media(SB_OFF, 4096).0, sb_before, "confirm() must not touch the Superblock");
}

#[test]
fn head_sector_destroyed_recovers_from_the_tail_and_is_rebuilt_exactly() {
    let t = Target::acquire(IMG);
    t.format();
    let before = t.peek(0, 4096);
    t.poke(0, &[0u8; 4096]);

    let mut c = t.mount(true).unwrap();
    assert_eq!(c.anchors()[0].used(), Some(Side::Tail));
    let r = c.check().unwrap();
    assert_eq!((r.anchor_slots_degraded, r.anchor_slots_repaired), (2, 2));
    drop(c);
    let (after, how) = t.peek_media(0, 4096);
    assert_eq!(after, before, "head rebuilt byte-for-byte ({how})");
}

#[test]
fn tail_region_destroyed_recovers_from_the_head_and_is_rebuilt_exactly() {
    let t = Target::acquire(IMG);
    t.format();
    let tail = t.size - ANCHOR_REGION_BYTES;
    let before = t.peek(tail, ANCHOR_REGION_BYTES as usize);
    t.poke(tail, &vec![0u8; ANCHOR_REGION_BYTES as usize]);

    let mut c = t.mount(true).unwrap();
    assert!(c.anchors().iter().all(|r| r.used() == Some(Side::Head)));
    let r = c.check().unwrap();
    assert_eq!((r.anchor_slots_degraded, r.anchor_slots_repaired), (6, 6));
    drop(c);
    let (after, how) = t.peek_media(tail, ANCHOR_REGION_BYTES as usize);
    assert_eq!(after, before, "tail rebuilt byte-for-byte ({how})");
}

#[test]
fn superblock_corruption_is_repaired_from_the_backup_at_the_end_of_the_device() {
    let t = Target::acquire(IMG);
    t.format();
    let before = t.peek(SB_OFF, 4096);
    let mut bad = before.clone();
    bad[100] ^= 0xFF;
    t.poke(SB_OFF, &bad);

    let mut c = t.mount(true).unwrap();
    let r = c.check().unwrap();
    assert!(r.superblock_ok && r.superblock_repaired);
    drop(c);
    assert_eq!(t.peek_media(SB_OFF, 4096).0, before);
    assert_eq!(t.peek(sb_backup_offset(t.size).0, 4096), before, "backup untouched");
}

#[test]
fn both_ends_of_lba0_destroyed_refuses_full_mount_but_bypass_still_opens() {
    let t = Target::acquire(IMG);
    t.format();
    t.poke(0, &[0u8; 2048]);
    t.poke(tail_slot_offset(0, t.size).0, &[0u8; 2048]);

    assert!(matches!(t.mount(false), Err(CafsError::BadMagic)));
    let mut c = t.bypass(false);
    let mut b = [0u8; 16];
    c.read_block(ByteOffset(SB_OFF), &mut b).unwrap(); // data still reachable raw
    drop(c);

    t.format();
    assert!(t.mount(false).is_ok(), "re-format restores a mountable volume");
}

#[test]
fn raw_io_works_at_the_edges_and_never_leaves_the_device() {
    let t = Target::acquire(IMG);
    t.format();
    let mut c = t.bypass(true);
    assert_eq!(c.device_size(), t.size);
    let spots = [0u64, 4096, t.size / 2 / 4096 * 4096, t.size - 4096];
    for (i, &off) in spots.iter().enumerate() {
        let pat = vec![0xC0 + i as u8; 4096];
        c.write_block(ByteOffset(off), &pat).unwrap();
    }
    c.sync().unwrap();
    // Unaligned (bounce) write at the very last bytes.
    c.write_block(ByteOffset(t.size - 3), &[1, 2, 3]).unwrap();
    c.sync().unwrap();
    assert_eq!(c.write_block(ByteOffset(t.size - 2), &[9, 9, 9]), Err(CafsError::OutOfRange));
    assert_eq!(c.write_block(ByteOffset(t.size), &[9]), Err(CafsError::OutOfRange));
    drop(c);

    for (i, &off) in spots.iter().enumerate() {
        let (got, how) = t.peek_media(off, 4096);
        let mut want = vec![0xC0 + i as u8; 4096];
        if off == t.size - 4096 {
            let n = want.len();
            want[n - 3..].copy_from_slice(&[1, 2, 3]);
        }
        assert_eq!(got, want, "pattern at {off} ({how})");
    }
}

#[test]
fn a_second_writer_is_refused_while_one_holds_the_device() {
    let t = Target::acquire(IMG);
    if !t.is_device() {
        eprintln!("skipped: exclusive claim (O_EXCL) exists only for block devices");
        return;
    }
    t.format();
    let held = t.bypass(true);
    assert!(matches!(cafs_io::CafsCtx::mount_with(&t.path, true, MountMode::Bypass), Err(CafsError::Open)));
    assert!(cafs_io::CafsCtx::mount_with(&t.path, false, MountMode::Bypass).is_ok(), "inspection stays possible");
    drop(held);
    assert!(cafs_io::CafsCtx::mount_with(&t.path, true, MountMode::Bypass).is_ok());
}
