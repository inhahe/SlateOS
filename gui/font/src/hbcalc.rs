//! HarfBuzz's own rounding, which is not the C library's.
//!
//! HarfBuzz replaces `roundf` throughout its source (`hb-algs.hh`):
//!
//! ```c
//! /* We want our rounding towards +infinity. */
//! static inline float _hb_roundf (float x) { return floorf (x + .5f); }
//! #define roundf(x) _hb_roundf(x)
//! ```
//!
//! So wherever HarfBuzz "rounds", a half goes *up* -- -2.5 becomes -2, where
//! the C library's `roundf` and Rust's [`f32::round`] make it -3 -- and the
//! sum `x + 0.5` is itself rounded to `f32` first, so the largest `f32`
//! below one half rounds up to 1, and an odd integer from 2^23 to 2^24 rounds
//! to the next one. The positive half of every input agrees with
//! [`f32::round`] except at those edges; the negative halves never do.
//!
//! This crate follows HarfBuzz to the unit wherever it computes something
//! HarfBuzz computes -- normalized coordinates ([`crate::var`]), the deltas
//! of an item variation store ([`crate::varstore`]) -- and those roundings go
//! through here. [`crate::ftcalc`] is the same for FreeType, whose rounding
//! is different again.

/// HarfBuzz's `roundf`: `floorf(x + 0.5f)`, the sum rounded to `f32`.
pub(crate) fn roundf(x: f32) -> f32 {
    (x + 0.5).floor()
}

/// `hb_font_t::scale_glyph_extents` at one unit per font unit, where it is
/// nearly the identity: each edge of the box -- left, top, and right and
/// bottom as the sums `x_bearing + width` and `y_bearing + height` -- goes
/// through `em_fscale_x` or `_y`, which take an `int16_t`, then the left and
/// top edges are floored, the right and bottom ceiled, and the width and
/// height taken between them again. So a box whose edges fit in 16 bits
/// comes back as it went in, and an edge past ±32,767 wraps as C's
/// conversion wraps it on every target HarfBuzz is built for. HarfBuzz
/// applies it to a `COLR` clip box and to an `sbix` picture's box, but not
/// to a box measured from a paint or a `CBDT` glyph's, scaled after it.
pub(crate) fn scale_glyph_extents_at_upem([x, y, width, height]: [i32; 4]) -> [i32; 4] {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "`int` to `int16_t`, which wraps, as HarfBuzz's call does"
    )]
    let edge = |v: i32| i32::from(v as i16);
    let (left, top) = (edge(x), edge(y));
    let (right, bottom) = (edge(x.wrapping_add(width)), edge(y.wrapping_add(height)));
    [
        left,
        top,
        right.wrapping_sub(left),
        bottom.wrapping_sub(top),
    ]
}

/// [`roundf`], converted to an integer as C converts a `float` to an `int`.
///
/// The conversion truncates, which after `roundf` is exact. For the `NaN`
/// or out-of-range result C leaves undefined, this saturates, as `as` does.
pub(crate) fn roundf_i32(x: f32) -> i32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "integral after `roundf`; `as` saturates what C leaves undefined"
    )]
    {
        roundf(x) as i32
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn a_box_that_fits_in_16_bits_scales_to_itself_and_one_that_does_not_wraps() {
        assert_eq!(
            scale_glyph_extents_at_upem([-120, 700, 640, -900]),
            [-120, 700, 640, -900]
        );
        // The right edge, 375,000, is -18,216 as an `int16_t`: HarfBuzz's
        // box for such an `sbix` picture (`bitmap_fixture`'s `sbix_wide`).
        assert_eq!(
            scale_glyph_extents_at_upem([0, 19, 375_000, -19]),
            [0, 19, -18_216, -19]
        );
        // The left edge wraps too, and the width is taken from both.
        assert_eq!(
            scale_glyph_extents_at_upem([40_000, 0, 100, 0]),
            [-25_536, 0, 100, 0]
        );
    }

    #[test]
    fn a_half_rounds_up_on_both_sides_of_zero() {
        assert_eq!(roundf(2.5), 3.0);
        assert_eq!(roundf(-2.5), -2.0);
        assert_eq!(roundf(-0.5), 0.0);
        assert_eq!(roundf_i32(-6.5), -6);
        // Everything that is not a half rounds to nearest, as usual.
        assert_eq!(roundf(-2.6), -3.0);
        assert_eq!(roundf(-2.4), -2.0);
        assert_eq!(roundf(2.4), 2.0);
    }

    #[test]
    fn the_sum_is_rounded_before_it_is_floored() {
        // The largest f32 below one half: plus 0.5 is a tie between the two
        // nearest f32s, which goes to the even one, 1.0.
        let below_half = 0.5f32.next_down();
        assert_eq!(roundf(below_half), 1.0);
        // An odd integer past 2^23, where f32 steps by 1: plus 0.5 is again
        // a tie, again resolved upward to the even neighbour.
        let odd = 8_388_609.0f32;
        assert_eq!(roundf(odd), 8_388_610.0);
        // Past 2^24 the step is 2, and 0.5 is lost altogether.
        assert_eq!(roundf(16_777_218.0), 16_777_218.0);
    }
}
