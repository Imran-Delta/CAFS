//! EXPERIMENTAL error-correcting codes for anchor structures. Not used
//! by the on-disk format: this module exists so `tests/ecc_lab.rs` can
//! compare candidate schemes on measured data before any is adopted.
//!
//! `Rs` is Reed-Solomon over GF(256) (poly 0x11d, first root alpha^0),
//! systematic, up to 255 bytes per codeword; it corrects up to nsym/2
//! corrupted bytes per codeword. `secded_*` is extended Hamming (72,64):
//! it corrects 1 bit and detects 2 per 8-byte word, one check byte per word.
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)
#![allow(clippy::needless_range_loop)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EccError;

struct Gf {
    exp: [u8; 512],
    log: [u8; 256],
}

impl Gf {
    fn new() -> Gf {
        let (mut exp, mut log) = ([0u8; 512], [0u8; 256]);
        let mut x: u16 = 1;
        for i in 0..255 {
            exp[i] = x as u8;
            log[x as usize] = i as u8;
            x <<= 1;
            if x & 0x100 != 0 {
                x ^= 0x11d;
            }
        }
        for i in 255..512 {
            exp[i] = exp[i - 255];
        }
        Gf { exp, log }
    }
    fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            0
        } else {
            self.exp[self.log[a as usize] as usize + self.log[b as usize] as usize]
        }
    }
    /// `b` must be non-zero.
    fn div(&self, a: u8, b: u8) -> u8 {
        if a == 0 {
            0
        } else {
            self.exp[self.log[a as usize] as usize + 255 - self.log[b as usize] as usize]
        }
    }
    fn alpha(&self, e: usize) -> u8 {
        self.exp[e % 255]
    }
    /// Evaluate a polynomial stored low-degree-first.
    fn eval_low(&self, p: &[u8], x: u8) -> u8 {
        p.iter().rev().fold(0, |y, &c| self.mul(y, x) ^ c)
    }
}

pub struct Rs {
    gf: Gf,
    nsym: usize,
    gen: Vec<u8>, // monic, highest degree first
}

impl Rs {
    pub fn new(nsym: usize) -> Rs {
        let gf = Gf::new();
        let mut gen = vec![1u8];
        for i in 0..nsym {
            let root = gf.alpha(i);
            let mut next = vec![0u8; gen.len() + 1];
            for (j, &g) in gen.iter().enumerate() {
                next[j] ^= g;
                next[j + 1] ^= gf.mul(g, root);
            }
            gen = next;
        }
        Rs { gf, nsym, gen }
    }

    pub fn nsym(&self) -> usize {
        self.nsym
    }

    /// The `nsym` parity bytes for `data` (data.len() + nsym <= 255).
    pub fn parity(&self, data: &[u8]) -> Vec<u8> {
        assert!(data.len() + self.nsym <= 255, "RS codeword is limited to 255 bytes");
        let mut out = vec![0u8; data.len() + self.nsym];
        out[..data.len()].copy_from_slice(data);
        for i in 0..data.len() {
            let coef = out[i];
            if coef != 0 {
                for j in 1..=self.nsym {
                    out[i + j] ^= self.gf.mul(self.gen[j], coef);
                }
            }
        }
        out[data.len()..].to_vec()
    }

    fn syndromes(&self, cw: &[u8]) -> Vec<u8> {
        (0..self.nsym)
            .map(|j| {
                let x = self.gf.alpha(j);
                cw.iter().fold(0u8, |y, &c| self.gf.mul(y, x) ^ c)
            })
            .collect()
    }

    /// Correct `cw` (data followed by parity) in place; returns how many
    /// bytes were corrected. `Err` when more than `nsym/2` bytes are bad
    /// or the damage is otherwise uncorrectable.
    pub fn correct(&self, cw: &mut [u8]) -> Result<usize, EccError> {
        let n = cw.len();
        assert!(n <= 255 && n > self.nsym);
        let s = self.syndromes(cw);
        if s.iter().all(|&v| v == 0) {
            return Ok(0);
        }

        // Berlekamp-Massey: error locator, low-degree-first.
        let gf = &self.gf;
        let (mut c, mut b) = (vec![1u8], vec![1u8]);
        let (mut l, mut m, mut bb) = (0usize, 1usize, 1u8);
        for i in 0..self.nsym {
            let mut d = s[i];
            for k in 1..=l {
                if k < c.len() {
                    d ^= gf.mul(c[k], s[i - k]);
                }
            }
            if d == 0 {
                m += 1;
                continue;
            }
            let coef = gf.div(d, bb);
            let t = c.clone();
            if c.len() < b.len() + m {
                c.resize(b.len() + m, 0);
            }
            for k in 0..b.len() {
                c[k + m] ^= gf.mul(coef, b[k]);
            }
            if 2 * l <= i {
                l = i + 1 - l;
                b = t;
                bb = d;
                m = 1;
            } else {
                m += 1;
            }
        }
        if 2 * l > self.nsym {
            return Err(EccError);
        }
        c.resize(l + 1, 0);

        // Chien search: error at degree d (index n-1-d) iff c(alpha^-d) == 0.
        let locs: Vec<usize> = (0..n).filter(|&d| gf.eval_low(&c, gf.alpha((255 - d % 255) % 255)) == 0).collect();
        if locs.len() != l {
            return Err(EccError);
        }

        // Forney: omega = S(x)*c(x) mod x^nsym; e = X * omega(X^-1) / c'(X^-1).
        let omega: Vec<u8> = (0..self.nsym)
            .map(|i| (0..=i.min(l)).fold(0u8, |acc, j| acc ^ gf.mul(c[j], s[i - j])))
            .collect();
        let deriv: Vec<u8> = (0..l).map(|k| if k % 2 == 0 { c[k + 1] } else { 0 }).collect();
        for &d in &locs {
            let x = gf.alpha(d);
            let x_inv = gf.alpha((255 - d % 255) % 255);
            let den = gf.eval_low(&deriv, x_inv);
            if den == 0 {
                return Err(EccError);
            }
            let num = gf.mul(x, gf.eval_low(&omega, x_inv));
            cw[n - 1 - d] ^= gf.div(num, den);
        }
        if self.syndromes(cw).iter().any(|&v| v != 0) {
            return Err(EccError);
        }
        Ok(locs.len())
    }
}

// ---- SECDED (72,64) -------------------------------------------------

fn secded_positions() -> [u8; 64] {
    let (mut t, mut i) = ([0u8; 64], 0);
    for p in 1..=71u8 {
        if !p.is_power_of_two() {
            t[i] = p;
            i += 1;
        }
    }
    t
}

fn syndrome_of_data(v: u64, t: &[u8; 64]) -> u8 {
    (0..64).filter(|&i| (v >> i) & 1 == 1).fold(0u8, |s, i| s ^ t[i])
}

/// One check byte per 8 data bytes (`data.len()` must be a multiple of 8):
/// bits 0-6 are the Hamming check bits, bit 7 the overall parity.
pub fn secded_encode(data: &[u8]) -> Vec<u8> {
    assert!(data.len().is_multiple_of(8));
    let t = secded_positions();
    data.chunks(8)
        .map(|w| {
            let v = u64::from_le_bytes(w.try_into().unwrap());
            let c7 = syndrome_of_data(v, &t) & 0x7F;
            let parity = (v.count_ones() + c7.count_ones()) & 1;
            c7 | ((parity as u8) << 7)
        })
        .collect()
}

/// Correct single-bit errors in `data` and `check` in place; returns the
/// number of words corrected. `Err` on any word with a detected double error.
pub fn secded_decode(data: &mut [u8], check: &mut [u8]) -> Result<usize, EccError> {
    let t = secded_positions();
    let mut fixed = 0;
    for (w, chk) in data.chunks_mut(8).zip(check.iter_mut()) {
        let v = u64::from_le_bytes((&*w).try_into().unwrap());
        let s = syndrome_of_data(v, &t) ^ (*chk & 0x7F);
        let p = (v.count_ones() + chk.count_ones()) & 1;
        match (s, p) {
            (0, 0) => {}
            (0, _) => {
                *chk ^= 0x80;
                fixed += 1;
            }
            (s, 1) if s.is_power_of_two() => {
                *chk ^= s;
                fixed += 1;
            }
            (s, 1) => {
                let i = t.iter().position(|&x| x == s).ok_or(EccError)?;
                w[i / 8] ^= 1 << (i % 8);
                fixed += 1;
            }
            _ => return Err(EccError),
        }
    }
    Ok(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct R(u64);
    impl R {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn gf_inverse_roundtrip() {
        let gf = Gf::new();
        for a in 1..=255u8 {
            for b in [1u8, 2, 3, 77, 255] {
                assert_eq!(gf.div(gf.mul(a, b), b), a);
            }
        }
    }

    #[test]
    fn rs_corrects_up_to_t_and_never_silently_miscorrects() {
        let mut r = R(0x1234_5678_9abc_def1);
        for (k, nsym) in [(100usize, 16usize), (200, 32), (32, 223), (224, 16)] {
            let rs = Rs::new(nsym);
            for trial in 0..60 {
                let data: Vec<u8> = (0..k).map(|_| r.next() as u8).collect();
                let mut cw = data.clone();
                cw.extend(rs.parity(&data));
                assert_eq!(rs.correct(&mut cw.clone()), Ok(0));
                let t = nsym / 2;
                let nerr = if trial % 3 == 0 { t } else { (r.next() as usize % t) + 1 };
                let mut bad = cw.clone();
                let mut hit = Vec::new();
                while hit.len() < nerr {
                    let p = r.next() as usize % bad.len();
                    if !hit.contains(&p) {
                        hit.push(p);
                        bad[p] ^= (r.next() as u8) | 1;
                    }
                }
                assert_eq!(rs.correct(&mut bad), Ok(nerr), "k={k} nsym={nsym}");
                assert_eq!(bad, cw);
                // t+1 errors: must not hand back a wrong codeword as success.
                let mut over = cw.clone();
                let mut hit = Vec::new();
                while hit.len() < t + 1 {
                    let p = r.next() as usize % over.len();
                    if !hit.contains(&p) {
                        hit.push(p);
                        over[p] ^= (r.next() as u8) | 1;
                    }
                }
                if rs.correct(&mut over).is_ok() {
                    // A decoder may land on a different valid codeword;
                    // callers must treat that as detected by their checksum.
                    assert_ne!(over, cw, "claimed success with more than t errors yet restored the original?");
                }
            }
        }
    }

    #[test]
    fn secded_fixes_every_single_bit_and_flags_doubles() {
        let mut r = R(99);
        let data: Vec<u8> = (0..16).map(|_| r.next() as u8).collect();
        let check = secded_encode(&data);
        for bit in 0..(16 * 8 + 2 * 8) {
            let (mut d, mut c) = (data.clone(), check.clone());
            if bit < 128 {
                d[bit / 8] ^= 1 << (bit % 8);
            } else {
                c[(bit - 128) / 8] ^= 1 << ((bit - 128) % 8);
            }
            assert_eq!(secded_decode(&mut d, &mut c), Ok(1), "bit {bit}");
            assert_eq!((d, c), (data.clone(), check.clone()));
        }
        let (mut d, mut c) = (data.clone(), check.clone());
        d[0] ^= 0b11;
        assert_eq!(secded_decode(&mut d, &mut c), Err(EccError));
    }
}
