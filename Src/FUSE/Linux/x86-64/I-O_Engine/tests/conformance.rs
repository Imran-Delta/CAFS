//! Conformance of the engine with DECIDED items in DECISIONS-FORMAT-8.md
//! and the accepted ADRs (fs.info is advisory only). Tests for decisions
//! the engine does not implement yet are #[ignore]d with the decision
//! quoted; `cargo test --test conformance -- --ignored` lists the delta.

mod common;
use common::*;

use cafs_io::fault::{FaultDevice, Op};
use cafs_io::layout::*;
use cafs_io::CafsCtx;

const SZ: u64 = 1024 * 1024;

#[test]
fn adr010_addressing_is_byte_offsets_and_the_magic_is_cafsptr_nul() {
    assert_eq!(&PTR_MAGIC, b"CAFSPTR\0");
    let img = formatted_image(SZ);
    let (_d, c) = mount_image(img, false).unwrap();
    // real_lba fields hold byte offsets: the Superblock pointer is 16384, not 4.
    assert_eq!(c.anchors()[0].sb_pair().unwrap().0 .0, SB_OFF);
}

#[test]
fn adr006_tail_mirror_is_a_byte_identical_copy_of_the_head() {
    let img = formatted_image(SZ);
    for slot in 0..SLOT_COUNT as u8 {
        let n = slot_block_size(slot) as usize;
        let (h, t) = (head_slot_offset(slot).0 as usize, tail_slot_offset(slot, SZ).0 as usize);
        assert_eq!(img[h..h + n], img[t..t + n], "slot {slot}");
    }
}

#[test]
fn adr006_only_the_superblock_has_a_second_copy() {
    let img = formatted_image(SZ);
    let copies = |off: u64| {
        let blk = &img[off as usize..off as usize + 4096];
        (0..img.len() / 4096).filter(|i| &img[i * 4096..i * 4096 + 4096] == blk).count()
    };
    assert_eq!(copies(SB_OFF), 2, "Superblock + its backup");
    assert_eq!(copies(CFG_OFF), 1, "Config Snapshot");
    assert_eq!(copies(PARITY_OFF), 1, "Parity Block");
    assert_eq!(copies(FT_OFF), 1, "Function Table");
}

#[test]
fn adr008_amended_confirm_leaves_the_superblock_and_parity_alone() {
    let dev = FaultDevice::from_image(formatted_image(SZ));
    let mut c = CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap();
    dev.begin_recording();
    c.confirm().unwrap();
    let writes: Vec<(u64, usize)> = dev.ops().iter().filter_map(|o| if let Op::Write { offset, data } = o { Some((*offset, data.len())) } else { None }).collect();
    assert_eq!(writes, vec![(SMART_OFF, SMART_TABLE_SIZE)]);
}

// ---- DECIDED in G2, not implemented ----

#[test]
#[ignore = "G2 DECIDED: anchor structures are 4096 B and 4096-aligned; pointer blocks LBA0-3 padded to 4096"]
fn g2_pointer_blocks_are_4096_bytes_and_4096_aligned() {
    for slot in 0..SLOT_COUNT as u8 {
        assert_eq!(slot_block_size(slot), 4096, "slot {slot} size");
        assert_eq!(head_slot_offset(slot).0 % 4096, 0, "slot {slot} alignment");
    }
}

#[test]
#[ignore = "G2 DECIDED: anchor region 16 KiB -> 24 KiB"]
fn g2_anchor_region_is_24_kib() {
    assert_eq!(ANCHOR_REGION_BYTES, 24 * 1024);
}

#[test]
#[ignore = "G2 DECIDED: all anchor writes go through one function writing whole 4096-aligned units"]
fn g2_every_anchor_write_is_a_whole_aligned_unit() {
    let mut img = formatted_image(SZ);
    img[..4096].fill(0); // head slots 0+1 gone, so check() has repairs to write
    let dev = FaultDevice::from_image(img);
    let mut c = CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap();
    dev.begin_recording();
    c.check().unwrap();
    c.confirm().unwrap();
    for op in dev.ops() {
        if let Op::Write { offset, data } = op {
            assert!(offset % 4096 == 0 && data.len() % 4096 == 0, "write of {} B at {offset}", data.len());
        }
    }
}
