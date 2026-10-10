//! The inverse MDCT as first translated: Tremor's `mdct_backward`
//! followed pointer step by pointer step, an index for each pointer. The
//! optimised one in `super` walks the same data in chunks; this is the
//! oracle it is tested against, at every size and on values that wrap.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "a test oracle, Tremor's arithmetic as Tremor writes it"
)]

use crate::misc::{mult31, xnprod31, xprod31, xprod32};
use crate::tables::{SINCOS_LOOKUP0, SINCOS_LOOKUP1};

const C_PI3_8: i32 = 0x30fb_c54d;
const C_PI2_8: i32 = 0x5a82_799a;
const C_PI1_8: i32 = 0x7641_af3d;

/// `mdct_butterfly_8`.
fn butterfly_8(x: &mut [i32]) {
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
#[inline]
fn sub_add_a(x: &mut [i32], a: usize, b: usize) -> i32 {
    let r = x[a].wrapping_sub(x[b]);
    x[a] = x[a].wrapping_add(x[b]);
    r
}

/// `r = x[a] - x[b]; x[b] += x[a]`.
#[inline]
fn sub_add_b(x: &mut [i32], a: usize, b: usize) -> i32 {
    let r = x[a].wrapping_sub(x[b]);
    x[b] = x[b].wrapping_add(x[a]);
    r
}

/// `mdct_butterfly_16`.
fn butterfly_16(x: &mut [i32]) {
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

    butterfly_8(&mut x[..8]);
    butterfly_8(&mut x[8..16]);
}

/// `mdct_butterfly_32`.
fn butterfly_32(x: &mut [i32]) {
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

    butterfly_16(&mut x[..16]);
    butterfly_16(&mut x[16..32]);
}

/// `mdct_butterfly_generic`: one stage over `x[..points]`, its twiddles
/// every `step` through the table, which it walks up, down, up and down
/// again; the four quarters differ in signs and in which factor comes
/// first.
fn butterfly_generic(x: &mut [i32], points: usize, step: usize) {
    let t = &SINCOS_LOOKUP0;
    let x = &mut x[..points];
    // Each quarter walks the table's 1024 in steps of `step`, four a turn.
    let turns = 256 / step;
    let mut x1 = points - 8;
    let mut x2 = points / 2 - 8;
    let mut ti = 0usize;
    let mut turn = |x1: usize, x2: usize, ti: &mut usize, quarter: u8| {
        for k in [6, 4, 2, 0] {
            let (a, b) = (x1 + k, x2 + k);
            let (t0, t1) = (t[*ti], t[*ti + 1]);
            match quarter {
                0 => {
                    let r0 = x[a].wrapping_sub(x[b]);
                    x[a] = x[a].wrapping_add(x[b]);
                    let r1 = x[b + 1].wrapping_sub(x[a + 1]);
                    x[a + 1] = x[a + 1].wrapping_add(x[b + 1]);
                    (x[b], x[b + 1]) = xprod31(r1, r0, t0, t1);
                    *ti += step;
                }
                1 => {
                    let r0 = x[a].wrapping_sub(x[b]);
                    x[a] = x[a].wrapping_add(x[b]);
                    let r1 = x[a + 1].wrapping_sub(x[b + 1]);
                    x[a + 1] = x[a + 1].wrapping_add(x[b + 1]);
                    (x[b], x[b + 1]) = xnprod31(r0, r1, t0, t1);
                    *ti = ti.wrapping_sub(step);
                }
                2 => {
                    let r0 = x[b].wrapping_sub(x[a]);
                    x[a] = x[a].wrapping_add(x[b]);
                    let r1 = x[b + 1].wrapping_sub(x[a + 1]);
                    x[a + 1] = x[a + 1].wrapping_add(x[b + 1]);
                    (x[b], x[b + 1]) = xprod31(r0, r1, t0, t1);
                    *ti += step;
                }
                _ => {
                    let r0 = x[a].wrapping_sub(x[b]);
                    x[a] = x[a].wrapping_add(x[b]);
                    let r1 = x[b + 1].wrapping_sub(x[a + 1]);
                    x[a + 1] = x[a + 1].wrapping_add(x[b + 1]);
                    (x[b], x[b + 1]) = xnprod31(r1, r0, t0, t1);
                    *ti = ti.wrapping_sub(step);
                }
            }
        }
    };
    for quarter in 0..4u8 {
        for _ in 0..turns {
            turn(x1, x2, &mut ti, quarter);
            // The last turn of all leaves `x2` 8 short of the start, as C's
            // pointer does; wrapping, it is never used.
            x1 = x1.wrapping_sub(8);
            x2 = x2.wrapping_sub(8);
        }
    }
}

/// `mdct_butterflies`: the stages over `x[..points]`, then the 32-point
/// butterflies that finish them.
fn butterflies(x: &mut [i32], points: usize, shift: u32) {
    for i in 0..7 - shift {
        let size = points >> i;
        for j in 0..1usize << i {
            butterfly_generic(&mut x[size * j..], size, 4 << (i + shift));
        }
    }
    for chunk in x[..points].chunks_exact_mut(32) {
        butterfly_32(chunk);
    }
}

/// Tremor's 12-bit reversal.
#[inline]
fn bitrev12(x: usize) -> usize {
    const BITREV: [usize; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
    BITREV[x >> 8] | (BITREV[(x & 0x0f0) >> 4] << 4) | (BITREV[x & 0x00f] << 8)
}

/// `mdct_bitreverse`: reads the butterflies' output in `out[n/2..]`, writes
/// `out[..n/2]`, two pairs from each end a turn.
fn bitreverse(out: &mut [i32], n: usize, step: usize, shift: u32) {
    let t: &[i32] = if step >= 4 {
        &SINCOS_LOOKUP0[step >> 1..]
    } else {
        &SINCOS_LOOKUP1
    };
    let x = n >> 1;
    let mut w0 = 0usize;
    let mut w1 = n >> 1;
    let mut bit = 0usize;
    let mut ti = 0usize;
    // Half the turns walk the table up, half back down.
    let turns = n >> 4;
    for turn in 0..turns {
        let up = turn < turns / 2;
        let pair = |out: &mut [i32], bit: usize, ti: &mut usize| -> (i32, i32, i32, i32) {
            let r3 = bitrev12(bit);
            let x0 = x + ((r3 ^ 0xfff) >> shift) - 1;
            let x1 = x + (r3 >> shift);
            let r0 = out[x0].wrapping_add(out[x1]);
            let r1 = out[x1 + 1].wrapping_sub(out[x0 + 1]);
            let (r2, r3) = if up {
                let v = xprod32(r0, r1, t[*ti + 1], t[*ti]);
                *ti += step;
                v
            } else {
                *ti -= step;
                xprod32(r0, r1, t[*ti], t[*ti + 1])
            };
            let s0 = out[x0 + 1].wrapping_add(out[x1 + 1]) >> 1;
            let s1 = out[x0].wrapping_sub(out[x1]) >> 1;
            (s0, s1, r2, r3)
        };
        let (r0, r1, r2, r3) = pair(out, bit, &mut ti);
        bit += 1;
        w1 -= 4;
        out[w0] = r0.wrapping_add(r2);
        out[w0 + 1] = r1.wrapping_add(r3);
        out[w1 + 2] = r0.wrapping_sub(r2);
        out[w1 + 3] = r3.wrapping_sub(r1);
        let (r0, r1, r2, r3) = pair(out, bit, &mut ti);
        bit += 1;
        out[w0 + 2] = r0.wrapping_add(r2);
        out[w0 + 3] = r1.wrapping_add(r3);
        out[w1] = r0.wrapping_sub(r2);
        out[w1 + 1] = r3.wrapping_sub(r1);
        w0 += 4;
    }
}

/// `mdct_backward`, in place: `buf[..n/2]` the spectrum in, `buf[..n]` the
/// block out. `n` is a power of two from 64 to 8192 (a Vorbis block size);
/// another `n` leaves `buf` as it was.
pub(crate) fn backward(n: usize, buf: &mut [i32]) {
    if !(64..=8192).contains(&n) || !n.is_power_of_two() || buf.len() < n {
        return;
    }
    let buf = &mut buf[..n];
    let n2 = n >> 1;
    let n4 = n >> 2;
    // 0 for 8192, 7 for 64.
    let shift = 13 - n.trailing_zeros();
    let step = 2usize << shift;
    let t = &SINCOS_LOOKUP0;

    // Rotate: the odd inputs, read from the end of the first half down,
    // into out[n2 .. n2 + n4], filled from its end down.
    let turns = n4 >> 3;
    let mut ix = n2 - 7;
    let mut ox = n2 + n4;
    let mut ti = 0usize;
    for half in 0..2 {
        for _ in 0..turns {
            ox -= 4;
            if half == 0 {
                (buf[ox + 2], buf[ox + 3]) = xprod31(buf[ix + 4], buf[ix + 6], t[ti], t[ti + 1]);
                ti += step;
                (buf[ox], buf[ox + 1]) = xprod31(buf[ix], buf[ix + 2], t[ti], t[ti + 1]);
                ti += step;
            } else {
                (buf[ox + 2], buf[ox + 3]) = xprod31(buf[ix + 4], buf[ix + 6], t[ti + 1], t[ti]);
                ti -= step;
                (buf[ox], buf[ox + 1]) = xprod31(buf[ix], buf[ix + 2], t[ti + 1], t[ti]);
                ti = ti.wrapping_sub(step);
            }
            ix = ix.wrapping_sub(8);
        }
    }
    // And the even ones into out[n2 + n4 ..], filled from its start up.
    let mut ix = n2 - 8;
    let mut ox = n2 + n4;
    let mut ti = 0usize;
    for half in 0..2 {
        for _ in 0..turns {
            if half == 0 {
                ti += step;
                (buf[ox], buf[ox + 1]) = xnprod31(buf[ix + 6], buf[ix + 4], t[ti], t[ti + 1]);
                ti += step;
                (buf[ox + 2], buf[ox + 3]) = xnprod31(buf[ix + 2], buf[ix], t[ti], t[ti + 1]);
            } else {
                ti -= step;
                (buf[ox], buf[ox + 1]) = xnprod31(buf[ix + 6], buf[ix + 4], t[ti + 1], t[ti]);
                ti -= step;
                (buf[ox + 2], buf[ox + 3]) = xnprod31(buf[ix + 2], buf[ix], t[ti + 1], t[ti]);
            }
            ix = ix.wrapping_sub(8);
            ox += 4;
        }
    }

    butterflies(&mut buf[n2..], n2, shift);
    bitreverse(buf, n, step, shift);

    // Rotate again, from out[..n2] into out[n2..], from its middle out.
    let step = step >> 2;
    let mut ox1 = n2 + n4;
    let mut ox2 = n2 + n4;
    let mut ix = 0usize;
    let turns = n >> 4;
    match step {
        0 => {
            // Interpolating the tables: at a quarter, in half-steps.
            let (tt, vv) = (&SINCOS_LOOKUP0, &SINCOS_LOOKUP1);
            let (mut ti, mut vi) = (2usize, 0usize);
            let (mut t0, mut t1) = (tt[0], tt[1]);
            for _ in 0..turns {
                ox1 -= 4;
                let (mut v0, mut v1) = (vv[vi], vv[vi + 1]);
                vi += 2;
                let q0 = v0.wrapping_sub(t0) >> 2;
                let q1 = v1.wrapping_sub(t1) >> 2;
                t0 = t0.wrapping_add(q0);
                t1 = t1.wrapping_add(q1);
                (buf[ox1 + 3], buf[ox2]) = xprod31(buf[ix], buf[ix + 1].wrapping_neg(), t0, t1);
                t0 = v0.wrapping_sub(q0);
                t1 = v1.wrapping_sub(q1);
                (buf[ox1 + 2], buf[ox2 + 1]) =
                    xprod31(buf[ix + 2], buf[ix + 3].wrapping_neg(), t0, t1);

                t0 = tt[ti];
                t1 = tt[ti + 1];
                ti += 2;
                let q0 = t0.wrapping_sub(v0) >> 2;
                let q1 = t1.wrapping_sub(v1) >> 2;
                v0 = v0.wrapping_add(q0);
                v1 = v1.wrapping_add(q1);
                (buf[ox1 + 1], buf[ox2 + 2]) =
                    xprod31(buf[ix + 4], buf[ix + 5].wrapping_neg(), v0, v1);
                v0 = t0.wrapping_sub(q0);
                v1 = t1.wrapping_sub(q1);
                (buf[ox1], buf[ox2 + 3]) = xprod31(buf[ix + 6], buf[ix + 7].wrapping_neg(), v0, v1);
                // t0 and t1, the table's last pair, carry into the next turn.
                ox2 += 4;
                ix += 8;
            }
        }
        1 => {
            // Interpolating the tables: at a half, in whole steps.
            let (tt, vv) = (&SINCOS_LOOKUP0, &SINCOS_LOOKUP1);
            let (mut ti, mut vi) = (2usize, 0usize);
            let mut t0 = tt[0] >> 1;
            let mut t1 = tt[1] >> 1;
            for _ in 0..turns {
                ox1 -= 4;
                let mut v0 = vv[vi] >> 1;
                let mut v1 = vv[vi + 1] >> 1;
                vi += 2;
                t0 = t0.wrapping_add(v0);
                t1 = t1.wrapping_add(v1);
                (buf[ox1 + 3], buf[ox2]) = xprod31(buf[ix], buf[ix + 1].wrapping_neg(), t0, t1);
                t0 = tt[ti] >> 1;
                t1 = tt[ti + 1] >> 1;
                ti += 2;
                v0 = v0.wrapping_add(t0);
                v1 = v1.wrapping_add(t1);
                (buf[ox1 + 2], buf[ox2 + 1]) =
                    xprod31(buf[ix + 2], buf[ix + 3].wrapping_neg(), v0, v1);
                v0 = vv[vi] >> 1;
                v1 = vv[vi + 1] >> 1;
                vi += 2;
                t0 = t0.wrapping_add(v0);
                t1 = t1.wrapping_add(v1);
                (buf[ox1 + 1], buf[ox2 + 2]) =
                    xprod31(buf[ix + 4], buf[ix + 5].wrapping_neg(), t0, t1);
                t0 = tt[ti] >> 1;
                t1 = tt[ti + 1] >> 1;
                ti += 2;
                v0 = v0.wrapping_add(t0);
                v1 = v1.wrapping_add(t1);
                (buf[ox1], buf[ox2 + 3]) = xprod31(buf[ix + 6], buf[ix + 7].wrapping_neg(), v0, v1);
                ox2 += 4;
                ix += 8;
            }
        }
        _ => {
            let t: &[i32] = if step >= 4 {
                &SINCOS_LOOKUP0[step >> 1..]
            } else {
                &SINCOS_LOOKUP1
            };
            let mut ti = 0usize;
            for _ in 0..turns {
                ox1 -= 4;
                (buf[ox1 + 3], buf[ox2]) =
                    xprod31(buf[ix], buf[ix + 1].wrapping_neg(), t[ti], t[ti + 1]);
                ti += step;
                (buf[ox1 + 2], buf[ox2 + 1]) =
                    xprod31(buf[ix + 2], buf[ix + 3].wrapping_neg(), t[ti], t[ti + 1]);
                ti += step;
                (buf[ox1 + 1], buf[ox2 + 2]) =
                    xprod31(buf[ix + 4], buf[ix + 5].wrapping_neg(), t[ti], t[ti + 1]);
                ti += step;
                (buf[ox1], buf[ox2 + 3]) =
                    xprod31(buf[ix + 6], buf[ix + 7].wrapping_neg(), t[ti], t[ti + 1]);
                ti += step;
                ox2 += 4;
                ix += 8;
            }
        }
    }

    // Unfold: out[n2 .. n2 + n4], reversed, to out[.. n4] and negated to
    // out[n4 .. n2]; then out[n2 + n4 ..], reversed, to out[n2 .. n2 + n4].
    let mut ix = n2 + n4;
    let mut ox1 = n4;
    let mut ox2 = n4;
    while ox2 < ix {
        ox1 -= 4;
        ix -= 4;
        for k in 0..4 {
            let v = buf[ix + 3 - k];
            buf[ox1 + 3 - k] = v;
            buf[ox2 + k] = v.wrapping_neg();
        }
        ox2 += 4;
    }
    let mut ix = n2 + n4;
    let mut ox1 = n2 + n4;
    while ox1 > n2 {
        ox1 -= 4;
        for k in 0..4 {
            buf[ox1 + k] = buf[ix + 3 - k];
        }
        ix += 4;
    }
}
