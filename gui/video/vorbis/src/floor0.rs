//! Floor type 0 (Vorbis I §6): the spectral envelope as an LSP filter's
//! response on a Bark-warped frequency scale. No encoder has written it
//! since Vorbis's betas, but a decoder must still read it.
//!
//! Translated into Rust from Tremor's `floor0.c`, copyright Xiph.Org, used
//! under its BSD licence (`licenses/tremor-COPYING`). Where Tremor divides
//! by zero (a rate of 1, or 32 amplitude bits) this refuses the floor or
//! leaves the channel silent instead; where C's arithmetic overflows or
//! shifts too far, this wraps or takes the shift modulo the width, as x86
//! does.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "the setup header's fields are 4 to 16 bits; the fixed-point products wrap where C's do"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "the map's values are below the Bark map's size, coefficients below the order, and the cosine index is checked before the lookup"
)]

use crate::bitpack::BitReader;
use crate::codebook::Book;
use crate::misc::{ilog, mult31_shift15, mult32};
use crate::tables::{
    BARKLOOK, COS_LOOKUP_I, FROM_DB, FROM_DB2, INVSQ_LOOKUP_I, INVSQ_LOOKUP_IDEL, MLOOP_1, MLOOP_2,
    MLOOP_3,
};

const COS_LOOKUP_I_SHIFT: u32 = 9;
const COS_LOOKUP_I_MASK: i64 = 511;
const COS_LOOKUP_I_SZ: i32 = 128;
const INVSQ_LOOKUP_I_SHIFT: u32 = 10;
const INVSQ_LOOKUP_I_MASK: i64 = 1023;
const FROM_DB_SHIFT: u32 = 5;
const FROM_DB2_SHIFT: u32 = 3;
const FROM_DB2_MASK: i32 = 31;
const FROM_DB_LOOKUP_SZ: i32 = 35;

/// A floor 0 as the setup header gives it (`vorbis_info_floor0`).
#[derive(Clone, Debug)]
pub(crate) struct Floor0 {
    order: i32,
    rate: i32,
    barkmap: i32,
    ampbits: i64,
    amp_db: i32,
    books: Vec<usize>,
}

/// Its lookups for one block size (`vorbis_look_floor0`).
#[derive(Clone, Debug)]
pub(crate) struct Floor0Look {
    /// The half block.
    n: usize,
    /// Each frequency's Bark bin, and -1 after the last.
    linearmap: Vec<i32>,
    /// Each Bark bin's cosine, in .14.
    lsp_look: Vec<i32>,
}

impl Floor0 {
    /// `floor0_unpack`: a floor from the setup header, its books checked
    /// against `books`.
    pub(crate) fn unpack(opb: &mut BitReader<'_>, books: &[Book]) -> Option<Self> {
        let order = opb.read(8);
        let rate = opb.read(16);
        let barkmap = opb.read(16);
        let ampbits = opb.read(6);
        let amp_db = opb.read(8);
        let numbooks = opb.read(4) + 1;
        if order < 1 || rate < 1 || barkmap < 1 || numbooks < 1 {
            return None;
        }
        let mut list = Vec::with_capacity(numbooks as usize);
        for _ in 0..numbooks {
            let book = usize::try_from(opb.read(8)).ok()?;
            let b = books.get(book)?;
            if b.maptype == 0 || b.dim < 1 {
                return None;
            }
            list.push(book);
        }
        Some(Self {
            order: order as i32,
            rate: rate as i32,
            barkmap: barkmap as i32,
            ampbits,
            amp_db: amp_db as i32,
            books: list,
        })
    }

    /// `floor0_look` for blocks of `2 * n`; none where Tremor would divide
    /// by zero (a rate of 1).
    pub(crate) fn look(&self, n: usize) -> Option<Floor0Look> {
        let ln = self.barkmap;
        let half_rate = self.rate / 2;
        let top = to_bark(half_rate);
        if top == 0 {
            return None;
        }
        let mut linearmap = Vec::with_capacity(n + 1);
        for j in 0..n as i32 {
            let x = half_rate * j / n as i32;
            let val = (ln * ((to_bark(x) << 11) / top)) >> 11;
            linearmap.push(val.min(ln - 1));
        }
        linearmap.push(-1);
        let lsp_look = (0..ln)
            .map(|j| coslook2(i64::from(0x10000i32.wrapping_mul(j) / ln)))
            .collect();
        Some(Floor0Look {
            n,
            linearmap,
            lsp_look,
        })
    }

    /// `floor0_inverse1`: this channel's LSP coefficients and amplitude
    /// from the packet into `lsp` (the amplitude last); false for a silent
    /// channel or a packet cut short.
    pub(crate) fn inverse1(
        &self,
        opb: &mut BitReader<'_>,
        books: &[Book],
        lsp: &mut Vec<i32>,
    ) -> bool {
        let ampraw = opb.read(self.ampbits as u32) as i32;
        if ampraw <= 0 {
            return false;
        }
        // C's int arithmetic, x86's results where it overflows; at 32 bits
        // its maximum is 0 and Tremor divides by it.
        let maxval = i64::from(1i32.wrapping_shl(self.ampbits as u32).wrapping_sub(1));
        if maxval == 0 {
            return false;
        }
        let amp = (i64::from(ampraw.wrapping_mul(self.amp_db).wrapping_shl(4)) / maxval) as i32;
        let booknum = opb.read(ilog(self.books.len() as u32) as u32);
        let Some(&book) = usize::try_from(booknum)
            .ok()
            .and_then(|b| self.books.get(b))
        else {
            return false;
        };
        let Some(b) = books.get(book) else {
            return false;
        };
        let m = self.order as usize;
        lsp.clear();
        lsp.resize(m + 1, 0);
        if b.decodev_set(lsp, opb, m, -24) == -1 {
            return false;
        }
        let mut last = 0i32;
        let mut j = 0;
        while j < m {
            let mut k = 0;
            while j < m && k < b.dim {
                lsp[j] = lsp[j].wrapping_add(last);
                j += 1;
                k += 1;
            }
            last = lsp[j - 1];
        }
        lsp[m] = amp;
        true
    }

    /// `floor0_inverse2`: `out`, the half block's spectrum, times the
    /// curve the coefficients `lsp` describe -- or zero for a channel with
    /// none.
    pub(crate) fn inverse2(&self, look: &Floor0Look, lsp: Option<&[i32]>, out: &mut [i32]) {
        let n = look.n.min(out.len());
        let Some(lsp) = lsp else {
            out[..n].fill(0);
            return;
        };
        let m = self.order as usize;
        lsp_to_curve(
            &mut out[..n],
            &look.linearmap,
            &lsp[..m],
            lsp[m],
            self.amp_db,
            &look.lsp_look,
        );
    }
}

/// `toBARK`: the Bark scale at `n` Hz, in 17.15.
fn to_bark(n: i32) -> i32 {
    let mut i = 0;
    while i < 27 {
        if n >= BARKLOOK[i] && n < BARKLOOK[i + 1] {
            break;
        }
        i += 1;
    }
    if i == 27 {
        27 << 15
    } else {
        let gap = BARKLOOK[i + 1] - BARKLOOK[i];
        let del = n - BARKLOOK[i];
        ((i as i32) << 15) + ((del << 15) / gap)
    }
}

/// `vorbis_coslook_i`: cos(a) for `a` in 0.16 from 0 to pi (exclusive),
/// in .14.
fn coslook(a: i64) -> i32 {
    let i = (a >> COS_LOOKUP_I_SHIFT) as usize;
    let d = (a & COS_LOOKUP_I_MASK) as i32;
    COS_LOOKUP_I[i] - ((d * (COS_LOOKUP_I[i] - COS_LOOKUP_I[i + 1])) >> COS_LOOKUP_I_SHIFT)
}

/// `vorbis_coslook2_i`: cos(a) for `a` in 0.16 where 2^16 is pi, folded
/// into 0 to pi, in .14.
fn coslook2(a: i64) -> i32 {
    let mut a = a & 0x1_ffff;
    if a > 0x1_0000 {
        a = 0x2_0000 - a;
    }
    let i = (a >> COS_LOOKUP_I_SHIFT) as usize;
    let d = (a & COS_LOOKUP_I_MASK) as i32;
    // At pi itself the next entry is past the table's end; C reads it and
    // multiplies it by a `d` of 0.
    let next = COS_LOOKUP_I.get(i + 1).copied().unwrap_or(0);
    (COS_LOOKUP_I[i].wrapping_shl(COS_LOOKUP_I_SHIFT) - d * (COS_LOOKUP_I[i] - next))
        >> COS_LOOKUP_I_SHIFT
}

/// `vorbis_invsqlook_i`: 1/sqrt(a 2^e) for a normalised `a`, in .21-ish
/// fixed point as Tremor computes it.
fn invsqlook(a: i64, e: i64) -> i32 {
    const ADJUST_SQRT2: [i64; 2] = [8192, 5792];
    let i = ((a & 0x7fff) >> (INVSQ_LOOKUP_I_SHIFT - 1)) as usize;
    let d = a & INVSQ_LOOKUP_I_MASK;
    let mut val = INVSQ_LOOKUP_I[i] - ((INVSQ_LOOKUP_IDEL[i] * d) >> INVSQ_LOOKUP_I_SHIFT);
    val = val.wrapping_mul(ADJUST_SQRT2[(e & 1) as usize]);
    let e = (e >> 1) + 21;
    val.wrapping_shr(e as u32) as i32
}

/// `vorbis_fromdBlook_i`: 10^(a/20) for `a` in dB, n.12, from -140 to 0.
fn from_db_look(a: i64) -> i32 {
    let i = ((-a) >> (12 - FROM_DB2_SHIFT)) as i32;
    if i < 0 {
        return 0x7fff_ffff;
    }
    if i >= FROM_DB_LOOKUP_SZ << FROM_DB_SHIFT {
        return 0;
    }
    FROM_DB[(i >> FROM_DB_SHIFT) as usize] * FROM_DB2[(i & FROM_DB2_MASK) as usize]
}

/// The normalising shift for `pi | qi` (Tremor's three-table lookup).
fn mloop(v: u32) -> u32 {
    let at = |table: &[u8], i: u32| u32::from(table.get(i as usize).copied().unwrap_or(0));
    let s = at(&MLOOP_1, v >> 25);
    if s != 0 {
        return s;
    }
    let s = at(&MLOOP_2, v >> 19);
    if s != 0 {
        return s;
    }
    at(&MLOOP_3, v >> 16)
}

/// `(x * |d|)` modulo 2^32, as C's `ogg_uint32_t *= labs(d)`.
#[inline]
fn times_abs(x: u32, d: i32) -> u32 {
    (u64::from(x).wrapping_mul(u64::from(d.unsigned_abs()))) as u32
}

/// `vorbis_lsp_to_curve`: `curve` times the amplitude response of the
/// LSP filter `lsp` (8.24, 0 to pi), each frequency at its Bark bin
/// `map[i]` whose cosine is `icos[map[i]]`.
fn lsp_to_curve(
    curve: &mut [i32],
    map: &[i32],
    lsp: &[i32],
    amp: i32,
    ampoffset: i32,
    icos: &[i32],
) {
    let n = curve.len();
    let m = lsp.len();
    let ampoffseti = ampoffset.wrapping_mul(4096);
    let ampi = amp;
    let mut ilsp = Vec::with_capacity(m);
    for &l in lsp {
        let val = mult32(l, 0x0051_7cc2);
        // A hostile stream's coefficient outside 0 to pi.
        if val < 0 || (val >> COS_LOOKUP_I_SHIFT) >= COS_LOOKUP_I_SZ {
            curve.fill(0);
            return;
        }
        ilsp.push(coslook(i64::from(val)));
    }
    let mut i = 0;
    while i < n {
        let k = map[i];
        let wi = icos[k as usize];
        let mut pi: u32 = 46341; // 2^-0.5 in 0.16
        let mut qi: u32 = 46341;
        let mut qexp: i32 = 0;
        let mut j = 1;
        if m > 1 {
            qi = times_abs(qi, ilsp[0] - wi);
            pi = times_abs(pi, ilsp[1] - wi);
            j += 2;
            while j < m {
                let shift = mloop(pi | qi);
                qi = times_abs(qi >> shift, ilsp[j - 1] - wi);
                pi = times_abs(pi >> shift, ilsp[j] - wi);
                qexp += shift as i32;
                j += 2;
            }
        }
        let shift = mloop(pi | qi);
        // pi and qi are normalised together, both tracked by qexp.
        if m & 1 != 0 {
            // Odd order: the last coefficient, slightly asymmetric.
            qi = times_abs(qi >> shift, ilsp[j - 1] - wi);
            pi = (pi >> shift) << 14;
            qexp += shift as i32;
            let shift = mloop(pi | qi);
            pi >>= shift;
            qi >>= shift;
            qexp += shift as i32 - 14 * ((m as i32 + 1) >> 1);
            pi = pi.wrapping_mul(pi) >> 16;
            qi = qi.wrapping_mul(qi) >> 16;
            qexp = qexp * 2 + m as i32;
            pi = pi.wrapping_mul(((1 << 14) - ((wi * wi) >> 14)) as u32);
            qi = qi.wrapping_add(pi >> 14);
        } else {
            // Even order: p *= p(1 - w), q *= q(1 + w).
            pi >>= shift;
            qi >>= shift;
            qexp += shift as i32 - 7 * m as i32;
            pi = pi.wrapping_mul(pi) >> 16;
            qi = qi.wrapping_mul(qi) >> 16;
            qexp = qexp * 2 + m as i32;
            pi = pi.wrapping_mul(((1 << 14) - wi) as u32);
            qi = qi.wrapping_mul(((1 << 14) + wi) as u32);
            qi = qi.wrapping_add(pi) >> 14;
        }
        // Normalised again for the lookup: at most one shift right, or
        // some left.
        if qi & 0xffff_0000 != 0 {
            qi >>= 1;
            qexp += 1;
        } else {
            while qi != 0 && qi & 0x8000 == 0 {
                qi <<= 1;
                qexp -= 1;
            }
        }
        let amp = from_db_look(i64::from(
            ampi.wrapping_mul(invsqlook(i64::from(qi), i64::from(qexp)))
                .wrapping_sub(ampoffseti),
        ));
        curve[i] = mult31_shift15(curve[i], amp);
        i += 1;
        while i < n && map[i] == k {
            curve[i] = mult31_shift15(curve[i], amp);
            i += 1;
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn the_bark_scale_is_piecewise_linear() {
        assert_eq!(to_bark(0), 0);
        assert_eq!(to_bark(100), 1 << 15);
        assert_eq!(to_bark(150), (1 << 15) + (50 << 15) / 100);
        assert_eq!(to_bark(40000), 27 << 15);
    }

    #[test]
    fn cosines_run_from_one_to_minus_one() {
        assert_eq!(coslook(0), 16384);
        assert_eq!(coslook(64 << 9), 0);
        assert_eq!(coslook2(0), 16384);
        assert_eq!(coslook2(0x1_0000), -16383);
        assert_eq!(coslook2(0x2_0000 - 5), coslook2(5));
    }

    #[test]
    fn decibels_come_from_the_tables() {
        assert_eq!(from_db_look(1), 0x7fff_ffff);
        assert_eq!(from_db_look(0), FROM_DB[0] * FROM_DB2[0]);
        assert_eq!(from_db_look(-(1120 << 9)), 0);
    }
}
