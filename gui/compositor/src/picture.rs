//! Live pictures of windows: what [`RenderCommand::WindowPicture`] draws --
//! the taskbar's preview, Aero Peek, the overview's cards, Alt-Tab
//! (`requests/c-f-let-the-shell-draw-a-live-picture-of-a-window.md`).
//!
//! A picture is the pictured window's client area, drawn as the window
//! draws it, scaled down to fit the command's rectangle with its proportions
//! kept and centred in it. Scaled *down* only: a window smaller than the
//! rectangle is drawn at its own size, centred, rather than blown up into a
//! blur.
//!
//! The scaling is an area average (a box filter over exactly the source
//! pixels each destination pixel covers, fractions included), in the
//! pictures' own encoding: what a high-quality downscale is, and what keeps
//! a window's text a grey line rather than the scattered dots a
//! nearest-neighbour sample of it would leave. The compositor's image path
//! samples nearest, which is right for an image drawn near its size and
//! wrong for a 1920-wide window drawn 200 wide, so a picture is made at its
//! final size here and drawn one pixel per pixel.
//!
//! Who draws what: [`fitted`] decides the size, [`downscale`] makes the
//! pixels, the compositor keeps them per viewer and pictured window until
//! the pictured window changes (`Compositor::thumbnails`), and a
//! [`PictureSet`] hands the ones a window's commands name to the render
//! engine for one draw.
//!
//! [`RenderCommand::WindowPicture`]: guitk::render::RenderCommand::WindowPicture

#![allow(
    clippy::arithmetic_side_effects,
    reason = "indices into a picture whose size was checked to fit a usize before any is formed, and float sums"
)]

use std::collections::HashMap;

use crate::ImageAsset;

/// The size, in whole pixels, a `src` (width, height) picture is drawn at in
/// a `rect` (width, height): as large as fits with its proportions kept, and
/// never larger than the source. `None` when nothing would show -- an empty
/// source, or a rectangle with no area (or not a finite one).
pub(crate) fn fitted(src: (u32, u32), rect: (f32, f32)) -> Option<(u32, u32)> {
    let (sw, sh) = src;
    let (rw, rh) = rect;
    if sw == 0 || sh == 0 || !rw.is_finite() || !rh.is_finite() || rw < 1.0 || rh < 1.0 {
        return None;
    }
    let scale = (f64::from(rw) / f64::from(sw))
        .min(f64::from(rh) / f64::from(sh))
        .min(1.0);
    let side = |s: u32| {
        // At most the source side (the scale is at most 1), so it fits a u32;
        // at least one pixel, so a very thin window still shows as a line.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a non-negative value no larger than the u32 it was scaled from"
        )]
        let px = (f64::from(s) * scale).round().max(1.0) as u32;
        px.min(s)
    };
    Some((side(sw), side(sh)))
}

/// `src` -- `sw` by `sh` packed `0xAARRGGBB` pixels, row-major -- scaled to
/// `dw` by `dh` by area averaging: each destination pixel the mean of the
/// source pixels under it, each weighted by how much of it lies under, all
/// four channels alike. `None` if the source is not `sw * sh` pixels, or
/// either size is empty, or the destination is larger than the source in
/// either direction ([`fitted`] never asks for that).
///
/// Separable, as an area average is: rows first, then columns, so the cost
/// is that of reading the source once and the narrower intermediate once.
pub(crate) fn downscale(src: &[u32], sw: u32, sh: u32, dw: u32, dh: u32) -> Option<Vec<u32>> {
    let (sw_us, sh_us) = (usize::try_from(sw).ok()?, usize::try_from(sh).ok()?);
    let (dw_us, dh_us) = (usize::try_from(dw).ok()?, usize::try_from(dh).ok()?);
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 || dw > sw || dh > sh {
        return None;
    }
    if src.len() != sw_us.checked_mul(sh_us)? {
        return None;
    }
    let spans_x = spans(sw, dw);
    let spans_y = spans(sh, dh);

    // Rows first: `sh` rows of `dw` four-channel sums.
    let mut rows = vec![[0f32; 4]; sh_us.checked_mul(dw_us)?];
    for (y, row) in src.chunks_exact(sw_us).enumerate() {
        let out = rows.get_mut(y * dw_us..(y + 1) * dw_us)?;
        for (cell, span) in out.iter_mut().zip(&spans_x) {
            *cell = span.mean(|x| row.get(x).copied().map(channels));
        }
    }

    // Then columns.
    let mut out = Vec::with_capacity(dw_us.checked_mul(dh_us)?);
    for span in &spans_y {
        for x in 0..dw_us {
            let mean = span.mean(|y| rows.get(y * dw_us + x).copied());
            out.push(pack(mean));
        }
    }
    Some(out)
}

/// `src` -- `sw` by `sh` packed `0xAARRGGBB` pixels -- scaled to `dw` by `dh`:
/// area averaging ([`downscale`]) when neither side grows, bilinear
/// interpolation ([`upscale`]) otherwise. For a cursor theme's picture drawn
/// at another size than the one asked for. Premultiplied pixels stay
/// premultiplied: both filters are linear in every channel alike.
pub(crate) fn resample(src: &[u32], sw: u32, sh: u32, dw: u32, dh: u32) -> Option<Vec<u32>> {
    if dw <= sw && dh <= sh {
        downscale(src, sw, sh, dw, dh)
    } else {
        upscale(src, sw, sh, dw, dh)
    }
}

/// `src` scaled to `dw` by `dh` by bilinear interpolation: each destination
/// pixel's centre mapped into the source, and the four source pixels around
/// it mixed by distance, the edges clamped. `None` if the source is not
/// `sw * sh` pixels or either size is empty.
///
/// For enlarging -- where an area average has nothing to average -- and
/// correct for shrinking too, only less smooth than [`downscale`].
pub(crate) fn upscale(src: &[u32], sw: u32, sh: u32, dw: u32, dh: u32) -> Option<Vec<u32>> {
    let (sw_us, sh_us) = (usize::try_from(sw).ok()?, usize::try_from(sh).ok()?);
    let (dw_us, dh_us) = (usize::try_from(dw).ok()?, usize::try_from(dh).ok()?);
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 || src.len() != sw_us.checked_mul(sh_us)? {
        return None;
    }
    // For an axis of `from` source pixels drawn as `to`, where destination
    // pixel `d` samples: the two source pixels and the second's weight.
    let taps = |from: usize, to: usize| -> Vec<(usize, usize, f32)> {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "pixel positions of a picture well under 2^24 pixels wide, clamped to the source first"
        )]
        (0..to)
            .map(|d| {
                let at = ((d as f64 + 0.5) * from as f64 / to as f64 - 0.5)
                    .clamp(0.0, (from - 1) as f64);
                let first = at.floor() as usize;
                let second = (first + 1).min(from - 1);
                (first, second, (at - first as f64) as f32)
            })
            .collect()
    };
    let (xs, ys) = (taps(sw_us, dw_us), taps(sh_us, dh_us));
    let sample = |x: usize, y: usize| src.get(y * sw_us + x).copied().map_or([0.0; 4], channels);
    let mut out = Vec::with_capacity(dw_us.checked_mul(dh_us)?);
    for &(y0, y1, fy) in &ys {
        for &(x0, x1, fx) in &xs {
            let (a, b, c, d) = (
                sample(x0, y0),
                sample(x1, y0),
                sample(x0, y1),
                sample(x1, y1),
            );
            let top = a.iter().zip(b).map(|(a, b)| a + (b - a) * fx);
            let bottom = c.iter().zip(d).map(|(c, d)| c + (d - c) * fx);
            let mut mixed = [0f32; 4];
            for (m, (top, bottom)) in mixed.iter_mut().zip(top.zip(bottom)) {
                *m = top + (bottom - top) * fy;
            }
            out.push(pack(mixed));
        }
    }
    Some(out)
}

/// The source pixels one destination pixel covers along one axis, with the
/// weight of each: whole pixels inside count 1, the partly covered ones at
/// either end their covered fraction.
#[derive(Debug)]
struct Span {
    /// The first source pixel touched.
    first: usize,
    /// The weight of each source pixel from `first` on.
    weights: Vec<f32>,
    /// The sum of `weights`: the span's width in source pixels.
    total: f32,
}

impl Span {
    /// The weighted mean of `sample` over the span; zero for a sample that
    /// is missing, which a span inside its source never meets.
    fn mean(&self, sample: impl Fn(usize) -> Option<[f32; 4]>) -> [f32; 4] {
        let mut sum = [0f32; 4];
        for (i, &w) in self.weights.iter().enumerate() {
            if let Some(px) = sample(self.first + i) {
                for (s, c) in sum.iter_mut().zip(px) {
                    *s += c * w;
                }
            }
        }
        sum.map(|s| s / self.total)
    }
}

/// For a `src`-pixel axis drawn as `dst` pixels, the span each destination
/// pixel covers.
fn spans(src: u32, dst: u32) -> Vec<Span> {
    let step = f64::from(src) / f64::from(dst);
    (0..dst)
        .map(|d| {
            let start = f64::from(d) * step;
            let end = (f64::from(d) + 1.0) * step;
            // Truncation is the floor: both are non-negative and at most `src`.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "non-negative positions no larger than the u32 source side"
            )]
            let (first, last) = (start.floor() as usize, end.ceil() as usize);
            let weights: Vec<f32> = (first..last)
                .map(|s| {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "a pixel index of a picture well under 2^24 pixels wide"
                    )]
                    let s = s as f64;
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "an overlap between 0 and 1"
                    )]
                    let w = ((s + 1.0).min(end) - s.max(start)).max(0.0) as f32;
                    w
                })
                .collect();
            let total = weights.iter().sum::<f32>().max(f32::MIN_POSITIVE);
            Span {
                first,
                weights,
                total,
            }
        })
        .collect()
}

/// A packed pixel's four channels, alpha first, as floats.
fn channels(px: u32) -> [f32; 4] {
    [24, 16, 8, 0].map(|shift| f32::from(((px >> shift) & 0xFF) as u8))
}

/// Four channels, alpha first, rounded back into a packed pixel.
fn pack(c: [f32; 4]) -> u32 {
    c.iter().fold(0u32, |acc, &v| {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to a byte first"
        )]
        let byte = v.round().clamp(0.0, 255.0) as u32;
        (acc << 8) | byte
    })
}

/// Which picture a [`PictureSet`] entry is: the pictured window, and the
/// rectangle's size exactly as the command gave it (its bits), so the
/// command and the entry made for it can only ever agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PictureKey {
    pub window: u64,
    pub width_bits: u32,
    pub height_bits: u32,
}

impl PictureKey {
    /// The key for a picture of `window` in a `width` by `height` rectangle.
    pub(crate) const fn new(window: u64, width: f32, height: f32) -> Self {
        Self {
            window,
            width_bits: width.to_bits(),
            height_bits: height.to_bits(),
        }
    }
}

/// The pictures one window's commands name, made and ready to draw: what the
/// render engine looks a [`RenderCommand::WindowPicture`] up in. A picture
/// with no entry -- a window gone, minimised or unknown -- draws nothing.
///
/// [`RenderCommand::WindowPicture`]: guitk::render::RenderCommand::WindowPicture
#[derive(Debug, Default)]
pub(crate) struct PictureSet<'a> {
    entries: HashMap<PictureKey, &'a ImageAsset>,
}

impl<'a> PictureSet<'a> {
    /// A set with no pictures: what every draw but a viewer's passes.
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    /// Add the picture for `key`; a second for the same key replaces the
    /// first, and is the same picture.
    pub(crate) fn insert(&mut self, key: PictureKey, image: &'a ImageAsset) {
        self.entries.insert(key, image);
    }

    /// The picture for `key`, if one was made.
    pub(crate) fn get(&self, key: PictureKey) -> Option<&'a ImageAsset> {
        self.entries.get(&key).copied()
    }
}

/// The most different pictures one window's commands may name -- other
/// windows, or one window at different sizes -- that are made; any after
/// them draw as nothing. A taskbar's previews, an overview of every desktop
/// and Alt-Tab name tens; the bound is for a drawing that names millions,
/// each a picture to make and keep.
pub(crate) const MAX_PICTURES_PER_WINDOW: usize = 64;

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[test]
    fn a_picture_fits_its_rectangle_with_its_proportions_kept() {
        // Wider than the rectangle's shape: bounded by the width.
        assert_eq!(fitted((400, 200), (100.0, 100.0)), Some((100, 50)));
        // Taller: bounded by the height.
        assert_eq!(fitted((200, 400), (100.0, 100.0)), Some((50, 100)));
        // Smaller than the rectangle: its own size, never blown up.
        assert_eq!(fitted((80, 60), (400.0, 300.0)), Some((80, 60)));
        // A very thin window still shows as a line.
        assert_eq!(fitted((4000, 2), (100.0, 100.0)), Some((100, 1)));
        // Nothing to show.
        assert_eq!(fitted((0, 10), (100.0, 100.0)), None);
        assert_eq!(fitted((10, 10), (0.5, 100.0)), None);
        assert_eq!(fitted((10, 10), (f32::NAN, 100.0)), None);
        assert_eq!(fitted((10, 10), (f32::INFINITY, 100.0)), None);
    }

    /// Each destination pixel is the mean of what it covers: a two-by-two
    /// checker of black and white becomes one grey pixel -- where a
    /// nearest-neighbour sample would keep one of the four and call the
    /// picture black or white.
    #[test]
    fn a_downscale_averages_what_each_pixel_covers() {
        let (b, w) = (0xFF00_0000, 0xFFFF_FFFF);
        let grey = downscale(&[b, w, w, b], 2, 2, 1, 1).unwrap();
        assert_eq!(grey, vec![0xFF80_8080]);
        // A fractional cover: three pixels into two, the middle one shared
        // half and half. Red, green, blue in a row become two pixels, each
        // two-thirds its own colour and one-third green.
        let rgb = downscale(&[0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF], 3, 1, 2, 1).unwrap();
        assert_eq!(rgb, vec![0xFFAA_5500, 0xFF00_55AA]);
        // The same size is the same picture.
        let same = [0xFF12_3456, 0x8000_00FF, 0x0000_0000, 0xFFFF_FFFF];
        assert_eq!(downscale(&same, 2, 2, 2, 2).unwrap(), same.to_vec());
    }

    #[test]
    fn a_downscale_refuses_what_it_cannot_do() {
        assert_eq!(downscale(&[0; 3], 2, 2, 1, 1), None, "short source");
        assert_eq!(
            downscale(&[0; 4], 2, 2, 3, 1),
            None,
            "larger than the source"
        );
        assert_eq!(downscale(&[0; 4], 2, 2, 0, 1), None, "empty");
    }

    /// Enlarging keeps a flat picture flat, keeps the corners where they
    /// were, and blends between neighbours rather than repeating them.
    #[test]
    fn an_upscale_blends_between_neighbours() {
        let flat = vec![0x8040_2010; 4];
        assert_eq!(upscale(&flat, 2, 2, 5, 3).unwrap(), vec![0x8040_2010; 15]);

        // Black on the left, white on the right, opaque.
        let src = [0xFF00_0000, 0xFFFF_FFFF];
        let out = upscale(&src, 2, 1, 4, 1).unwrap();
        assert_eq!(out[0], 0xFF00_0000, "the left edge stays black");
        assert_eq!(out[3], 0xFFFF_FFFF, "the right edge stays white");
        let grey = |px: u32| px & 0xFF;
        assert!(grey(out[1]) > 0 && grey(out[1]) < grey(out[2]) && grey(out[2]) < 255);
    }

    #[test]
    fn a_resample_shrinks_by_averaging_and_grows_by_blending() {
        let src = [0xFF00_0000, 0xFFFF_FFFF, 0xFF00_0000, 0xFFFF_FFFF];
        // Halved: the mean of the two.
        assert_eq!(
            resample(&src, 4, 1, 2, 1).unwrap(),
            downscale(&src, 4, 1, 2, 1).unwrap()
        );
        // Doubled: blended.
        assert_eq!(
            resample(&src, 4, 1, 8, 1).unwrap(),
            upscale(&src, 4, 1, 8, 1).unwrap()
        );
        assert_eq!(upscale(&src, 3, 1, 6, 1), None, "not width times height");
        assert_eq!(upscale(&src, 4, 1, 0, 1), None, "empty");
    }
}
