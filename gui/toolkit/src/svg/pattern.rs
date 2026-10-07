//! Patterns: a tile of content repeated to paint a shape, as
//! `fill="url(#p)"` names one.
//!
//! # What is drawn
//!
//! - The **tile** is the rectangle `x`, `y`, `width`, `height` -- in fractions
//!   of the painted shape's box under `patternUnits="objectBoundingBox"`, the
//!   default, or in user units -- repeated every `width` across and every
//!   `height` down, without end, and carried by `patternTransform`.
//! - Its **content** starts at the tile's corner, in user units
//!   (`patternContentUnits`'s default), in fractions of the box, or fitted to
//!   the tile from a `viewBox` as `preserveAspectRatio` says; what overflows
//!   the tile is cut, as SVG's own style sheet has it for every pattern.
//! - A pattern names another with `href` (or `xlink:href`) to take from it
//!   whatever of these it does not say itself, and its content if it has none.
//!
//! The tile is drawn once per shape it paints, on a scratch surface as large
//! as it shows on the screen, and the shape's pixels read it -- sampled
//! between its pixels, so a tile turned or scaled by the pattern's transform
//! is not stepped. A tile with no area paints nothing. A pattern whose content
//! is painted with patterns is followed as deep as clip paths are,
//! [`super::MAX_CLIP_DEPTH`], and every tile comes out of the drawing's budget
//! for scratch surfaces ([`super::SCRATCH_PER_PIXEL`]).
//!
//! Not read: a pattern's `overflow: visible`, which would draw its content
//! past the tile -- browsers do not either.

use super::clip::Referable;
use super::paint::Units;
use super::{
    AspectRatio, SvgNode, Transform, XmlElement, parse_transform, parse_viewbox, reference,
    viewport_length,
};
use crate::color::Color;

/// The longest chain of patterns one may follow through `href`s. Real ones are
/// one or two long; a chain is followed to its end, or to a pattern met
/// before, or to this many.
const MAX_CHAIN: usize = 16;

/// The widest and tallest a tile is drawn, in pixels: wider than any screen.
pub(super) const MAX_TILE_SIDE: u32 = 4096;

/// A `<pattern>`, built: where its tile is, and what is drawn in it.
#[derive(Clone, Debug)]
pub(super) struct PatternDef {
    /// What the tile's rectangle is measured in.
    pub(super) units: Units,
    /// What the content is measured in, where there is no view box.
    pub(super) content_units: Units,
    /// The tile, `x, y, width, height`: fractions of the shape's box in
    /// [`Units::ObjectBoundingBox`], user units otherwise.
    pub(super) rect: [f32; 4],
    /// `patternTransform`.
    pub(super) transform: Transform,
    /// The content's view box, fitted to the tile.
    pub(super) view_box: Option<(f32, f32, f32, f32)>,
    /// How the view box is fitted.
    pub(super) aspect: AspectRatio,
    /// What is drawn in the tile.
    pub(super) children: Vec<SvgNode>,
}

/// The pattern at `place` and the patterns it names through `href`, nearest
/// first: at most [`MAX_CHAIN`], none twice.
pub(super) fn chain<'x>(place: usize, ids: &Referable<'x>) -> Vec<&'x XmlElement> {
    let mut chain: Vec<&'x XmlElement> = Vec::new();
    let mut at = Some(place);
    while let Some(place) = at {
        let Some(&elem) = ids.elements.get(place) else {
            break;
        };
        if chain.len() >= MAX_CHAIN || chain.iter().any(|seen| core::ptr::eq(*seen, elem)) {
            break;
        }
        chain.push(elem);
        at = reference(elem).and_then(|id| ids.place_of(id));
    }
    chain
}

/// The first of `chain` to say `name`, and what it says.
fn said<'x>(chain: &[&'x XmlElement], name: &str) -> Option<&'x str> {
    chain.iter().find_map(|elem| elem.attr(name))
}

/// The pattern `chain` begins with, but for its content, which is the
/// builder's to give it: its units, its tile -- percentages of `viewport`
/// where it is in user space -- its transform, view box and fitting.
///
/// # Errors
///
/// A `patternTransform` that cannot be read, which leaves the pattern out
/// rather than drawing it somewhere it was not meant to be.
pub(super) fn pattern_frame(
    chain: &[&XmlElement],
    viewport: (f32, f32),
) -> Result<PatternDef, super::SvgError> {
    let units_of = |name: &str, default: Units| match said(chain, name).map(str::trim) {
        Some("objectBoundingBox") => Units::ObjectBoundingBox,
        Some("userSpaceOnUse") => Units::UserSpaceOnUse,
        _ => default,
    };
    let units = units_of("patternUnits", Units::ObjectBoundingBox);
    let content_units = units_of("patternContentUnits", Units::UserSpaceOnUse);
    let (view_w, view_h) = viewport;
    let side = |name: &str, extent: f32| {
        let whole = match units {
            Units::ObjectBoundingBox => 1.0,
            Units::UserSpaceOnUse => extent,
        };
        said(chain, name)
            .and_then(|value| viewport_length(value, whole))
            .unwrap_or(0.0)
    };
    let rect = [
        side("x", view_w),
        side("y", view_h),
        side("width", view_w),
        side("height", view_h),
    ];
    let transform = said(chain, "patternTransform")
        .map(parse_transform)
        .transpose()?
        .unwrap_or(Transform::IDENTITY);
    let view_box = said(chain, "viewBox").and_then(|value| parse_viewbox(value).ok());
    let aspect =
        said(chain, "preserveAspectRatio").map_or(AspectRatio::DEFAULT, AspectRatio::parse);
    Ok(PatternDef {
        units,
        content_units,
        rect,
        transform,
        view_box,
        aspect,
        children: Vec::new(),
    })
}

/// A pattern that paints nothing: its tile has no area.
pub(super) fn nothing() -> PatternDef {
    PatternDef {
        units: Units::UserSpaceOnUse,
        content_units: Units::UserSpaceOnUse,
        rect: [0.0; 4],
        transform: Transform::IDENTITY,
        view_box: None,
        aspect: AspectRatio::DEFAULT,
        children: Vec::new(),
    }
}

/// The first of `chain` with content of its own: a pattern with none takes
/// the content of the one it names.
pub(super) fn content_of<'x>(chain: &[&'x XmlElement]) -> Option<&'x XmlElement> {
    chain.iter().copied().find(|elem| !elem.children.is_empty())
}

/// A tile, drawn: its pixels, straight alpha, row by row.
#[derive(Clone, Debug)]
pub(super) struct Tile {
    pub(super) pixels: Vec<u8>,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl Tile {
    /// The colour at `(u, v)` in the tile's pixels, the tile repeated without
    /// end: read between its four nearest pixels' centres, so a tile drawn
    /// turned or scaled is smooth, and exactly a pixel's colour at its centre.
    pub(super) fn color_at(&self, u: f32, v: f32) -> Color {
        let (Some(fx), Some(fy)) = (wrapped(u, self.width), wrapped(v, self.height)) else {
            return Color::rgba(0, 0, 0, 0);
        };
        let (x0, tx) = split(fx, self.width);
        let (y0, ty) = split(fy, self.height);
        let x1 = next(x0, self.width);
        let y1 = next(y0, self.height);
        // Mixed with their alphas, so a transparent pixel's colour does not
        // bleed into its neighbour's.
        let mut sum = [0.0f32; 4];
        for (x, y, weight) in [
            (x0, y0, (1.0 - tx) * (1.0 - ty)),
            (x1, y0, tx * (1.0 - ty)),
            (x0, y1, (1.0 - tx) * ty),
            (x1, y1, tx * ty),
        ] {
            let [r, g, b, a] = self.pixel(x, y);
            let alpha = f32::from(a) / 255.0 * weight;
            sum[0] += f32::from(r) * alpha;
            sum[1] += f32::from(g) * alpha;
            sum[2] += f32::from(b) * alpha;
            sum[3] += alpha;
        }
        let [r, g, b, a] = sum;
        if a <= 0.0 {
            return Color::rgba(0, 0, 0, 0);
        }
        Color::rgba(byte(r / a), byte(g / a), byte(b / a), byte(a * 255.0))
    }

    /// The pixel at `(x, y)`, transparent past the tile.
    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = u64::from(y)
            .checked_mul(u64::from(self.width))
            .and_then(|row| row.checked_add(u64::from(x)))
            .and_then(|i| i.checked_mul(4))
            .and_then(|i| usize::try_from(i).ok());
        at.and_then(|i| self.pixels.get(i..i.checked_add(4)?))
            .and_then(|p| <[u8; 4]>::try_from(p).ok())
            .unwrap_or([0; 4])
    }
}

/// `at`, half a pixel back to measure from pixel centres, taken round a tile
/// `side` pixels long -- or `None` where it is no place at all.
fn wrapped(at: f32, side: u32) -> Option<f32> {
    #[allow(
        clippy::cast_precision_loss,
        reason = "a tile's side, at most MAX_TILE_SIDE, exact in f32"
    )]
    let side = side as f32;
    (at.is_finite() && side > 0.0).then(|| (at - 0.5).rem_euclid(side))
}

/// The pixel at or before `at`, in a tile `side` long, and how far past it
/// `at` is.
///
/// Taken round the tile, not held to its last pixel: `at` comes from
/// `rem_euclid`, which can round up to `side` itself -- a point a hair's
/// breadth before the tile's start, as a turn's float error leaves one, is
/// the start of the next tile, not the end of this one.
fn split(at: f32, side: u32) -> (u32, f32) {
    let floor = at.floor();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "`at` is in 0..=side, wrapped, so its floor is at most the tile's side"
    )]
    let pixel = (floor as u32).checked_rem(side).unwrap_or(0);
    (pixel, (at - floor).clamp(0.0, 1.0))
}

/// The pixel after `pixel`, round a tile `side` long.
fn next(pixel: u32, side: u32) -> u32 {
    let after = pixel.saturating_add(1);
    if after >= side { 0 } else { after }
}

/// `value` held to a byte and rounded.
fn byte(value: f32) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "held to 0..=255 first"
    )]
    let byte = (value.clamp(0.0, 255.0) + 0.5) as u8;
    byte
}

#[cfg(test)]
#[path = "pattern_tests.rs"]
mod tests;
