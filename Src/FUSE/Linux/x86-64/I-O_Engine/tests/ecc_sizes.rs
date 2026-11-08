//! Step 2c — ECC types across structure sizes. One 4096 B anchor unit
//! holds D bytes of structure data; the rest is free for parity. How does
//! each ECC type behave as D grows from tiny to nearly full?
//!
//! Types:  chk      checksum only                         (baseline)
//!         secded   extended Hamming per 8 B word, trailer inside the code
//!         rs16o    RS nsym=16, trailer OUTSIDE the code, strict rule (= ADR-056 draft)
//!         rs16o~   same, relaxed rule (clean codeword accepted without trailer)
//!         rs16i    RS nsym=16, trailer INSIDE the code, codewords = ceil(len/239)
//!         rs16i2   as rs16i but at least 2 codewords
//!         rs32i / rs64i  nsym 32 / 64, trailer inside
//!         rs16is   as rs16i but NOT interleaved (sequential split)
//!
//! "Trailer inside": the 16 B BLAKE3-128 of the data is part of the
//! message the code protects, so damage to the checksum itself is
//! correctable. "Outside": it sits at bytes 4080-4095 as in G2's unit.
//! Strict rule: corrected data is accepted only if its hash equals the
//! stored trailer exactly.
//!
//! One sector per structure, damage only to that sector. Percent of
//! trials fully recovered; "n/a" = does not fit in the unit. Damage
//! models are assumptions, not measurements of any device.
//! Trials: CAFS_ECC_TRIALS (default 60 release / 12 debug).
#![allow(clippy::needless_range_loop)]

use std::collections::HashSet;
use std::time::Instant;

use cafs_io::ecc::{secded_decode, secded_encode, Rs};

const SECTOR: usize = 4096;
const TRAILER_AT: usize = SECTOR - 16;
const CHUNK: usize = 512;
const SIZES: [usize; 6] = [48, 128, 448, 1024, 2048, 3584];

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn fill(&mut self, b: &mut [u8]) {
        for x in b {
            *x = self.next() as u8;
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Checksum,
    Secded,
    Rs { nsym: usize, interleave: bool, cw_min: usize },
}

#[derive(Clone, Copy)]
struct Spec {
    name: &'static str,
    kind: Kind,
    trailer_in: bool,
    relaxed: bool,
}

#[derive(Clone, Copy)]
struct Plan {
    d: usize,
    cw: usize,
    nsym: usize,
    mlen: usize,   // padded message (data [+ trailer] [+ pad]) bytes
    parity: usize, // parity bytes
}
impl Plan {
    fn body(&self) -> usize {
        self.mlen + self.parity
    }
}

fn plan(spec: &Spec, d: usize) -> Option<Plan> {
    let tr = if spec.trailer_in { 16 } else { 0 };
    let p = match spec.kind {
        Kind::Checksum => Plan { d, cw: 0, nsym: 0, mlen: d, parity: 0 },
        Kind::Secded => {
            let mlen = (d + tr).div_ceil(8) * 8;
            Plan { d, cw: 0, nsym: 0, mlen, parity: mlen / 8 }
        }
        Kind::Rs { nsym, cw_min, .. } => {
            let raw = d + tr;
            let cw = cw_min.max(raw.div_ceil(255 - nsym));
            let k = raw.div_ceil(cw);
            Plan { d, cw, nsym, mlen: k * cw, parity: cw * nsym }
        }
    };
    let limit = if spec.trailer_in { SECTOR } else { TRAILER_AT };
    (p.body() <= limit).then_some(p)
}

fn hash16(b: &[u8]) -> [u8; 16] {
    blake3::hash(b).as_bytes()[..16].try_into().unwrap()
}
fn hamming(a: &[u8; 16], b: &[u8; 16]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

fn parity_index(spec: &Spec, p: &Plan, c: usize, j: usize) -> usize {
    let interleave = matches!(spec.kind, Kind::Rs { interleave: true, .. });
    p.mlen + if interleave { j * p.cw + c } else { c * p.nsym + j }
}
fn msg_index(spec: &Spec, p: &Plan, c: usize, j: usize) -> usize {
    let interleave = matches!(spec.kind, Kind::Rs { interleave: true, .. });
    if interleave { j * p.cw + c } else { c * (p.mlen / p.cw) + j }
}

fn build(spec: &Spec, p: &Plan, data: &[u8], rs: Option<&Rs>) -> Vec<u8> {
    let mut s = vec![0u8; SECTOR];
    s[..p.d].copy_from_slice(data);
    if spec.trailer_in {
        let t = hash16(data);
        s[p.d..p.d + 16].copy_from_slice(&t);
    }
    match spec.kind {
        Kind::Secded => {
            let par = secded_encode(&s[..p.mlen]);
            s[p.mlen..p.mlen + par.len()].copy_from_slice(&par);
        }
        Kind::Rs { .. } => {
            let k = p.mlen / p.cw;
            for c in 0..p.cw {
                let word: Vec<u8> = (0..k).map(|j| s[msg_index(spec, p, c, j)]).collect();
                let par = rs.unwrap().parity(&word);
                for j in 0..p.nsym {
                    s[parity_index(spec, p, c, j)] = par[j];
                }
            }
        }
        Kind::Checksum => {}
    }
    if !spec.trailer_in {
        let t = hash16(&s[..p.body()]);
        s[TRAILER_AT..].copy_from_slice(&t);
    }
    s
}

fn decode(spec: &Spec, p: &Plan, sec: &[u8], rs: Option<&Rs>) -> Option<Vec<u8>> {
    let stored_out: [u8; 16] = sec[TRAILER_AT..].try_into().unwrap();
    let mut msg = sec[..p.mlen].to_vec();
    let mut par = sec[p.mlen..p.body()].to_vec();
    let mut fixed = 0;
    match spec.kind {
        Kind::Checksum => {
            return (hash16(&sec[..p.body()]) == stored_out).then(|| sec[..p.d].to_vec());
        }
        Kind::Secded => {
            fixed = secded_decode(&mut msg, &mut par).ok()?;
        }
        Kind::Rs { interleave, .. } => {
            let k = p.mlen / p.cw;
            for c in 0..p.cw {
                let mut word: Vec<u8> = (0..k).map(|j| sec[msg_index(spec, p, c, j)]).collect();
                word.extend((0..p.nsym).map(|j| sec[parity_index(spec, p, c, j)]));
                fixed += rs.unwrap().correct(&mut word).ok()?;
                for j in 0..k {
                    msg[msg_index(spec, p, c, j)] = word[j];
                }
                for j in 0..p.nsym {
                    par[parity_index(spec, p, c, j) - p.mlen] = word[k + j];
                }
            }
            let _ = interleave;
        }
    }
    let data = msg[..p.d].to_vec();
    if spec.trailer_in {
        let t: [u8; 16] = msg[p.d..p.d + 16].try_into().unwrap();
        return (hash16(&data) == t).then_some(data);
    }
    let mut body = msg;
    body.extend_from_slice(&par);
    let dist = hamming(&hash16(&body), &stored_out);
    if spec.relaxed {
        let strong = matches!(spec.kind, Kind::Rs { .. });
        if fixed == 0 { (dist <= 24 || strong).then_some(data) } else { (dist <= 24).then_some(data) }
    } else {
        (dist == 0).then_some(data)
    }
}

#[derive(Clone, Copy)]
enum Fault {
    Clean,
    Bits(usize),
    Burst(usize),
    TornSmall, // new version differs from old in 16 bytes
    TornFull,  // new version is a complete rewrite
}

fn flip_bits(sec: &mut [u8], spec: &Spec, p: &Plan, k: usize, r: &mut Rng) {
    let extra = if spec.trailer_in { 0 } else { 16 };
    let total = (p.body() + extra) * 8;
    let mut hit = HashSet::new();
    while hit.len() < k {
        let b = r.below(total);
        if hit.insert(b) {
            let byte = if b / 8 < p.body() { b / 8 } else { TRAILER_AT + (b / 8 - p.body()) };
            sec[byte] ^= 1 << (b % 8);
        }
    }
}

fn trial(spec: &Spec, p: &Plan, rs: Option<&Rs>, fault: Fault, seed: u64) -> (bool, bool, f64) {
    let mut r = Rng::new(seed);
    let mut a = vec![0u8; p.d];
    r.fill(&mut a);
    let mut truths = vec![a.clone()];
    let mut sec = build(spec, p, &a, rs);
    match fault {
        Fault::Clean => {}
        Fault::Bits(k) => flip_bits(&mut sec, spec, p, k, &mut r),
        Fault::Burst(len) => {
            let start = r.below(p.body());
            for i in start..(start + len).min(p.body()) {
                sec[i] ^= (r.next() as u8) | 1;
            }
        }
        Fault::TornSmall | Fault::TornFull => {
            let mut b = a.clone();
            if matches!(fault, Fault::TornFull) {
                r.fill(&mut b);
            } else {
                for _ in 0..16 {
                    let i = r.below(p.d);
                    b[i] ^= (r.next() as u8) | 1;
                }
            }
            let new = build(spec, p, &b, rs);
            for c in 0..SECTOR / CHUNK {
                if r.next() & 1 == 1 {
                    sec[c * CHUNK..(c + 1) * CHUNK].copy_from_slice(&new[c * CHUNK..(c + 1) * CHUNK]);
                }
            }
            truths.push(b);
        }
    }
    let t0 = Instant::now();
    let out = decode(spec, p, &sec, rs);
    let us = t0.elapsed().as_secs_f64() * 1e6;
    match out {
        Some(x) if truths.contains(&x) => (true, false, us),
        Some(_) => (false, true, us),
        None => (false, false, us),
    }
}

#[test]
fn ecc_types_across_structure_sizes() {
    let trials: usize = std::env::var("CAFS_ECC_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if cfg!(debug_assertions) { 12 } else { 60 });
    let rsk = |nsym, interleave, cw_min| Kind::Rs { nsym, interleave, cw_min };
    let specs = [
        Spec { name: "chk", kind: Kind::Checksum, trailer_in: false, relaxed: false },
        Spec { name: "secded", kind: Kind::Secded, trailer_in: true, relaxed: false },
        Spec { name: "rs16o", kind: rsk(16, true, 1), trailer_in: false, relaxed: false },
        Spec { name: "rs16o~", kind: rsk(16, true, 1), trailer_in: false, relaxed: true },
        Spec { name: "rs16i", kind: rsk(16, true, 1), trailer_in: true, relaxed: false },
        Spec { name: "rs16i2", kind: rsk(16, true, 2), trailer_in: true, relaxed: false },
        Spec { name: "rs32i", kind: rsk(32, true, 1), trailer_in: true, relaxed: false },
        Spec { name: "rs64i", kind: rsk(64, true, 1), trailer_in: true, relaxed: false },
        Spec { name: "rs16is", kind: rsk(16, false, 1), trailer_in: true, relaxed: false },
    ];
    let rss: Vec<Option<Rs>> = specs
        .iter()
        .map(|s| if let Kind::Rs { nsym, .. } = s.kind { Some(Rs::new(nsym)) } else { None })
        .collect();
    let faults: [(&str, Fault); 7] = [
        ("clean", Fault::Clean),
        ("8 scattered bits", Fault::Bits(8)),
        ("64 scattered bits", Fault::Bits(64)),
        ("burst 16 B", Fault::Burst(16)),
        ("burst 128 B", Fault::Burst(128)),
        ("torn write, small update (16 B differ), 512 B chunks", Fault::TornSmall),
        ("torn write, full rewrite, 512 B chunks", Fault::TornFull),
    ];

    // result[fault][size][spec] = Option<percent>
    let mut res = vec![vec![vec![None::<usize>; specs.len()]; SIZES.len()]; faults.len()];
    let mut dec_us = vec![vec![None::<f64>; specs.len()]; SIZES.len()];
    for (fi, (_, fault)) in faults.iter().enumerate() {
        for (di, &d) in SIZES.iter().enumerate() {
            for (si, spec) in specs.iter().enumerate() {
                let Some(p) = plan(spec, d) else { continue };
                let n = trials;
                let (mut ok, mut wrong, mut us) = (0, 0, 0.0);
                for i in 0..n {
                    let (o, w, t) = trial(spec, &p, rss[si].as_ref(), *fault, (fi * 100_000 + di * 1000 + si * 100 + i) as u64 + 1);
                    ok += o as usize;
                    wrong += w as usize;
                    us += t;
                }
                if fi == 1 {
                    dec_us[di][si] = Some(us / n as f64);
                }
                assert_eq!(wrong, 0, "{} D={d} / {}: silently wrong data", spec.name, faults[fi].0);
                res[fi][di][si] = Some(ok * 100 / n);
            }
        }
    }

    println!("\nstructure data D in a 4096 B unit; {trials} trials per cell; cell = % fully recovered (n/a = does not fit)\n");
    println!("-- parity bytes / codewords");
    print!("{:>6}", "D");
    for s in &specs {
        print!(" {:>9}", s.name);
    }
    println!();
    for &d in &SIZES {
        print!("{d:>6}");
        for s in &specs {
            match plan(s, d) {
                Some(p) => print!(" {:>9}", format!("{}/{}", p.parity, p.cw)),
                None => print!(" {:>9}", "n/a"),
            }
        }
        println!();
    }
    for (fi, (name, _)) in faults.iter().enumerate().skip(1) {
        println!("\n-- {name}");
        print!("{:>6}", "D");
        for s in &specs {
            print!(" {:>9}", s.name);
        }
        println!();
        for (di, &d) in SIZES.iter().enumerate() {
            print!("{d:>6}");
            for si in 0..specs.len() {
                match res[fi][di][si] {
                    Some(v) => print!(" {:>8}%", v),
                    None => print!(" {:>9}", "n/a"),
                }
            }
            println!();
        }
    }
    println!("\n-- decode time only, 8 flipped bits (microseconds, release build)");
    print!("{:>6}", "D");
    for s in &specs {
        print!(" {:>9}", s.name);
    }
    println!();
    for (di, &d) in SIZES.iter().enumerate() {
        print!("{d:>6}");
        for si in 0..specs.len() {
            match dec_us[di][si] {
                Some(v) => print!(" {:>9.0}", v),
                None => print!(" {:>9}", "n/a"),
            }
        }
        println!();
    }

    let at = |f: usize, d: usize, name: &str| -> Option<usize> {
        let di = SIZES.iter().position(|&x| x == d).unwrap();
        let si = specs.iter().position(|s| s.name == name).unwrap();
        res[f][di][si]
    };
    for (di, _) in SIZES.iter().enumerate() {
        for si in 0..specs.len() {
            if let Some(v) = res[0][di][si] {
                assert_eq!(v, 100, "{}: undamaged data must always decode", specs[si].name);
            }
        }
    }
    for &d in &SIZES {
        for n in ["rs16i", "rs16i2", "rs32i", "rs64i"] {
            if let Some(v) = at(1, d, n) {
                assert_eq!(v, 100, "{n} D={d}: 8 scattered bits are always within t");
            }
        }
        if let Some(v) = at(3, d, "rs16i2") {
            assert_eq!(v, 100, "rs16i2 D={d}: a 16 B burst spreads over >=2 codewords");
        }
    }
    for d in [48, 128, 448] {
        for f in [5, 6] {
            assert_eq!(at(f, d, "rs16i"), Some(100), "everything inside one 512 B chunk cannot be torn apart (D={d})");
        }
    }
    assert!(at(6, 3584, "rs16i").unwrap() < 20, "a full rewrite spread over many chunks does not survive tearing");
    assert!(at(1, 48, "rs16o").unwrap() < at(1, 48, "rs16i").unwrap(), "a trailer outside the code is a weak spot when D is small");
}
