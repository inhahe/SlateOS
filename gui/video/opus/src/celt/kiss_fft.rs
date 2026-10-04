//! The FFT under CELT's MDCT: KISS FFT's mixed-radix decimation in time, in
//! fixed point, for the four sizes the 48 kHz mode uses (480, 240, 120 and 60
//! points: radices 5, 3, 4 and 2).
//!
//! Translated into Rust from libopus 1.5.2's `celt/kiss_fft.c`,
//! `celt/_kiss_fft_guts.h` and the FFT states of
//! `celt/static_modes_fixed.h` (`FIXED_POINT`), copyright Xiph.Org, Mark
//! Borgerding and the contributors named in its `COPYING`, used under
//! libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every butterfly reads and writes within the FFT's own buffer of `nfft` points, at offsets its factors bound (each stage's `m * p` is the size of the sub-transform it combines); the twiddle indices stay below 480, the table's length, for the strides the four states use"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "indices and strides bounded as above; the sample arithmetic wraps where libopus's `_ovflw` macros say it may, and is otherwise within 32 bits for the MDCT's scaled input"
)]

use super::tables::{
    FFT_BITREV60, FFT_BITREV120, FFT_BITREV240, FFT_BITREV480, FFT_TWIDDLES48000_960,
};
use crate::fixed::{mult16_32_q15, qconst16};

/// A complex sample, `kiss_fft_cpx` in fixed point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cpx {
    pub r: i32,
    pub i: i32,
}

/// One FFT size's setup: `kiss_fft_state`.
#[derive(Debug)]
pub(crate) struct FftState {
    /// How many times the twiddle table's stride is doubled; -1 for none.
    shift: i32,
    /// Radix and remaining size, a pair per stage.
    factors: [i16; 16],
    bitrev: &'static [i16],
}

/// The four FFTs of the 48 kHz mode (`fft_state48000_960_0` .. `_3`).
pub(crate) static FFT_STATES: [FftState; 4] = [
    FftState {
        shift: -1,
        factors: [5, 96, 3, 32, 4, 8, 2, 4, 4, 1, 0, 0, 0, 0, 0, 0],
        bitrev: &FFT_BITREV480,
    },
    FftState {
        shift: 1,
        factors: [5, 48, 3, 16, 4, 4, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        bitrev: &FFT_BITREV240,
    },
    FftState {
        shift: 2,
        factors: [5, 24, 3, 8, 2, 4, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        bitrev: &FFT_BITREV120,
    },
    FftState {
        shift: 3,
        factors: [5, 12, 3, 4, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        bitrev: &FFT_BITREV60,
    },
];

/// `S_MUL`: a sample times a Q15 twiddle.
#[inline]
fn s_mul(a: i32, b: i32) -> i32 {
    mult16_32_q15(b, a)
}

/// The twiddle at `k`.
#[inline]
fn tw(k: usize) -> Cpx {
    let (r, i) = FFT_TWIDDLES48000_960[k];
    Cpx {
        r: i32::from(r),
        i: i32::from(i),
    }
}

/// `C_MUL`: `a * b` for a sample and a twiddle.
#[inline]
fn c_mul(a: Cpx, b: Cpx) -> Cpx {
    Cpx {
        r: s_mul(a.r, b.r).wrapping_sub(s_mul(a.i, b.i)),
        i: s_mul(a.r, b.i).wrapping_add(s_mul(a.i, b.r)),
    }
}

#[inline]
fn c_add(a: Cpx, b: Cpx) -> Cpx {
    Cpx {
        r: a.r.wrapping_add(b.r),
        i: a.i.wrapping_add(b.i),
    }
}

#[inline]
fn c_sub(a: Cpx, b: Cpx) -> Cpx {
    Cpx {
        r: a.r.wrapping_sub(b.r),
        i: a.i.wrapping_sub(b.i),
    }
}

/// `kf_bfly2`, after a radix-4 stage (`m` is 4).
fn bfly2(f: &mut [Cpx], n: usize) {
    // libopus's `0.7071067812f`: the same `f32` as the constant.
    let tw = qconst16(std::f32::consts::FRAC_1_SQRT_2, 15);
    for i in 0..n {
        let base = i * 8;
        let (a, b) = (base, base + 4);
        let t = f[b];
        f[b] = c_sub(f[a], t);
        f[a] = c_add(f[a], t);

        let t = Cpx {
            r: s_mul(f[b + 1].r.wrapping_add(f[b + 1].i), tw),
            i: s_mul(f[b + 1].i.wrapping_sub(f[b + 1].r), tw),
        };
        f[b + 1] = c_sub(f[a + 1], t);
        f[a + 1] = c_add(f[a + 1], t);

        let t = Cpx {
            r: f[b + 2].i,
            i: f[b + 2].r.wrapping_neg(),
        };
        f[b + 2] = c_sub(f[a + 2], t);
        f[a + 2] = c_add(f[a + 2], t);

        let t = Cpx {
            r: s_mul(f[b + 3].i.wrapping_sub(f[b + 3].r), tw),
            i: s_mul(f[b + 3].i.wrapping_add(f[b + 3].r).wrapping_neg(), tw),
        };
        f[b + 3] = c_sub(f[a + 3], t);
        f[a + 3] = c_add(f[a + 3], t);
    }
}

/// `kf_bfly4`.
fn bfly4(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    if m == 1 {
        // All the twiddles are 1.
        for i in 0..n {
            let o = i * 4;
            let scratch0 = c_sub(f[o], f[o + 2]);
            f[o] = c_add(f[o], f[o + 2]);
            let mut scratch1 = c_add(f[o + 1], f[o + 3]);
            f[o + 2] = c_sub(f[o], scratch1);
            f[o] = c_add(f[o], scratch1);
            scratch1 = c_sub(f[o + 1], f[o + 3]);
            f[o + 1] = Cpx {
                r: scratch0.r.wrapping_add(scratch1.i),
                i: scratch0.i.wrapping_sub(scratch1.r),
            };
            f[o + 3] = Cpx {
                r: scratch0.r.wrapping_sub(scratch1.i),
                i: scratch0.i.wrapping_add(scratch1.r),
            };
        }
    } else {
        let (m2, m3) = (2 * m, 3 * m);
        for i in 0..n {
            let (mut t1, mut t2, mut t3) = (0usize, 0usize, 0usize);
            for o in i * mm..i * mm + m {
                let s0 = c_mul(f[o + m], tw(t1));
                let s1 = c_mul(f[o + m2], tw(t2));
                let s2 = c_mul(f[o + m3], tw(t3));
                let s5 = c_sub(f[o], s1);
                f[o] = c_add(f[o], s1);
                let s3 = c_add(s0, s2);
                let s4 = c_sub(s0, s2);
                f[o + m2] = c_sub(f[o], s3);
                t1 += fstride;
                t2 += fstride * 2;
                t3 += fstride * 3;
                f[o] = c_add(f[o], s3);
                f[o + m] = Cpx {
                    r: s5.r.wrapping_add(s4.i),
                    i: s5.i.wrapping_sub(s4.r),
                };
                f[o + m3] = Cpx {
                    r: s5.r.wrapping_sub(s4.i),
                    i: s5.i.wrapping_add(s4.r),
                };
            }
        }
    }
}

/// `kf_bfly3`.
fn bfly3(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    let m2 = 2 * m;
    // epi3's imaginary part, -sin(2*pi/3) in Q15.
    let epi3_i = -28378;
    for i in 0..n {
        let (mut t1, mut t2) = (0usize, 0usize);
        for o in i * mm..i * mm + m {
            let s1 = c_mul(f[o + m], tw(t1));
            let s2 = c_mul(f[o + m2], tw(t2));
            let s3 = c_add(s1, s2);
            let mut s0 = c_sub(s1, s2);
            t1 += fstride;
            t2 += fstride * 2;
            f[o + m] = Cpx {
                r: f[o].r.wrapping_sub(s3.r >> 1),
                i: f[o].i.wrapping_sub(s3.i >> 1),
            };
            s0 = Cpx {
                r: s_mul(s0.r, epi3_i),
                i: s_mul(s0.i, epi3_i),
            };
            f[o] = c_add(f[o], s3);
            f[o + m2] = Cpx {
                r: f[o + m].r.wrapping_add(s0.i),
                i: f[o + m].i.wrapping_sub(s0.r),
            };
            f[o + m] = Cpx {
                r: f[o + m].r.wrapping_sub(s0.i),
                i: f[o + m].i.wrapping_add(s0.r),
            };
        }
    }
}

/// `kf_bfly5`.
fn bfly5(f: &mut [Cpx], fstride: usize, m: usize, n: usize, mm: usize) {
    let ya = Cpx {
        r: 10126,
        i: -31164,
    };
    let yb = Cpx {
        r: -26510,
        i: -19261,
    };
    for i in 0..n {
        let base = i * mm;
        let (o0, o1, o2, o3, o4) = (base, base + m, base + 2 * m, base + 3 * m, base + 4 * m);
        for u in 0..m {
            let s0 = f[o0 + u];
            let s1 = c_mul(f[o1 + u], tw(u * fstride));
            let s2 = c_mul(f[o2 + u], tw(2 * u * fstride));
            let s3 = c_mul(f[o3 + u], tw(3 * u * fstride));
            let s4 = c_mul(f[o4 + u], tw(4 * u * fstride));

            let s7 = c_add(s1, s4);
            let s10 = c_sub(s1, s4);
            let s8 = c_add(s2, s3);
            let s9 = c_sub(s2, s3);

            f[o0 + u] = Cpx {
                r: f[o0 + u].r.wrapping_add(s7.r.wrapping_add(s8.r)),
                i: f[o0 + u].i.wrapping_add(s7.i.wrapping_add(s8.i)),
            };

            let s5 = Cpx {
                r: s0
                    .r
                    .wrapping_add(s_mul(s7.r, ya.r).wrapping_add(s_mul(s8.r, yb.r))),
                i: s0
                    .i
                    .wrapping_add(s_mul(s7.i, ya.r).wrapping_add(s_mul(s8.i, yb.r))),
            };
            let s6 = Cpx {
                r: s_mul(s10.i, ya.i).wrapping_add(s_mul(s9.i, yb.i)),
                i: s_mul(s10.r, ya.i)
                    .wrapping_add(s_mul(s9.r, yb.i))
                    .wrapping_neg(),
            };
            f[o1 + u] = c_sub(s5, s6);
            f[o4 + u] = c_add(s5, s6);

            let s11 = Cpx {
                r: s0
                    .r
                    .wrapping_add(s_mul(s7.r, yb.r).wrapping_add(s_mul(s8.r, ya.r))),
                i: s0
                    .i
                    .wrapping_add(s_mul(s7.i, yb.r).wrapping_add(s_mul(s8.i, ya.r))),
            };
            let s12 = Cpx {
                r: s_mul(s9.i, ya.i).wrapping_sub(s_mul(s10.i, yb.i)),
                i: s_mul(s10.r, yb.i).wrapping_sub(s_mul(s9.r, ya.i)),
            };
            f[o2 + u] = c_add(s11, s12);
            f[o3 + u] = c_sub(s11, s12);
        }
    }
}

impl FftState {
    /// `opus_fft_impl`: the transform in place, the input already in
    /// bit-reversed order.
    pub(crate) fn fft_impl(&self, fout: &mut [Cpx]) {
        let shift = u32::try_from(self.shift.max(0)).unwrap_or(0);
        let mut fstride = [0usize; 9];
        fstride[0] = 1;
        let mut l = 0usize;
        let mut m;
        loop {
            let p = usize::try_from(self.factors[2 * l]).unwrap_or(1);
            m = usize::try_from(self.factors[2 * l + 1]).unwrap_or(1);
            fstride[l + 1] = fstride[l] * p;
            l += 1;
            if m == 1 {
                break;
            }
        }
        m = usize::try_from(self.factors[2 * l - 1]).unwrap_or(1);
        for i in (0..l).rev() {
            let m2 = if i != 0 {
                usize::try_from(self.factors[2 * i - 1]).unwrap_or(1)
            } else {
                1
            };
            match self.factors[2 * i] {
                2 => bfly2(fout, fstride[i]),
                4 => bfly4(fout, fstride[i] << shift, m, fstride[i], m2),
                3 => bfly3(fout, fstride[i] << shift, m, fstride[i], m2),
                5 => bfly5(fout, fstride[i] << shift, m, fstride[i], m2),
                _ => {}
            }
            m = m2;
        }
    }

    /// The output position of input `i`.
    pub(crate) fn bitrev(&self, i: usize) -> usize {
        usize::try_from(self.bitrev[i]).unwrap_or(0)
    }
}
