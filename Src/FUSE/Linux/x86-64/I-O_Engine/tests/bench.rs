//! Latency of engine operations on the chosen target. Numbers from a
//! disk image are page-cache numbers; point CAFS_TEST_DEVICE at a real
//! partition (see adaptive.rs) for device numbers. Reads are buffered,
//! so a warm run measures the page cache, not the platter.

mod common;
use common::*;

use std::time::{Duration, Instant};

use cafs_io::layout::*;
use cafs_io::{ByteOffset, CafsCtx};

const IMG: u64 = 4 * 1024 * 1024;
const N: usize = 15;

fn stats(mut v: Vec<Duration>) -> (f64, f64, f64) {
    v.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    (ms(v[0]), ms(v[v.len() / 2]), ms(v[v.len() - 1]))
}

fn row(name: &str, v: Vec<Duration>) {
    let (min, med, max) = stats(v);
    println!("{name:<46} {min:>9.3} {med:>9.3} {max:>9.3}");
}

#[test]
fn bench_operation_latency() {
    let t = Target::acquire(IMG);
    println!("\ntarget: {}", t.describe());
    let t0 = Instant::now();
    t.format();
    println!("mkfs_cafs (wipe + format + fsync): {:.1} ms\n", t0.elapsed().as_secs_f64() * 1000.0);
    println!("{:<46} {:>9} {:>9} {:>9}", "operation (ms)", "min", "median", "max");

    let mut v = Vec::new();
    for _ in 0..N {
        let t0 = Instant::now();
        let c = CafsCtx::mount(&t.path, false).unwrap();
        v.push(t0.elapsed());
        drop(c);
    }
    row("mount (16 reads)", v);

    let (mut vc, mut vf) = (Vec::new(), Vec::new());
    for _ in 0..N {
        let mut c = t.mount(true).unwrap();
        let t0 = Instant::now();
        c.check().unwrap();
        vc.push(t0.elapsed());
        let t0 = Instant::now();
        c.confirm().unwrap();
        vf.push(t0.elapsed());
    }
    row("check, clean volume", vc);
    row("confirm (1 write + 1 sync, was 4 + 4)", vf);

    let mut c = t.bypass(true);
    let mut timed = |name: &str, off: u64, len: usize| {
        let mut cur = vec![0u8; len];
        c.read_block(ByteOffset(off), &mut cur).unwrap();
        let mut v = Vec::new();
        for _ in 0..N {
            let t0 = Instant::now();
            c.write_block(ByteOffset(off), &cur).unwrap(); // same bytes: content unchanged
            c.sync().unwrap();
            v.push(t0.elapsed());
        }
        row(name, v);
    };
    timed("write 144 B + sync @ M-SMART (start of disk)", SMART_OFF, SMART_TABLE_SIZE);
    timed("write 2048 B + sync @ LBA0 head (start)", 0, 2048);
    timed("write 4096 B + sync @ LBA4 head (start)", LBA4_OFFSET.0, 4096);
    timed("write 4096 B + sync @ LBA5 tail (END of device)", tail_slot_offset(5, t.size).0, 4096);
    timed("write 4096 B + sync @ Superblock Backup (end)", sb_backup_offset(t.size).0, 4096);
    drop(c);

    let mut v = Vec::new();
    for _ in 0..N {
        t.poke(tail_slot_offset(0, t.size).0, &[0u8; 2048]);
        let mut c = t.mount(true).unwrap();
        let t0 = Instant::now();
        let r = c.check().unwrap();
        v.push(t0.elapsed());
        assert_eq!(r.anchor_slots_repaired, 1);
    }
    row("check + rebuild tail LBA0 (end) incl. 12-block rescan", v);

    t.format();
    println!("\nbuffered I/O: 2048 B vs 4096 B rows compare page-cache behaviour, not raw device RMW.");
}
