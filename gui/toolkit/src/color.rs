//! Color types for the GUI toolkit.

use core::num::NonZeroU32;

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
