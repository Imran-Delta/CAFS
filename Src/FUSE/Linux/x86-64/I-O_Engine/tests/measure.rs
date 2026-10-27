//! Phase 4 — what the engine costs in device operations. Counts are
//! deterministic (device-independent); latency is not measured here.

mod common;
use common::*;

use cafs_io::CafsCtx;

const SZ: u64 = 1024 * 1024;

#[test]
fn measure_operation_costs() {
    let img = formatted_image(SZ);

    let dev = cafs_io::fault::FaultDevice::from_image(img.clone());
    let c0 = dev.counters();
    let mut ctx = CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap();
    let mount = dev.counters().since(c0);

    let c0 = dev.counters();
    let r = ctx.check().unwrap();
    let check_clean = dev.counters().since(c0);
    assert!(r.superblock_ok && !r.superblock_repaired);

    let c0 = dev.counters();
    ctx.confirm().unwrap();
    let confirm = dev.counters().since(c0);

    let dev2 = cafs_io::fault::FaultDevice::from_image(img.clone());
    dev2.flip_byte(SB_OFF + 100);
    let mut ctx2 = CafsCtx::mount_device(Box::new(dev2.clone()), true).unwrap();
    let c0 = dev2.counters();
    ctx2.check().unwrap();
    let check_repair = dev2.counters().since(c0);

    let dev3 = cafs_io::fault::FaultDevice::from_image(img);
    dev3.poke(0, &[0u8; 4096]);
    let mut ctx3 = CafsCtx::mount_device(Box::new(dev3.clone()), true).unwrap();
    let c0 = dev3.counters();
    ctx3.check().unwrap();
    let check_ptr = dev3.counters().since(c0);

    println!("\nop                  reads  writes  syncs  bytes_read  bytes_written");
    for (name, c) in [("mount", mount), ("check clean", check_clean), ("check repair SB", check_repair), ("check repair 2 ptrs", check_ptr), ("confirm", confirm)] {
        println!("{name:<19} {:>5}  {:>6}  {:>5}  {:>10}  {:>13}", c.reads, c.writes, c.syncs, c.bytes_read, c.bytes_written);
    }

    // Pinned numbers for the code as it stands at this commit.
    // BEFORE (as uploaded, same harness): mount 8 reads / 18656 B;
    // check repair SB 1 write, 0 syncs; confirm 4 writes, 4 syncs, 12432 B.
    assert_eq!((mount.reads, mount.bytes_read, mount.writes), (16, 41184, 0));
    assert_eq!((confirm.writes, confirm.syncs, confirm.bytes_written), (1, 1, 144));
    assert_eq!((check_repair.writes, check_repair.syncs, check_repair.bytes_written), (1, 1, 4096));
    assert_eq!((check_ptr.writes, check_ptr.syncs, check_ptr.bytes_written), (2, 2, 4096));
    assert_eq!((check_clean.writes, check_clean.syncs), (0, 0));
}
