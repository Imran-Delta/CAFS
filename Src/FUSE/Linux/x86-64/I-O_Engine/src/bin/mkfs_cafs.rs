//! mkfs_cafs — CAFS V1 format tool. Writes a fresh, spec-compliant
//! anchor region so the engine has something valid to mount. Rust
//! port of the retired C `mkfs_cafs.c`; device layout unchanged, see
//! that file's history for the original layout diagram.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::os::unix::fs::FileExt;
use cafs_io::layout::*;

const OFF_SUPERBLOCK: u64 = 16384;
const OFF_CONFIG_SNAPSHOT: u64 = 20480;
const OFF_PARITY: u64 = 24576;
const OFF_META_PLACEHOLDER: u64 = 28672;
const OFF_FUNCTION_TABLE: u64 = 32768;
const OFF_SMART_MAIN: u64 = 36864;
const FRONT_REGION_SIZE: u64 = 40960;
const END_REGION_SIZE: u64 = 8192;
const MIN_DEVICE_SIZE: u64 = 1024 * 1024;
const DEFAULT_DEVICE_SIZE: u64 = 4 * 1024 * 1024;

fn die(msg: &str) -> ! {
    eprintln!("mkfs.cafs: {msg}");
    std::process::exit(1);
}

fn build_ptr_block(block_len: u32, real_offset: u64, real_size: u64, fallback: Option<(u64, u64)>) -> Vec<u8> {
    let mut b = vec![0u8; block_len as usize];
    let small = block_len == PTR_BLOCK_SMALL_SIZE;
    let (m, rlba, rsize, cksum_off, hashed_len) = if small {
        (PTR2K_OFF_MAGIC, PTR2K_OFF_REAL_LBA, PTR2K_OFF_REAL_SIZE, PTR2K_OFF_CHECKSUM, PTR2K_PRIMARY_HASHED_LEN)
    } else {
        (PTR4K_OFF_MAGIC, PTR4K_OFF_REAL_LBA, PTR4K_OFF_REAL_SIZE, PTR4K_OFF_CHECKSUM, PTR4K_PRIMARY_HASHED_LEN)
    };
    b[m..m + 8].copy_from_slice(&PTR_MAGIC);
    b[rlba..rlba + 8].copy_from_slice(&real_offset.to_le_bytes());
    b[rsize..rsize + 8].copy_from_slice(&real_size.to_le_bytes());
    let c = crc32c::crc32c(&b[0..hashed_len]);
    b[cksum_off..cksum_off + 4].copy_from_slice(&c.to_le_bytes());

    if let Some((fb_off, fb_size)) = fallback {
        let (fm, flba, fsize, fcksum_off, fhashed_len) = if small {
            (PTR2K_OFF_FALLBACK_MAGIC, PTR2K_OFF_FALLBACK_REAL_LBA, PTR2K_OFF_FALLBACK_REAL_SIZE, PTR2K_OFF_FALLBACK_CHECKSUM, PTR2K_FALLBACK_HASHED_LEN)
        } else {
            (PTR4K_OFF_FALLBACK_MAGIC, PTR4K_OFF_FALLBACK_REAL_LBA, PTR4K_OFF_FALLBACK_REAL_SIZE, PTR4K_OFF_FALLBACK_CHECKSUM, PTR4K_FALLBACK_HASHED_LEN)
        };
        b[fm..fm + 8].copy_from_slice(&PTR_MAGIC);
        b[flba..flba + 8].copy_from_slice(&fb_off.to_le_bytes());
        b[fsize..fsize + 8].copy_from_slice(&fb_size.to_le_bytes());
        let fc = crc32c::crc32c(&b[fm..fm + fhashed_len]);
        b[fcksum_off..fcksum_off + 4].copy_from_slice(&fc.to_le_bytes());
    }
    b
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
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args.len() > 3 {
        eprintln!(
            "usage: mkfs_cafs <path> [size_bytes]\n  Creates/truncates <path> as a regular file of the given size\n  (default {DEFAULT_DEVICE_SIZE} bytes) and writes a fresh CAFS V1 anchor region."
        );
        std::process::exit(1);
    }
    let path = &args[1];
    let size: u64 = if args.len() == 3 {
        args[2].parse().unwrap_or_else(|_| die("invalid size"))
    } else {
        DEFAULT_DEVICE_SIZE
    };
    if size < MIN_DEVICE_SIZE {
        die("size too small (minimum 1 MiB)");
    }
    if size < FRONT_REGION_SIZE + END_REGION_SIZE + 4096 {
        die("size too small to fit the front and end regions without overlap");
    }

    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap_or_else(|_| die("could not create/open target path"));
    f.set_len(size).unwrap_or_else(|_| die("could not extend target to full size"));

    let sb_backup_off = size - 4096;
    let bsmart_off = size - 8192;

    let sb = build_superblock();
    let cs = build_config_snapshot();
    let parity = build_parity(&sb, &cs);
    let meta = [0u8; REAL_BLOCK_4K as usize]; // placeholder; never read by the V1 engine
    let ft = build_function_table(OFF_SMART_MAIN, bsmart_off);
    let smart = build_smart();

    let lba0 = build_ptr_block(PTR_BLOCK_SMALL_SIZE, OFF_SUPERBLOCK, REAL_BLOCK_4K as u64, Some((sb_backup_off, REAL_BLOCK_4K as u64)));
    let lba1 = build_ptr_block(PTR_BLOCK_SMALL_SIZE, OFF_CONFIG_SNAPSHOT, REAL_BLOCK_4K as u64, None);
    let lba2 = build_ptr_block(PTR_BLOCK_SMALL_SIZE, OFF_PARITY, REAL_BLOCK_4K as u64, None);
    let lba3 = build_ptr_block(PTR_BLOCK_SMALL_SIZE, OFF_META_PLACEHOLDER, REAL_BLOCK_4K as u64, None);
    let lba4 = build_ptr_block(PTR_BLOCK_LARGE_SIZE, OFF_FUNCTION_TABLE, FT_MIN_REAL_SIZE, None);
    let lba5 = lba4.clone(); // byte-for-byte copy, fs.info §2

    let write_at = |off: u64, buf: &[u8]| {
        f.write_all_at(buf, off).unwrap_or_else(|_| die("write failed (target too small, or permission denied)"));
    };

    write_at(LBA0_OFFSET.0, &lba0);
    write_at(LBA1_OFFSET.0, &lba1);
    write_at(LBA2_OFFSET.0, &lba2);
    write_at(LBA3_OFFSET.0, &lba3);
    write_at(LBA4_OFFSET.0, &lba4);
    write_at(LBA5_OFFSET.0, &lba5);

    write_at(OFF_SUPERBLOCK, &sb);
    write_at(OFF_CONFIG_SNAPSHOT, &cs);
    write_at(OFF_PARITY, &parity);
    write_at(OFF_META_PLACEHOLDER, &meta);
    write_at(OFF_FUNCTION_TABLE, &ft);
    write_at(OFF_SMART_MAIN, &smart);

    write_at(sb_backup_off, &sb);
    write_at(bsmart_off, &smart);

    f.sync_data().ok();
    println!("mkfs.cafs: formatted {path} ({size} bytes)");
}
