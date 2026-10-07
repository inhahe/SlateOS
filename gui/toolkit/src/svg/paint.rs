//! Paint servers: the gradients a fill or a stroke names with `url(#id)`.
//!
//! A gradient is collected from wherever the document defines it -- in a
//! `<defs>` or not, before or after what uses it -- under its `id`. One that
//! names another with `href` (or SVG 1.1's `xlink:href`) takes that one's
//! stops when it has none of its own, and every attribute it does not set,
//! following the chain as SVG says: the geometry only from a gradient of its
//! own kind, the units, transform and spread from either.
//!
//! # What is drawn
//!
//! - **Linear** (`x1 y1 x2 y2`) and **radial** (`cx cy r fx fy`), the focal
//!   point held inside the circle as SVG 1.1 does.
//! - **`gradientUnits`**: `objectBoundingBox` (the default: the numbers are
//!   fractions of the shape's box) or `userSpaceOnUse`; **`gradientTransform`**;
//!   **`spreadMethod`** `pad`, `reflect` or `repeat`.
//! - **Stops** with `offset`, `stop-color` and `stop-opacity`, as attributes or
//!   in `style`; each offset held to 0..1 and to no less than the one before.
//!   Colours between stops are mixed channel by channel, alpha with the rest
//!   and not applied first -- SVG 2: "SVG does not calculate gradients in
//!   pre-multiplied space, so 'transparent' really means transparent black".
//!   A fade to a transparent stop of another colour passes through that
//!   colour, which is why a fade is written with one colour in both stops,
//!   as Inkscape writes it; icons are drawn to look right this way.
//!
//! Not drawn: SVG 2's focal radius `fr` and a focal point on or outside the
//! circle as a cone (it is moved inside, as SVG 1.1 has it), `<pattern>`,
//! `<solidcolor>`/`<meshgradient>`, and `color-interpolation: linearRGB`.
//!
//! SVG's degenerate cases are answered as it answers them: a gradient with no
//! stops paints nothing, one stop paints that stop's colour, and a line of no
//! length or a circle of no radius paints the last stop's colour. A gradient
//! that names one that does not exist, or a chain of names that loops, keeps
//! what it set itself.

use std::collections::HashMap;

use super::{SvgPaint, Transform, XmlElement, declared, parse_color, parse_transform};
use crate::color::Color;

/// The most gradients a chain of `href`s is followed through: far more than
/// any document needs, and an end to one that names itself in a ring.
const MAX_CHAIN: usize = 16;

/// How a gradient's numbers are measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Units {
    /// As fractions of the painted shape's bounding box.
    ObjectBoundingBox,
    /// In the shape's own user space.
    UserSpaceOnUse,
}

/// What a gradient paints past its ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Spread {
    /// The end stops' colours, on and on.
    Pad,
    /// The gradient again, mirrored each time.
    Reflect,
    /// The gradient again from its start.
    Repeat,
}

/// A gradient's geometry, in gradient space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Shape {
    /// From `(x1, y1)` at offset 0 to `(x2, y2)` at offset 1.
    Linear { x1: f32, y1: f32, x2: f32, y2: f32 },
    /// From the focal point `(fx, fy)` at offset 0 to the circle round
    /// `(cx, cy)` of radius `r` at offset 1.
    Radial {
        cx: f32,
        cy: f32,
        r: f32,
        fx: f32,
        fy: f32,
    },
}

/// A gradient, resolved: its geometry, how it is measured and placed, and
/// its stops.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub(super) shape: Shape,
    pub(super) units: Units,
    pub(super) transform: Transform,
    pub(super) spread: Spread,
    /// Offset in 0..=1, never less than the one before, and the colour there,
    /// straight alpha.
    pub(super) stops: Vec<(f32, Color)>,
}

impl Gradient {
    /// Where gradient space is, as a map from device pixels to it, for a
    /// shape whose user-space bounding box is `bbox` (`x, y, width, height`)
    /// drawn into device space by `ctm`. `None` where the gradient cannot be
    /// laid over the shape: a box with no width or no height measured in
    /// fractions of it, or a placement that flattens the plane.
    pub(super) fn device_to_gradient(
        &self,
        ctm: Transform,
        bbox: (f32, f32, f32, f32),
    ) -> Option<Transform> {
        let user = match self.units {
            Units::UserSpaceOnUse => Transform::IDENTITY,
            Units::ObjectBoundingBox => {
                let (x, y, w, h) = bbox;
                if w.is_nan() || h.is_nan() || w <= 0.0 || h <= 0.0 {
                    return None;
                }
                Transform::translate(x, y).then(Transform::scale(w, h))
            }
        };
        ctm.then(user).then(self.transform).inverse()
    }

    /// The colour at the gradient-space point `(x, y)`, straight alpha.
    pub(super) fn color_at(&self, x: f32, y: f32) -> Color {
        let t = match self.shape {
            Shape::Linear { x1, y1, x2, y2 } => {
                let (dx, dy) = (x2 - x1, y2 - y1);
                let length = dx * dx + dy * dy;
                if length > 0.0 {
                    ((x - x1) * dx + (y - y1) * dy) / length
                } else {
                    1.0
                }
            }
            Shape::Radial { cx, cy, r, fx, fy } => radial_offset(x, y, cx, cy, r, fx, fy),
        };
        sample(&self.stops, spread(t, self.spread))
    }
}

/// How far `(x, y)` is from the focal point `(fx, fy)` to the circle round
/// `(cx, cy)` of radius `r`: the `t` whose circle -- the focal point moved
/// `t` of the way to the centre, radius `t * r` -- passes through the point.
fn radial_offset(x: f32, y: f32, cx: f32, cy: f32, r: f32, fx: f32, fy: f32) -> f32 {
    if r.is_nan() || r <= 0.0 {
        return 1.0;
    }
    // SVG 1.1: a focal point on or outside the circle is moved inside it.
    let (mut ex, mut ey) = (cx - fx, cy - fy);
    let distance = (ex * ex + ey * ey).sqrt();
    let limit = r * 0.999;
    let (fx, fy) = if distance > limit {
        let pull = limit / distance;
        ex *= pull;
        ey *= pull;
        (cx - ex, cy - ey)
    } else {
        (fx, fy)
    };
    // |d - t e| = t r, with d from the focal point to (x, y) and e from it
    // to the centre: a quadratic in t whose leading term is negative while
    // the focal point is inside the circle, so the root below is the one
    // that is not negative.
    let (dx, dy) = (x - fx, y - fy);
    let a = ex * ex + ey * ey - r * r;
    let de = dx * ex + dy * ey;
    let dd = dx * dx + dy * dy;
    let root = (de * de - a * dd).max(0.0).sqrt();
    if a.abs() < f32::EPSILON {
        return 1.0;
    }
    (de - root) / a
}

/// `t` folded into 0..=1 as `spread` says.
fn spread(t: f32, spread: Spread) -> f32 {
    if !t.is_finite() {
        return 1.0;
    }
    match spread {
        Spread::Pad => t.clamp(0.0, 1.0),
        Spread::Repeat => t - t.floor(),
        Spread::Reflect => {
            let folded = t.rem_euclid(2.0);
            if folded > 1.0 { 2.0 - folded } else { folded }
        }
    }
}

/// The colour `stops` give at `t` in 0..=1: the stops either side mixed as
/// SVG mixes them ([`mix`]).
fn sample(stops: &[(f32, Color)], t: f32) -> Color {
    let Some(&(first_at, first)) = stops.first() else {
        return Color::TRANSPARENT;
    };
    if t <= first_at {
        return first;
    }
    for pair in stops.windows(2) {
        let [(at0, c0), (at1, c1)] = pair else {
            continue;
        };
        if t <= *at1 {
            let span = at1 - at0;
            let share = if span > 0.0 { (t - at0) / span } else { 1.0 };
            return mix(*c0, *c1, share);
        }
    }
    stops.last().map_or(Color::TRANSPARENT, |&(_, last)| last)
}

/// `a` and `b` mixed `share` of the way to `b`, channel by channel and alpha
/// with the rest -- not premultiplied, as SVG says.
fn mix(a: Color, b: Color, share: f32) -> Color {
    let share = share.clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| {
        let (x, y) = (f32::from(x), f32::from(y));
        to_byte(x + (y - x) * share)
    };
    Color::rgba(
        channel(a.r, b.r),
        channel(a.g, b.g),
        channel(a.b, b.b),
        channel(a.a, b.a),
    )
}

/// A channel value in 0..=255, rounded.
fn to_byte(value: f32) -> u8 {
    // In 0..=255 by the clamp; `as` on a value already rounded.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "held to 0..=255 and rounded first"
    )]
    let byte = (value.clamp(0.0, 255.0) + 0.5) as u8;
    byte
}

// ============================================================================
// Collecting and resolving them
// ============================================================================

/// Every paint server in a document, by `id`.
#[derive(Clone, Debug, Default)]
pub(super) struct Defs {
    gradients: Vec<Gradient>,
    ids: HashMap<String, usize>,
}

impl Defs {
    /// Every gradient in the document `root`, resolved, with lengths in
    /// percent of the viewport `viewport` (`width, height`) where a gradient
    /// is measured in user space.
    pub(super) fn collect(root: &XmlElement, viewport: (f32, f32)) -> Self {
        let mut raw: HashMap<String, Raw> = HashMap::new();
        gather(root, &mut raw);
        let mut defs = Self::default();
        let mut ids: Vec<&String> = raw.keys().collect();
        // A stable order, so a document's gradients are numbered the same
        // way every time it is read.
        ids.sort();
        for id in ids {
            if let Some(gradient) = resolve(id, &raw, viewport) {
                defs.ids.insert(id.clone(), defs.gradients.len());
                defs.gradients.push(gradient);
            }
        }
        defs
    }

    /// The gradient numbered `index`.
    pub(super) fn gradient(&self, index: usize) -> Option<&Gradient> {
        self.gradients.get(index)
    }

    /// The paint a `url(#id)` value names -- a gradient, or what stands in
    /// for one that cannot paint (a colour for one with a single stop or no
    /// length, nothing for one with no stops) -- or else the fallback written
    /// after it, or nothing.
    pub(super) fn paint(&self, value: &str) -> SvgPaint {
        let value = value.trim();
        let Some(rest) = value.strip_prefix("url(") else {
            return parse_color(value).unwrap_or(SvgPaint::None);
        };
        let (reference, fallback) = rest.split_once(')').unwrap_or((rest, ""));
        let id = reference
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim_start_matches('#');
        if let Some(&index) = self.ids.get(id) {
            if let Some(gradient) = self.gradients.get(index) {
                return match gradient.stops.as_slice() {
                    [] => SvgPaint::None,
                    [(_, only)] => SvgPaint::Color(*only),
                    stops if degenerate(gradient.shape) => stops
                        .last()
                        .map_or(SvgPaint::None, |&(_, last)| SvgPaint::Color(last)),
                    _ => SvgPaint::Server(index),
                };
            }
        }
        let fallback = fallback.trim();
        if fallback.is_empty() {
            SvgPaint::None
        } else {
            parse_color(fallback).unwrap_or(SvgPaint::None)
        }
    }
}

/// Whether `shape` has no extent: a line of no length, a circle of no
/// radius -- which SVG paints in the last stop's colour.
fn degenerate(shape: Shape) -> bool {
    match shape {
        Shape::Linear { x1, y1, x2, y2 } => {
            (x2 - x1).abs() <= f32::EPSILON && (y2 - y1).abs() <= f32::EPSILON
        }
        Shape::Radial { r, .. } => r.is_nan() || r <= 0.0,
    }
}

/// A length a gradient attribute gives: a number, or a percentage.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Length {
    value: f32,
    percent: bool,
}

impl Length {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (number, percent) = match text.strip_suffix('%') {
            Some(number) => (number.trim(), true),
            None => (text, false),
        };
        let value = number.parse::<f32>().ok().filter(|v| v.is_finite())?;
        Some(Self { value, percent })
    }

    /// In the gradient's units: a percentage of 1 for a box, of `extent` --
    /// the viewport's width, height or diagonal measure -- in user space.
    fn resolve(self, units: Units, extent: f32) -> f32 {
        match (self.percent, units) {
            (false, _) => self.value,
            (true, Units::ObjectBoundingBox) => self.value / 100.0,
            (true, Units::UserSpaceOnUse) => self.value / 100.0 * extent,
        }
    }
}

/// One gradient element, as written: what it sets, and whom it names.
#[derive(Clone, Debug, Default)]
struct Raw {
    linear: bool,
    lengths: HashMap<&'static str, Length>,
    units: Option<Units>,
    transform: Option<Transform>,
    spread: Option<Spread>,
    stops: Vec<(f32, Color)>,
    href: Option<String>,
}

/// The attributes a gradient's geometry is written in.
const LINEAR_ATTRS: [&str; 4] = ["x1", "y1", "x2", "y2"];
const RADIAL_ATTRS: [&str; 5] = ["cx", "cy", "r", "fx", "fy"];

/// Every gradient element under `elem`, by its `id`.
fn gather(elem: &XmlElement, out: &mut HashMap<String, Raw>) {
    let tag = elem.tag.as_str();
    if tag == "linearGradient" || tag == "radialGradient" {
        if let Some(id) = elem.attr("id").map(str::trim).filter(|id| !id.is_empty()) {
            out.entry(id.to_owned())
                .or_insert_with(|| read_raw(elem, tag));
        }
    }
    for child in &elem.children {
        gather(child, out);
    }
}

/// A gradient element's own attributes and stops.
fn read_raw(elem: &XmlElement, tag: &str) -> Raw {
    let linear = tag == "linearGradient";
    let names: &[&'static str] = if linear { &LINEAR_ATTRS } else { &RADIAL_ATTRS };
    let lengths = names
        .iter()
        .filter_map(|&name| elem.attr(name).and_then(Length::parse).map(|l| (name, l)))
        .collect();
    let units = match elem.attr("gradientUnits").map(str::trim) {
        Some("userSpaceOnUse") => Some(Units::UserSpaceOnUse),
        Some("objectBoundingBox") => Some(Units::ObjectBoundingBox),
        _ => None,
    };
    let spread = match elem.attr("spreadMethod").map(str::trim) {
        Some("pad") => Some(Spread::Pad),
        Some("reflect") => Some(Spread::Reflect),
        Some("repeat") => Some(Spread::Repeat),
        _ => None,
    };
    let transform = elem
        .attr("gradientTransform")
        .and_then(|t| parse_transform(t).ok());
    let href = elem
        .attr("href")
        .or_else(|| elem.attr("xlink:href"))
        .map(|h| h.trim().trim_start_matches('#').to_owned())
        .filter(|h| !h.is_empty());
    let mut stops: Vec<(f32, Color)> = Vec::new();
    for child in &elem.children {
        if child.tag == "stop" {
            let floor = stops.last().map_or(0.0, |&(at, _)| at);
            stops.push(read_stop(child, floor));
        }
    }
    Raw {
        linear,
        lengths,
        units,
        transform,
        spread,
        stops,
        href,
    }
}

/// A `<stop>`: its offset, held to `floor..=1`, and its colour with its
/// opacity applied.
fn read_stop(stop: &XmlElement, floor: f32) -> (f32, Color) {
    let property = |name: &str| {
        stop.attr("style")
            .and_then(|style| declared(style, name))
            .or_else(|| stop.attr(name))
    };
    let offset = stop
        .attr("offset")
        .and_then(Length::parse)
        .map_or(0.0, |l| if l.percent { l.value / 100.0 } else { l.value });
    let offset = offset.clamp(0.0, 1.0).max(floor);
    let color = match property("stop-color").map(parse_color) {
        Some(Ok(SvgPaint::Color(c))) => c,
        // The initial value; `currentColor` has no colour to take here.
        _ => Color::BLACK,
    };
    let opacity = property("stop-opacity")
        .and_then(|o| o.trim().parse::<f32>().ok())
        .filter(|o| o.is_finite())
        .map_or(1.0, |o| o.clamp(0.0, 1.0));
    let alpha = to_byte(f32::from(color.a) * opacity);
    (offset, Color::rgba(color.r, color.g, color.b, alpha))
}

/// The gradient `id`, its `href` chain followed: `None` if it is not one.
fn resolve(id: &str, raw: &HashMap<String, Raw>, viewport: (f32, f32)) -> Option<Gradient> {
    let own = raw.get(id)?;
    // The chain, this gradient first, ending where a name is missing or
    // would be met a second time.
    let mut chain: Vec<&Raw> = vec![own];
    let mut seen: Vec<&str> = vec![id];
    let mut next = own.href.as_deref();
    while let Some(name) = next {
        if chain.len() >= MAX_CHAIN || seen.contains(&name) {
            break;
        }
        let Some(found) = raw.get(name) else {
            break;
        };
        chain.push(found);
        seen.push(name);
        next = found.href.as_deref();
    }
    let units = chain
        .iter()
        .find_map(|r| r.units)
        .unwrap_or(Units::ObjectBoundingBox);
    let transform = chain
        .iter()
        .find_map(|r| r.transform)
        .unwrap_or(Transform::IDENTITY);
    let spread = chain.iter().find_map(|r| r.spread).unwrap_or(Spread::Pad);
    let stops = chain
        .iter()
        .find(|r| !r.stops.is_empty())
        .map(|r| r.stops.clone())
        .unwrap_or_default();
    // Geometry only from gradients of this one's own kind.
    let length = |name: &'static str| {
        chain
            .iter()
            .filter(|r| r.linear == own.linear)
            .find_map(|r| r.lengths.get(name).copied())
    };
    let (vw, vh) = viewport;
    // SVG's measure for a length with no direction: the root of the mean
    // of the squares of the viewport's sides.
    let diagonal = f32::midpoint(vw * vw, vh * vh).sqrt();
    let at = |name: &'static str, default: f32, extent: f32| {
        length(name).map_or(default, |l| l.resolve(units, extent))
    };
    // Defaults are fractions of the box, so in user space they are the
    // same percentages of the viewport.
    let fraction = |share: f32, extent: f32| match units {
        Units::ObjectBoundingBox => share,
        Units::UserSpaceOnUse => share * extent,
    };
    let shape = if own.linear {
        Shape::Linear {
            x1: at("x1", fraction(0.0, vw), vw),
            y1: at("y1", fraction(0.0, vh), vh),
            x2: at("x2", fraction(1.0, vw), vw),
            y2: at("y2", fraction(0.0, vh), vh),
        }
    } else {
        let cx = at("cx", fraction(0.5, vw), vw);
        let cy = at("cy", fraction(0.5, vh), vh);
        Shape::Radial {
            cx,
            cy,
            r: at("r", fraction(0.5, diagonal), diagonal),
            fx: at("fx", cx, vw),
            fy: at("fy", cy, vh),
        }
    };
    Some(Gradient {
        shape,
        units,
        transform,
        spread,
        stops,
    })
}

#[cfg(test)]
#[path = "paint_tests.rs"]
mod tests;
