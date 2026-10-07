//! The block measures the encoder's searches compare candidates by: the sum
//! of absolute differences, the variance and the sum of squared errors of a
//! block against a prediction, the variance against a reference at a
//! sub-pixel position (bilinear), block averages, and the integral
//! projections of the rows and columns a coarse motion estimate matches.
//!
//! Each is libvpx's C version, which its SIMD versions are tested against:
//! the decisions they feed -- partitions, modes, vectors -- must come out
//! as libvpx's, so the measures must too, rounding and all.
//!
//! A block is given as a slice that starts at its top-left sample, and the
//! stride between its rows. A sub-pixel measure reads one column and one
//! row past the block, which the slice must hold (the encoder's references
//! are padded for it).
//!
//! Translated into Rust from libvpx v1.17.0's `vpx_dsp/sad.c`,
//! `vpx_dsp/variance.c` and `vpx_dsp/avg.c` (copyright the WebM project
//! authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "sums of at most 64x64 differences of 8-bit samples: 4096 * 255 fits i32, 4096 * 255^2 fits u32; the narrowings are libvpx's own"
)]

/// The sample at row `r`, column `c` of a block, or 0 past the slice.
#[inline(always)]
fn at(s: &[u8], stride: usize, r: usize, c: usize) -> i32 {
    i32::from(s.get(r * stride + c).copied().unwrap_or(0))
}

/// `W` samples of a slice from `start`, as an array: `None` past its end.
#[inline(always)]
fn row<const W: usize>(s: &[u8], start: usize) -> Option<&[u8; W]> {
    s.get(start..start.checked_add(W)?)?.try_into().ok()
}

/// Run `fixed` for the width of block it is given, if it is one of the
/// five widths blocks have: the measures below are written once, for rows
/// of a width the compiler knows, which it vectorises.
macro_rules! by_width {
    ($w:expr, $fixed:ident($($arg:expr),*)) => {
        match $w {
            4 => $fixed::<4>($($arg),*),
            8 => $fixed::<8>($($arg),*),
            16 => $fixed::<16>($($arg),*),
            32 => $fixed::<32>($($arg),*),
            64 => $fixed::<64>($($arg),*),
            _ => None,
        }
    };
}

/// The sum of absolute differences of a `w` x `h` block: libvpx's
/// `vpx_sad{W}x{H}`. A block not wholly in its slices is measured over the
/// samples it has.
pub(crate) fn sad(
    src: &[u8],
    src_stride: usize,
    r: &[u8],
    ref_stride: usize,
    w: usize,
    h: usize,
) -> u32 {
    if let Some(sad) = by_width!(w, sad_rows(src, src_stride, r, ref_stride, h)) {
        return sad;
    }
    let mut sad = 0u32;
    for y in 0..h {
        let (a, b) = (
            src.get(y * src_stride..).unwrap_or(&[]),
            r.get(y * ref_stride..).unwrap_or(&[]),
        );
        for (&p, &q) in a.iter().zip(b).take(w) {
            sad += u32::from(p.abs_diff(q));
        }
    }
    sad
}

/// `sad` for a block `W` wide; `None` if a row is past a slice.
#[inline(always)]
fn sad_rows<const W: usize>(
    src: &[u8],
    src_stride: usize,
    r: &[u8],
    ref_stride: usize,
    h: usize,
) -> Option<u32> {
    let mut sad = 0u32;
    for y in 0..h {
        let (a, b) = (row::<W>(src, y * src_stride)?, row::<W>(r, y * ref_stride)?);
        let mut row_sad = 0u32;
        for (&p, &q) in a.iter().zip(b) {
            row_sad += u32::from(p.abs_diff(q));
        }
        sad += row_sad;
    }
    Some(sad)
}

/// The sum of squared differences and the sum of differences of a `w` x `h`
/// block, `src` less `r`: libvpx's `variance`. A block not wholly in its
/// slices is measured over the samples it has.
pub(crate) fn sse_sum(
    src: &[u8],
    src_stride: usize,
    r: &[u8],
    ref_stride: usize,
    w: usize,
    h: usize,
) -> (u32, i32) {
    if let Some(sums) = by_width!(w, sse_sum_rows(src, src_stride, r, ref_stride, h)) {
        return sums;
    }
    let (mut sse, mut sum) = (0u32, 0i32);
    for y in 0..h {
        let (a, b) = (
            src.get(y * src_stride..).unwrap_or(&[]),
            r.get(y * ref_stride..).unwrap_or(&[]),
        );
        for (&p, &q) in a.iter().zip(b).take(w) {
            let d = i32::from(p) - i32::from(q);
            sum += d;
            sse = sse.wrapping_add((d * d) as u32);
        }
    }
    (sse, sum)
}

/// `sse_sum` for a block `W` wide; `None` if a row is past a slice. The
/// differences are 16-bit and a row's squares fit 32 bits (64 * 255^2), so
/// the compiler can multiply and add them pairwise.
#[inline(always)]
fn sse_sum_rows<const W: usize>(
    src: &[u8],
    src_stride: usize,
    r: &[u8],
    ref_stride: usize,
    h: usize,
) -> Option<(u32, i32)> {
    let (mut sse, mut sum) = (0u32, 0i32);
    for y in 0..h {
        let (a, b) = (row::<W>(src, y * src_stride)?, row::<W>(r, y * ref_stride)?);
        let (mut row_sse, mut row_sum) = (0i32, 0i32);
        for (&p, &q) in a.iter().zip(b) {
            let d = i32::from(i16::from(p) - i16::from(q));
            row_sum += d;
            row_sse += d * d;
        }
        sse = sse.wrapping_add(row_sse as u32);
        sum += row_sum;
    }
    Some((sse, sum))
}

/// The variance of a `w` x `h` block against a prediction, and its sum of
/// squared differences: libvpx's `vpx_variance{W}x{H}` (the variance is
/// the sum of squares less the square of the sum over the samples).
pub(crate) fn variance(
    src: &[u8],
    src_stride: usize,
    r: &[u8],
    ref_stride: usize,
    w: usize,
    h: usize,
) -> (u32, u32) {
    let (sse, sum) = sse_sum(src, src_stride, r, ref_stride, w, h);
    let n = (w * h) as i64;
    let var = sse.wrapping_sub(((i64::from(sum) * i64::from(sum)) / n.max(1)) as u32);
    (var, sse)
}

/// libvpx's `bilinear_filters`, by eighth-pixel offset.
const BILINEAR: [[i32; 2]; 8] = [
    [128, 0],
    [112, 16],
    [96, 32],
    [80, 48],
    [64, 64],
    [48, 80],
    [32, 96],
    [16, 112],
];

/// The variance of a `w` x `h` block against the reference `pre` at an
/// eighth-pixel offset (`x_off`, `y_off`, 0 to 7) from its top-left, the
/// reference filtered bilinearly in two passes, and the sum of squared
/// differences: libvpx's `vpx_sub_pixel_variance{W}x{H}`.
///
/// The motion search's commonest measure, so it is done a row at a time for
/// the five widths blocks have -- the first pass's two rows in hand, the
/// second pass and the differences taken as they come -- in 16-bit lanes the
/// compiler vectorises: every value of either pass is at most 255 * 128 +
/// 64, as in libvpx's C, so the result is the same to the bit. A block whose
/// rows are not all in the slices takes the general path below, which reads
/// a sample past them as 0.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sub_pixel_variance(
    pre: &[u8],
    pre_stride: usize,
    x_off: usize,
    y_off: usize,
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
) -> (u32, u32) {
    let [fx0, fx1] = BILINEAR.get(x_off & 7).copied().unwrap_or([128, 0]);
    let [fy0, fy1] = BILINEAR.get(y_off & 7).copied().unwrap_or([128, 0]);
    let (w, h) = (w.min(64), h.min(64));
    let taps = |a: i32, b: i32| [a as u16, b as u16];
    let (fx, fy) = (taps(fx0, fx1), taps(fy0, fy1));
    let rows = by_width!(w, subpel_rows(pre, pre_stride, fx, fy, src, src_stride, h));
    if let Some((sse, sum)) = rows {
        let n = (w * h) as i64;
        let var = sse.wrapping_sub(((i64::from(sum) * i64::from(sum)) / n.max(1)) as u32);
        return (var, sse);
    }
    sub_pixel_variance_any(
        pre,
        pre_stride,
        [fx0, fx1],
        [fy0, fy1],
        src,
        src_stride,
        w,
        h,
    )
}

/// The first bilinear pass of one row of a block `W` wide, from the row
/// (`a`) and the row one sample on (`b`).
#[inline(always)]
fn subpel_first<const W: usize>(a: &[u8; W], b: &[u8; W], [f0, f1]: [u16; 2]) -> [u16; W] {
    let mut out = [0u16; W];
    for ((o, &a), &b) in out.iter_mut().zip(a).zip(b) {
        *o = (u16::from(a) * f0 + u16::from(b) * f1 + 64) >> 7;
    }
    out
}

/// `sub_pixel_variance`'s sums for a block `W` wide: the sum of squared
/// differences and the sum of differences, the prediction less the source;
/// `None` if a row it needs is past a slice.
#[inline(always)]
fn subpel_rows<const W: usize>(
    pre: &[u8],
    pre_stride: usize,
    fx: [u16; 2],
    [fy0, fy1]: [u16; 2],
    src: &[u8],
    src_stride: usize,
    h: usize,
) -> Option<(u32, i32)> {
    let first = |y: usize| {
        let start = y * pre_stride;
        Some(subpel_first::<W>(
            row::<W>(pre, start)?,
            row::<W>(pre, start + 1)?,
            fx,
        ))
    };
    let mut above = first(0)?;
    let (mut sse, mut sum) = (0u32, 0i32);
    for y in 0..h {
        let below = first(y + 1)?;
        let s = row::<W>(src, y * src_stride)?;
        let (mut row_sse, mut row_sum) = (0i32, 0i32);
        for ((&a, &b), &q) in above.iter().zip(&below).zip(s) {
            // Both passes' values are at most 255, so the difference is an
            // i16 and its square a sum of products pmaddwd-style.
            let p = (a * fy0 + b * fy1 + 64) >> 7;
            let d = i32::from(p as i16 - i16::from(q));
            row_sum += d;
            row_sse += d * d;
        }
        sse = sse.wrapping_add(row_sse as u32);
        sum += row_sum;
        above = below;
    }
    Some((sse, sum))
}

/// `sub_pixel_variance` for any block up to 64x64 and any slices, a sample
/// past a slice read as 0: the two passes whole, as libvpx's C does them.
#[allow(clippy::too_many_arguments)]
fn sub_pixel_variance_any(
    pre: &[u8],
    pre_stride: usize,
    [fx0, fx1]: [i32; 2],
    [fy0, fy1]: [i32; 2],
    src: &[u8],
    src_stride: usize,
    w: usize,
    h: usize,
) -> (u32, u32) {
    // var_filter_block2d_bil_first_pass: h + 1 rows, across.
    let mut first = [0u16; 65 * 64];
    for (y, row) in first.chunks_exact_mut(w.max(1)).take(h + 1).enumerate() {
        for (x, out) in row.iter_mut().enumerate() {
            let v = at(pre, pre_stride, y, x) * fx0 + at(pre, pre_stride, y, x + 1) * fx1;
            *out = ((v + 64) >> 7) as u16;
        }
    }
    // The second pass: down.
    let mut second = [0u8; 64 * 64];
    let at_first = |i: usize| i32::from(first.get(i).copied().unwrap_or(0));
    for (i, out) in second.iter_mut().take(w * h).enumerate() {
        let v = at_first(i) * fy0 + at_first(i + w) * fy1;
        *out = ((v + 64) >> 7) as u8;
    }
    variance(&second, w, src, src_stride, w, h)
}

/// The rounded average of an 8x8 block: libvpx's `vpx_avg_8x8`.
pub(crate) fn avg_8x8(s: &[u8], stride: usize) -> i32 {
    let mut sum = 0;
    for y in 0..8 {
        for x in 0..8 {
            sum += at(s, stride, y, x);
        }
    }
    (sum + 32) >> 6
}

/// The rounded average of a 4x4 block: libvpx's `vpx_avg_4x4`.
pub(crate) fn avg_4x4(s: &[u8], stride: usize) -> i32 {
    let mut sum = 0;
    for y in 0..4 {
        for x in 0..4 {
            sum += at(s, stride, y, x);
        }
    }
    (sum + 8) >> 4
}

/// Sixteen column sums of a block `height` rows tall, each divided by half
/// the height: libvpx's `vpx_int_pro_row`.
pub(crate) fn int_pro_row(hbuf: &mut [i16], r: &[u8], stride: usize, height: usize) {
    let norm = (height >> 1).max(1) as i32;
    for (idx, out) in hbuf.iter_mut().take(16).enumerate() {
        let mut sum = 0i32;
        for y in 0..height {
            sum += at(r, stride, y, idx);
        }
        // libvpx sums into an int16_t, which holds 64 rows of 255.
        *out = ((sum as i16) as i32 / norm) as i16;
    }
}

/// The sum of a row of `width` samples: libvpx's `vpx_int_pro_col`.
pub(crate) fn int_pro_col(r: &[u8], width: usize) -> i16 {
    let mut sum = 0i32;
    for &v in r.iter().take(width) {
        sum += i32::from(v);
    }
    sum as i16
}

/// The variance of the difference of two projections `4 << bwl` long:
/// libvpx's `vpx_vector_var`.
pub(crate) fn vector_var(r: &[i16], src: &[i16], bwl: u32) -> i32 {
    let width = 4usize << bwl;
    let (mut sse, mut mean) = (0i32, 0i32);
    for (&a, &b) in r.iter().zip(src).take(width) {
        let diff = i32::from(a) - i32::from(b);
        mean += diff;
        sse += diff * diff;
    }
    sse - ((mean * mean) >> (bwl + 2))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u8 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 24) as u8
        }
    }

    /// The measures by their definitions, on random blocks of every size.
    #[test]
    fn the_measures_are_their_definitions() {
        let mut rng = Lcg(7);
        let a: Vec<u8> = (0..80 * 80).map(|_| rng.next()).collect();
        let b: Vec<u8> = (0..80 * 80).map(|_| rng.next()).collect();
        for (w, h) in [
            (4, 4),
            (4, 8),
            (8, 4),
            (8, 8),
            (16, 8),
            (8, 16),
            (16, 16),
            (32, 64),
            (64, 64),
        ] {
            let (mut sad_want, mut sse, mut sum) = (0u32, 0u64, 0i64);
            for y in 0..h {
                for x in 0..w {
                    let d = i64::from(a[y * 80 + x]) - i64::from(b[y * 80 + x]);
                    sad_want += d.unsigned_abs() as u32;
                    sse += (d * d) as u64;
                    sum += d;
                }
            }
            assert_eq!(sad(&a, 80, &b, 80, w, h), sad_want);
            let (var, got_sse) = variance(&a, 80, &b, 80, w, h);
            assert_eq!(u64::from(got_sse), sse);
            assert_eq!(i64::from(var), sse as i64 - sum * sum / (w * h) as i64);
            // At offset zero the bilinear filter copies.
            assert_eq!(
                sub_pixel_variance(&b, 80, 0, 0, &a, 80, w, h),
                (var, got_sse)
            );
        }
    }

    /// A block whose reference slice stops one sample short is measured over
    /// the samples it has, as is a block of a width the fast paths do not
    /// take; a block that just fits takes them, at the slice's very end.
    #[test]
    fn a_block_cut_short_is_measured_over_what_it_has() {
        let mut rng = Lcg(3);
        let a: Vec<u8> = (0..80 * 80).map(|_| rng.next()).collect();
        let b: Vec<u8> = (0..80 * 80).map(|_| rng.next()).collect();
        for (w, h) in [(4, 4), (8, 4), (16, 8), (32, 32), (64, 64), (12, 7)] {
            for missing in [1, 0] {
                let r = &b[..(h - 1) * 80 + w - missing];
                let (mut sad_want, mut sse, mut sum) = (0u32, 0u32, 0i32);
                for y in 0..h {
                    for x in 0..w {
                        let Some(&q) = r.get(y * 80 + x) else {
                            continue;
                        };
                        let d = i32::from(a[y * 80 + x]) - i32::from(q);
                        sad_want += d.unsigned_abs();
                        sse += (d * d) as u32;
                        sum += d;
                    }
                }
                assert_eq!(
                    sad(&a, 80, r, 80, w, h),
                    sad_want,
                    "{w}x{h}, {missing} missing"
                );
                assert_eq!(
                    sse_sum(&a, 80, r, 80, w, h),
                    (sse, sum),
                    "{w}x{h}, {missing} missing"
                );
            }
        }
    }

    /// `sub_pixel_variance` by libvpx's definition: both passes whole, in
    /// wide arithmetic, a reference sample past the slice read as 0.
    #[allow(clippy::too_many_arguments)]
    fn sub_pixel_variance_by_definition(
        pre: &[u8],
        pre_stride: usize,
        x_off: usize,
        y_off: usize,
        src: &[u8],
        src_stride: usize,
        w: usize,
        h: usize,
    ) -> (u32, u32) {
        let [fx0, fx1] = BILINEAR[x_off];
        let [fy0, fy1] = BILINEAR[y_off];
        let get = |y: usize, x: usize| pre.get(y * pre_stride + x).map_or(0, |&v| i64::from(v));
        let first: Vec<Vec<i64>> = (0..=h)
            .map(|y| {
                (0..w)
                    .map(|x| {
                        (get(y, x) * i64::from(fx0) + get(y, x + 1) * i64::from(fx1) + 64) >> 7
                    })
                    .collect()
            })
            .collect();
        let (mut sse, mut sum) = (0i64, 0i64);
        for y in 0..h {
            for x in 0..w {
                let p = (first[y][x] * i64::from(fy0) + first[y + 1][x] * i64::from(fy1) + 64) >> 7;
                let d = p - i64::from(src[y * src_stride + x]);
                sse += d * d;
                sum += d;
            }
        }
        ((sse - sum * sum / (w * h) as i64) as u32, sse as u32)
    }

    /// Every block size at every eighth-pixel offset, on random samples,
    /// on the extremes (the largest sums there are), and with a reference
    /// slice that stops one sample short of the last row's extra column,
    /// which the fast path leaves to the general one.
    #[test]
    fn the_sub_pixel_variance_is_its_definition() {
        const SIZES: [(usize, usize); 13] = [
            (4, 4),
            (4, 8),
            (8, 4),
            (8, 8),
            (8, 16),
            (16, 8),
            (16, 16),
            (16, 32),
            (32, 16),
            (32, 32),
            (32, 64),
            (64, 32),
            (64, 64),
        ];
        let stride = 80;
        let mut rng = Lcg(11);
        let random: Vec<u8> = (0..stride * 80).map(|_| rng.next()).collect();
        let src: Vec<u8> = (0..stride * 80).map(|_| rng.next()).collect();
        let white = vec![255u8; stride * 80];
        let black = vec![0u8; stride * 80];
        for (w, h) in SIZES {
            for x_off in 0..8 {
                for y_off in 0..8 {
                    for (pre, src) in [(&random, &src), (&white, &black), (&black, &white)] {
                        // At a few places in the plane, the last at its edge.
                        for at in [0, 3 * stride + 5, (80 - h - 1) * stride + (stride - w - 1)] {
                            let got = sub_pixel_variance(
                                &pre[at..],
                                stride,
                                x_off,
                                y_off,
                                src,
                                stride,
                                w,
                                h,
                            );
                            let want = sub_pixel_variance_by_definition(
                                &pre[at..],
                                stride,
                                x_off,
                                y_off,
                                src,
                                stride,
                                w,
                                h,
                            );
                            assert_eq!(got, want, "{w}x{h} at ({x_off}, {y_off}) from {at}");
                        }
                    }
                    let tight = &random[..h * stride + w];
                    assert_eq!(
                        sub_pixel_variance(tight, stride, x_off, y_off, &src, stride, w, h),
                        sub_pixel_variance_by_definition(
                            tight, stride, x_off, y_off, &src, stride, w, h
                        ),
                        "{w}x{h} at ({x_off}, {y_off}), the slice one short"
                    );
                }
            }
        }
        // The largest sum of squares: 64x64 differences of 255.
        let (var, sse) = sub_pixel_variance(&white, stride, 3, 5, &black, stride, 64, 64);
        assert_eq!((var, sse), (0, 4096 * 255 * 255));
    }

    /// The half-pixel position averages neighbours with rounding, each pass.
    #[test]
    fn the_half_pixel_filter_rounds_each_pass() {
        // A 4x4 block of a ramp: 0, 1, 2, ... across, and 10 more each row.
        let pre: Vec<u8> = (0..5 * 5).map(|i| ((i % 5) + 10 * (i / 5)) as u8).collect();
        // Horizontal half: (64 * x + 64 * (x + 1) + 64) >> 7 = x + 1 (rounded up from x + 0.5).
        let src: Vec<u8> = (0..16)
            .map(|i| ((i % 4) + 1 + 10 * (i / 4)) as u8)
            .collect();
        let (var, sse) = sub_pixel_variance(&pre, 5, 4, 0, &src, 4, 4, 4);
        assert_eq!((var, sse), (0, 0));
    }

    #[test]
    fn averages_round_to_nearest() {
        let s = [1u8; 64];
        assert_eq!(avg_8x8(&s, 8), 1);
        let mut t = [0u8; 16];
        t[0] = 8;
        assert_eq!(avg_4x4(&t, 4), 1, "8 / 16 rounds up");
        t[0] = 7;
        assert_eq!(avg_4x4(&t, 4), 0, "7 / 16 rounds down");
    }

    #[test]
    fn projections_sum_and_compare() {
        let r = [2u8; 64 * 4];
        let mut h = [0i16; 16];
        int_pro_row(&mut h, &r, 64, 4);
        assert_eq!(h, [4; 16], "four rows of 2, over half the height");
        assert_eq!(int_pro_col(&r, 16), 32);
        let a = [3i16; 16];
        let b = [1i16; 16];
        assert_eq!(
            vector_var(&a, &b, 2),
            0,
            "a constant difference has no variance"
        );
    }
}
