//! The pixel operations SVG's filter primitives are made of -- blurs,
//! offsets, floods, the compositing and blending operators, colour matrices
//! and transfer functions, convolution, morphology, displacement, tiling,
//! lighting and turbulence -- each as Filter Effects Module Level 1 defines
//! it.
//!
//! Nothing here knows about documents, units or transforms: every operation
//! takes images, and numbers already measured in the images' pixels, and the
//! `filter` module turns a `<filter>`'s primitives into calls of these. Every
//! operation answers a whole image the size of its input, transparent outside
//! the [`Area`] it was asked to fill -- a primitive's subregion -- which is
//! how a result is cut to its subregion.
//!
//! # Colour
//!
//! An [`Image`] holds premultiplied RGBA, each channel in `0.0..=1.0`, in
//! whichever colour space the primitive works in: sRGB, or linear-light RGB,
//! which is `color-interpolation-filters`' initial value ([`convert`]). The
//! operations the specification defines on colour rather than on
//! premultiplied colour -- the colour matrix, the transfer functions, a
//! displacement map's channels, a convolution that preserves alpha -- divide
//! the alpha out, work, and multiply it back.
//!
//! Pixels outside an image are transparent black wherever an operation reads
//! past its edge, except where the operation says otherwise (a convolution's
//! `edgeMode`).

use std::collections::VecDeque;

// ─── Areas and images ───────────────────────────────────────────────────────

/// A rectangle of an image's pixels: columns `x0..x1`, rows `y0..y1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Area {
    pub(super) x0: usize,
    pub(super) y0: usize,
    pub(super) x1: usize,
    pub(super) y1: usize,
}

impl Area {
    /// No pixels at all.
    pub(super) const EMPTY: Self = Self {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };

    /// Every pixel of an image `width` by `height`.
    pub(super) const fn whole(width: usize, height: usize) -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: width,
            y1: height,
        }
    }

    /// Whether it holds no pixel.
    pub(super) const fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    /// Whether the pixel `(x, y)` is in it.
    pub(super) const fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    /// The pixels both hold.
    pub(super) fn intersect(self, other: Self) -> Self {
        let both = Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        };
        if both.is_empty() { Self::EMPTY } else { both }
    }

    /// How many columns it spans.
    pub(super) const fn width(&self) -> usize {
        self.x1.saturating_sub(self.x0)
    }

    /// How many rows it spans.
    pub(super) const fn height(&self) -> usize {
        self.y1.saturating_sub(self.y0)
    }

    /// How many pixels it holds.
    pub(super) const fn pixels(&self) -> usize {
        self.width().saturating_mul(self.height())
    }
}

/// One pixel: premultiplied red, green, blue and alpha, each 0 to 1.
pub(super) type Pixel = [f32; 4];

/// Transparent black.
const CLEAR: Pixel = [0.0; 4];

/// An image a filter works on: premultiplied RGBA, row by row.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Image {
    width: usize,
    height: usize,
    pixels: Vec<Pixel>,
}

impl Image {
    /// Transparent black, `width` by `height` -- or no pixels at all where
    /// that many could not be counted.
    pub(super) fn transparent(width: usize, height: usize) -> Self {
        match width.checked_mul(height) {
            Some(count) => Self {
                width,
                height,
                pixels: vec![CLEAR; count],
            },
            None => Self {
                width: 0,
                height: 0,
                pixels: Vec::new(),
            },
        }
    }

    /// Its width in pixels.
    pub(super) const fn width(&self) -> usize {
        self.width
    }

    /// Its height in pixels.
    pub(super) const fn height(&self) -> usize {
        self.height
    }

    /// All of it, as an area.
    pub(super) const fn area(&self) -> Area {
        Area::whole(self.width, self.height)
    }

    /// Where `(x, y)` is among the pixels, if it is in the image.
    fn index(&self, x: usize, y: usize) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        y.checked_mul(self.width)?.checked_add(x)
    }

    /// The pixel at `(x, y)`; transparent outside the image.
    pub(super) fn pixel(&self, x: usize, y: usize) -> Pixel {
        self.index(x, y)
            .and_then(|i| self.pixels.get(i))
            .copied()
            .unwrap_or(CLEAR)
    }

    /// The pixel at `(x, y)`, which may be outside the image -- transparent
    /// there.
    pub(super) fn at(&self, x: isize, y: isize) -> Pixel {
        match (usize::try_from(x), usize::try_from(y)) {
            (Ok(x), Ok(y)) => self.pixel(x, y),
            _ => CLEAR,
        }
    }

    /// Set the pixel at `(x, y)`; nothing outside the image.
    pub(super) fn set(&mut self, x: usize, y: usize, value: Pixel) {
        if let Some(cell) = self.index(x, y).and_then(|i| self.pixels.get_mut(i)) {
            *cell = value;
        }
    }

    /// Row `y`; empty outside the image.
    pub(super) fn row(&self, y: usize) -> &[Pixel] {
        let start = y.checked_mul(self.width);
        let end = start.and_then(|s| s.checked_add(self.width));
        match (start, end) {
            (Some(start), Some(end)) if y < self.height => {
                self.pixels.get(start..end).unwrap_or(&[])
            }
            _ => &[],
        }
    }

    /// Row `y`, to change; empty outside the image.
    fn row_mut(&mut self, y: usize) -> &mut [Pixel] {
        let start = y.checked_mul(self.width);
        let end = start.and_then(|s| s.checked_add(self.width));
        match (start, end) {
            (Some(start), Some(end)) if y < self.height => {
                self.pixels.get_mut(start..end).unwrap_or(&mut [])
            }
            _ => &mut [],
        }
    }

    /// Read straight-alpha bytes `[r, g, b, a]`, row by row, premultiplying.
    /// Short input leaves the rest transparent.
    pub(super) fn from_rgba8(width: usize, height: usize, bytes: &[u8]) -> Self {
        let mut image = Self::transparent(width, height);
        for (cell, px) in image.pixels.iter_mut().zip(bytes.chunks_exact(4)) {
            if let [r, g, b, a] = *px {
                let alpha = f32::from(a) / 255.0;
                *cell = [
                    f32::from(r) / 255.0 * alpha,
                    f32::from(g) / 255.0 * alpha,
                    f32::from(b) / 255.0 * alpha,
                    alpha,
                ];
            }
        }
        image
    }

    /// Write as straight-alpha bytes `[r, g, b, a]`, row by row, dividing the
    /// alpha out and rounding; as many pixels as `out` has room for.
    pub(super) fn write_rgba8(&self, out: &mut [u8]) {
        for (px, cell) in out.chunks_exact_mut(4).zip(&self.pixels) {
            let [r, g, b, a] = *cell;
            let a = a.clamp(0.0, 1.0);
            let straight = |c: f32| {
                if a > 0.0 {
                    (c / a).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            };
            px.copy_from_slice(&[
                byte(straight(r)),
                byte(straight(g)),
                byte(straight(b)),
                byte(a),
            ]);
        }
    }

    /// The same image with everything outside `area` transparent.
    pub(super) fn cropped(mut self, area: Area) -> Self {
        if area == self.area() {
            return self;
        }
        for y in 0..self.height {
            let row = self.row_mut(y);
            for (x, cell) in row.iter_mut().enumerate() {
                if !area.contains(x, y) {
                    *cell = CLEAR;
                }
            }
        }
        self
    }
}

/// A share, 0 to 1, as a byte, rounded.
fn byte(share: f32) -> u8 {
    // In 0..=255 by the clamp, rounded first.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "held to 0..=255 and rounded first"
    )]
    let value = (share.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    value
}

/// `n` as an `isize`, held to its range.
fn signed(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

// ─── Colour spaces ──────────────────────────────────────────────────────────

/// The colour space a primitive works in: `color-interpolation-filters`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Space {
    /// sRGB, as colours are written.
    Srgb,
    /// Linear-light RGB: the initial value, and what a blur or a blend is
    /// physically right in.
    LinearRgb,
}

/// An sRGB channel, 0 to 1, in linear light.
pub(super) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A linear-light channel, 0 to 1, in sRGB.
pub(super) fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// `pixel` with `f` applied to its colour channels, the alpha divided out
/// first and multiplied back after: each channel held to 0 to 1.
fn map_color(pixel: Pixel, f: impl Fn(f32) -> f32) -> Pixel {
    let [r, g, b, a] = pixel;
    if a <= 0.0 {
        return CLEAR;
    }
    [
        f((r / a).clamp(0.0, 1.0)).clamp(0.0, 1.0) * a,
        f((g / a).clamp(0.0, 1.0)).clamp(0.0, 1.0) * a,
        f((b / a).clamp(0.0, 1.0)).clamp(0.0, 1.0) * a,
        a,
    ]
}

/// `image`, whose colour is in `from`, with its colour in `to`.
pub(super) fn convert(mut image: Image, from: Space, to: Space) -> Image {
    let f = match (from, to) {
        (Space::Srgb, Space::LinearRgb) => srgb_to_linear,
        (Space::LinearRgb, Space::Srgb) => linear_to_srgb,
        _ => return image,
    };
    for cell in &mut image.pixels {
        *cell = map_color(*cell, f);
    }
    image
}

/// A straight-alpha sRGB colour `[r, g, b, a]` -- as `flood-color` and
/// `lighting-color` are written -- premultiplied, in `space`.
pub(super) fn color_in(rgba: [f32; 4], space: Space) -> Pixel {
    let [r, g, b, a] = rgba.map(|c| c.clamp(0.0, 1.0));
    let channel = |c: f32| match space {
        Space::Srgb => c,
        Space::LinearRgb => srgb_to_linear(c),
    };
    [channel(r) * a, channel(g) * a, channel(b) * a, a]
}

// ─── Flood, offset, tile, merge ─────────────────────────────────────────────

/// `area` filled with `color`, premultiplied: `feFlood`.
pub(super) fn flood(width: usize, height: usize, color: Pixel, area: Area) -> Image {
    let mut out = Image::transparent(width, height);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            out.set(x, y, color);
        }
    }
    out
}

/// `input` moved `dx` pixels right and `dy` down: `feOffset`.
pub(super) fn offset(input: &Image, dx: isize, dy: isize, area: Area) -> Image {
    let mut out = Image::transparent(input.width, input.height);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let from = input.at(signed(x).saturating_sub(dx), signed(y).saturating_sub(dy));
            out.set(x, y, from);
        }
    }
    out
}

/// `source` -- the part of `input` its subregion holds -- repeated across
/// `area`: `feTile`.
pub(super) fn tile(input: &Image, source: Area, area: Area) -> Image {
    let mut out = Image::transparent(input.width, input.height);
    let (tw, th) = (signed(source.width()), signed(source.height()));
    if tw == 0 || th == 0 {
        return out;
    }
    for y in area.y0..area.y1 {
        let ty = signed(y)
            .saturating_sub(signed(source.y0))
            .rem_euclid(th)
            .saturating_add(signed(source.y0));
        for x in area.x0..area.x1 {
            let tx = signed(x)
                .saturating_sub(signed(source.x0))
                .rem_euclid(tw)
                .saturating_add(signed(source.x0));
            out.set(x, y, input.at(tx, ty));
        }
    }
    out
}

/// `layers` laid one over another, the first at the bottom: `feMerge`.
pub(super) fn merge(width: usize, height: usize, layers: &[&Image], area: Area) -> Image {
    let mut out = Image::transparent(width, height);
    for layer in layers {
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let below = out.pixel(x, y);
                out.set(x, y, over(layer.pixel(x, y), below));
            }
        }
    }
    out
}

/// `top` over `bottom`, premultiplied.
fn over(top: Pixel, bottom: Pixel) -> Pixel {
    let rest = 1.0 - top[3];
    [
        top[0] + bottom[0] * rest,
        top[1] + bottom[1] * rest,
        top[2] + bottom[2] * rest,
        top[3] + bottom[3] * rest,
    ]
}

// ─── Gaussian blur ──────────────────────────────────────────────────────────

/// `input` blurred by a Gaussian of standard deviation `sx` pixels across
/// and `sy` down: `feGaussianBlur`. A deviation of nought or less blurs
/// nothing along its axis.
///
/// Small deviations, under 2, are blurred with the Gaussian itself, three
/// deviations either side; larger ones with the three box blurs the
/// specification gives, which come within 3% of it and cost the same at any
/// deviation.
pub(super) fn blur(input: &Image, sx: f32, sy: f32, area: Area) -> Image {
    let mut work = input.clone();
    let mut line = Vec::new();
    if sx > 0.0 && sx.is_finite() {
        for y in 0..work.height {
            line.clear();
            line.extend_from_slice(work.row(y));
            let blurred = blur_line(&line, sx);
            put_row(&mut work, y, &blurred);
        }
    }
    if sy > 0.0 && sy.is_finite() {
        for x in 0..work.width {
            line.clear();
            line.extend((0..work.height).map(|y| work.pixel(x, y)));
            let blurred = blur_line(&line, sy);
            for (y, value) in blurred.into_iter().enumerate() {
                work.set(x, y, value);
            }
        }
    }
    work.cropped(area)
}

/// Row `y` of `image` made `values`, where it is as long as the row.
fn put_row(image: &mut Image, y: usize, values: &[Pixel]) {
    let row = image.row_mut(y);
    if row.len() == values.len() {
        row.copy_from_slice(values);
    }
}

/// One line blurred by a Gaussian of deviation `sigma`, transparent past its
/// ends.
fn blur_line(line: &[Pixel], sigma: f32) -> Vec<Pixel> {
    if sigma < 2.0 {
        return gaussian_line(line, sigma);
    }
    // The specification's box size, d = floor(s * 3 * sqrt(2 pi) / 4 + 0.5):
    // three boxes of d for an odd d; for an even one, two of d a half-pixel
    // either side and one of d + 1 centred.
    let size = (sigma * 3.0 * (2.0 * core::f32::consts::PI).sqrt() / 4.0 + 0.5).floor();
    // A deviation past the line's length blurs it into nothing but its mean
    // anyway; held there so the box fits in a usize.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "at least 1 here, and held to the line's length"
    )]
    let d = (size.min(line.len().saturating_mul(2) as f32).max(1.0)) as usize;
    let half = d >> 1;
    if d & 1 == 1 {
        let once = box_line(line, half, half);
        let twice = box_line(&once, half, half);
        box_line(&twice, half, half)
    } else {
        let once = box_line(line, half, half.saturating_sub(1));
        let twice = box_line(&once, half.saturating_sub(1), half);
        box_line(&twice, half, half)
    }
}

/// The mean over a window `left` pixels before each pixel and `right` after
/// it, the window's whole width the divisor: transparent past the ends
/// counts as nought.
fn box_line(line: &[Pixel], left: usize, right: usize) -> Vec<Pixel> {
    // Running sums, in f64 so a long line does not drift.
    let mut prefix: Vec<[f64; 4]> = Vec::with_capacity(line.len().saturating_add(1));
    let mut sum = [0.0f64; 4];
    prefix.push(sum);
    for px in line {
        for (s, c) in sum.iter_mut().zip(px) {
            *s += f64::from(*c);
        }
        prefix.push(sum);
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "a window's width, far inside f64's exact range"
    )]
    let size = left.saturating_add(right).saturating_add(1) as f64;
    let n = line.len();
    (0..n)
        .map(|x| {
            let start = x.saturating_sub(left);
            let end = x.saturating_add(right).saturating_add(1).min(n);
            let hi = prefix.get(end).copied().unwrap_or(sum);
            let lo = prefix.get(start).copied().unwrap_or([0.0; 4]);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a mean of channels in 0..=1, back to f32"
            )]
            let mean = |c: usize| {
                ((hi.get(c).copied().unwrap_or(0.0) - lo.get(c).copied().unwrap_or(0.0)) / size)
                    as f32
            };
            [mean(0), mean(1), mean(2), mean(3)]
        })
        .collect()
}

/// One line convolved with a Gaussian of deviation `sigma`, sampled three
/// deviations either side and normalised.
fn gaussian_line(line: &[Pixel], sigma: f32) -> Vec<Pixel> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "three deviations of one under 2: at most 6"
    )]
    let radius = (sigma * 3.0).ceil().max(1.0) as usize;
    let weights: Vec<f32> = (0..=radius.saturating_mul(2))
        .map(|k| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "an offset of at most a dozen pixels"
            )]
            let d = k as f32 - radius as f32;
            (-(d * d) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let total: f32 = weights.iter().sum();
    let n = signed(line.len());
    (0..n)
        .map(|x| {
            let mut acc = [0.0f32; 4];
            for (k, w) in weights.iter().enumerate() {
                let at = x.saturating_add(signed(k)).saturating_sub(signed(radius));
                if at < 0 || at >= n {
                    continue;
                }
                let px = usize::try_from(at)
                    .ok()
                    .and_then(|i| line.get(i))
                    .copied()
                    .unwrap_or(CLEAR);
                for (a, c) in acc.iter_mut().zip(px) {
                    *a += c * w;
                }
            }
            acc.map(|c| c / total)
        })
        .collect()
}

// ─── Morphology ─────────────────────────────────────────────────────────────

/// `input` thinned (`dilate` false: each channel the least in a window) or
/// fattened (the most), the window `rx` pixels either side across and `ry`
/// down: `feMorphology`. Past the image's edge is transparent, so a window
/// reaching past it erodes to nothing.
pub(super) fn morphology(input: &Image, rx: usize, ry: usize, dilate: bool, area: Area) -> Image {
    let mut work = input.clone();
    let mut line = Vec::new();
    for y in 0..work.height {
        line.clear();
        line.extend_from_slice(work.row(y));
        let out = extreme_line(&line, rx, dilate);
        put_row(&mut work, y, &out);
    }
    for x in 0..work.width {
        line.clear();
        line.extend((0..work.height).map(|y| work.pixel(x, y)));
        let out = extreme_line(&line, ry, dilate);
        for (y, value) in out.into_iter().enumerate() {
            work.set(x, y, value);
        }
    }
    work.cropped(area)
}

/// Each channel's least (or most) over a window `radius` either side of each
/// pixel -- transparent past the ends -- found with a monotonic queue, so the
/// cost does not grow with the radius.
fn extreme_line(line: &[Pixel], radius: usize, most: bool) -> Vec<Pixel> {
    let n = line.len();
    let mut out = vec![CLEAR; n];
    for channel in 0..4 {
        let value = |i: usize| {
            line.get(i)
                .and_then(|p| p.get(channel))
                .copied()
                .unwrap_or(0.0)
        };
        let better = |a: f32, b: f32| if most { a >= b } else { a <= b };
        let mut queue: VecDeque<usize> = VecDeque::new();
        // The window of pixel x is x - radius ..= x + radius; `next` is the
        // first index not yet in the queue.
        let mut next = 0usize;
        for (x, cell) in out.iter_mut().enumerate() {
            let last = x.saturating_add(radius).min(n.saturating_sub(1));
            while next <= last && next < n {
                let v = value(next);
                while queue.back().is_some_and(|&b| better(v, value(b))) {
                    queue.pop_back();
                }
                queue.push_back(next);
                next = next.saturating_add(1);
            }
            let first = x.saturating_sub(radius);
            while queue.front().is_some_and(|&f| f < first) {
                queue.pop_front();
            }
            let reaches_past = x < radius || x.saturating_add(radius) >= n;
            let found = queue.front().map_or(0.0, |&f| value(f));
            // Past the ends is transparent: nought, which only the least can
            // find.
            let result = if reaches_past && !most { 0.0 } else { found };
            if let Some(slot) = cell.get_mut(channel) {
                *slot = result;
            }
        }
    }
    out
}

// ─── Compositing and blending ───────────────────────────────────────────────

/// An `feComposite` operator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CompositeOp {
    Over,
    In,
    Out,
    Atop,
    Xor,
    /// Filter Effects 2's `lighter`: the sum, held to one.
    Lighter,
    /// `k1 * a * b + k2 * a + k3 * b + k4`, per channel.
    Arithmetic([f32; 4]),
}

/// `a` (`in`) combined with `b` (`in2`) by `op`, on premultiplied colour:
/// `feComposite`.
pub(super) fn composite(a: &Image, b: &Image, op: CompositeOp, area: Area) -> Image {
    let mut out = Image::transparent(a.width, a.height);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let (s, d) = (a.pixel(x, y), b.pixel(x, y));
            let (sa, da) = (s[3], d[3]);
            let mix = |fs: f32, fd: f32| -> Pixel {
                [
                    s[0] * fs + d[0] * fd,
                    s[1] * fs + d[1] * fd,
                    s[2] * fs + d[2] * fd,
                    sa * fs + da * fd,
                ]
            };
            let px = match op {
                CompositeOp::Over => mix(1.0, 1.0 - sa),
                CompositeOp::In => mix(da, 0.0),
                CompositeOp::Out => mix(1.0 - da, 0.0),
                CompositeOp::Atop => mix(da, 1.0 - sa),
                CompositeOp::Xor => mix(1.0 - da, 1.0 - sa),
                CompositeOp::Lighter => valid(mix(1.0, 1.0)),
                CompositeOp::Arithmetic([k1, k2, k3, k4]) => {
                    let ch = |i: usize| {
                        let (sc, dc) = (
                            s.get(i).copied().unwrap_or(0.0),
                            d.get(i).copied().unwrap_or(0.0),
                        );
                        k1 * sc * dc + k2 * sc + k3 * dc + k4
                    };
                    valid([ch(0), ch(1), ch(2), ch(3)])
                }
            };
            out.set(x, y, px);
        }
    }
    out
}

/// A premultiplied pixel made valid: alpha held to 0 to 1, and each colour
/// channel to 0 to the alpha.
fn valid(px: Pixel) -> Pixel {
    let a = px[3].clamp(0.0, 1.0);
    [
        px[0].clamp(0.0, a),
        px[1].clamp(0.0, a),
        px[2].clamp(0.0, a),
        a,
    ]
}

/// An `feBlend` mode: Compositing and Blending Level 1's blend modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// The mode a `mode` attribute names, if it names one.
    pub(super) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "normal" => Self::Normal,
            "multiply" => Self::Multiply,
            "screen" => Self::Screen,
            "overlay" => Self::Overlay,
            "darken" => Self::Darken,
            "lighten" => Self::Lighten,
            "color-dodge" => Self::ColorDodge,
            "color-burn" => Self::ColorBurn,
            "hard-light" => Self::HardLight,
            "soft-light" => Self::SoftLight,
            "difference" => Self::Difference,
            "exclusion" => Self::Exclusion,
            "hue" => Self::Hue,
            "saturation" => Self::Saturation,
            "color" => Self::Color,
            "luminosity" => Self::Luminosity,
            _ => return None,
        })
    }
}

/// `source` (`in`) blended over `backdrop` (`in2`) by `mode`: `feBlend`.
///
/// `co = cs (1 - ab) + cb (1 - as) + as ab B(Cb, Cs)`, with `cs`, `cb`
/// premultiplied and `Cs`, `Cb` not, and the alpha `as + ab (1 - as)` --
/// source-over with the mode's mix where the two overlap.
pub(super) fn blend(source: &Image, backdrop: &Image, mode: BlendMode, area: Area) -> Image {
    let mut out = Image::transparent(source.width, source.height);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let (s, b) = (source.pixel(x, y), backdrop.pixel(x, y));
            out.set(x, y, blend_pixel(s, b, mode));
        }
    }
    out
}

/// One pixel of [`blend`].
fn blend_pixel(s: Pixel, b: Pixel, mode: BlendMode) -> Pixel {
    let (sa, ba) = (s[3], b[3]);
    let straight = |p: Pixel| -> [f32; 3] {
        if p[3] > 0.0 {
            [p[0] / p[3], p[1] / p[3], p[2] / p[3]].map(|c| c.clamp(0.0, 1.0))
        } else {
            [0.0; 3]
        }
    };
    let (cs, cb) = (straight(s), straight(b));
    let mixed = blended(cb, cs, mode);
    let alpha = sa + ba * (1.0 - sa);
    let channel = |i: usize| {
        let si = s.get(i).copied().unwrap_or(0.0);
        let bi = b.get(i).copied().unwrap_or(0.0);
        let mi = mixed.get(i).copied().unwrap_or(0.0);
        si * (1.0 - ba) + bi * (1.0 - sa) + sa * ba * mi
    };
    valid([channel(0), channel(1), channel(2), alpha])
}

/// `B(Cb, Cs)`: the colour mode `mode` makes of backdrop `cb` and source
/// `cs`, both straight.
fn blended(cb: [f32; 3], cs: [f32; 3], mode: BlendMode) -> [f32; 3] {
    let each = |f: fn(f32, f32) -> f32| [f(cb[0], cs[0]), f(cb[1], cs[1]), f(cb[2], cs[2])];
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => each(|b, s| b * s),
        BlendMode::Screen => each(screen),
        BlendMode::Overlay => each(|b, s| hard_light(s, b)),
        BlendMode::Darken => each(f32::min),
        BlendMode::Lighten => each(f32::max),
        BlendMode::ColorDodge => each(|b, s| {
            if b <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }),
        BlendMode::ColorBurn => each(|b, s| {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }),
        BlendMode::HardLight => each(hard_light),
        BlendMode::SoftLight => each(|b, s| {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }),
        BlendMode::Difference => each(|b, s| (b - s).abs()),
        BlendMode::Exclusion => each(|b, s| b + s - 2.0 * b * s),
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
    }
}

fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

/// `hard-light`'s `B(Cb, Cs)`.
fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b * 2.0 * s
    } else {
        screen(b, 2.0 * s - 1.0)
    }
}

/// The blend modes' luminosity of a colour.
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// `c` with its luminosity made `l`, its colour brought back inside the cube.
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    let c = c.map(|x| x + d);
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 && l - n > 0.0 {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 && x - l > 0.0 {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    c.map(|v| v.clamp(0.0, 1.0))
}

/// The blend modes' saturation of a colour.
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// `c` with its saturation made `s`.
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max <= min {
        return [0.0; 3];
    }
    c.map(|v| (v - min) * s / (max - min))
}

// ─── Colour matrices and transfer functions ─────────────────────────────────

/// The identity colour matrix.
pub(super) const IDENTITY_MATRIX: [f32; 20] = [
    1.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, 0.0,
];

/// `feColorMatrix type="saturate"`'s matrix for `s`.
pub(super) fn saturate_matrix(s: f32) -> [f32; 20] {
    [
        0.213 + 0.787 * s,
        0.715 - 0.715 * s,
        0.072 - 0.072 * s,
        0.0,
        0.0,
        0.213 - 0.213 * s,
        0.715 + 0.285 * s,
        0.072 - 0.072 * s,
        0.0,
        0.0,
        0.213 - 0.213 * s,
        0.715 - 0.715 * s,
        0.072 + 0.928 * s,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

/// `feColorMatrix type="hueRotate"`'s matrix for `degrees`.
pub(super) fn hue_rotate_matrix(degrees: f32) -> [f32; 20] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [
        0.213 + cos * 0.787 - sin * 0.213,
        0.715 - cos * 0.715 - sin * 0.715,
        0.072 - cos * 0.072 + sin * 0.928,
        0.0,
        0.0,
        0.213 - cos * 0.213 + sin * 0.143,
        0.715 + cos * 0.285 + sin * 0.140,
        0.072 - cos * 0.072 - sin * 0.283,
        0.0,
        0.0,
        0.213 - cos * 0.213 - sin * 0.787,
        0.715 - cos * 0.715 + sin * 0.715,
        0.072 + cos * 0.928 + sin * 0.072,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
    ]
}

/// `feColorMatrix type="luminanceToAlpha"`'s matrix.
pub(super) const LUMINANCE_TO_ALPHA: [f32; 20] = [
    0.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 0.0, 0.0, //
    0.2125, 0.7154, 0.0721, 0.0, 0.0,
];

/// `input` with each pixel's straight colour and alpha multiplied by
/// `matrix`, four rows of five: `feColorMatrix`.
pub(super) fn color_matrix(input: &Image, matrix: &[f32; 20], area: Area) -> Image {
    let mut out = Image::transparent(input.width, input.height);
    let m = |i: usize| matrix.get(i).copied().unwrap_or(0.0);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let [r, g, b, a] = input.pixel(x, y);
            let (r, g, b) = if a > 0.0 {
                (r / a, g / a, b / a)
            } else {
                (0.0, 0.0, 0.0)
            };
            let row = |k: usize| {
                let base = k.saturating_mul(5);
                (m(base) * r
                    + m(base.saturating_add(1)) * g
                    + m(base.saturating_add(2)) * b
                    + m(base.saturating_add(3)) * a
                    + m(base.saturating_add(4)))
                .clamp(0.0, 1.0)
            };
            let alpha = row(3);
            out.set(
                x,
                y,
                [row(0) * alpha, row(1) * alpha, row(2) * alpha, alpha],
            );
        }
    }
    out
}

/// One channel's `feFuncR`/`G`/`B`/`A`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Transfer {
    Identity,
    Table(Vec<f32>),
    Discrete(Vec<f32>),
    Linear {
        slope: f32,
        intercept: f32,
    },
    Gamma {
        amplitude: f32,
        exponent: f32,
        offset: f32,
    },
}

impl Transfer {
    /// What it makes of a channel `c`, 0 to 1.
    pub(super) fn apply(&self, c: f32) -> f32 {
        let c = c.clamp(0.0, 1.0);
        let out = match self {
            Self::Identity => c,
            Self::Table(values) => match values.len() {
                0 => c,
                1 => values.first().copied().unwrap_or(c),
                len => {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "a table's length, far inside f32's exact range"
                    )]
                    let n = (len.saturating_sub(1)) as f32;
                    let at = c * n;
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "c is in 0..=1, so at is in 0..=n"
                    )]
                    let k = (at.floor() as usize).min(len.saturating_sub(2));
                    let v0 = values.get(k).copied().unwrap_or(0.0);
                    let v1 = values.get(k.saturating_add(1)).copied().unwrap_or(v0);
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "an index into a table, far inside f32's exact range"
                    )]
                    let t = at - k as f32;
                    v0 + t * (v1 - v0)
                }
            },
            Self::Discrete(values) => match values.len() {
                0 => c,
                len => {
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "a table's length, far inside f32's exact range"
                    )]
                    let n = len as f32;
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "c is in 0..=1, so c * n is in 0..=n"
                    )]
                    let k = ((c * n).floor() as usize).min(len.saturating_sub(1));
                    values.get(k).copied().unwrap_or(c)
                }
            },
            Self::Linear { slope, intercept } => slope * c + intercept,
            Self::Gamma {
                amplitude,
                exponent,
                offset,
            } => amplitude * c.powf(*exponent) + offset,
        };
        if out.is_finite() {
            out.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// `input` with each straight channel passed through its function -- red,
/// green, blue and alpha in that order: `feComponentTransfer`.
pub(super) fn component_transfer(input: &Image, funcs: &[Transfer; 4], area: Area) -> Image {
    let mut out = Image::transparent(input.width, input.height);
    let [fr, fg, fb, fa] = funcs;
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let [r, g, b, a] = input.pixel(x, y);
            let (r, g, b) = if a > 0.0 {
                (r / a, g / a, b / a)
            } else {
                (0.0, 0.0, 0.0)
            };
            let alpha = fa.apply(a);
            out.set(
                x,
                y,
                [
                    fr.apply(r) * alpha,
                    fg.apply(g) * alpha,
                    fb.apply(b) * alpha,
                    alpha,
                ],
            );
        }
    }
    out
}

// ─── Convolution ────────────────────────────────────────────────────────────

/// How a convolution reads past the image's edge: `edgeMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EdgeMode {
    /// The nearest edge pixel.
    Duplicate,
    /// The other side of the image.
    Wrap,
    /// Transparent black.
    None,
}

/// An `feConvolveMatrix`, its attributes checked.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Kernel {
    pub(super) order_x: usize,
    pub(super) order_y: usize,
    /// `order_x * order_y` values, row by row, as written.
    pub(super) values: Vec<f32>,
    pub(super) divisor: f32,
    pub(super) bias: f32,
    pub(super) target_x: usize,
    pub(super) target_y: usize,
    pub(super) edge: EdgeMode,
    pub(super) preserve_alpha: bool,
}

/// `input` convolved with `kernel`: `feConvolveMatrix`.
///
/// The kernel is turned half a turn, as the specification's formula has it
/// (convolution, not correlation). With `preserve_alpha` the alpha is kept
/// and the kernel applied to straight colour; without, to all four
/// premultiplied channels, the bias times the result's alpha added to each
/// colour.
pub(super) fn convolve(input: &Image, kernel: &Kernel, area: Area) -> Image {
    let mut out = Image::transparent(input.width, input.height);
    let (w, h) = (signed(input.width), signed(input.height));
    if w == 0 || h == 0 || kernel.divisor == 0.0 {
        return out;
    }
    let read = |x: isize, y: isize| -> Pixel {
        let (x, y) = match kernel.edge {
            EdgeMode::None => (x, y),
            EdgeMode::Duplicate => (
                x.clamp(0, w.saturating_sub(1)),
                y.clamp(0, h.saturating_sub(1)),
            ),
            EdgeMode::Wrap => (x.rem_euclid(w), y.rem_euclid(h)),
        };
        let px = input.at(x, y);
        if kernel.preserve_alpha && px[3] > 0.0 {
            [px[0] / px[3], px[1] / px[3], px[2] / px[3], px[3]]
        } else if kernel.preserve_alpha {
            CLEAR
        } else {
            px
        }
    };
    let (ox, oy) = (kernel.order_x, kernel.order_y);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let mut sum = [0.0f32; 4];
            for i in 0..oy {
                for j in 0..ox {
                    let sx = signed(x)
                        .saturating_sub(signed(kernel.target_x))
                        .saturating_add(signed(j));
                    let sy = signed(y)
                        .saturating_sub(signed(kernel.target_y))
                        .saturating_add(signed(i));
                    // kernelMatrix[orderX - j - 1, orderY - i - 1]
                    let ki = oy
                        .saturating_sub(i)
                        .saturating_sub(1)
                        .saturating_mul(ox)
                        .saturating_add(ox.saturating_sub(j).saturating_sub(1));
                    let k = kernel.values.get(ki).copied().unwrap_or(0.0);
                    let px = read(sx, sy);
                    for (s, c) in sum.iter_mut().zip(px) {
                        *s += c * k;
                    }
                }
            }
            let px = if kernel.preserve_alpha {
                let a = input.pixel(x, y)[3];
                let ch = |i: usize| {
                    (sum.get(i).copied().unwrap_or(0.0) / kernel.divisor + kernel.bias)
                        .clamp(0.0, 1.0)
                };
                [ch(0) * a, ch(1) * a, ch(2) * a, a]
            } else {
                let a = (sum[3] / kernel.divisor + kernel.bias).clamp(0.0, 1.0);
                let ch = |i: usize| {
                    sum.get(i).copied().unwrap_or(0.0) / kernel.divisor + kernel.bias * a
                };
                valid([ch(0), ch(1), ch(2), a])
            };
            out.set(x, y, px);
        }
    }
    out
}

// ─── Displacement ───────────────────────────────────────────────────────────

/// `input` with each pixel taken from where `map` says:
/// `P'(x, y) = P(x + sx (XC(x, y) - 0.5), y + sy (YC(x, y) - 0.5))`, `XC`
/// and `YC` the straight channels `x_channel` and `y_channel` (0 red to 3
/// alpha) of the map: `feDisplacementMap`.
pub(super) fn displace(
    input: &Image,
    map: &Image,
    scale: (f32, f32),
    channels: (usize, usize),
    area: Area,
) -> Image {
    let mut out = Image::transparent(input.width, input.height);
    let straight = |px: Pixel, c: usize| -> f32 {
        let a = px[3];
        if c == 3 {
            a
        } else if a > 0.0 {
            (px.get(c).copied().unwrap_or(0.0) / a).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let m = map.pixel(x, y);
            #[allow(
                clippy::cast_precision_loss,
                reason = "a pixel coordinate, far inside f32's exact range"
            )]
            let fx = x as f32 + 0.5 + scale.0 * (straight(m, channels.0) - 0.5);
            #[allow(
                clippy::cast_precision_loss,
                reason = "a pixel coordinate, far inside f32's exact range"
            )]
            let fy = y as f32 + 0.5 + scale.1 * (straight(m, channels.1) - 0.5);
            out.set(x, y, input.at(floor_signed(fx), floor_signed(fy)));
        }
    }
    out
}

/// `v` rounded down to a whole pixel, held to `isize`'s range; a value that
/// is not a number is far off the image.
fn floor_signed(v: f32) -> isize {
    if !v.is_finite() {
        return isize::MIN;
    }
    // Held to a range far inside isize's first.
    #[allow(clippy::cast_possible_truncation, reason = "clamped to +-2^30 first")]
    let whole = v.floor().clamp(-1.0e9, 1.0e9) as isize;
    whole
}

// ─── Lighting ───────────────────────────────────────────────────────────────

/// A light source, in the image's pixels.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Light {
    /// `feDistantLight`: from a direction, in degrees.
    Distant { azimuth: f32, elevation: f32 },
    /// `fePointLight`.
    Point { x: f32, y: f32, z: f32 },
    /// `feSpotLight`.
    Spot {
        x: f32,
        y: f32,
        z: f32,
        at: (f32, f32, f32),
        exponent: f32,
        /// `limitingConeAngle`, in degrees: no light outside it.
        cone: Option<f32>,
    },
}

/// What a lighting primitive computes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Lighting {
    /// `feDiffuseLighting`, with its `diffuseConstant`.
    Diffuse { constant: f32 },
    /// `feSpecularLighting`, with its `specularConstant` and
    /// `specularExponent`.
    Specular { constant: f32, exponent: f32 },
}

/// The light `light` of colour `color` -- in the image's colour space --
/// throws on the surface `input`'s alpha makes, `surface_scale` high:
/// `feDiffuseLighting` or `feSpecularLighting`.
///
/// The surface's normal is found with the specification's Sobel kernels,
/// their own forms at the edges and corners of the image.
pub(super) fn light(
    input: &Image,
    surface_scale: f32,
    lighting: Lighting,
    light: &Light,
    color: [f32; 3],
    area: Area,
) -> Image {
    let mut out = Image::transparent(input.width, input.height);
    let (w, h) = (input.width, input.height);
    if w == 0 || h == 0 {
        return out;
    }
    let alpha = |x: isize, y: isize| input.at(x, y)[3];
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let (nx, ny) = surface_normal(&alpha, x, y, w, h, surface_scale);
            let n = normalize([nx, ny, 1.0]);
            #[allow(
                clippy::cast_precision_loss,
                reason = "a pixel coordinate, far inside f32's exact range"
            )]
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let z = surface_scale * alpha(signed(x), signed(y));
            let (l, strength) = light_at(light, px, py, z);
            let lit = color.map(|c| c * strength);
            let pixel = match lighting {
                Lighting::Diffuse { constant } => {
                    let k = (constant * dot(n, l)).max(0.0);
                    [lit[0] * k, lit[1] * k, lit[2] * k, 1.0].map(|c| c.clamp(0.0, 1.0))
                }
                Lighting::Specular { constant, exponent } => {
                    let half = normalize([l[0], l[1], l[2] + 1.0]);
                    let nh = dot(n, half).max(0.0);
                    let k = constant * nh.powf(exponent);
                    let c = lit.map(|c| (c * k).clamp(0.0, 1.0));
                    let a = c[0].max(c[1]).max(c[2]);
                    [c[0], c[1], c[2], a]
                }
            };
            out.set(x, y, pixel);
        }
    }
    out
}

/// The unit vector from a surface point `(x, y, z)` to `light`, and how much
/// of the light's colour reaches it.
fn light_at(light: &Light, x: f32, y: f32, z: f32) -> ([f32; 3], f32) {
    match *light {
        Light::Distant { azimuth, elevation } => {
            let (az, el) = (azimuth.to_radians(), elevation.to_radians());
            ([az.cos() * el.cos(), az.sin() * el.cos(), el.sin()], 1.0)
        }
        Light::Point {
            x: lx,
            y: ly,
            z: lz,
        } => (normalize([lx - x, ly - y, lz - z]), 1.0),
        Light::Spot {
            x: lx,
            y: ly,
            z: lz,
            at,
            exponent,
            cone,
        } => {
            let l = normalize([lx - x, ly - y, lz - z]);
            let s = normalize([at.0 - lx, at.1 - ly, at.2 - lz]);
            let minus_ls = -dot(l, s);
            if minus_ls <= 0.0 {
                return (l, 0.0);
            }
            if let Some(cone) = cone
                && minus_ls < cone.abs().to_radians().cos()
            {
                return (l, 0.0);
            }
            (l, minus_ls.powf(exponent))
        }
    }
}

/// The surface's slope at `(x, y)`: `Nx` and `Ny` before normalising, by the
/// specification's kernels for an interior pixel, an edge or a corner.
fn surface_normal(
    alpha: &impl Fn(isize, isize) -> f32,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    scale: f32,
) -> (f32, f32) {
    let (xi, yi) = (signed(x), signed(y));
    let i = |dx: isize, dy: isize| alpha(xi.saturating_add(dx), yi.saturating_add(dy));
    let left = x == 0;
    let right = x.saturating_add(1) >= w;
    let top = y == 0;
    let bottom = y.saturating_add(1) >= h;
    // A one-pixel-wide or one-pixel-high image has no slope across it.
    let (fx, gx) = if left && right {
        (0.0, 0.0)
    } else if left {
        // Left column: the pixel and the one right of it.
        match (top, bottom) {
            (true, false) => (2.0 / 3.0, 2.0 * i(1, 0) + i(1, 1) - 2.0 * i(0, 0) - i(0, 1)),
            (false, true) => (
                2.0 / 3.0,
                i(1, -1) + 2.0 * i(1, 0) - i(0, -1) - 2.0 * i(0, 0),
            ),
            (true, true) => (1.0, i(1, 0) - i(0, 0)),
            (false, false) => (
                0.5,
                i(1, -1) + 2.0 * i(1, 0) + i(1, 1) - i(0, -1) - 2.0 * i(0, 0) - i(0, 1),
            ),
        }
    } else if right {
        match (top, bottom) {
            (true, false) => (
                2.0 / 3.0,
                2.0 * i(0, 0) + i(0, 1) - 2.0 * i(-1, 0) - i(-1, 1),
            ),
            (false, true) => (
                2.0 / 3.0,
                i(0, -1) + 2.0 * i(0, 0) - i(-1, -1) - 2.0 * i(-1, 0),
            ),
            (true, true) => (1.0, i(0, 0) - i(-1, 0)),
            (false, false) => (
                0.5,
                i(0, -1) + 2.0 * i(0, 0) + i(0, 1) - i(-1, -1) - 2.0 * i(-1, 0) - i(-1, 1),
            ),
        }
    } else {
        match (top, bottom) {
            (true, false) => (
                1.0 / 3.0,
                2.0 * i(1, 0) + i(1, 1) - 2.0 * i(-1, 0) - i(-1, 1),
            ),
            (false, true) => (
                1.0 / 3.0,
                i(1, -1) + 2.0 * i(1, 0) - i(-1, -1) - 2.0 * i(-1, 0),
            ),
            (true, true) => (0.5, i(1, 0) - i(-1, 0)),
            (false, false) => (
                0.25,
                i(1, -1) + 2.0 * i(1, 0) + i(1, 1) - i(-1, -1) - 2.0 * i(-1, 0) - i(-1, 1),
            ),
        }
    };
    let (fy, gy) = if top && bottom {
        (0.0, 0.0)
    } else if top {
        match (left, right) {
            (true, false) => (2.0 / 3.0, 2.0 * i(0, 1) + i(1, 1) - 2.0 * i(0, 0) - i(1, 0)),
            (false, true) => (
                2.0 / 3.0,
                i(-1, 1) + 2.0 * i(0, 1) - i(-1, 0) - 2.0 * i(0, 0),
            ),
            (true, true) => (1.0, i(0, 1) - i(0, 0)),
            (false, false) => (
                0.5,
                i(-1, 1) + 2.0 * i(0, 1) + i(1, 1) - i(-1, 0) - 2.0 * i(0, 0) - i(1, 0),
            ),
        }
    } else if bottom {
        match (left, right) {
            (true, false) => (
                2.0 / 3.0,
                2.0 * i(0, 0) + i(1, 0) - 2.0 * i(0, -1) - i(1, -1),
            ),
            (false, true) => (
                2.0 / 3.0,
                i(-1, 0) + 2.0 * i(0, 0) - i(-1, -1) - 2.0 * i(0, -1),
            ),
            (true, true) => (1.0, i(0, 0) - i(0, -1)),
            (false, false) => (
                0.5,
                i(-1, 0) + 2.0 * i(0, 0) + i(1, 0) - i(-1, -1) - 2.0 * i(0, -1) - i(1, -1),
            ),
        }
    } else {
        match (left, right) {
            (true, false) => (
                1.0 / 3.0,
                2.0 * i(0, 1) + i(1, 1) - 2.0 * i(0, -1) - i(1, -1),
            ),
            (false, true) => (
                1.0 / 3.0,
                i(-1, 1) + 2.0 * i(0, 1) - i(-1, -1) - 2.0 * i(0, -1),
            ),
            (true, true) => (0.5, i(0, 1) - i(0, -1)),
            (false, false) => (
                0.25,
                i(-1, 1) + 2.0 * i(0, 1) + i(1, 1) - i(-1, -1) - 2.0 * i(0, -1) - i(1, -1),
            ),
        }
    };
    (-scale * fx * gx, -scale * fy * gy)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `v` scaled to a length of one; nought stays nought.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len > 0.0 && len.is_finite() {
        v.map(|c| c / len)
    } else {
        [0.0; 3]
    }
}

// ─── Turbulence ─────────────────────────────────────────────────────────────

/// An `feTurbulence`, its attributes read.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Turbulence {
    /// `baseFrequency`, across and down.
    pub(super) base: (f64, f64),
    /// `numOctaves`.
    pub(super) octaves: u32,
    /// `seed`, already truncated to a whole number.
    pub(super) seed: i64,
    /// `type="fractalNoise"` rather than `turbulence`.
    pub(super) fractal: bool,
    /// For `stitchTiles="stitch"`: the tile it is stitched to, `x, y,
    /// width, height`, in the space the noise is evaluated in.
    pub(super) stitch: Option<(f64, f64, f64, f64)>,
}

/// The lattice and gradients the specification's reference code builds
/// from a seed.
struct Lattice {
    selector: Vec<usize>,
    gradient: [Vec<[f64; 2]>; 4],
}

/// The reference code's `BSize`: the lattice's size.
const B_SIZE: usize = 0x100;
/// `BSize`, signed, for the arithmetic on lattice coordinates.
const B_SIZE_SIGNED: i64 = 0x100;
/// `BSize + BSize`.
const TWO_B_SIZE: i64 = 0x200;
/// `BM`.
const B_MASK: i64 = 0xff;
/// `PerlinN`, as the offset added to every coordinate.
const PERLIN_N: f64 = 4096.0;
/// `PerlinN`, as stitching subtracts it once per octave.
const PERLIN_N_WHOLE: i64 = 4096;
/// The generator's modulus `RAND_m`, and the largest seed, `RAND_m - 1`.
const RAND_M: i64 = 2_147_483_647;
const RAND_M_LESS_ONE: i64 = 2_147_483_646;
const RAND_A: i64 = 16_807;
/// `RAND_m / RAND_a` and `RAND_m % RAND_a`: Schrage's method, so nothing
/// overflows 32 bits.
const RAND_Q: i64 = 127_773;
const RAND_R: i64 = 2_836;

/// The reference code's `setup_seed`.
fn setup_seed(seed: i64) -> i64 {
    let mut seed = seed;
    if seed <= 0 {
        seed = seed
            .wrapping_rem(RAND_M_LESS_ONE)
            .saturating_neg()
            .saturating_add(1);
    }
    if seed > RAND_M_LESS_ONE {
        seed = RAND_M_LESS_ONE;
    }
    seed
}

/// The reference code's `random`: Park and Miller's minimal standard. By
/// Schrage's method no step leaves 32 bits, so nothing here saturates.
fn random(seed: i64) -> i64 {
    let result = RAND_A
        .saturating_mul(seed.wrapping_rem(RAND_Q))
        .saturating_sub(RAND_R.saturating_mul(seed.wrapping_div(RAND_Q)));
    if result <= 0 {
        result.saturating_add(RAND_M)
    } else {
        result
    }
}

impl Lattice {
    /// The reference code's `init`.
    fn new(seed: i64) -> Self {
        let size = B_SIZE.saturating_mul(2).saturating_add(2);
        let mut selector = vec![0usize; size];
        let mut gradient: [Vec<[f64; 2]>; 4] = core::array::from_fn(|_| vec![[0.0; 2]; size]);
        let mut seed = setup_seed(seed);
        #[allow(clippy::cast_precision_loss, reason = "B_SIZE is 256, exact in f64")]
        let b = B_SIZE_SIGNED as f64;
        for grad in &mut gradient {
            for (i, sel) in selector.iter_mut().enumerate().take(B_SIZE) {
                *sel = i;
                let mut v = [0.0f64; 2];
                for c in &mut v {
                    seed = random(seed);
                    #[allow(
                        clippy::cast_precision_loss,
                        reason = "a value under 512, exact in f64"
                    )]
                    let r = seed.wrapping_rem(TWO_B_SIZE) as f64;
                    *c = (r - b) / b;
                }
                let s = (v[0] * v[0] + v[1] * v[1]).sqrt();
                if s > 0.0 {
                    v = [v[0] / s, v[1] / s];
                }
                if let Some(slot) = grad.get_mut(i) {
                    *slot = v;
                }
            }
        }
        let mut i = B_SIZE;
        while i > 1 {
            i = i.saturating_sub(1);
            seed = random(seed);
            let j = usize::try_from(seed.rem_euclid(B_SIZE_SIGNED)).unwrap_or(0);
            if i < selector.len() && j < selector.len() {
                selector.swap(i, j);
            }
        }
        for i in 0..B_SIZE.saturating_add(2) {
            let src = i;
            let dst = B_SIZE.saturating_add(i);
            if let Some(&v) = selector.get(src) {
                if let Some(slot) = selector.get_mut(dst) {
                    *slot = v;
                }
            }
            for grad in &mut gradient {
                if let Some(&v) = grad.get(src) {
                    if let Some(slot) = grad.get_mut(dst) {
                        *slot = v;
                    }
                }
            }
        }
        Self { selector, gradient }
    }

    fn sel(&self, i: i64) -> i64 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.selector.get(i))
            .and_then(|&v| i64::try_from(v).ok())
            .unwrap_or(0)
    }

    fn grad(&self, channel: usize, i: i64) -> [f64; 2] {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.gradient.get(channel)?.get(i))
            .copied()
            .unwrap_or([0.0; 2])
    }

    /// The reference code's `noise2`.
    fn noise2(&self, channel: usize, vec: [f64; 2], stitch: Option<&Stitch>) -> f64 {
        let t = vec[0] + PERLIN_N;
        let mut bx0 = whole(t);
        let mut bx1 = bx0.saturating_add(1);
        let rx0 = t - t.trunc();
        let rx1 = rx0 - 1.0;
        let t = vec[1] + PERLIN_N;
        let mut by0 = whole(t);
        let mut by1 = by0.saturating_add(1);
        let ry0 = t - t.trunc();
        let ry1 = ry0 - 1.0;
        if let Some(st) = stitch {
            if bx0 >= st.wrap_x {
                bx0 = bx0.saturating_sub(st.width);
            }
            if bx1 >= st.wrap_x {
                bx1 = bx1.saturating_sub(st.width);
            }
            if by0 >= st.wrap_y {
                by0 = by0.saturating_sub(st.height);
            }
            if by1 >= st.wrap_y {
                by1 = by1.saturating_sub(st.height);
            }
        }
        let (bx0, bx1, by0, by1) = (bx0 & B_MASK, bx1 & B_MASK, by0 & B_MASK, by1 & B_MASK);
        let i = self.sel(bx0);
        let j = self.sel(bx1);
        let b00 = self.sel(i.saturating_add(by0));
        let b10 = self.sel(j.saturating_add(by0));
        let b01 = self.sel(i.saturating_add(by1));
        let b11 = self.sel(j.saturating_add(by1));
        let sx = s_curve(rx0);
        let sy = s_curve(ry0);
        let q = self.grad(channel, b00);
        let u = rx0 * q[0] + ry0 * q[1];
        let q = self.grad(channel, b10);
        let v = rx1 * q[0] + ry0 * q[1];
        let a = lerp(sx, u, v);
        let q = self.grad(channel, b01);
        let u = rx0 * q[0] + ry1 * q[1];
        let q = self.grad(channel, b11);
        let v = rx1 * q[0] + ry1 * q[1];
        let b = lerp(sx, u, v);
        lerp(sy, a, b)
    }
}

/// The reference code's `StitchInfo`.
struct Stitch {
    width: i64,
    height: i64,
    wrap_x: i64,
    wrap_y: i64,
}

/// `t` as C's `(int)t` makes it, for the positive values the noise uses.
fn whole(t: f64) -> i64 {
    if !t.is_finite() {
        return 0;
    }
    // Truncation toward nought, as C's cast; held far inside i64 first.
    #[allow(clippy::cast_possible_truncation, reason = "clamped to +-2^52 first")]
    let n = t.clamp(-4.0e15, 4.0e15).trunc() as i64;
    n
}

fn s_curve(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

impl Turbulence {
    /// The reference code's `turbulence` for one channel at `point`.
    fn at(&self, lattice: &Lattice, channel: usize, point: (f64, f64)) -> f64 {
        let (mut fx, mut fy) = self.base;
        let mut stitch = None;
        if let Some((tx, ty, tw, th)) = self.stitch {
            if fx != 0.0 && tw > 0.0 {
                let lo = (tw * fx).floor() / tw;
                let hi = (tw * fx).ceil() / tw;
                fx = if lo > 0.0 && fx / lo < hi / fx {
                    lo
                } else {
                    hi
                };
            }
            if fy != 0.0 && th > 0.0 {
                let lo = (th * fy).floor() / th;
                let hi = (th * fy).ceil() / th;
                fy = if lo > 0.0 && fy / lo < hi / fy {
                    lo
                } else {
                    hi
                };
            }
            let width = whole(tw * fx + 0.5);
            let height = whole(th * fy + 0.5);
            stitch = Some(Stitch {
                width,
                height,
                wrap_x: whole(tx * fx + PERLIN_N).saturating_add(width),
                wrap_y: whole(ty * fy + PERLIN_N).saturating_add(height),
            });
        }
        let mut sum = 0.0;
        let mut vec = [point.0 * fx, point.1 * fy];
        let mut ratio = 1.0;
        for _ in 0..self.octaves {
            let n = lattice.noise2(channel, vec, stitch.as_ref());
            sum += if self.fractal { n } else { n.abs() } / ratio;
            vec = [vec[0] * 2.0, vec[1] * 2.0];
            ratio *= 2.0;
            if let Some(st) = stitch.as_mut() {
                st.width = st.width.saturating_mul(2);
                st.wrap_x = st.wrap_x.saturating_mul(2).saturating_sub(PERLIN_N_WHOLE);
                st.height = st.height.saturating_mul(2);
                st.wrap_y = st.wrap_y.saturating_mul(2).saturating_sub(PERLIN_N_WHOLE);
            }
        }
        sum
    }
}

/// `params`' noise over `area` of an image `width` by `height`, each
/// pixel's top-left corner carried by `to_noise` into the space the noise is
/// evaluated in: `feTurbulence`. The colour it makes is in the primitive's
/// colour space, straight, and premultiplied here.
pub(super) fn turbulence(
    params: &Turbulence,
    width: usize,
    height: usize,
    to_noise: impl Fn(f64, f64) -> (f64, f64),
    area: Area,
) -> Image {
    let mut out = Image::transparent(width, height);
    if params.base.0 < 0.0 || params.base.1 < 0.0 {
        return out;
    }
    let lattice = Lattice::new(params.seed);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a pixel coordinate, exact in f64"
            )]
            let point = to_noise(x as f64, y as f64);
            let mut rgba = [0.0f32; 4];
            for (channel, slot) in rgba.iter_mut().enumerate() {
                let n = params.at(&lattice, channel, point);
                let v = if params.fractal {
                    f64::midpoint(n, 1.0)
                } else {
                    n
                };
                #[allow(clippy::cast_possible_truncation, reason = "held to 0..=1 first")]
                let v = v.clamp(0.0, 1.0) as f32;
                *slot = v;
            }
            let a = rgba[3];
            out.set(x, y, [rgba[0] * a, rgba[1] * a, rgba[2] * a, a]);
        }
    }
    out
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
