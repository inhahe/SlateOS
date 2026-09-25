//! Color types for the GUI toolkit.

use core::num::{NonZeroU32, NonZeroU64};

/// RGBA color (8 bits per channel).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as u8,
            g: ((hex >> 8) & 0xFF) as u8,
            b: (hex & 0xFF) as u8,
            a: 255,
        }
    }

    /// Blend this color over `below` using alpha compositing.
    pub fn over(self, below: Color) -> Color {
        if self.a == 255 {
            return self;
        }
        if self.a == 0 {
            return below;
        }

        // Use u32 for the intermediates: a naive `channel * da * inv_sa` can
        // reach 255*255*255 ≈ 16.6M, which overflows u16 and panics in debug.
        // Every product below is bounded by 255*255 = 65_025 and every sum by
        // twice that, so the saturating forms never actually saturate; they
        // are here so the bound is stated in the operation rather than in a
        // comment a later edit can leave behind.
        let sa = u32::from(self.a);
        let da = u32::from(below.a);
        // The opaque and transparent cases returned above, so `sa` is 1..=254
        // and `inv_sa` is 1..=254 too.
        let inv_sa = 255_u32.saturating_sub(sa);

        // Destination alpha contribution once covered by the source (0..=255).
        let da_contrib = da.saturating_mul(inv_sa) / 255;
        // The divisor for every channel. Carrying it as a `NonZeroU32` is what
        // makes the division below safe *here*, rather than because of an
        // `out_a == 0` test standing two statements away from it — and it is
        // the same reason the compiler can drop the check. `sa >= 1` already,
        // so the `else` is unreachable; `Color::TRANSPARENT` is nonetheless
        // the right answer for "nothing is covering anything".
        let Some(out_a) = NonZeroU32::new(sa.saturating_add(da_contrib)) else {
            return Color::TRANSPARENT;
        };

        // Numerator peaks at 255*255 + 255*255 = 130_050, well within u32, and
        // the quotient cannot exceed 255 because the weights sum to `out_a`.
        let blend = |src: u8, dst: u8| -> u8 {
            let weighted = u32::from(src)
                .saturating_mul(sa)
                .saturating_add(u32::from(dst).saturating_mul(da_contrib));
            u8::try_from(weighted / out_a).unwrap_or(u8::MAX)
        };
        let r = blend(self.r, below.r);
        let g = blend(self.g, below.g);
        let b = blend(self.b, below.b);

        Color::rgba(r, g, b, u8::try_from(out_a.get()).unwrap_or(u8::MAX))
    }

    /// Linear interpolation between two colors.
    ///
    /// `t` outside `0.0..=1.0` is clamped to it. A NaN `t` is read as 0 — no
    /// progress — because `f32::clamp` passes NaN straight through, and the
    /// interpolation would then compute `NaN as f32 as u8`, which is 0 in
    /// every channel: a flash of transparent black instead of either end
    /// colour. NaN is not hypothetical here; it is what an animation gets the
    /// instant it divides elapsed time by a zero duration.
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        let inv_t = 1.0 - t;
        Color::rgba(
            (self.r as f32 * inv_t + other.r as f32 * t) as u8,
            (self.g as f32 * inv_t + other.g as f32 * t) as u8,
            (self.b as f32 * inv_t + other.b as f32 * t) as u8,
            (self.a as f32 * inv_t + other.a as f32 * t) as u8,
        )
    }

    /// The mean of a region of pixels, or `None` if there were none: its
    /// alpha is the plain mean of their alphas, and its colour the mean of
    /// their colours **each weighted by its alpha** -- by how much of it
    /// shows. A region nothing in which shows comes out clear.
    ///
    /// This is what a box filter needs -- the average of a source region --
    /// and both halves of the rule are why it lives here rather than at the
    /// call site.
    ///
    /// **Weighted, because a hidden colour is not a colour.** A fully
    /// transparent pixel's colour is never seen and is usually black. A plain
    /// per-channel mean counted it anyway, so half a cell of opaque red over
    /// transparent black became *dark* red at half opacity, and every
    /// see-through edge of a shrunken icon, PNG or GIF came out with a dark
    /// rim. For an opaque region the two rules are the same arithmetic and
    /// give the same bytes. `imagecodec`'s scaled decode applies the same
    /// rule with the same rounding (lane F, 2026-09-25), and must: a
    /// thumbnail is a scaled decode followed by this, and the two would
    /// otherwise disagree about the same picture.
    ///
    /// **`None` for nothing.** "Divide by the number of samples" is only
    /// meaningful when there was at least one, and a caller that writes
    /// `if n > 0 { .. sum / n .. }` has put that condition in a different
    /// statement from the division it licenses. Here the count is a
    /// `NonZeroU64` by the time it is divided by, so the condition and the
    /// division are the same expression -- and the same holds for the alpha
    /// sum the colours are divided by.
    ///
    /// The sums are `u64`: a pixel contributes at most 255 to the alpha sum
    /// and 255 x 255 to each weighted colour sum, so on a 64-bit machine they
    /// are within range for any canvas that could be allocated. The
    /// saturating operations state that bound rather than leaving it to this
    /// comment.
    pub fn mean(colors: impl IntoIterator<Item = Color>) -> Option<Color> {
        let (mut r, mut g, mut b, mut a) = (0_u64, 0_u64, 0_u64, 0_u64);
        let mut count = 0_u64;
        for c in colors {
            let alpha = u64::from(c.a);
            r = r.saturating_add(u64::from(c.r).saturating_mul(alpha));
            g = g.saturating_add(u64::from(c.g).saturating_mul(alpha));
            b = b.saturating_add(u64::from(c.b).saturating_mul(alpha));
            a = a.saturating_add(alpha);
            count = count.saturating_add(1);
        }
        let count = NonZeroU64::new(count)?;
        // Nothing in the region shows, so it has no colour to average.
        let Some(shown) = NonZeroU64::new(a) else {
            return Some(Color::TRANSPARENT);
        };
        // The alpha sum is at most `count * 255` and each weighted sum at most
        // `shown * 255`, so every quotient is at most 255 and the `unwrap_or`s
        // are unreachable -- here so the ceiling is stated in the operation
        // rather than in this sentence.
        let byte = |sum: u64, over: NonZeroU64| u8::try_from(sum / over).unwrap_or(u8::MAX);
        Some(Color::rgba(
            byte(r, shown),
            byte(g, shown),
            byte(b, shown),
            byte(a, count),
        ))
    }

    // Common color constants
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(220, 50, 50);
    pub const GREEN: Color = Color::rgb(50, 180, 50);
    pub const BLUE: Color = Color::rgb(50, 100, 220);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    pub const LIGHT_GRAY: Color = Color::rgb(200, 200, 200);
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    #[test]
    fn an_opaque_or_transparent_source_needs_no_blending() {
        let red = Color::rgb(200, 0, 0);
        let blue = Color::rgb(0, 0, 200);
        assert_eq!(red.over(blue), red, "an opaque source hides what is below");
        assert_eq!(
            Color::rgba(200, 0, 0, 0).over(blue),
            blue,
            "a transparent source changes nothing"
        );
        assert_eq!(
            Color::TRANSPARENT.over(Color::TRANSPARENT),
            Color::TRANSPARENT,
            "nothing over nothing is still nothing"
        );
    }

    #[test]
    fn a_half_transparent_source_lands_halfway() {
        // 128/255 of white over black. The exact answer is not 128 — the
        // weights are integers — but it is within one of half, and it must be
        // the same for all three channels.
        let blended = Color::rgba(255, 255, 255, 128).over(Color::BLACK);
        assert_eq!(
            blended.a, 255,
            "over an opaque backdrop the result is opaque"
        );
        for channel in [blended.r, blended.g, blended.b] {
            assert!(
                (127..=129).contains(&channel),
                "expected about half, got {channel}"
            );
        }
    }

    #[test]
    fn compositing_over_a_transparent_backdrop_keeps_the_source_colour() {
        // Nothing to mix with, so the colour survives untouched and only the
        // alpha carries over. A divisor bug shows up here first: `out_a` is
        // exactly the source alpha, so any other divisor changes the hue.
        for alpha in 1..=254u8 {
            let src = Color::rgba(10, 120, 240, alpha);
            let out = src.over(Color::TRANSPARENT);
            assert_eq!(
                (out.r, out.g, out.b, out.a),
                (10, 120, 240, alpha),
                "alpha {alpha}"
            );
        }
    }

    #[test]
    fn no_pair_of_colours_can_make_over_panic_or_overflow() {
        // The whole alpha cross-product, with the channel extremes. The
        // intermediates reach 255*255 and the divisor is computed rather than
        // constant, which is exactly the shape that used to need an
        // `out_a == 0` test standing apart from the division it guarded.
        for sa in 0..=255u8 {
            for da in 0..=255u8 {
                let out = Color::rgba(255, 0, 255, sa).over(Color::rgba(0, 255, 0, da));
                // Compositing never produces less alpha than either input.
                assert!(out.a >= sa, "alpha shrank: {sa} over {da} gave {}", out.a);
                assert!(out.a >= da || sa == 0, "alpha shrank below the backdrop");
            }
        }
    }

    #[test]
    fn lerp_hits_both_ends_and_is_clamped_outside_them() {
        let a = Color::rgba(0, 50, 100, 150);
        let b = Color::rgba(200, 100, 0, 255);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(a.lerp(b, -5.0), a, "t below 0 clamps to the start");
        assert_eq!(a.lerp(b, 5.0), b, "t above 1 clamps to the end");
        // `f32::clamp` does *not* fold NaN into the range — it returns NaN,
        // and `NaN as u8` is 0, so this used to produce transparent black
        // rather than a colour anywhere between the two.
        assert_eq!(a.lerp(b, f32::NAN), a, "a NaN factor means no progress");
    }

    #[test]
    fn the_mean_of_nothing_is_nothing_and_the_mean_of_one_is_itself() {
        assert_eq!(Color::mean(core::iter::empty()), None);
        let lone = Color::rgba(3, 141, 59, 26);
        assert_eq!(Color::mean([lone]), Some(lone));
    }

    /// The colour is weighted by alpha; the alpha is a plain mean. This test
    /// used to assert the old rule -- `(100, 100, 40, 8)`, the transparent
    /// black counted as a third of the colour.
    #[test]
    fn the_mean_weighs_each_colour_by_how_much_of_it_shows() {
        let mean = Color::mean([
            Color::rgba(0, 0, 0, 0),
            Color::rgba(100, 200, 40, 8),
            Color::rgba(200, 100, 80, 16),
        ])
        .unwrap();
        // Colour: (100*8 + 200*16) / 24, and so on; alpha: 24 / 3.
        assert_eq!(mean, Color::rgba(166, 133, 66, 8));
    }

    /// The case the rule exists for: an opaque edge over transparency keeps
    /// its colour and loses only opacity.
    #[test]
    fn an_edge_over_transparency_keeps_its_colour() {
        let red = Color::rgba(200, 30, 40, 255);
        let mean = Color::mean([red, Color::TRANSPARENT]).unwrap();
        assert_eq!(mean, Color::rgba(200, 30, 40, 127));
        // Whatever colour the clear pixel happens to hold.
        let mean = Color::mean([red, Color::rgba(255, 255, 255, 0)]).unwrap();
        assert_eq!(mean, Color::rgba(200, 30, 40, 127));
    }

    /// For an opaque region the weighted rule is the plain per-channel mean,
    /// byte for byte -- so no opaque picture changes.
    #[test]
    fn an_opaque_region_averages_exactly_as_the_channels_do() {
        let pixels = [
            Color::rgba(0, 10, 255, 255),
            Color::rgba(13, 200, 7, 255),
            Color::rgba(254, 1, 99, 255),
        ];
        let plain = |f: fn(&Color) -> u8| {
            u8::try_from(pixels.iter().map(|c| u32::from(f(c))).sum::<u32>() / 3).unwrap()
        };
        assert_eq!(
            Color::mean(pixels).unwrap(),
            Color::rgba(plain(|c| c.r), plain(|c| c.g), plain(|c| c.b), 255)
        );
    }

    /// A region nothing in which shows is clear, whatever colours it hides.
    #[test]
    fn a_region_nothing_shows_in_is_clear() {
        let hidden = [Color::rgba(255, 0, 0, 0), Color::rgba(0, 255, 0, 0)];
        assert_eq!(Color::mean(hidden), Some(Color::TRANSPARENT));
    }

    /// The extremes cannot overflow the accumulator or the cast back down, and
    /// a uniform region must come back exactly as it went in — the property a
    /// box filter relies on to leave flat areas untouched.
    #[test]
    fn the_mean_of_many_identical_colours_is_that_colour() {
        for channel in [0u8, 1, 127, 254, 255] {
            let c = Color::rgba(channel, channel, channel, channel);
            let mean = Color::mean(core::iter::repeat_n(c, 1000)).unwrap();
            assert_eq!(mean, c, "channel {channel}");
        }
    }

    #[test]
    fn from_hex_reads_the_channels_in_the_order_css_writes_them() {
        assert_eq!(Color::from_hex(0x00_00_00), Color::BLACK);
        assert_eq!(Color::from_hex(0xFF_FF_FF), Color::WHITE);
        assert_eq!(
            Color::from_hex(0x12_34_56),
            Color::rgba(0x12, 0x34, 0x56, 255)
        );
        // Anything above the low 24 bits is not part of the colour.
        assert_eq!(
            Color::from_hex(0xAB_12_34_56),
            Color::rgba(0x12, 0x34, 0x56, 255)
        );
    }
}
