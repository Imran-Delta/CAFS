//! Step 2b — ECC lab. Which protection survives which kind of damage to
//! ONE anchor structure that has no backup of its own (Config Snapshot,
//! Parity Block, Meta/User Table, Function Table at K=1)?
//!
//! A structure is a 4096 B unit: [data 448 B][ECC parity][zero pad][16 B
//! BLAKE3-128 trailer over data+parity]. Schemes differ in the parity and
//! in what else is stored (a second copy; an XOR-parity group of four).
//! Light damage (bit flips, bursts) is applied to EVERY stored sector of a
//! scheme independently, so a second copy is not magically spared. A lost
//! sector and a torn write hit only the sector being written/read.
//!
//! "wrong" = a scheme returned data that is neither the original nor (for
//! torn writes) the new version. It must be zero everywhere.
//!
//! Damage models are assumptions about media, not measurements of any
//! real device; the table says what each scheme covers, not how often
//! the damage happens. Trials: CAFS_ECC_TRIALS (default 100 in release, 20
//! in debug builds; percentages are coarser with fewer trials).
#![allow(clippy::needless_range_loop)]

use std::collections::HashSet;
use std::time::Instant;

use cafs_io::ecc::{secded_decode, secded_encode, Rs};

const SECTOR: usize = 4096;
const TRAILER: usize = SECTOR - 16;
const D: usize = 448;
const CHUNK: usize = 512;
/// A corrected body whose hash differs from the stored trailer in at most
/// this many of 128 bits is attributed to trailer damage; a wrong body
/// differs in ~64 +- 6 bits, so 24 is about seven standard deviations away.
const TRAILER_SLACK: u32 = 24;

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
enum Ecc {
    None,
    Search1,
    Secded,
    Rs { cw: usize, nsym: usize },
}

#[derive(Clone, Copy, PartialEq)]
enum Red {
    Single,
    Copy,
    Xor4,
}

struct Scheme {
    name: &'static str,
    ecc: Ecc,
    red: Red,
    rs: Option<Rs>,
}

fn scheme(name: &'static str, ecc: Ecc, red: Red) -> Scheme {
    let rs = if let Ecc::Rs { nsym, .. } = ecc { Some(Rs::new(nsym)) } else { None };
    Scheme { name, ecc, red, rs }
}

fn parity_len(e: Ecc) -> usize {
    match e {
        Ecc::None | Ecc::Search1 => 0,
        Ecc::Secded => D / 8,
        Ecc::Rs { cw, nsym } => cw * nsym,
    }
}
fn body_len(e: Ecc) -> usize {
    D + parity_len(e)
}

fn trailer_of(body: &[u8]) -> [u8; 16] {
    blake3::hash(body).as_bytes()[..16].try_into().unwrap()
}
fn hamming(a: &[u8; 16], b: &[u8; 16]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

fn build_sector(sch: &Scheme, data: &[u8], ecc: Ecc) -> Vec<u8> {
    let mut s = vec![0u8; SECTOR];
    s[..D].copy_from_slice(data);
    match ecc {
        Ecc::Secded => {
            let p = secded_encode(data);
            s[D..D + p.len()].copy_from_slice(&p);
        }
        Ecc::Rs { cw, nsym } => {
            for c in 0..cw {
                let part: Vec<u8> = (0..D / cw).map(|j| data[j * cw + c]).collect();
                let p = sch.rs.as_ref().unwrap().parity(&part);
                for j in 0..nsym {
                    s[D + j * cw + c] = p[j]; // parity is interleaved like the data, so a burst spreads over all codewords
                }
            }
        }
        _ => {}
    }
    let t = trailer_of(&s[..body_len(ecc)]);
    s[TRAILER..].copy_from_slice(&t);
    s
}

fn build_stored(sch: &Scheme, data: &[u8], others_seed: u64) -> Vec<Vec<u8>> {
    let sector = build_sector(sch, data, sch.ecc);
    match sch.red {
        Red::Single => vec![sector],
        Red::Copy => vec![sector.clone(), sector],
        Red::Xor4 => {
            let mut r = Rng::new(others_seed);
            let others: Vec<Vec<u8>> = (0..3)
                .map(|_| {
                    let mut d = vec![0u8; D];
                    r.fill(&mut d);
                    build_sector(sch, &d, Ecc::None)
                })
                .collect();
            let mut parity = sector.clone();
            for o in &others {
                for i in 0..SECTOR {
                    parity[i] ^= o[i];
                }
            }
            let mut v = vec![sector];
            v.extend(others);
            v.push(parity);
            v
        }
    }
}

fn finish(sch: &Scheme, fixed: usize, data: Vec<u8>, par: &[u8], stored: &[u8; 16]) -> Option<Vec<u8>> {
    let mut body = data.clone();
    body.extend_from_slice(par);
    let d = hamming(&trailer_of(&body), stored);
    let strong = matches!(sch.ecc, Ecc::Rs { .. });
    // A codeword that needed no correction is accepted without the
    // checksum only for the strong code; a weak one could be hit by a
    // multi-bit pattern that is itself a valid codeword.
    if fixed == 0 {
        (d <= TRAILER_SLACK || strong).then_some(data)
    } else {
        (d <= TRAILER_SLACK).then_some(data)
    }
}

fn decode_sector(sec: &[u8], sch: &Scheme) -> Option<Vec<u8>> {
    let bl = body_len(sch.ecc);
    let stored: [u8; 16] = sec[TRAILER..].try_into().unwrap();
    match sch.ecc {
        Ecc::None => (trailer_of(&sec[..bl]) == stored).then(|| sec[..D].to_vec()),
        Ecc::Search1 => {
            let h = trailer_of(&sec[..D]);
            if hamming(&h, &stored) <= TRAILER_SLACK && h == stored {
                return Some(sec[..D].to_vec());
            }
            if hamming(&h, &stored) <= 2 {
                return Some(sec[..D].to_vec()); // trailer-only damage
            }
            let mut d = sec[..D].to_vec();
            for bit in 0..D * 8 {
                d[bit / 8] ^= 1 << (bit % 8);
                if trailer_of(&d) == stored {
                    return Some(d);
                }
                d[bit / 8] ^= 1 << (bit % 8);
            }
            None
        }
        Ecc::Secded => {
            let mut data = sec[..D].to_vec();
            let mut chk = sec[D..D + D / 8].to_vec();
            let fixed = secded_decode(&mut data, &mut chk).ok()?;
            finish(sch, fixed, data, &chk, &stored)
        }
        Ecc::Rs { cw, nsym } => {
            let rs = sch.rs.as_ref().unwrap();
            let (k, mut data, mut par, mut fixed) = (D / cw, vec![0u8; D], vec![0u8; cw * nsym], 0);
            for c in 0..cw {
                let mut word: Vec<u8> = (0..k).map(|j| sec[j * cw + c]).collect();
                word.extend((0..nsym).map(|j| sec[D + j * cw + c]));
                fixed += rs.correct(&mut word).ok()?;
                for j in 0..k {
                    data[j * cw + c] = word[j];
                }
                for j in 0..nsym {
                    par[j * cw + c] = word[k + j];
                }
            }
            finish(sch, fixed, data, &par, &stored)
        }
    }
}

fn decode_scheme(sch: &Scheme, stored: &[Vec<u8>]) -> Option<Vec<u8>> {
    match sch.red {
        Red::Single => decode_sector(&stored[0], sch),
        Red::Copy => decode_sector(&stored[0], sch).or_else(|| decode_sector(&stored[1], sch)),
        Red::Xor4 => decode_sector(&stored[0], sch).or_else(|| {
            let mut r = stored[4].clone();
            for m in &stored[1..4] {
                for i in 0..SECTOR {
                    r[i] ^= m[i];
                }
            }
            decode_sector(&r, sch)
        }),
    }
}

#[derive(Clone, Copy)]
enum Fault {
    Clean,
    Bits(usize),
    Burst(usize),
    BitsAll(usize),
    BurstAll(usize),
    SectorLost,
    Torn,
}

fn flip_bits(sec: &mut [u8], bl: usize, k: usize, r: &mut Rng) {
    let total = (bl + 16) * 8;
    let mut hit = HashSet::new();
    while hit.len() < k {
        let p = r.below(total);
        if hit.insert(p) {
            let byte = if p / 8 < bl { p / 8 } else { TRAILER + (p / 8 - bl) };
            sec[byte] ^= 1 << (p % 8);
        }
    }
}

fn burst(sec: &mut [u8], bl: usize, len: usize, r: &mut Rng) {
    let start = r.below(bl);
    for i in start..(start + len).min(bl) {
        sec[i] ^= (r.next() as u8) | 1;
    }
}

#[derive(Default, Clone, Copy)]
struct Tally {
    ok: usize,
    wrong: usize,
    n: usize,
}

fn trial(sch: &Scheme, fault: Fault, seed: u64) -> (bool, bool) {
    let mut r = Rng::new(seed);
    let mut a = vec![0u8; D];
    r.fill(&mut a);
    let others = r.next();
    let bl = body_len(sch.ecc);
    let mut truths = vec![a.clone()];
    let stored = match fault {
        Fault::Torn => {
            let mut b = a.clone();
            for _ in 0..16 {
                let i = r.below(D);
                b[i] ^= (r.next() as u8) | 1;
            }
            let (old, new) = (build_stored(sch, &a, others), build_stored(sch, &b, others));
            let mut stored = old;
            for c in 0..SECTOR / CHUNK {
                if r.next() & 1 == 1 {
                    stored[0][c * CHUNK..(c + 1) * CHUNK].copy_from_slice(&new[0][c * CHUNK..(c + 1) * CHUNK]);
                }
            }
            truths.push(b);
            stored
        }
        _ => {
            let mut stored = build_stored(sch, &a, others);
            match fault {
                Fault::Clean => {}
                Fault::Bits(k) => flip_bits(&mut stored[0], bl, k, &mut r),
                Fault::Burst(len) => burst(&mut stored[0], bl, len, &mut r),
                Fault::BitsAll(k) => stored.iter_mut().for_each(|s| flip_bits(s, bl, k, &mut r)),
                Fault::BurstAll(len) => stored.iter_mut().for_each(|s| burst(s, bl, len, &mut r)),
                Fault::SectorLost => r.fill(&mut stored[0]),
                Fault::Torn => unreachable!(),
            }
            stored
        }
    };
    match decode_scheme(sch, &stored) {
        Some(x) if truths.contains(&x) => (true, false),
        Some(_) => (false, true),
        None => (false, false),
    }
}

#[test]
fn ecc_matrix() {
    let trials: usize = std::env::var("CAFS_ECC_TRIALS").ok().and_then(|v| v.parse().ok()).unwrap_or(if cfg!(debug_assertions) { 20 } else { 100 });
    let schemes = vec![
        scheme("checksum", Ecc::None, Red::Single),
        scheme("search-1bit", Ecc::Search1, Red::Single),
        scheme("secded", Ecc::Secded, Red::Single),
        scheme("rs-light", Ecc::Rs { cw: 2, nsym: 16 }, Red::Single),
        scheme("rs-mid", Ecc::Rs { cw: 4, nsym: 32 }, Red::Single),
        scheme("rs-heavy", Ecc::Rs { cw: 7, nsym: 64 }, Red::Single),
        scheme("rs-max", Ecc::Rs { cw: 14, nsym: 223 }, Red::Single),
        scheme("copy", Ecc::None, Red::Copy),
        scheme("xor-4+1", Ecc::None, Red::Xor4),
        scheme("rs-mid+copy", Ecc::Rs { cw: 4, nsym: 32 }, Red::Copy),
    ];
    let cols: [(&str, Fault); 10] = [
        ("clean", Fault::Clean),
        ("1 bit", Fault::Bits(1)),
        ("8 bits", Fault::Bits(8)),
        ("64 bits", Fault::Bits(64)),
        ("burst16", Fault::Burst(16)),
        ("burst128", Fault::Burst(128)),
        ("8b x all", Fault::BitsAll(8)),
        ("b128 x all", Fault::BurstAll(128)),
        ("sect.lost", Fault::SectorLost),
        ("torn", Fault::Torn),
    ];

    println!("\nstructure: {D} B of data in a 4096 B unit; every cell = % of {trials} trials fully recovered (wrong results counted separately)");
    print!("{:<12} {:>6} {:>7} {:>8} |", "scheme", "parity", "sectors", "upd.writes");
    for (n, _) in &cols {
        print!(" {n:>9}");
    }
    println!(" | enc µs  dec µs");

    let mut table: Vec<Vec<Tally>> = Vec::new();
    for (si, sch) in schemes.iter().enumerate() {
        let mut row = Vec::new();
        let mut dec_us = 0.0;
        for (ci, (_, fault)) in cols.iter().enumerate() {
            let n = if sch.ecc == Ecc::Search1 { trials.min(10) } else { trials };
            let mut t = Tally { n, ..Default::default() };
            let t0 = Instant::now();
            for i in 0..n {
                let (ok, wrong) = trial(sch, *fault, (si * 1000 + ci * 100 + i) as u64 + 1);
                t.ok += ok as usize;
                t.wrong += wrong as usize;
            }
            if ci == 2 {  // timing taken on the 8-bit column
                dec_us = t0.elapsed().as_secs_f64() * 1e6 / n as f64;
            }
            row.push(t);
        }
        let t0 = Instant::now();
        let reps = 30;
        for i in 0..reps {
            build_stored(sch, &vec![i as u8; D], 7);
        }
        let enc_us = t0.elapsed().as_secs_f64() * 1e6 / reps as f64;
        let (sectors, upd) = match sch.red {
            Red::Single => ("1", "0"),
            Red::Copy => ("2", "+1"),
            Red::Xor4 => ("1.25", "+1 (RMW)"),
        };
        print!("{:<12} {:>5}B {:>7} {:>8} |", sch.name, parity_len(sch.ecc), sectors, upd);
        for t in &row {
            print!(" {:>8}%", t.ok * 100 / t.n);
        }
        println!(" | {enc_us:>6.0} {dec_us:>7.0}");
        table.push(row);
    }
    println!("columns 1 bit .. burst128 damage only the primary sector; 'x all' damages EVERY stored sector independently (worst case)");
    println!("sector lost = one 4096 B sector unreadable; torn = old/new mix at 512 B granularity, success = old or new version returned");
    println!("upd.writes = extra device writes per update beyond the structure itself; sectors = 4 KiB units stored per structure");

    let pct = |s: &str, c: &str| {
        let si = schemes.iter().position(|x| x.name == s).unwrap();
        let ci = cols.iter().position(|x| x.0 == c).unwrap();
        let t = table[si][ci];
        t.ok * 100 / t.n
    };
    for (si, row) in table.iter().enumerate() {
        for (ci, t) in row.iter().enumerate() {
            assert_eq!(t.wrong, 0, "{} / {}: silently wrong data returned", schemes[si].name, cols[ci].0);
        }
        assert_eq!(row[0].ok, row[0].n, "{}: undamaged data must always decode", schemes[si].name);
    }
    assert_eq!(pct("checksum", "1 bit"), 0);
    assert_eq!(pct("search-1bit", "1 bit"), 100);
    assert_eq!(pct("secded", "1 bit"), 100);
    assert_eq!(pct("rs-light", "8 bits"), 100);
    assert_eq!(pct("rs-light", "burst16"), 100);
    assert_eq!(pct("rs-mid", "burst16"), 100);
    assert_eq!(pct("rs-heavy", "burst128"), 100);
    assert_eq!(pct("rs-max", "64 bits"), 100);
    for s in ["checksum", "search-1bit", "secded", "rs-light", "rs-mid", "rs-heavy", "rs-max"] {
        assert_eq!(pct(s, "sect.lost"), 0, "{s}: in-sector ECC cannot survive a lost sector");
    }
    for s in ["copy", "xor-4+1", "rs-mid+copy"] {
        for c in ["1 bit", "8 bits", "64 bits", "burst16", "burst128", "sect.lost", "torn"] {
            assert_eq!(pct(s, c), 100, "{s} / {c}: one intact sector is enough");
        }
    }
    assert_eq!(pct("copy", "8b x all"), 0, "damage to both copies defeats plain copies");
    assert_eq!(pct("rs-mid+copy", "8b x all"), 100, "ECC repairs each copy");
}
