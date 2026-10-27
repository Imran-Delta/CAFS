//! mkfs_cafs — CAFS V1 format tool. Writes a fresh, spec-compliant
//! anchor region so the engine has something valid to mount. Rust
//! port of the retired C `mkfs_cafs.c`; device layout unchanged, see
//! that file's history for the original layout diagram.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::os::unix::fs::{FileExt, OpenOptionsExt};
use cafs_io::anchor::{build_ptr_block, PtrAlgo};
use cafs_io::layout::*;

const OFF_SUPERBLOCK: u64 = 16384;
const OFF_CONFIG_SNAPSHOT: u64 = 20480;
const OFF_PARITY: u64 = 24576;
const OFF_META_PLACEHOLDER: u64 = 28672;
const OFF_FUNCTION_TABLE: u64 = 32768;
const OFF_SMART_MAIN: u64 = 36864;
const FRONT_REGION_SIZE: u64 = 40960;
// End of device (fs.info §12): [B-SMART][Superblock Backup][tail anchors].
const END_REGION_SIZE: u64 = END_REGION_BYTES;
const MIN_DEVICE_SIZE: u64 = 1024 * 1024;
const DEFAULT_DEVICE_SIZE: u64 = 4 * 1024 * 1024;

/// (slot, real offset, real size, optional fallback (offset, size))
type HeadSlot = (u8, u64, u64, Option<(u64, u64)>);

fn die(msg: &str) -> ! {
    eprintln!("mkfs.cafs: {msg}");
    std::process::exit(1);
}

fn blake3_128(data: &[u8]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(data);
    let mut out = [0u8; 16];
    hasher.finalize_xof().fill(&mut out);
    out
}

fn build_superblock() -> [u8; REAL_BLOCK_4K as usize] {
    let mut sb = [0u8; REAL_BLOCK_4K as usize];
    sb[SB_OFF_MAGIC..SB_OFF_MAGIC + 4].copy_from_slice(&SUPERBLOCK_MAGIC.to_le_bytes());
    sb[SB_OFF_FORMAT_VERSION..SB_OFF_FORMAT_VERSION + 4].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    for i in 0..16u8 {
        sb[SB_OFF_VOLUME_UUID + i as usize] = 0xA0 + i; // deterministic test UUID
    }
    sb[SB_OFF_CONFIG_GENERATION..SB_OFF_CONFIG_GENERATION + 8].copy_from_slice(&1u64.to_le_bytes());
    // Slot INDEX (4), not a byte offset — ADR-010 resolution #4.
    sb[SB_OFF_FUNCTION_TABLE_ANCHOR_SLOT..SB_OFF_FUNCTION_TABLE_ANCHOR_SLOT + 8].copy_from_slice(&4u64.to_le_bytes());
    // wal_sequence, last_mount_time: 0 (never mounted yet).
    sb[SB_OFF_DATA_CHECKSUM_ALGO..SB_OFF_DATA_CHECKSUM_ALGO + 4].copy_from_slice(&DATA_ALGO_BLAKE3.to_le_bytes());
    sb[SB_OFF_ANCHOR_CHECKSUM_ALGO..SB_OFF_ANCHOR_CHECKSUM_ALGO + 4].copy_from_slice(&ANCHOR_ALGO_BLAKE3_128.to_le_bytes());
    sb[SB_OFF_POINTER_CHECKSUM_ALGO..SB_OFF_POINTER_CHECKSUM_ALGO + 4].copy_from_slice(&PTR_ALGO_CRC32C.to_le_bytes());
    // last_applied_wal_seq: 0, root_hash_algo: 0.
    let cksum = blake3_128(&sb[0..SB_HASHED_LEN]);
    sb[SB_OFF_CHECKSUM..SB_OFF_CHECKSUM + SB_CHECKSUM_LEN].copy_from_slice(&cksum);
    sb
}

fn build_config_snapshot() -> [u8; REAL_BLOCK_4K as usize] {
    let mut cs = [0u8; REAL_BLOCK_4K as usize];
    let toml = b"[identity]\nname = \"test-volume\"\n\n[physical]\nblock_size = 4096\npointer_checksum_algo = \"crc32c\"\n\n[integrity]\ndata_checksum_algo = \"blake3\"\nverify_data = true\n";
    if toml.len() >= CFGSNAP_HASHED_LEN {
        die("internal: config snapshot template too large");
    }
    cs[0..toml.len()].copy_from_slice(toml);
    let cksum = blake3_128(&cs[0..CFGSNAP_HASHED_LEN]);
    cs[CFGSNAP_OFF_CHECKSUM..CFGSNAP_OFF_CHECKSUM + CFGSNAP_CHECKSUM_LEN].copy_from_slice(&cksum);
    cs
}

fn build_parity(sb: &[u8; REAL_BLOCK_4K as usize], cs: &[u8; REAL_BLOCK_4K as usize]) -> [u8; REAL_BLOCK_4K as usize] {
    let mut p = [0u8; REAL_BLOCK_4K as usize];
    for i in 0..REAL_BLOCK_4K as usize {
        p[i] = sb[i] ^ cs[i];
    }
    p
}

fn build_function_table(smart_main_lba: u64, smart_backup_lba: u64) -> [u8; REAL_BLOCK_4K as usize] {
    let mut ft = [0u8; REAL_BLOCK_4K as usize];
    ft[FT_OFF_SMART_MAIN_LBA..FT_OFF_SMART_MAIN_LBA + 8].copy_from_slice(&smart_main_lba.to_le_bytes());
    ft[FT_OFF_SMART_BACKUP_LBA..FT_OFF_SMART_BACKUP_LBA + 8].copy_from_slice(&smart_backup_lba.to_le_bytes());
    let cksum = blake3_128(&ft[0..FT_HASHED_LEN]);
    ft[FT_OFF_CHECKSUM..FT_OFF_CHECKSUM + FT_CHECKSUM_LEN].copy_from_slice(&cksum);
    ft
}

fn build_smart() -> [u8; REAL_BLOCK_4K as usize] {
    let mut sm = [0u8; REAL_BLOCK_4K as usize];
    sm[SMART_OFF_MAGIC..SMART_OFF_MAGIC + 4].copy_from_slice(&SMART_MAGIC.to_le_bytes());
    sm[SMART_OFF_VERSION..SMART_OFF_VERSION + 4].copy_from_slice(&1u32.to_le_bytes());
    sm[SMART_OFF_SEQUENCE..SMART_OFF_SEQUENCE + 8].copy_from_slice(&1u64.to_le_bytes());
    sm[SMART_OFF_CLEAN_UNMOUNT_FLAG] = 1; // freshly formatted = clean
    let cksum = blake3_128(&sm[0..SMART_HASHED_LEN]);
    sm[SMART_OFF_CHECKSUM..SMART_OFF_CHECKSUM + SMART_CHECKSUM_LEN].copy_from_slice(&cksum);
    sm
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let force_device = args.iter().any(|a| a == "--force-device");
    args.retain(|a| a != "--force-device");
    if args.is_empty() || args.len() > 2 {
        eprintln!(
            "usage: mkfs_cafs <path> [size_bytes] [--force-device]\n  \
             regular file : created/truncated to size_bytes (default {DEFAULT_DEVICE_SIZE}), then formatted\n  \
             block device : size comes from the device; needs --force-device. Refused if it is mounted,\n                 active swap, a whole disk with partitions, or holds a non-empty/non-swap/non-CAFS\n                 filesystem signature (wipe it yourself with `wipefs -a`)."
        );
        std::process::exit(1);
    }
    let path = &args[0];
    let is_blk = std::fs::metadata(path).map(|m| std::os::unix::fs::FileTypeExt::is_block_device(&m.file_type())).unwrap_or(false);

    let (f, size) = if is_blk {
        if !force_device {
            die("target is a block device; pass --force-device to format it");
        }
        let info = cafs_io::safety::check_target(path, &cafs_io::safety::Signature::OVERWRITABLE).unwrap_or_else(|e| die(&e));
        if args.len() == 2 && args[1].parse::<u64>().ok() != Some(info.size) {
            die(&format!("size argument does not match the device size ({} bytes); omit it", info.size));
        }
        // O_EXCL: the kernel's own refusal if anything still holds it.
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(rustix::fs::OFlags::EXCL.bits() as i32)
            .open(path)
            .unwrap_or_else(|e| die(&format!("could not open {path} exclusively: {e}")));
        (f, info.size)
    } else {
        let size: u64 = if args.len() == 2 {
            args[1].parse().unwrap_or_else(|_| die("invalid size"))
        } else {
            DEFAULT_DEVICE_SIZE
        };
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap_or_else(|_| die("could not create/open target path"));
        f.set_len(size).unwrap_or_else(|_| die("could not extend target to full size"));
        (f, size)
    };
    if size < MIN_DEVICE_SIZE {
        die("size too small (minimum 1 MiB)");
    }
    if size < FRONT_REGION_SIZE + END_REGION_SIZE + 4096 {
        die("size too small to fit the front and end regions without overlap");
    }

    let sb_backup_off = sb_backup_offset(size).0;
    let bsmart_off = smart_backup_offset(size).0;

    let sb = build_superblock();
    let cs = build_config_snapshot();
    let parity = build_parity(&sb, &cs);
    let meta = [0u8; REAL_BLOCK_4K as usize]; // placeholder; never read by the V1 engine
    let ft = build_function_table(OFF_SMART_MAIN, bsmart_off);
    let smart = build_smart();

    let algo = PtrAlgo::Crc32c;
    let rb = REAL_BLOCK_4K as u64;
    // (slot, real offset, real size, fallback) — head copy (fs.info §2).
    let head: [HeadSlot; 6] = [
        (0, OFF_SUPERBLOCK, rb, Some((sb_backup_off, rb))),
        (1, OFF_CONFIG_SNAPSHOT, rb, None),
        (2, OFF_PARITY, rb, None),
        (3, OFF_META_PLACEHOLDER, rb, None),
        (4, OFF_FUNCTION_TABLE, FT_MIN_REAL_SIZE, None),
        (5, OFF_FUNCTION_TABLE, FT_MIN_REAL_SIZE, None), // byte-for-byte copy of LBA4
    ];

    let write_at = |off: u64, buf: &[u8]| {
        f.write_all_at(buf, off).unwrap_or_else(|_| die("write failed (target too small, or permission denied)"));
    };

    // Real structures first (fs.info §13.2 step 2), then the tail
    // mirror, then the head (steps 5-6).
    write_at(OFF_SUPERBLOCK, &sb);
    write_at(OFF_CONFIG_SNAPSHOT, &cs);
    write_at(OFF_PARITY, &parity);
    write_at(OFF_META_PLACEHOLDER, &meta);
    write_at(OFF_FUNCTION_TABLE, &ft);
    write_at(OFF_SMART_MAIN, &smart);
    write_at(sb_backup_off, &sb);
    write_at(bsmart_off, &smart);
    f.sync_data().ok();

    for (slot, real, rsize, fb) in head {
        let len = slot_block_size(slot);
        // Tail LBA0 swaps primary and fallback: its primary is the
        // Superblock Backup, its fallback the real Superblock (§12).
        let tail = if slot == 0 {
            build_ptr_block(len, algo, sb_backup_off, rb, Some((OFF_SUPERBLOCK, rb)))
        } else {
            build_ptr_block(len, algo, real, rsize, fb)
        };
        write_at(tail_slot_offset(slot, size).0, &tail);
    }
    f.sync_data().ok();
    for (slot, real, rsize, fb) in head {
        let hd = build_ptr_block(slot_block_size(slot), algo, real, rsize, fb);
        write_at(head_slot_offset(slot).0, &hd);
    }

    f.sync_data().ok();
    println!("mkfs.cafs: formatted {path} ({size} bytes)");
}
