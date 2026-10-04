//! The inverse MDCT, in place (Tremor's `mdct_backward`): a block's `n / 2`
//! spectral values in, its `n` time-domain values out, in 32-bit fixed
//! point -- a pre-rotation, a split-radix butterfly network, a
//! bit-reversal, and a post-rotation that also unfolds the result.
//!
//! Translated into Rust from Tremor's `mdct.c`, copyright Xiph.Org, used
//! under its BSD licence (`licenses/tremor-COPYING`). Tremor walks the
//! block with pointers, several at once in opposite directions; here each
//! walk is an iterator over fixed-size chunks of the block, in the order
//! the pointer visits them, so that within a chunk every index is a
//! constant. The arithmetic is Tremor's, every sum wrapping as C's does on
//! the machines Tremor runs on; `direct` (the first, pointer-for-pointer
//! translation) is what the tests hold this to.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "index arithmetic bounded by the block size (64 to 8192); the signal's sums wrap explicitly"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "within a chunk every index is a constant below its size; a table index is a step through 513 pairs that stops at the last"
)]

#[cfg(test)]
mod direct;

use crate::misc::{mult31, xnprod31, xprod31, xprod32};
use crate::tables::{SINCOS_LOOKUP0, SINCOS_LOOKUP1};

const C_PI3_8: i32 = 0x30fb_c54d;
const C_PI2_8: i32 = 0x5a82_799a;
const C_PI1_8: i32 = 0x7641_af3d;

/// A table entry: `[sin, cos]`.
type Pair = [i32; 2];

/// `sincos_lookup0` as pairs: 513, for angles 0 to pi/4 in 512 steps.
fn pairs0() -> &'static [Pair] {
    SINCOS_LOOKUP0.as_chunks::<2>().0
}

/// `sincos_lookup1` as pairs: 512, at the half steps.
fn pairs1() -> &'static [Pair] {
    SINCOS_LOOKUP1.as_chunks::<2>().0
}

/// `mdct_butterfly_8`.
#[inline(always)]
fn butterfly_8(x: &mut [i32; 8]) {
    let r0 = x[4].wrapping_add(x[0]);
    let r1 = x[4].wrapping_sub(x[0]);
    let r2 = x[5].wrapping_add(x[1]);
    let r3 = x[5].wrapping_sub(x[1]);
    let r4 = x[6].wrapping_add(x[2]);
    let r5 = x[6].wrapping_sub(x[2]);
    let r6 = x[7].wrapping_add(x[3]);
    let r7 = x[7].wrapping_sub(x[3]);
    x[0] = r5.wrapping_add(r3);
    x[1] = r7.wrapping_sub(r1);
    x[2] = r5.wrapping_sub(r3);
    x[3] = r7.wrapping_add(r1);
    x[4] = r4.wrapping_sub(r0);
    x[5] = r6.wrapping_sub(r2);
    x[6] = r4.wrapping_add(r0);
    x[7] = r6.wrapping_add(r2);
}

/// `r = x[a] - x[b]; x[a] += x[b]`.
#[inline(always)]
fn sub_add_a<const N: usize>(x: &mut [i32; N], a: usize, b: usize) -> i32 {
    let r = x[a].wrapping_sub(x[b]);
    x[a] = x[a].wrapping_add(x[b]);
    r
}

/// `r = x[a] - x[b]; x[b] += x[a]`.
#[inline(always)]
fn sub_add_b<const N: usize>(x: &mut [i32; N], a: usize, b: usize) -> i32 {
    let r = x[a].wrapping_sub(x[b]);
    x[b] = x[b].wrapping_add(x[a]);
    r
}

/// `mdct_butterfly_16`.
#[inline(always)]
fn butterfly_16(x: &mut [i32; 16]) {
    let r0 = sub_add_b(x, 0, 8);
    let r1 = sub_add_b(x, 1, 9);
    x[0] = mult31(r0.wrapping_add(r1), C_PI2_8);
    x[1] = mult31(r1.wrapping_sub(r0), C_PI2_8);

    let r0 = sub_add_a(x, 10, 2);
    let r1 = sub_add_b(x, 3, 11);
    x[2] = r1;
    x[3] = r0;

    let r0 = sub_add_a(x, 12, 4);
    let r1 = sub_add_a(x, 13, 5);
    x[4] = mult31(r0.wrapping_sub(r1), C_PI2_8);
    x[5] = mult31(r0.wrapping_add(r1), C_PI2_8);

    let r0 = sub_add_a(x, 14, 6);
    let r1 = sub_add_a(x, 15, 7);
    x[6] = r0;
    x[7] = r1;

    for half in x.as_chunks_mut::<8>().0 {
        butterfly_8(half);
    }
}

/// `mdct_butterfly_32`.
fn butterfly_32(x: &mut [i32; 32]) {
    let r0 = sub_add_a(x, 30, 14);
    let r1 = sub_add_a(x, 31, 15);
    x[14] = r0;
    x[15] = r1;

    let r0 = sub_add_a(x, 28, 12);
    let r1 = sub_add_a(x, 29, 13);
    (x[12], x[13]) = xnprod31(r0, r1, C_PI1_8, C_PI3_8);

    let r0 = sub_add_a(x, 26, 10);
    let r1 = sub_add_a(x, 27, 11);
    x[10] = mult31(r0.wrapping_sub(r1), C_PI2_8);
    x[11] = mult31(r0.wrapping_add(r1), C_PI2_8);

    let r0 = sub_add_a(x, 24, 8);
    let r1 = sub_add_a(x, 25, 9);
    (x[8], x[9]) = xnprod31(r0, r1, C_PI3_8, C_PI1_8);

    let r0 = sub_add_a(x, 22, 6);
    let r1 = sub_add_b(x, 7, 23);
    x[6] = r1;
    x[7] = r0;

    let r0 = sub_add_b(x, 4, 20);
    let r1 = sub_add_b(x, 5, 21);
    (x[4], x[5]) = xprod31(r0, r1, C_PI3_8, C_PI1_8);

    let r0 = sub_add_b(x, 2, 18);
    let r1 = sub_add_b(x, 3, 19);
    x[2] = mult31(r1.wrapping_add(r0), C_PI2_8);
    x[3] = mult31(r1.wrapping_sub(r0), C_PI2_8);

    let r0 = sub_add_b(x, 0, 16);
    let r1 = sub_add_b(x, 1, 17);
    (x[0], x[1]) = xprod31(r0, r1, C_PI1_8, C_PI3_8);

    for half in x.as_chunks_mut::<16>().0 {
        butterfly_16(half);
    }
}

/// `mdct_butterfly_generic`: one stage over a block, its upper half (`hi`,
/// C's `x1`) against its lower (`lo`, `x2`), both walked down from their
/// ends eight at a time. Each turn takes four twiddles `S` pairs apart, and
/// the four quarters walk the table up, down, up and down again: a turn's
/// four are one chunk of `S4 = 4 S` pairs, read at constant offsets.
fn butterfly_generic<const S: usize, const S4: usize>(x: &mut [i32]) {
    let half = x.len() / 2;
    let (lo, hi) = x.split_at_mut(half);
    let lo = lo.as_chunks_mut::<8>().0;
    let hi = hi.as_chunks_mut::<8>().0;
    let turns = lo.len() / 4;
    let t = pairs0();
    // Up from pair 0: each chunk's 0th, S-th, 2S-th and 3S-th.
    let up = t.as_chunks::<S4>().0;
    // Down from pair 512 (1024 entries): chunks of the pairs above 0, from
    // the top, each read from its last pair down.
    let down = t[1..].as_rchunks::<S4>().1;
    let ups = || up.iter().map(|c| [c[0], c[S], c[2 * S], c[3 * S]]);
    let downs = || {
        down.iter().rev().map(|c| {
            [
                c[S4 - 1],
                c[S4 - 1 - S],
                c[S4 - 1 - 2 * S],
                c[S4 - 1 - 3 * S],
            ]
        })
    };
    let mut chunks = lo.iter_mut().rev().zip(hi.iter_mut().rev());

    // r0 = x1 - x2, r1 = x2 - x1 (the odd ones); XPROD31(r1, r0).
    for ((l, h), tt) in chunks.by_ref().take(turns).zip(ups()) {
        for (k, [t0, t1]) in [6, 4, 2, 0].into_iter().zip(tt) {
            let r0 = h[k].wrapping_sub(l[k]);
            h[k] = h[k].wrapping_add(l[k]);
            let r1 = l[k + 1].wrapping_sub(h[k + 1]);
            h[k + 1] = h[k + 1].wrapping_add(l[k + 1]);
            (l[k], l[k + 1]) = xprod31(r1, r0, t0, t1);
        }
    }
    // r0 = x1 - x2, r1 = x1 - x2; XNPROD31(r0, r1).
    for ((l, h), tt) in chunks.by_ref().take(turns).zip(downs()) {
        for (k, [t0, t1]) in [6, 4, 2, 0].into_iter().zip(tt) {
            let r0 = h[k].wrapping_sub(l[k]);
            h[k] = h[k].wrapping_add(l[k]);
            let r1 = h[k + 1].wrapping_sub(l[k + 1]);
            h[k + 1] = h[k + 1].wrapping_add(l[k + 1]);
            (l[k], l[k + 1]) = xnprod31(r0, r1, t0, t1);
        }
    }
    // r0 = x2 - x1, r1 = x2 - x1; XPROD31(r0, r1).
    for ((l, h), tt) in chunks.by_ref().take(turns).zip(ups()) {
        for (k, [t0, t1]) in [6, 4, 2, 0].into_iter().zip(tt) {
            let r0 = l[k].wrapping_sub(h[k]);
            h[k] = h[k].wrapping_add(l[k]);
            let r1 = l[k + 1].wrapping_sub(h[k + 1]);
            h[k + 1] = h[k + 1].wrapping_add(l[k + 1]);
            (l[k], l[k + 1]) = xprod31(r0, r1, t0, t1);
        }
    }
    // r0 = x1 - x2, r1 = x2 - x1; XNPROD31(r1, r0).
    for ((l, h), tt) in chunks.take(turns).zip(downs()) {
        for (k, [t0, t1]) in [6, 4, 2, 0].into_iter().zip(tt) {
            let r0 = h[k].wrapping_sub(l[k]);
            h[k] = h[k].wrapping_add(l[k]);
            let r1 = l[k + 1].wrapping_sub(h[k + 1]);
            h[k + 1] = h[k + 1].wrapping_add(l[k + 1]);
            (l[k], l[k + 1]) = xnprod31(r1, r0, t0, t1);
        }
    }
}

/// `mdct_butterflies`: the generic stages over `x` (the block's upper
/// half), each on blocks half the size of the last, then the 32-point
/// butterflies that finish them.
fn butterflies(x: &mut [i32], shift: u32) {
    let points = x.len();
    for i in 0..7 - shift {
        let size = points >> i;
        // Tremor's step, `4 << (i + shift)` table entries: in pairs, 2 to
        // 128, each its own copy of the stage.
        let stage = match i + shift {
            0 => butterfly_generic::<2, 8>,
            1 => butterfly_generic::<4, 16>,
            2 => butterfly_generic::<8, 32>,
            3 => butterfly_generic::<16, 64>,
            4 => butterfly_generic::<32, 128>,
            5 => butterfly_generic::<64, 256>,
            _ => butterfly_generic::<128, 512>,
        };
        for block in x.chunks_exact_mut(size) {
            stage(block);
        }
    }
    for chunk in x.as_chunks_mut::<32>().0 {
        butterfly_32(chunk);
    }
}

/// Tremor's 12-bit reversal.
#[inline(always)]
fn bitrev12(x: usize) -> usize {
    const BITREV: [usize; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
    BITREV[(x >> 8) & 15] | (BITREV[(x >> 4) & 15] << 4) | (BITREV[x & 15] << 8)
}

/// One bit-reversed pair of `mdct_bitreverse`'s input (`upper` as pairs:
/// both its places are even), rotated by the twiddle `(ta, tb)`.
#[inline(always)]
fn reversed_pair(
    upper: &[[i32; 2]],
    bit: usize,
    shift: u32,
    ta: i32,
    tb: i32,
) -> (i32, i32, i32, i32) {
    let r3 = bitrev12(bit);
    let [a0, a1] = upper[(((r3 ^ 0xfff) >> shift) - 1) / 2];
    let [b0, b1] = upper[(r3 >> shift) / 2];
    let (r2, r3) = xprod32(a0.wrapping_add(b0), b1.wrapping_sub(a1), ta, tb);
    (a1.wrapping_add(b1) >> 1, a0.wrapping_sub(b0) >> 1, r2, r3)
}

/// `mdct_bitreverse`: reads the butterflies' output (`upper`, the block's
/// second half) at bit-reversed places, writes the first half, `lower`,
/// four values at a time from each end (C's `w0` up, `w1` down). `table`
/// is the twiddles' table, `stride` pairs a step through it.
fn bitreverse(lower: &mut [i32], upper: &[i32], table: &[Pair], stride: usize, shift: u32) {
    let quarter = lower.len() / 2;
    let (front, back) = lower.split_at_mut(quarter);
    let front = front.as_chunks_mut::<4>().0;
    let back = back.as_chunks_mut::<4>().0;
    let upper = upper.as_chunks::<2>().0;
    let turns = front.len();
    for (k, (w0, w1)) in front.iter_mut().zip(back.iter_mut().rev()).enumerate() {
        // The first half of the turns walks the table up, taking each pair
        // cos first; the second walks it down from 1024 entries, sin first.
        let ((s0, s1, r2, r3), (u0, u1, q2, q3)) = if k < turns / 2 {
            let [t0, t1] = table[2 * k * stride];
            let [v0, v1] = table[(2 * k + 1) * stride];
            (
                reversed_pair(upper, 2 * k, shift, t1, t0),
                reversed_pair(upper, 2 * k + 1, shift, v1, v0),
            )
        } else {
            let k2 = k - turns / 2;
            let [t0, t1] = table[512 - (2 * k2 + 1) * stride];
            let [v0, v1] = table[512 - (2 * k2 + 2) * stride];
            (
                reversed_pair(upper, 2 * k, shift, t0, t1),
                reversed_pair(upper, 2 * k + 1, shift, v0, v1),
            )
        };
        w0[0] = s0.wrapping_add(r2);
        w0[1] = s1.wrapping_add(r3);
        w1[2] = s0.wrapping_sub(r2);
        w1[3] = r3.wrapping_sub(s1);
        w0[2] = u0.wrapping_add(q2);
        w0[3] = u1.wrapping_add(q3);
        w1[0] = u0.wrapping_sub(q2);
        w1[1] = q3.wrapping_sub(u1);
    }
}

/// `mdct_backward`, in place: `buf[..n/2]` the spectrum in, `buf[..n]` the
/// block out. `n` is a power of two from 64 to 8192 (a Vorbis block size);
/// another `n` leaves `buf` as it was.
#[inline(never)]
pub(crate) fn backward(n: usize, buf: &mut [i32]) {
    if !(64..=8192).contains(&n) || !n.is_power_of_two() || buf.len() < n {
        return;
    }
    let buf = &mut buf[..n];
    let n2 = n >> 1;
    let n4 = n >> 2;
    // 0 for 8192, 7 for 64.
    let shift = 13 - n.trailing_zeros();
    // Tremor's step, `2 << shift` table entries: in pairs, `1 << shift`.
    let step = 2usize << shift;
    let s = step / 2;
    let t = pairs0();

    // Rotate: the input's eight-value chunks, from its end down, the odd
    // values into out[n2 .. n2 + n4] (filled from its end down) and the
    // even into out[n2 + n4 ..] (from its start up); the first half of the
    // chunks walks the table up, the second down from 1024 entries.
    {
        let (input, output) = buf.split_at_mut(n2);
        let (odd, even) = output.split_at_mut(n4);
        let input = input.as_chunks::<8>().0;
        let odd = odd.as_chunks_mut::<4>().0;
        let even = even.as_chunks_mut::<4>().0;
        let half = input.len() / 2;
        for (c, ((x, o), e)) in input
            .iter()
            .rev()
            .zip(odd.iter_mut().rev())
            .zip(even.iter_mut())
            .enumerate()
        {
            if c < half {
                let [a0, a1] = t[2 * c * s];
                let [b0, b1] = t[(2 * c + 1) * s];
                let [c0, c1] = t[(2 * c + 2) * s];
                (o[2], o[3]) = xprod31(x[5], x[7], a0, a1);
                (o[0], o[1]) = xprod31(x[1], x[3], b0, b1);
                (e[0], e[1]) = xnprod31(x[6], x[4], b0, b1);
                (e[2], e[3]) = xnprod31(x[2], x[0], c0, c1);
            } else {
                let c = c - half;
                let [a0, a1] = t[512 - 2 * c * s];
                let [b0, b1] = t[512 - (2 * c + 1) * s];
                let [c0, c1] = t[512 - (2 * c + 2) * s];
                (o[2], o[3]) = xprod31(x[5], x[7], a1, a0);
                (o[0], o[1]) = xprod31(x[1], x[3], b1, b0);
                (e[0], e[1]) = xnprod31(x[6], x[4], b1, b0);
                (e[2], e[3]) = xnprod31(x[2], x[0], c1, c0);
            }
        }
    }

    {
        let (lower, upper) = buf.split_at_mut(n2);
        butterflies(upper, shift);
        let (table, stride) = if step >= 4 {
            (&t[step / 4..], s)
        } else {
            (pairs1(), 1)
        };
        bitreverse(lower, upper, table, stride, shift);
    }

    // Rotate again, out[..n2] eight at a time into out[n2..], four from
    // its middle down (C's oX1) and four up (oX2).
    let step = step >> 2;
    {
        let (lower, upper) = buf.split_at_mut(n2);
        let (down, up) = upper.split_at_mut(n4);
        let src = lower.as_chunks::<8>().0;
        let down = down.as_chunks_mut::<4>().0;
        let up = up.as_chunks_mut::<4>().0;
        let turns = src.iter().zip(down.iter_mut().rev()).zip(up.iter_mut());
        let neg = i32::wrapping_neg;
        match step {
            0 => {
                // The tables interpolated at a quarter, in half-steps: a
                // pair of each table a turn.
                let [mut t0, mut t1] = t[0];
                for (((x, a), b), (&[v0, v1], &[n0, n1])) in turns.zip(pairs1().iter().zip(&t[1..]))
                {
                    let q0 = v0.wrapping_sub(t0) >> 2;
                    let q1 = v1.wrapping_sub(t1) >> 2;
                    (a[3], b[0]) =
                        xprod31(x[0], neg(x[1]), t0.wrapping_add(q0), t1.wrapping_add(q1));
                    (a[2], b[1]) =
                        xprod31(x[2], neg(x[3]), v0.wrapping_sub(q0), v1.wrapping_sub(q1));
                    let q0 = n0.wrapping_sub(v0) >> 2;
                    let q1 = n1.wrapping_sub(v1) >> 2;
                    (a[1], b[2]) =
                        xprod31(x[4], neg(x[5]), v0.wrapping_add(q0), v1.wrapping_add(q1));
                    (a[0], b[3]) =
                        xprod31(x[6], neg(x[7]), n0.wrapping_sub(q0), n1.wrapping_sub(q1));
                    (t0, t1) = (n0, n1);
                }
            }
            1 => {
                // At a half, in whole steps: two pairs of each a turn.
                let [h0, h1] = t[0];
                let (mut t0, mut t1) = (h0 >> 1, h1 >> 1);
                let vs = pairs1().as_chunks::<2>().0;
                let ts = t[1..].as_chunks::<2>().0;
                for (((x, a), b), (&[[va0, va1], [vb0, vb1]], &[[ta0, ta1], [tb0, tb1]])) in
                    turns.zip(vs.iter().zip(ts))
                {
                    let (v0, v1) = (va0 >> 1, va1 >> 1);
                    (a[3], b[0]) =
                        xprod31(x[0], neg(x[1]), t0.wrapping_add(v0), t1.wrapping_add(v1));
                    (t0, t1) = (ta0 >> 1, ta1 >> 1);
                    (a[2], b[1]) =
                        xprod31(x[2], neg(x[3]), v0.wrapping_add(t0), v1.wrapping_add(t1));
                    let (v0, v1) = (vb0 >> 1, vb1 >> 1);
                    (a[1], b[2]) =
                        xprod31(x[4], neg(x[5]), t0.wrapping_add(v0), t1.wrapping_add(v1));
                    (t0, t1) = (tb0 >> 1, tb1 >> 1);
                    (a[0], b[3]) =
                        xprod31(x[6], neg(x[7]), v0.wrapping_add(t0), v1.wrapping_add(t1));
                }
            }
            _ => {
                let (table, stride) = if step >= 4 {
                    (&t[step / 4..], step / 2)
                } else {
                    (pairs1(), 1)
                };
                for (k, ((x, a), b)) in turns.enumerate() {
                    let at = 4 * k * stride;
                    let [p0, p1] = table[at];
                    let [q0, q1] = table[at + stride];
                    let [r0, r1] = table[at + 2 * stride];
                    let [s0, s1] = table[at + 3 * stride];
                    (a[3], b[0]) = xprod31(x[0], neg(x[1]), p0, p1);
                    (a[2], b[1]) = xprod31(x[2], neg(x[3]), q0, q1);
                    (a[1], b[2]) = xprod31(x[4], neg(x[5]), r0, r1);
                    (a[0], b[3]) = xprod31(x[6], neg(x[7]), s0, s1);
                }
            }
        }
    }

    // Unfold: out[n2 .. n2 + n4], reversed, to out[..n4], and negated to
    // out[n4 .. n2]; then out[n2 + n4 ..], reversed, to out[n2 .. n2 + n4].
    let (lower, upper) = buf.split_at_mut(n2);
    let (first, second) = lower.split_at_mut(n4);
    let (third, fourth) = upper.split_at_mut(n4);
    for ((src, a), b) in third
        .as_chunks::<4>()
        .0
        .iter()
        .rev()
        .zip(first.as_chunks_mut::<4>().0.iter_mut().rev())
        .zip(second.as_chunks_mut::<4>().0.iter_mut())
    {
        *a = *src;
        *b = [
            src[3].wrapping_neg(),
            src[2].wrapping_neg(),
            src[1].wrapping_neg(),
            src[0].wrapping_neg(),
        ];
    }
    for (src, a) in fourth
        .as_chunks::<4>()
        .0
        .iter()
        .zip(third.as_chunks_mut::<4>().0.iter_mut().rev())
    {
        *a = [src[3], src[2], src[1], src[0]];
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

    /// The IMDCT as Vorbis I defines it (section 1.3.2), in floating
    /// point: `y[i] = sum_k X[k] cos(2 pi / n (i + n0)(k + 1/2))`, with
    /// `n0 = (n / 2 + 1) / 2`.
    fn definition(x: &[f64]) -> Vec<f64> {
        let n = x.len() * 2;
        let n0 = f64::midpoint(n as f64 / 2.0, 1.0);
        let w = std::f64::consts::PI * 2.0 / n as f64;
        (0..n)
            .map(|i| {
                x.iter()
                    .enumerate()
                    .map(|(k, &v)| v * (w * (i as f64 + n0) * (k as f64 + 0.5)).cos())
                    .sum()
            })
            .collect()
    }

    fn noise(seed: u32, len: usize, shift: u32) -> Vec<i32> {
        let mut s = seed;
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s as i32) >> shift
            })
            .collect()
    }

    #[test]
    fn every_size_is_the_definition_scaled() {
        for k in 6..=13 {
            let n = 1usize << k;
            let input = noise(0x1234_5678 + k, n / 2, 11);
            let mut buf = input.clone();
            buf.resize(n, 0);
            backward(n, &mut buf);
            let reference = definition(&input.iter().map(|&v| f64::from(v)).collect::<Vec<_>>());
            let num: f64 = buf
                .iter()
                .zip(&reference)
                .map(|(&a, &b)| f64::from(a) * b)
                .sum();
            let den: f64 = reference.iter().map(|b| b * b).sum();
            let scale = num / den;
            let err: f64 = buf
                .iter()
                .zip(&reference)
                .map(|(&a, &b)| (f64::from(a) - scale * b).powi(2))
                .sum();
            let snr = 10.0 * (scale * scale * den / err).log10();
            assert!(snr > 100.0, "n {n}: {snr:.1} dB");
            assert!((scale - 1.0).abs() < 1e-6, "n {n}: scale {scale}");
        }
    }

    /// The chunked walk is the pointer walk: the same output, bit for bit,
    /// at every size, on quiet input and on input loud enough to wrap.
    #[test]
    fn every_size_is_the_direct_translation() {
        for k in 6..=13u32 {
            let n = 1usize << k;
            for (seed, shift) in [(1, 11), (2, 2), (3, 0), (4, 20)] {
                let mut a = noise(seed * 977 + k, n, shift);
                let mut b = a.clone();
                backward(n, &mut a);
                direct::backward(n, &mut b);
                assert_eq!(a, b, "n {n}, input >> {shift}");
            }
        }
    }

    #[test]
    fn other_sizes_leave_the_buffer_alone() {
        let mut buf = vec![7; 100];
        backward(96, &mut buf);
        backward(32, &mut buf);
        backward(128, &mut buf);
        assert!(buf.iter().all(|&v| v == 7));
    }
}
