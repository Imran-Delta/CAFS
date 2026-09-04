//! Exercises the built library against a real image produced by
//! `mkfs_cafs`, mirroring the retired C engine's `test_pipeline.c`
//! test-for-test so this rewrite is checked against the same
//! behavior, not a new/looser spec.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::process::Command;

use cafs_io::layout::PTR_ALGO_CRC32C;
use cafs_io::{CafsCtx, MisalignedAction, MountOpts};

const IMG_SIZE: u64 = 4 * 1024 * 1024;

fn mkfs(path: &str) {
    let bin = env!("CARGO_BIN_EXE_mkfs_cafs");
    let status = Command::new(bin)
        .arg(path)
        .arg(IMG_SIZE.to_string())
        .status()
        .expect("failed to run mkfs_cafs");
    assert!(status.success(), "mkfs_cafs exited non-zero");
}

fn corrupt_byte(path: &str, offset: u64) {
    use std::os::unix::fs::FileExt;
    let f = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
    let mut b = [0u8; 1];
    f.read_exact_at(&mut b, offset).unwrap();
    b[0] ^= 0xFF;
    f.write_all_at(&b, offset).unwrap();
}

fn read_byte(path: &str, offset: u64) -> u8 {
    use std::os::unix::fs::FileExt;
    let f = std::fs::File::open(path).unwrap();
    let mut b = [0u8; 1];
    f.read_exact_at(&mut b, offset).unwrap();
    b[0]
}

#[test]
fn full_pipeline() {
    // (Checksum known-vectors are covered by `cargo test --lib`;
    // not re-verified here to avoid a test-only public API surface.)

    let dir = tempfile::tempdir().unwrap();
    let img = dir.path().join("cafs_test.img");
    let img_path = img.to_str().unwrap();

    mkfs(img_path);

    // --- fresh mount / check / remount / confirm ---
    let mut ctx = CafsCtx::mount(img_path, true).expect("mount succeeds on a freshly formatted image");

    let chk = ctx.check().expect("check() returns Ok");
    assert!(chk.superblock_ok, "superblock_ok on a clean image");
    assert!(!chk.superblock_repaired, "no repair needed on a clean image");
    assert_eq!(
        chk.pointer_checksum_algo_used,
        Some(cafs_io::PtrAlgo::Crc32c),
        "bootstrap picked CRC32C (the format-time default)"
    );

    let opts = MountOpts {
        readonly: false,
        direct_io: false,
        direct_io_required: false,
        sync_open: false,
        misaligned_action: MisalignedAction::Copy,
        destroy_flush: true,
    };
    let snap = ctx.remount(&opts).expect("remount() returns Ok");
    assert_eq!(snap.block_size, Some(4096));
    assert_eq!(snap.pointer_checksum_algo_id, Some(PTR_ALGO_CRC32C));
    assert_eq!(snap.data_checksum_algo_id, Some(cafs_io::layout::DATA_ALGO_BLAKE3));
    assert_eq!(snap.verify_data, Some(true));

    ctx.confirm().expect("confirm() returns Ok");

    // --- raw block read/write proof of concept ---
    let wbuf = [0xABu8; 512];
    let scratch_off = cafs_io::ByteOffset(512 * 1024);
    ctx.write_block(scratch_off, &wbuf).expect("write_block into free space");
    let mut rbuf = [0u8; 512];
    ctx.read_block(scratch_off, &mut rbuf).expect("read_block back");
    assert_eq!(wbuf, rbuf, "read_block returns exactly what write_block wrote");

    let sc = ctx.smart_counters();
    assert!(sc.total_reads >= 1 && sc.total_writes >= 1, "in-RAM SMART counters incremented");

    ctx.close().unwrap();

    // --- re-mount after confirm(): last_mount_time persisted, checksum still valid ---
    let mut ctx = CafsCtx::mount(img_path, false).expect("re-mount (readonly) succeeds");
    let chk = ctx.check().unwrap();
    assert!(chk.superblock_ok && !chk.superblock_repaired, "re-check() still passes after confirm()'s Superblock rewrite");
    ctx.close().unwrap();

    // --- corruption + auto-repair ---
    // Primary Superblock lives at byte offset 16384; corrupt a byte
    // well inside its hashed range (0-4079) so the checksum breaks.
    corrupt_byte(img_path, 16384 + 100);

    let mut ctx = CafsCtx::mount(img_path, true).expect("mount still succeeds with a corrupted primary Superblock");
    let chk = ctx.check().expect("check() returns Ok even on a corrupted Superblock (no crash)");
    assert!(chk.superblock_ok, "superblock_ok after repair");
    assert!(chk.superblock_repaired, "superblock_repaired == true");
    ctx.close().unwrap();

    let fixed_byte = read_byte(img_path, 16384 + 100);
    let backup_byte = read_byte(img_path, IMG_SIZE - 4096 + 100);
    assert_eq!(fixed_byte, backup_byte, "primary Superblock byte 100 was actually rewritten to match the backup on disk");

    // --- re-check after repair: no repair needed the second time ---
    let mut ctx = CafsCtx::mount(img_path, true).expect("mount succeeds after repair was persisted");
    let chk = ctx.check().unwrap();
    assert!(chk.superblock_ok && !chk.superblock_repaired, "primary now clean, no repair triggered");
    ctx.close().unwrap();
}
