//! Fault-injecting in-memory [`Device`] for deterministic tests.
//!
//! Model (an ASSUMPTION about devices, not a measurement of any real
//! one — it is the model every crash test here is proved under):
//!  * writes land in a volatile cache; reads see them immediately;
//!  * `sync()` is the only durability barrier;
//!  * on power loss, writes issued since the last `sync()` may land
//!    or not, independently per "granule" (whole write, 512 B, 4096 B);
//!  * writes before the last `sync()` are durable and intact.
//!
//! [`FaultDevice`] is a cheap clone-able handle onto shared state, so
//! a test can hand one clone to the engine and keep another to inspect
//! counters, the op log and crash images.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::sync::{Arc, Mutex};

use crate::device::Device;
use crate::error::{CafsError, CafsResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Write { offset: u64, data: Vec<u8> },
    Sync,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub reads: u64,
    pub writes: u64,
    pub syncs: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
}

impl Counters {
    pub fn since(self, earlier: Counters) -> Counters {
        Counters {
            reads: self.reads - earlier.reads,
            writes: self.writes - earlier.writes,
            syncs: self.syncs - earlier.syncs,
            bytes_read: self.bytes_read - earlier.bytes_read,
            bytes_written: self.bytes_written - earlier.bytes_written,
        }
    }
}

/// What survives power loss among writes not yet covered by a sync.
#[derive(Clone, Copy, Debug)]
pub struct CrashModel {
    /// 0 = a write lands whole or not at all; otherwise tearing at
    /// absolute multiples of this many bytes (512, 4096, ...).
    pub granule: u64,
    /// Chance (0-100) that each in-flight granule lands.
    pub keep_percent: u8,
    pub seed: u64,
}

impl CrashModel {
    pub const DROP_ALL: CrashModel = CrashModel { granule: 0, keep_percent: 0, seed: 0 };
    pub const KEEP_ALL: CrashModel = CrashModel { granule: 0, keep_percent: 100, seed: 0 };
    pub fn torn(granule: u64, seed: u64) -> CrashModel {
        CrashModel { granule, keep_percent: 50, seed }
    }
}

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng((seed ^ 0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn chance(&mut self, percent: u8) -> bool {
        (self.next() % 100) < percent as u64
    }
}

struct Inner {
    cache: Vec<u8>,
    durable: Vec<u8>,
    epoch_base: Vec<u8>,
    log: Vec<Op>,
    counters: Counters,
    bad_reads: Vec<(u64, u64)>,
    fail_writes_after: Option<u64>,
    writes_ok: u64,
}

#[derive(Clone)]
pub struct FaultDevice {
    inner: Arc<Mutex<Inner>>,
}

impl FaultDevice {
    pub fn from_image(image: Vec<u8>) -> Self {
        FaultDevice {
            inner: Arc::new(Mutex::new(Inner {
                cache: image.clone(),
                durable: image.clone(),
                epoch_base: image,
                log: Vec::new(),
                counters: Counters::default(),
                bad_reads: Vec::new(),
                fail_writes_after: None,
                writes_ok: 0,
            })),
        }
    }

    /// Start recording from a quiescent point: syncs, snapshots the
    /// durable image as the crash baseline, clears the op log and
    /// counters.
    pub fn begin_recording(&self) {
        let mut s = self.inner.lock().unwrap();
        s.durable = s.cache.clone();
        s.epoch_base = s.durable.clone();
        s.log.clear();
        s.counters = Counters::default();
    }

    pub fn counters(&self) -> Counters {
        self.inner.lock().unwrap().counters
    }

    pub fn ops(&self) -> Vec<Op> {
        self.inner.lock().unwrap().log.clone()
    }

    /// What a reader sees right now (media + cache).
    pub fn visible_image(&self) -> Vec<u8> {
        self.inner.lock().unwrap().cache.clone()
    }

    /// What survives a power cut right now with every unsynced write
    /// dropped.
    pub fn durable_image(&self) -> Vec<u8> {
        self.inner.lock().unwrap().durable.clone()
    }

    /// Media corruption injected outside the op stream (setup only:
    /// call before `begin_recording`).
    pub fn poke(&self, offset: u64, bytes: &[u8]) {
        let mut guard = self.inner.lock().unwrap();
        let s = &mut *guard;
        let o = offset as usize;
        for img in [&mut s.cache, &mut s.durable, &mut s.epoch_base] {
            img[o..o + bytes.len()].copy_from_slice(bytes);
        }
    }

    pub fn flip_byte(&self, offset: u64) {
        let b = self.visible_image()[offset as usize] ^ 0xFF;
        self.poke(offset, &[b]);
    }

    /// Reads overlapping `[offset, offset+len)` fail with `Io`
    /// (an unreadable sector).
    pub fn add_bad_read_range(&self, offset: u64, len: u64) {
        self.inner.lock().unwrap().bad_reads.push((offset, len));
    }

    /// After `n` successful writes, every further write fails with `Io`.
    pub fn fail_writes_after(&self, n: u64) {
        let mut s = self.inner.lock().unwrap();
        s.fail_writes_after = Some(n);
        s.writes_ok = 0;
    }

    /// The media image after power loss at op-log position `prefix`
    /// (ops `0..prefix` were issued) under `model`.
    pub fn crash_image(&self, prefix: usize, model: CrashModel) -> Vec<u8> {
        let s = self.inner.lock().unwrap();
        let mut img = s.epoch_base.clone();
        let ops = &s.log[..prefix];
        let durable_end = ops.iter().rposition(|o| matches!(o, Op::Sync)).map_or(0, |i| i + 1);

        for op in &ops[..durable_end] {
            if let Op::Write { offset, data } = op {
                img[*offset as usize..*offset as usize + data.len()].copy_from_slice(data);
            }
        }

        let mut rng = Rng::new(model.seed);
        for op in &ops[durable_end..] {
            let Op::Write { offset, data } = op else { continue };
            let (start, end) = (*offset, *offset + data.len() as u64);
            if model.granule == 0 {
                if rng.chance(model.keep_percent) {
                    img[start as usize..end as usize].copy_from_slice(data);
                }
                continue;
            }
            let mut pos = start;
            while pos < end {
                let next = ((pos / model.granule) + 1) * model.granule;
                let chunk_end = next.min(end);
                if rng.chance(model.keep_percent) {
                    img[pos as usize..chunk_end as usize]
                        .copy_from_slice(&data[(pos - start) as usize..(chunk_end - start) as usize]);
                }
                pos = chunk_end;
            }
        }
        img
    }
}

impl Device for FaultDevice {
    fn size(&self) -> u64 {
        self.inner.lock().unwrap().cache.len() as u64
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> CafsResult<()> {
        let mut s = self.inner.lock().unwrap();
        let end = offset + buf.len() as u64;
        if end > s.cache.len() as u64 {
            return Err(CafsError::Io);
        }
        if s.bad_reads.iter().any(|&(o, l)| offset < o + l && o < end) {
            return Err(CafsError::Io);
        }
        buf.copy_from_slice(&s.cache[offset as usize..end as usize]);
        s.counters.reads += 1;
        s.counters.bytes_read += buf.len() as u64;
        Ok(())
    }

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> CafsResult<()> {
        let mut s = self.inner.lock().unwrap();
        let end = offset + buf.len() as u64;
        if end > s.cache.len() as u64 {
            return Err(CafsError::Io);
        }
        if let Some(n) = s.fail_writes_after {
            if s.writes_ok >= n {
                return Err(CafsError::Io);
            }
        }
        s.writes_ok += 1;
        s.cache[offset as usize..end as usize].copy_from_slice(buf);
        s.log.push(Op::Write { offset, data: buf.to_vec() });
        s.counters.writes += 1;
        s.counters.bytes_written += buf.len() as u64;
        Ok(())
    }

    fn sync(&mut self) -> CafsResult<()> {
        let mut s = self.inner.lock().unwrap();
        s.durable = s.cache.clone();
        s.log.push(Op::Sync);
        s.counters.syncs += 1;
        Ok(())
    }

    fn apply_flags(&mut self, direct: bool, _sync: bool, direct_required: bool) -> CafsResult<()> {
        if direct && direct_required {
            return Err(CafsError::Unsupported);
        }
        Ok(())
    }
}
