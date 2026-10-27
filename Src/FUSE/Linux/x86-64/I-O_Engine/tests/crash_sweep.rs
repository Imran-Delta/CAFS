//! Phase 3 — deterministic power-loss sweeps over the engine's own
//! write sequences, under the FaultDevice crash model.

mod common;
use common::*;

use cafs_io::fault::{CrashModel, FaultDevice};
use cafs_io::CafsCtx;

const SZ: u64 = 1024 * 1024;

fn models() -> Vec<CrashModel> {
    let mut v = vec![CrashModel::DROP_ALL, CrashModel::KEEP_ALL];
    for granule in [0u64, 512, 4096] {
        for seed in 0..8 {
            v.push(CrashModel::torn(granule, seed));
        }
    }
    v
}

/// Mount + check every crash image; return the images that fail.
fn sweep(dev: &FaultDevice) -> (usize, Vec<String>) {
    let n = dev.ops().len();
    let mut failures = Vec::new();
    let mut total = 0;
    for prefix in 0..=n {
        for m in models() {
            total += 1;
            let img = dev.crash_image(prefix, m);
            let label = format!("prefix={prefix}/{n} granule={} keep={}% seed={}", m.granule, m.keep_percent, m.seed);
            match CafsCtx::mount_device(Box::new(FaultDevice::from_image(img)), true) {
                Err(e) => failures.push(format!("{label}: mount failed: {e}")),
                Ok(mut c) if !c.anchors().iter().all(|r| r.head_ok() || r.tail_ok()) => {
                    failures.push(format!("{label}: an anchor slot has no verifying end"));
                    let _ = &mut c;
                }
                Ok(mut c) => match c.check() {
                    Ok(r) if r.superblock_ok => {}
                    Ok(_) => failures.push(format!("{label}: superblock unrecoverable")),
                    Err(e) => failures.push(format!("{label}: check failed: {e}")),
                },
            }
        }
    }
    (total, failures)
}

#[test]
fn sweep_confirm_every_prefix_stays_mountable() {
    let dev = FaultDevice::from_image(formatted_image(SZ));
    let mut ctx = CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap();
    dev.begin_recording();
    ctx.confirm().unwrap();
    let (total, failures) = sweep(&dev);
    println!("confirm(): {} ops, {} crash images, {} failures", dev.ops().len(), total, failures.len());
    assert!(failures.is_empty(), "{:#?}", &failures[..failures.len().min(5)]);
}

#[test]
fn sweep_superblock_repair_stays_mountable() {
    let dev = FaultDevice::from_image(formatted_image(SZ));
    dev.flip_byte(SB_OFF + 100);
    let mut ctx = CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap();
    dev.begin_recording();
    let r = ctx.check().unwrap();
    assert!(r.superblock_repaired);
    let (total, failures) = sweep(&dev);
    println!("check() repair: {} ops, {} crash images, {} failures", dev.ops().len(), total, failures.len());
    assert!(failures.is_empty(), "{:#?}", &failures[..failures.len().min(5)]);
}

#[test]
fn char_smart_checksum_is_not_verified_before_read_modify_write() {
    // A corrupted M-SMART table is read, incremented and rewritten
    // with a freshly computed (valid) checksum: the corruption is
    // laundered into a table that now verifies.
    let dev = FaultDevice::from_image(formatted_image(SZ));
    dev.flip_byte(SMART_OFF + 20); // inside total_reads, hashed range
    let before = dev.visible_image();
    assert!(!blake3_128_ok(&before[SMART_OFF as usize..], 128), "setup: table is corrupt");
    let mut ctx = CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap();
    ctx.confirm().unwrap();
    let after = dev.visible_image();
    assert!(blake3_128_ok(&after[SMART_OFF as usize..], 128), "table now verifies");
    assert_eq!(after[SMART_OFF as usize + 20], before[SMART_OFF as usize + 20], "corrupt byte preserved");
}

#[test]
fn sweep_pointer_repair_stays_mountable() {
    let dev = FaultDevice::from_image(formatted_image(SZ));
    dev.poke(0, &[0u8; 4096]); // LBA0 + LBA1 heads gone
    let mut ctx = CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap();
    dev.begin_recording();
    let r = ctx.check().unwrap();
    assert_eq!(r.anchor_slots_repaired, 2);
    let (total, failures) = sweep(&dev);
    println!("pointer repair: {} ops, {} crash images, {} failures", dev.ops().len(), total, failures.len());
    assert!(failures.is_empty(), "{:#?}", &failures[..failures.len().min(5)]);
}
