//! Clip paths: what a `clip-path="url(#id)"` names, and the masks drawing
//! multiplies each pixel's coverage by while it draws what is clipped.
//!
//! # What is drawn
//!
//! - A `<clipPath>`'s shapes, and `<use>`s of shapes, each filled under its
//!   `clip-rule` (inherited from the `<clipPath>`, `nonzero` by default) into
//!   one mask. Their strokes, colours and opacities play no part.
//! - **`clipPathUnits`**: `userSpaceOnUse` (the default: the clipped
//!   element's user space) or `objectBoundingBox` (fractions of its box).
//!   The `<clipPath>`'s `transform` applies outside the box's mapping, as
//!   Chromium and resvg place it.
//! - A `clip-path` on the `<clipPath>` itself clips the clip; clips around
//!   clips multiply.
//!
//! A reference to nothing, or to an element that is not a `<clipPath>`, clips
//! nothing, as CSS Masking says; a `<clipPath>` with nothing in it clips
//! everything away, as does one measured against a box with no area.
//!
//! Not drawn: a `clip-path` on a `<clipPath>`'s children, `<text>` or a `<g>`
//! in one (SVG allows only shapes, text and `<use>` there).

use std::collections::HashMap;

use super::paint::Units;
use super::{FillRule, SvgNode, Transform, XmlElement, keyword, parse_transform, property};

/// How many clip paths deep a clip path's own `clip-path` is followed: one
/// that names itself through others ends here.
pub(super) const MAX_CLIP_DEPTH: usize = 8;

/// What an element is clipped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    /// The `<clipPath>` at this place among the document's.
    Path(usize),
}

/// A `<clipPath>`, built: what it holds, and how it is laid over what it
/// clips.
#[derive(Clone, Debug)]
pub(super) struct ClipPath {
    pub(super) units: Units,
    pub(super) transform: Transform,
    /// Its own `clip-rule`, which what it holds inherits.
    pub(super) rule: Option<FillRule>,
    /// Its own `clip-path`: the clip of the clip.
    pub(super) clip: Option<Clip>,
    /// Its shapes and `<use>`s.
    pub(super) children: Vec<SvgNode>,
}

/// The `<clipPath>`s of a document, by `id`, found before anything is built so
/// a `clip-path` can name one defined after it.
pub(super) struct ClipIds<'x> {
    places: HashMap<&'x str, usize>,
    /// In order of place.
    pub(super) elements: Vec<&'x XmlElement>,
}

impl<'x> ClipIds<'x> {
    /// Every `<clipPath>` under `root` that `by_id` -- the first element with
    /// each `id` -- names: one whose `id` an earlier element took is not found
    /// by it.
    pub(super) fn collect(root: &'x XmlElement, by_id: &HashMap<&'x str, &'x XmlElement>) -> Self {
        let mut ids = Self {
            places: HashMap::new(),
            elements: Vec::new(),
        };
        ids.gather(root, by_id);
        ids
    }

    fn gather(&mut self, elem: &'x XmlElement, by_id: &HashMap<&'x str, &'x XmlElement>) {
        if elem.tag == "clipPath"
            && let Some((&id, &first)) = elem
                .attr("id")
                .map(str::trim)
                .and_then(|id| by_id.get_key_value(id))
            && core::ptr::eq(first, elem)
        {
            self.places.insert(id, self.elements.len());
            self.elements.push(elem);
        }
        for child in &elem.children {
            self.gather(child, by_id);
        }
    }

    /// What a `clip-path` value says: the `<clipPath>` a `url(#id)` names,
    /// or nothing -- for `none`, a name in no `<clipPath>`, or anything else.
    pub(super) fn clip(&self, value: &str) -> Option<Clip> {
        let reference = value.trim().strip_prefix("url(")?;
        let (reference, _) = reference.split_once(')')?;
        let id = reference
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .strip_prefix('#')?;
        self.places.get(id).map(|&place| Clip::Path(place))
    }
}

/// The parts of a `<clipPath>` that are not its children: its units,
/// transform, `clip-rule` and own `clip-path`.
pub(super) fn clip_path_frame(
    elem: &XmlElement,
    ids: &ClipIds<'_>,
) -> (Units, Transform, Option<FillRule>, Option<Clip>) {
    let units = match elem.attr("clipPathUnits").map(str::trim) {
        Some("objectBoundingBox") => Units::ObjectBoundingBox,
        _ => Units::UserSpaceOnUse,
    };
    // A transform that cannot be read is not said, as an invalid attribute
    // is not.
    let transform = elem
        .attr("transform")
        .and_then(|t| parse_transform(t).ok())
        .unwrap_or(Transform::IDENTITY);
    let rule = clip_rule(elem);
    let clip = property(elem, "clip-path").and_then(|value| ids.clip(value));
    (units, transform, rule, clip)
}

/// An element's `clip-rule`, if it says one this renderer knows.
pub(super) fn clip_rule(elem: &XmlElement) -> Option<FillRule> {
    keyword(
        property(elem, "clip-rule"),
        &[
            ("nonzero", FillRule::NonZero),
            ("evenodd", FillRule::EvenOdd),
        ],
    )
}

/// Whether `tag` may stand in a `<clipPath>`: a shape or a `<use>`.
pub(super) fn may_clip(tag: &str) -> bool {
    matches!(
        tag,
        "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" | "path" | "use"
    )
}

/// What a clip leaves of each pixel of a region of the surface: none of it
/// outside the region.
#[derive(Clone, Debug)]
pub(super) struct Mask {
    x0: u32,
    y0: u32,
    width: u32,
    height: u32,
    /// What is left of each pixel of the region, 0 to 255, row by row.
    left: Vec<u8>,
}

impl Mask {
    /// A mask that leaves nothing anywhere.
    pub(super) fn nothing() -> Self {
        Self {
            x0: 0,
            y0: 0,
            width: 0,
            height: 0,
            left: Vec::new(),
        }
    }

    /// A mask over the region `x0..x1` by `y0..y1` of the surface that leaves
    /// nothing yet: shapes are added to it.
    pub(super) fn over(x0: u32, y0: u32, x1: u32, y1: u32) -> Self {
        let (width, height) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let size = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(w, h)| w.checked_mul(h))
            .unwrap_or(0);
        if size == 0 {
            return Self::nothing();
        }
        Self {
            x0,
            y0,
            width,
            height,
            left: vec![0; size],
        }
    }

    /// Where the pixel `(x, y)` is in `left`, if it is in the region.
    fn index(&self, x: u32, y: u32) -> Option<usize> {
        let (dx, dy) = (x.checked_sub(self.x0)?, y.checked_sub(self.y0)?);
        if dx >= self.width || dy >= self.height {
            return None;
        }
        let at = dy.checked_mul(self.width)?.checked_add(dx)?;
        usize::try_from(at).ok()
    }

    /// What it leaves of the pixel `(x, y)`: 0 to 1.
    pub(super) fn share(&self, x: u32, y: u32) -> f32 {
        self.index(x, y)
            .and_then(|i| self.left.get(i))
            .map_or(0.0, |&left| f32::from(left) / 255.0)
    }

    /// Leave `coverage` more of each pixel of row `row` from column
    /// `first_col` on: the shapes of a clip path are a union, each adding
    /// what it covers -- so two that meet along an edge leave all of the
    /// pixels on it, as one shape would, rather than a seam.
    pub(super) fn add_row(&mut self, row: u32, first_col: u32, coverage: &[f32]) {
        for (col, &cov) in (first_col..).zip(coverage) {
            if cov <= 0.0 {
                continue;
            }
            if let Some(cell) = self.index(col, row).and_then(|i| self.left.get_mut(i)) {
                let added = to_byte(cov);
                *cell = cell.saturating_add(added);
            }
        }
    }

    /// What two masks both leave: their regions' overlap, each pixel's
    /// shares multiplied.
    pub(super) fn intersect(&self, other: &Self) -> Self {
        let x0 = self.x0.max(other.x0);
        let y0 = self.y0.max(other.y0);
        let x1 = (self.x0.saturating_add(self.width)).min(other.x0.saturating_add(other.width));
        let y1 = (self.y0.saturating_add(self.height)).min(other.y0.saturating_add(other.height));
        let mut both = Self::over(x0, y0, x1, y1);
        for y in y0..y1 {
            for x in x0..x1 {
                let left = self.share(x, y) * other.share(x, y);
                if let Some(cell) = both.index(x, y).and_then(|i| both.left.get_mut(i)) {
                    *cell = to_byte(left);
                }
            }
        }
        both
    }
}

/// A share, 0 to 1, as a byte, rounded.
fn to_byte(share: f32) -> u8 {
    // In 0..=255 by the clamp; `as` on a value already rounded.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "held to 0..=255 and rounded first"
    )]
    let byte = (share.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    byte
}

#[cfg(test)]
#[path = "clip_tests.rs"]
mod tests;
