//! The Adjustments tab's changes to the picture -- brightness, contrast,
//! gamma, saturation, hue and sharpness -- applied to each frame's pixels on
//! the decoding thread (`pictures`), after the frame is converted.
//!
//! A [`Grade`] is computed once per change of the adjustments, so that a
//! frame costs a table lookup and a small matrix a pixel, and a sharpening
//! pass where sharpness is asked for:
//!
//! 1. **Brightness, contrast and gamma** act on each channel alike, so they
//!    fold into one 256-entry table: contrast about the middle grey, then
//!    brightness added (-1 takes everything to black, +1 to white), then the
//!    gamma curve (above 1 lifts the middle tones, below 1 deepens them) --
//!    the order of mpv's and FFmpeg's `eq`.
//! 2. **Saturation and hue** turn the colour about the grey of the same
//!    lightness: one 3x3 matrix, the product of the two that SVG's
//!    `feColorMatrix` and CSS's `saturate()` and `hue-rotate()` define (on
//!    Rec. 709's weights), in fixed point.
//! 3. **Sharpness** is an unsharp mask: each pixel pushed away from the
//!    average of its 3x3 neighbourhood by the amount asked, the edges of the
//!    picture left as they are.
//!
//! Alpha is untouched. A grade with every adjustment at its neutral value is
//! no grade ([`Grade::new`] gives `None`), so an unadjusted film costs
//! nothing.

use crate::VideoAdjustments;
use videocodec::Frame;

/// Fixed-point scale of the colour matrix: 12 bits after the point.
const ONE: i32 = 1 << 12;

/// What a frame's pixels go through for one setting of the adjustments.
#[derive(Clone, Debug, PartialEq)]
pub struct Grade {
    /// Brightness, contrast and gamma: each 8-bit channel value's new value.
    table: [u8; 256],
    /// Saturation and hue, row by row, in [`ONE`]ths; `None` where both are
    /// neutral.
    matrix: Option<[[i32; 3]; 3]>,
    /// Sharpness: how far a pixel is pushed from its neighbourhood's
    /// average, in [`ONE`]ths; 0 for none.
    sharpen: i32,
}

/// Whether `value` is within a rounding error of `neutral`.
fn neutral(value: f32, neutral: f32) -> bool {
    (value - neutral).abs() < 1e-3
}

impl Grade {
    /// The grade for `adjust`, or `None` where every adjustment is neutral
    /// (or not a number, which the tab cannot set).
    #[must_use]
    pub fn new(adjust: &VideoAdjustments) -> Option<Self> {
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        let brightness = finite(adjust.brightness, 0.0).clamp(-1.0, 1.0);
        let contrast = finite(adjust.contrast, 1.0).clamp(0.0, 2.0);
        let saturation = finite(adjust.saturation, 1.0).clamp(0.0, 3.0);
        let hue = finite(adjust.hue, 0.0).clamp(-180.0, 180.0);
        let gamma = finite(adjust.gamma, 1.0).clamp(0.1, 3.0);
        let sharpness = finite(adjust.sharpness, 0.0).clamp(0.0, 2.0);
        let tonal = !(neutral(brightness, 0.0) && neutral(contrast, 1.0) && neutral(gamma, 1.0));
        let colour = !(neutral(saturation, 1.0) && neutral(hue, 0.0));
        let sharp = !neutral(sharpness, 0.0);
        if !(tonal || colour || sharp) {
            return None;
        }
        Some(Self {
            table: tone_table(brightness, contrast, gamma),
            matrix: colour.then(|| colour_matrix(saturation, hue)),
            sharpen: if sharp { to_fixed(sharpness) } else { 0 },
        })
    }

    /// Put `frame`'s pixels through the grade.
    pub fn apply(&self, frame: &mut Frame) {
        for px in &mut frame.pixels {
            *px = self.colour(*px);
        }
        if self.sharpen > 0 {
            sharpen(frame, self.sharpen);
        }
    }

    /// One `0xAARRGGBB` pixel through the table and the matrix.
    fn colour(&self, px: u32) -> u32 {
        let [a, r, g, b] = px.to_be_bytes();
        let t = |c: u8| self.table.get(usize::from(c)).copied().unwrap_or(c);
        let (r, g, b) = (t(r), t(g), t(b));
        let (r, g, b) = match &self.matrix {
            Some(m) => {
                let rgb = [i32::from(r), i32::from(g), i32::from(b)];
                let row = |row: &[i32; 3]| {
                    let sum = row.iter().zip(rgb).fold(ONE / 2, |acc, (k, c)| {
                        acc.saturating_add(k.saturating_mul(c))
                    });
                    clamp_channel(sum >> 12)
                };
                (row(&m[0]), row(&m[1]), row(&m[2]))
            }
            None => (r, g, b),
        };
        u32::from_be_bytes([a, r, g, b])
    }
}

/// `value` in [`ONE`]ths, rounded.
#[allow(
    clippy::cast_possible_truncation,
    reason = "every value converted is a small factor, far inside i32"
)]
fn to_fixed(value: f32) -> i32 {
    (value * ONE as f32).round() as i32
}

/// A channel value held to 0..=255.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255 first"
)]
fn clamp_channel(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// Brightness, contrast and gamma as one table over the 256 values a channel
/// takes.
fn tone_table(brightness: f32, contrast: f32, gamma: f32) -> [u8; 256] {
    let mut table = [0_u8; 256];
    for (i, out) in (0_u8..=255).zip(table.iter_mut()) {
        let v = f32::from(i) / 255.0;
        let v = ((v - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);
        let v = v.powf(1.0 / gamma);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "v is in 0..=1, so the product is in 0..=255"
        )]
        let byte = (v * 255.0).round().clamp(0.0, 255.0) as u8;
        *out = byte;
    }
    table
}

/// Saturation then hue as one matrix: SVG's `saturate` and `hueRotate`, on
/// Rec. 709's luma weights, multiplied (hue after saturation), in [`ONE`]ths.
fn colour_matrix(saturation: f32, hue_degrees: f32) -> [[i32; 3]; 3] {
    let s = saturation;
    let saturate = [
        [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
    ];
    let (sin, cos) = hue_degrees.to_radians().sin_cos();
    let rotate = [
        [
            0.213 + cos * 0.787 - sin * 0.213,
            0.715 - cos * 0.715 - sin * 0.715,
            0.072 - cos * 0.072 + sin * 0.928,
        ],
        [
            0.213 - cos * 0.213 + sin * 0.143,
            0.715 + cos * 0.285 + sin * 0.140,
            0.072 - cos * 0.072 - sin * 0.283,
        ],
        [
            0.213 - cos * 0.213 - sin * 0.787,
            0.715 - cos * 0.715 + sin * 0.715,
            0.072 + cos * 0.928 + sin * 0.072,
        ],
    ];
    let mut out = [[0_i32; 3]; 3];
    for (out_row, rot_row) in out.iter_mut().zip(rotate) {
        for (j, cell) in out_row.iter_mut().enumerate() {
            let sum: f32 = rot_row
                .iter()
                .zip(saturate)
                .map(|(r, sat_row)| r * sat_row.get(j).copied().unwrap_or(0.0))
                .sum();
            *cell = to_fixed(sum);
        }
    }
    out
}

/// Push each pixel of `frame` away from the average of its 3x3
/// neighbourhood by `amount` [`ONE`]ths of the difference; the picture's
/// outermost rows and columns, which have no whole neighbourhood, stay.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "x and y run over 1..w-1 and 1..h-1 with w and h at least 3, so every neighbour's index is inside the w*h pixels checked above; nine channel values sum to at most 2295, and 9c - sum times an amount of at most 2.0 in ONEths (8192) is under 2e7, far inside i32"
)]
fn sharpen(frame: &mut Frame, amount: i32) {
    let (Ok(w), Ok(h)) = (usize::try_from(frame.width), usize::try_from(frame.height)) else {
        return;
    };
    if w < 3 || h < 3 || frame.pixels.len() < w.saturating_mul(h) {
        return;
    }
    let source = frame.pixels.clone();
    let at = |x: usize, y: usize| source.get(y * w + x).copied().unwrap_or(0);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let centre = at(x, y);
            let mut sums = [0_i32; 3];
            for dy in 0..3 {
                for dx in 0..3 {
                    let [_, r, g, b] = at(x + dx - 1, y + dy - 1).to_be_bytes();
                    for (sum, c) in sums.iter_mut().zip([r, g, b]) {
                        *sum += i32::from(c);
                    }
                }
            }
            let [a, r, g, b] = centre.to_be_bytes();
            let push = |c: u8, sum: i32| {
                let c = i32::from(c);
                // Nine pixels' sum: the difference from their average is
                // (9c - sum) / 9.
                let lift = (9 * c - sum).saturating_mul(amount) / (9 * ONE);
                clamp_channel(c + lift)
            };
            let [sr, sg, sb] = sums;
            if let Some(px) = frame.pixels.get_mut(y * w + x) {
                *px = u32::from_be_bytes([a, push(r, sr), push(g, sg), push(b, sb)]);
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn adjust() -> VideoAdjustments {
        VideoAdjustments::default()
    }

    fn frame(width: u32, height: u32, pixels: Vec<u32>) -> Frame {
        Frame {
            time: 0,
            duration: 0,
            keyframe: true,
            width,
            height,
            pixels,
        }
    }

    fn graded(a: &VideoAdjustments, px: u32) -> [u8; 4] {
        let mut f = frame(1, 1, vec![px]);
        Grade::new(a).expect("a grade").apply(&mut f);
        f.pixels[0].to_be_bytes()
    }

    #[test]
    fn neutral_adjustments_are_no_grade() {
        assert_eq!(Grade::new(&adjust()), None);
        let mut a = adjust();
        a.hue = f32::NAN;
        assert_eq!(Grade::new(&a), None, "a NaN is read as neutral");
    }

    #[test]
    fn brightness_lifts_and_lowers_every_channel() {
        let mut a = adjust();
        a.brightness = 1.0;
        assert_eq!(
            graded(&a, 0xFF00_0000),
            [255, 255, 255, 255],
            "black to white"
        );
        a.brightness = -1.0;
        assert_eq!(
            graded(&a, 0x80FF_FFFF),
            [0x80, 0, 0, 0],
            "white to black, alpha kept"
        );
        a.brightness = 0.2;
        let [_, r, ..] = graded(&a, 0xFF64_0000);
        assert_eq!(r, 100 + 51, "a fifth of the range added");
    }

    #[test]
    fn contrast_turns_about_the_middle() {
        let mut a = adjust();
        a.contrast = 0.0;
        assert_eq!(
            graded(&a, 0xFFFF_0010),
            [255, 128, 128, 128],
            "every value to the middle"
        );
        // At one and a half, x becomes 1.5x - 63.75: 30, 128 and 200 go to
        // nothing, 128.25 and 236.25.
        a.contrast = 1.5;
        let [_, r, g, b] = graded(&a, 0xFF1E_80C8);
        assert_eq!((r, g, b), (0, 128, 236), "spread from the middle");
    }

    #[test]
    fn gamma_above_one_lifts_the_middle_and_keeps_the_ends() {
        let mut a = adjust();
        a.gamma = 2.0;
        let [_, r, g, b] = graded(&a, 0xFF00_80FF);
        assert_eq!((r, b), (0, 255), "the ends stay");
        assert!(g > 128, "the middle is lifted: {g}");
        a.gamma = 0.5;
        let [_, _, g, _] = graded(&a, 0xFF00_80FF);
        assert!(g < 128, "below one the middle deepens: {g}");
    }

    #[test]
    fn no_saturation_is_grey_and_a_half_turn_of_hue_is_the_complement() {
        let mut a = adjust();
        a.saturation = 0.0;
        let [_, r, g, b] = graded(&a, 0xFFFF_0000);
        assert!(r == g && g == b, "red without colour: {r} {g} {b}");
        assert!((50..=56).contains(&r), "red's lightness on Rec. 709: {r}");
        a.saturation = 1.0;
        a.hue = 180.0;
        let [_, r, g, b] = graded(&a, 0xFFFF_0000);
        assert!(
            r < 60 && g > 100 && b > 100,
            "red half-turned is cyan: {r} {g} {b}"
        );
        // Grey has no colour to turn.
        assert_eq!(graded(&a, 0xFF80_8080)[1..], [128, 128, 128]);
        // A third of a turn: red goes to green one way, to blue the other.
        a.hue = 120.0;
        let [_, r, g, b] = graded(&a, 0xFFFF_0000);
        assert!(
            g > 90 && r < 30 && b < 30,
            "red a third turned is green: {r} {g} {b}"
        );
        a.hue = -120.0;
        let [_, r, g, b] = graded(&a, 0xFFFF_0000);
        assert!(b > 200 && r < 30, "the other way it is blue: {r} {g} {b}");
    }

    #[test]
    fn sharpness_raises_an_edge_and_leaves_flat_colour_and_the_border() {
        let mut a = adjust();
        a.sharpness = 1.0;
        let grade = Grade::new(&a).unwrap();
        // Flat: nothing to sharpen.
        let mut flat = frame(4, 4, vec![0xFF60_6060; 16]);
        grade.apply(&mut flat);
        assert!(flat.pixels.iter().all(|&p| p == 0xFF60_6060));
        // A dark left half beside a light right half, 6 wide.
        let row = [0x40, 0x40, 0x40, 0xC0, 0xC0, 0xC0];
        let pixels: Vec<u32> = (0..18)
            .map(|i| {
                let v = row[i % 6];
                u32::from_be_bytes([0xFF, v, v, v])
            })
            .collect();
        let mut edge = frame(6, 3, pixels.clone());
        grade.apply(&mut edge);
        let at = |f: &Frame, x: usize, y: usize| f.pixels[y * 6 + x].to_be_bytes()[1];
        assert!(at(&edge, 2, 1) < 0x40, "the dark side of the edge darkens");
        assert!(at(&edge, 3, 1) > 0xC0, "the light side lightens");
        assert_eq!(at(&edge, 0, 1), 0x40, "the border stays");
        assert_eq!(edge.pixels[0], pixels[0], "the top row stays");
    }
}
