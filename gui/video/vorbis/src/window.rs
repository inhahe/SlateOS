//! Vorbis's window (Tremor's `window.c`): the power-complementary
//! `sin(pi/2 sin^2(x))` slopes, from Tremor's tables, applied to an
//! inverse-transformed block before it is overlapped with its neighbours.
//!
//! Translated into Rust from Tremor's `window.c`, copyright Xiph.Org, used
//! under its BSD licence (`licenses/tremor-COPYING`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "offsets within a block of 64 to 8192, a neighbour never longer than the block"
)]

use crate::misc::mult31;
use crate::tables::{VWIN64, VWIN128, VWIN256, VWIN512, VWIN1024, VWIN2048, VWIN4096, VWIN8192};

/// `_vorbis_window(0, half)`: the rising slope for a block of `2 * half`;
/// none for a size Vorbis has not.
pub(crate) fn slope(half: usize) -> Option<&'static [i32]> {
    Some(match half {
        32 => &VWIN64,
        64 => &VWIN128,
        128 => &VWIN256,
        256 => &VWIN512,
        512 => &VWIN1024,
        1024 => &VWIN2048,
        2048 => &VWIN4096,
        4096 => &VWIN8192,
        _ => return None,
    })
}

/// `_vorbis_apply_window`: the first `n` of `d`, a block `n` long, shaped
/// by a rising slope as long as the previous block's (`ln`) and a falling
/// one as long as the next's (`rn`), and zero outside them. `slope_l` and
/// `slope_r` are those slopes, `ln / 2` and `rn / 2` long.
///
/// A short block's neighbours count as short (the packet's flags are read
/// only for long blocks), so neither neighbour is longer than the block;
/// were one, the block would be left as it is.
#[inline(never)]
pub(crate) fn apply(
    d: &mut [i32],
    n: usize,
    (ln, slope_l): (usize, &[i32]),
    (rn, slope_r): (usize, &[i32]),
) {
    if ln > n || rn > n || slope_l.len() != ln / 2 || slope_r.len() != rn / 2 {
        return;
    }
    let Some(d) = d.get_mut(..n) else { return };
    let leftbegin = n / 4 - ln / 4;
    let leftend = leftbegin + ln / 2;
    let rightbegin = n / 2 + n / 4 - rn / 4;
    let (head, rest) = d.split_at_mut(leftbegin);
    head.fill(0);
    let (rise, rest) = rest.split_at_mut(ln / 2);
    for (v, &s) in rise.iter_mut().zip(slope_l) {
        *v = mult31(*v, s);
    }
    let (_, rest) = rest.split_at_mut(rightbegin - leftend);
    let (fall, tail) = rest.split_at_mut(rn / 2);
    // The slope is exactly as long as the span: walked back from its end.
    for (v, &s) in fall.iter_mut().zip(slope_r.iter().rev()) {
        *v = mult31(*v, s);
    }
    tail.fill(0);
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
    fn every_block_size_has_a_slope_of_half_its_length() {
        for k in 6..=13 {
            let n = 1usize << k;
            assert_eq!(slope(n / 2).map(<[i32]>::len), Some(n / 2));
        }
        assert!(slope(48).is_none());
    }

    #[test]
    fn a_long_block_between_short_ones_is_flat_in_the_middle() {
        let (n, s) = (512, 64);
        let mut d = vec![1 << 20; n];
        let short = slope(s / 2).unwrap();
        apply(&mut d, n, (s, short), (s, short));
        // Zero, the short slope, flat, the short slope falling, zero.
        let lb = n / 4 - s / 4;
        assert!(d[..lb].iter().all(|&v| v == 0));
        assert_eq!(d[lb], mult31(1 << 20, short[0]));
        assert!(
            d[lb + s / 2..n / 2 + n / 4 - s / 4]
                .iter()
                .all(|&v| v == 1 << 20)
        );
        assert_eq!(d[n / 2 + n / 4 + s / 4 - 1], mult31(1 << 20, short[0]));
        assert!(d[n / 2 + n / 4 + s / 4..].iter().all(|&v| v == 0));
        // The slopes are power complementary: w(i)^2 + w(n/2-1-i)^2 = 1.
        let long = slope(n / 2).unwrap();
        for i in 0..n / 2 {
            let (a, b) = (f64::from(long[i]), f64::from(long[n / 2 - 1 - i]));
            let sum = (a * a + b * b) / f64::from(i32::MAX).powi(2);
            assert!((sum - 1.0).abs() < 1e-6, "{i}: {sum}");
        }
    }
}
