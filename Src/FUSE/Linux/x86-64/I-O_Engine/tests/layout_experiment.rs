//! Step 2a — does it matter whether anchor pointer blocks share a
//! physical sector? Three layouts are put through the same repair
//! scenario under the same crash models, with and without sector-level
//! hazards (a sector write that is not atomic, plus the drive's
//! read-modify-write window for partial-sector writes).
//!
//!   PairedPartial  today's engine: 2048 B blocks, two per sector, 2048 B writes
//!   PairedUnit     same layout, every write is the whole 4096 B unit
//!   PerSector      G2 as decided: each block padded to its own 4096 B sector
//!
//! Scenario: slot 0's head copy is damaged and slot 1's tail copy has
//! latent damage (undetected until now). check() rebuilds both, in that
//! order, each write followed by a sync. A crash may land anywhere.
//! "Lost" = a slot with no verifying copy at either end afterwards.
//!
//! The PairedPartial simulation is validated against the REAL engine:
//! same op sequence and crash models must give identical counts.
//!
//! The hazard model is an assumption about devices, not a measurement.

mod common;
use common::*;

use cafs_io::anchor::verify_ptr_block;
use cafs_io::device::Device;
use cafs_io::fault::{CrashModel, FaultDevice};
use cafs_io::layout::*;
use cafs_io::CafsCtx;

const SZ: u64 = 1024 * 1024;
const SECTOR: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    PairedPartial,
    PairedUnit,
    PerSector,
}

impl Layout {
    fn region(self) -> u64 {
        if self == Layout::PerSector { 6 * 4096 } else { 16384 }
    }
    fn head(self, slot: u8) -> u64 {
        if self == Layout::PerSector { slot as u64 * 4096 } else { head_slot_offset(slot).0 }
    }
    fn tail(self, slot: u8) -> u64 {
        SZ - self.region() + self.head(slot)
    }
    fn stored(self, slot: u8) -> usize {
        if self == Layout::PerSector { 4096 } else { slot_block_size(slot) as usize }
    }
}

fn block_len(slot: u8) -> usize {
    slot_block_size(slot) as usize
}

/// The six pointer blocks as `mkfs_cafs` writes them.
fn blocks() -> Vec<Vec<u8>> {
    let img = formatted_image(SZ);
    (0..6u8)
        .map(|s| img[head_slot_offset(s).0 as usize..head_slot_offset(s).0 as usize + block_len(s)].to_vec())
        .collect()
}

fn build(layout: Layout, blocks: &[Vec<u8>]) -> Vec<u8> {
    let mut img = vec![0u8; SZ as usize];
    for s in 0..6u8 {
        for off in [layout.head(s), layout.tail(s)] {
            img[off as usize..off as usize + blocks[s as usize].len()].copy_from_slice(&blocks[s as usize]);
        }
    }
    img
}

fn ok_at(img: &[u8], off: u64, slot: u8) -> bool {
    verify_ptr_block(&img[off as usize..off as usize + block_len(slot)]).is_ok()
}

/// (slots with no verifying copy, slot 1's head copy damaged)
fn evaluate(img: &[u8], layout: Layout) -> (usize, bool) {
    let lost = (0..6u8).filter(|&s| !ok_at(img, layout.head(s), s) && !ok_at(img, layout.tail(s), s)).count();
    (lost, !ok_at(img, layout.head(1), 1))
}

fn write_block(layout: Layout, dev: &FaultDevice, off: u64, block: &[u8]) {
    let mut d = dev.clone();
    match layout {
        Layout::PairedPartial => d.write_at(off, block).unwrap(),
        Layout::PairedUnit => {
            let unit = off / SECTOR * SECTOR;
            let mut buf = vec![0u8; SECTOR as usize];
            d.read_at(unit, &mut buf).unwrap();
            buf[(off - unit) as usize..(off - unit) as usize + block.len()].copy_from_slice(block);
            d.write_at(unit, &buf).unwrap();
        }
        Layout::PerSector => {
            let mut buf = block.to_vec();
            buf.resize(4096, 0);
            d.write_at(off, &buf).unwrap();
        }
    }
    d.clone().sync().unwrap();
}

fn models(hazard: bool) -> Vec<CrashModel> {
    let mut base = vec![CrashModel::DROP_ALL, CrashModel::KEEP_ALL];
    for granule in [0u64, 512, 4096] {
        for seed in 0..64 {
            base.push(CrashModel::torn(granule, seed));
        }
    }
    base.into_iter().map(|m| if hazard { m.with_hazard(SECTOR, 25, 25) } else { m }).collect()
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct Outcome {
    images: usize,
    lost: usize,
    collateral: usize,
}

fn sweep(dev: &FaultDevice, layout: Layout, hazard: bool) -> Outcome {
    let mut o = Outcome::default();
    for prefix in 0..=dev.ops().len() {
        for m in models(hazard) {
            let (lost, collateral) = evaluate(&dev.crash_image(prefix, m), layout);
            o.images += 1;
            o.lost += (lost > 0) as usize;
            o.collateral += collateral as usize;
        }
    }
    o
}

fn simulate(layout: Layout, blocks: &[Vec<u8>]) -> (FaultDevice, cafs_io::fault::Counters) {
    let dev = FaultDevice::from_image(build(layout, blocks));
    dev.poke(layout.head(0), &vec![0u8; layout.stored(0)]); // damaged head copy
    dev.poke(layout.tail(1), &vec![0u8; layout.stored(1)]); // latent damage
    dev.begin_recording();
    write_block(layout, &dev, layout.head(0), &blocks[0]);
    write_block(layout, &dev, layout.tail(1), &blocks[1]);
    let c = dev.counters();
    (dev, c)
}

fn real_engine() -> FaultDevice {
    let mut img = formatted_image(SZ);
    img[head_slot_offset(0).0 as usize..][..2048].fill(0);
    img[tail_slot_offset(1, SZ).0 as usize..][..2048].fill(0);
    let dev = FaultDevice::from_image(img);
    let mut c = CafsCtx::mount_device(Box::new(dev.clone()), true).unwrap();
    dev.begin_recording();
    let r = c.check().unwrap();
    assert_eq!((r.anchor_slots_degraded, r.anchor_slots_repaired), (2, 2));
    dev
}

#[test]
fn sector_sharing_layouts_compared_under_crash_and_sector_hazards() {
    let blocks = blocks();
    let layouts = [Layout::PairedPartial, Layout::PairedUnit, Layout::PerSector];

    println!("\nscenario: repair slot 0 head, then slot 1 tail (slot 1 head shares slot 0's sector in the paired layouts)");
    println!("{:<14} {:>8} {:>7} {:>7} {:>7}   {:>22}   {:>22}", "layout", "region", "writes", "bytes", "syncs", "no hazard: lost/collat.", "hazard: lost/collat.");
    let mut res = Vec::new();
    for l in layouts {
        let (dev, c) = simulate(l, &blocks);
        let (h0, h1) = (sweep(&dev, l, false), sweep(&dev, l, true));
        println!(
            "{:<14} {:>6}Ki {:>7} {:>7} {:>7}   {:>10}/{:<11}   {:>10}/{:<11}",
            format!("{l:?}"), l.region() / 1024, c.writes, c.bytes_written, c.syncs, h0.lost, h0.collateral, h1.lost, h1.collateral
        );
        res.push((l, h0, h1));
    }
    println!("(out of {} crash images per cell; hazard = 25% sector destroyed per touched sector, +25% if partially covered)", res[0].1.images);

    for (l, h0, _) in &res {
        assert_eq!(h0.lost, 0, "{l:?}: without sector hazards the two-ended protocol never loses a slot");
    }
    let get = |l: Layout| res.iter().find(|r| r.0 == l).unwrap();
    assert!(get(Layout::PairedPartial).2.lost > 0, "partial writes into a shared sector can lose a sibling slot");
    assert!(get(Layout::PairedUnit).2.lost > 0, "whole-unit writes do not remove the shared-sector exposure");
    assert_eq!(get(Layout::PerSector).2.lost, 0, "one slot per sector: nothing shared, nothing lost");
    assert!(get(Layout::PairedPartial).2.lost >= get(Layout::PairedUnit).2.lost);

    // Validate the simulation against the real engine on the same scenario.
    let engine = real_engine();
    let sim = simulate(Layout::PairedPartial, &blocks).0;
    assert_eq!(engine.ops(), sim.ops(), "engine and simulation issue the same writes");
    for hazard in [false, true] {
        assert_eq!(
            sweep(&engine, Layout::PairedPartial, hazard),
            sweep(&sim, Layout::PairedPartial, hazard),
            "real engine and simulation agree (hazard={hazard})"
        );
    }
}
