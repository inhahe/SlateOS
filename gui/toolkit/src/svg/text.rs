//! Text: a `<text>` and the `<tspan>`s in it, laid out when the document is
//! built and drawn as the shapes of their glyphs.
//!
//! Each run of characters becomes a path -- the outlines of its glyphs, from
//! the toolkit's fonts (`crate::text::outlined`), unhinted as SVG renderers
//! draw text -- so text is filled, stroked, dashed, painted with gradients
//! and patterns, clipped, masked and filtered exactly as any shape is, by
//! the machinery that already does it. A `<text>` is a group of those paths
//! holding its own transform and style; a `<tspan>` is a group inside it.
//!
//! # What is read
//!
//! - **Where**: `x` and `y` on a `<text>` or a `<tspan>` move the pen to
//!   that point, and `dx` and `dy` move it by that much -- the first value of
//!   each (a list giving each character its own place is read as its first).
//!   A run that does not move the pen continues where the last one ended.
//! - **`text-anchor`** -- `start`, `middle`, `end` -- places each piece of
//!   text that starts at a new `x` (SVG's "text chunk") by its whole width.
//! - **The font**: `font-family`, as a CSS list, the first installed name
//!   taken (`crate::text::family_from_list`); `font-size`, a number, `px`,
//!   `pt`, `em`, `%` or a CSS keyword, 16 where none is said; `font-weight`,
//!   bold from 600 up as the toolkit draws bold. All four are inherited, from
//!   any element down -- a `<g font-size="20">` sizes the text in it.
//! - **White space**, as SVG's default `xml:space` has it: line breaks
//!   removed, tabs made spaces, runs of spaces made one, and none at the
//!   start or the end of the text. `xml:space="preserve"` keeps every space,
//!   making line breaks and tabs spaces.
//!
//! # What is not
//!
//! Lines and wrapping (`inline-size`, `<textPath>`, SVG 2's `shape-inside`),
//! vertical writing, `rotate`, `textLength`, `letter-spacing` and
//! `word-spacing`, `font-style` (the toolkit draws no italic yet), the `font`
//! shorthand, and `baseline-shift` and the other baselines -- every run sits on
//! the alphabetic baseline. A `<text>` inside a `<clipPath>` does not clip.
//!
//! # Bounds
//!
//! At most [`MAX_CHARS`] characters of one `<text>` are laid out: shaping a
//! hostile document's megabyte of text, glyph by glyph, every time it is
//! built, would be the renderer's cost and not its author's.

use crate::render::{FontFamily, FontWeightHint};
use osfont::sfnt::PathCmd;

use std::cell::RefCell;

use super::{
    Builder, MarkerRef, PathCommand, SvgError, SvgNode, SvgStyle, Transform, XmlElement,
    parse_style_attrs, parse_transform, property, viewport_length,
};

/// The most characters of one `<text>` laid out; the rest are not drawn.
pub(super) const MAX_CHARS: usize = 4096;

/// The tag the parser gives a run of characters inside a `<text>` or a
/// `<tspan>`, kept among the element's children in document order.
pub(super) const TEXT_NODE: &str = "#text";

/// Whether the parser keeps `tag`'s characters, in order among its children.
pub(super) fn holds_text(tag: &str) -> bool {
    matches!(tag, "text" | "tspan")
}

/// The font properties an element passes on to what it holds, as CSS
/// inherits them: `font-family`, `font-size`, `font-weight` and
/// `text-anchor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TextFont {
    family: FontFamily,
    /// In user units.
    size: f32,
    bold: bool,
    anchor: Anchor,
}

impl TextFont {
    /// What a document says nothing about: the UI face, 16 units -- CSS's
    /// `medium` -- regular, from the start.
    pub(super) const DEFAULT: Self = Self {
        family: FontFamily::Ui,
        size: 16.0,
        bold: false,
        anchor: Anchor::Start,
    };

    fn weight(self) -> FontWeightHint {
        if self.bold {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        }
    }
}

/// `text-anchor`: which end of a text chunk is at its `x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    Start,
    Middle,
    End,
}

/// The largest font size read, in user units: a size past it is a
/// document's mistake or its attack, and shaping at it gains nothing.
const MAX_FONT_SIZE: f32 = 10_000.0;

/// Every font an element of a document changes to, by place: what a
/// [`Builder`] names its font by, since it is copied into every frame of the
/// document's recursion and a font would multiply that frame.
pub(super) struct Fonts(RefCell<Vec<TextFont>>);

impl Fonts {
    /// The place of the font the outermost element inherits:
    /// [`TextFont::DEFAULT`].
    pub(super) const INHERITED_BY_THE_ROOT: u32 = 0;

    pub(super) fn new() -> Self {
        Self(RefCell::new(vec![TextFont::DEFAULT]))
    }

    /// The font at `place`; the default for a place there is none at.
    fn get(&self, place: u32) -> TextFont {
        usize::try_from(place)
            .ok()
            .and_then(|at| self.0.borrow().get(at).copied())
            .unwrap_or(TextFont::DEFAULT)
    }

    /// Keep `font`, and answer its place: the root's for one past what a
    /// `u32` counts, which no document reaches.
    fn add(&self, font: TextFont) -> u32 {
        let mut fonts = self.0.borrow_mut();
        let Ok(place) = u32::try_from(fonts.len()) else {
            return Self::INHERITED_BY_THE_ROOT;
        };
        fonts.push(font);
        place
    }
}

/// The font properties CSS inherits that this reads.
const FONT_PROPERTIES: [&str; 4] = ["font-family", "font-size", "font-weight", "text-anchor"];

/// The place of the font `elem` changes to, when it says any font property;
/// `None`, keeping the inherited place, when it says none -- as nearly every
/// element does. Not inlined: the font it makes would otherwise be a slot in
/// the frame of every element built.
#[inline(never)]
pub(super) fn changed_font(elem: &XmlElement, b: Builder<'_>) -> Option<u32> {
    if !FONT_PROPERTIES
        .iter()
        .any(|name| property(elem, name).is_some())
    {
        return None;
    }
    let fonts = b.tables.fonts;
    Some(fonts.add(font_of(elem, fonts.get(b.font))))
}

/// `inherited`, as `elem` says otherwise.
pub(super) fn font_of(elem: &XmlElement, inherited: TextFont) -> TextFont {
    let mut font = inherited;
    if let Some(list) = property(elem, "font-family") {
        font.family = crate::text::family_from_list(list);
    }
    if let Some(size) = property(elem, "font-size").and_then(|v| font_size(v, inherited.size)) {
        font.size = size;
    }
    if let Some(weight) = property(elem, "font-weight") {
        font.bold = bold(weight, inherited.bold);
    }
    if let Some(anchor) = property(elem, "text-anchor") {
        font.anchor = match anchor.trim() {
            "start" => Anchor::Start,
            "middle" => Anchor::Middle,
            "end" => Anchor::End,
            _ => font.anchor,
        };
    }
    font
}

/// A `font-size`: a number of user units, with `px`, `pt` or none; `em`
/// and `%` of the size inherited; or a CSS keyword. `None` for what is none
/// of these, or not a size above nought, which leaves the inherited one.
fn font_size(value: &str, inherited: f32) -> Option<f32> {
    let value = value.trim();
    let keyword = match value {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "xxx-large" => Some(48.0),
        "larger" => Some(inherited * 1.2),
        "smaller" => Some(inherited / 1.2),
        _ => None,
    };
    let size = keyword.or_else(|| {
        let (number, scale) = if let Some(n) = value.strip_suffix("px") {
            (n, 1.0)
        } else if let Some(n) = value.strip_suffix("pt") {
            (n, 4.0 / 3.0)
        } else if let Some(n) = value.strip_suffix("em") {
            (n, inherited)
        } else if let Some(n) = value.strip_suffix('%') {
            (n, inherited / 100.0)
        } else {
            (value, 1.0)
        };
        number.trim().parse::<f32>().ok().map(|n| n * scale)
    })?;
    (size.is_finite() && size > 0.0).then_some(size.min(MAX_FONT_SIZE))
}

/// Whether a `font-weight` is bold: `bold` and `bolder`, and a number from
/// 600 up, as the toolkit draws bold; `inherited` for what is not a weight.
fn bold(value: &str, inherited: bool) -> bool {
    match value.trim() {
        "bold" | "bolder" => true,
        "normal" | "lighter" => false,
        number => number
            .parse::<f32>()
            .ok()
            .filter(|w| w.is_finite())
            .map_or(inherited, |w| w >= 600.0),
    }
}

/// The `<text>` `elem`: a group of its runs' glyph shapes, with its own
/// transform and style. `b.font` names its font, its own changes in it.
pub(super) fn build(elem: &XmlElement, b: Builder<'_>) -> Result<SvgNode, SvgError> {
    let preserve = elem
        .attr("xml:space")
        .or_else(|| elem.attr("space"))
        .is_some_and(|v| v.trim() == "preserve");
    let pieces = collapsed(elem, preserve);
    let font = b.tables.fonts.get(b.font);
    // Laid out twice: once for each chunk's width, which its anchor places
    // it by, and once to build the shapes where the anchors put them.
    let mut measure = Layout::new(b.viewport, None);
    walk(elem, b, font, &pieces, &mut measure)?;
    let shifts: Vec<f32> = measure
        .chunks
        .iter()
        .map(|chunk| match chunk.anchor {
            Anchor::Start => 0.0,
            Anchor::Middle => -chunk.width / 2.0,
            Anchor::End => -chunk.width,
        })
        .collect();
    let mut draw = Layout::new(b.viewport, Some(&shifts));
    let children = walk(elem, b, font, &pieces, &mut draw)?;
    let transform = elem
        .attr("transform")
        .map(parse_transform)
        .transpose()?
        .unwrap_or(Transform::IDENTITY);
    Ok(SvgNode::Group {
        transform,
        style: parse_style_attrs(elem, b)?,
        children,
    })
}

/// One text chunk: what its anchor says, and how wide its runs came to.
struct Chunk {
    anchor: Anchor,
    width: f32,
}

/// The pen, and the chunks it has laid out.
struct Layout<'s> {
    x: f32,
    y: f32,
    /// The viewport's width and height: what a percentage `x` or `y` is of.
    viewport: (f32, f32),
    chunks: Vec<Chunk>,
    /// How far each chunk is moved by its anchor, when building; `None`
    /// while measuring.
    shifts: Option<&'s [f32]>,
    /// The next of the collapsed pieces, in document order.
    next_piece: usize,
}

impl<'s> Layout<'s> {
    fn new(viewport: (f32, f32), shifts: Option<&'s [f32]>) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            viewport,
            chunks: Vec::new(),
            shifts,
            next_piece: 0,
        }
    }

    /// Move the pen as `elem` says: `x` and `y` to a point -- an `x`
    /// starting a new chunk -- and `dx` and `dy` by an amount.
    fn place(&mut self, elem: &XmlElement, font: TextFont) {
        let (view_w, view_h) = self.viewport;
        let first = |name: &str, extent: f32| {
            elem.attr(name)
                .and_then(|list| {
                    list.split([' ', ',', '\t', '\n', '\r'])
                        .find(|v| !v.is_empty())
                })
                .and_then(|value| viewport_length(value, extent))
        };
        if let Some(x) = first("x", view_w) {
            self.x = x;
            self.chunks.push(Chunk {
                anchor: font.anchor,
                width: 0.0,
            });
        }
        if let Some(y) = first("y", view_h) {
            self.y = y;
        }
        if let Some(dx) = first("dx", view_w) {
            self.x += dx;
        }
        if let Some(dy) = first("dy", view_h) {
            self.y += dy;
        }
    }

    /// Lay out `text` at the pen in `font`: its shape, when building, and the
    /// pen moved past it either way.
    fn run(&mut self, text: &str, font: TextFont) -> Option<SvgNode> {
        if self.chunks.is_empty() {
            // Text before any `x`: a chunk starting at the pen's origin.
            self.chunks.push(Chunk {
                anchor: font.anchor,
                width: 0.0,
            });
        }
        let at = self.chunks.len().saturating_sub(1);
        let (outline, advance) = crate::text::outlined(text, font.size, font.weight(), font.family);
        let node = self.shifts.map(|shifts| {
            let shift = shifts.get(at).copied().unwrap_or(0.0);
            glyph_path(&outline, self.x + shift, self.y)
        });
        if let Some(chunk) = self.chunks.last_mut() {
            chunk.width += advance;
        }
        self.x += advance;
        node
    }
}

/// The text inside `elem` -- its own and its `<tspan>`s', in order -- laid
/// out on `layout`'s pen in `font`, and the nodes of what it draws.
fn walk(
    elem: &XmlElement,
    b: Builder<'_>,
    font: TextFont,
    pieces: &[String],
    layout: &mut Layout<'_>,
) -> Result<Vec<SvgNode>, SvgError> {
    layout.place(elem, font);
    let mut nodes = Vec::new();
    for child in &elem.children {
        if child.tag == TEXT_NODE {
            let piece = pieces.get(layout.next_piece).map_or("", String::as_str);
            layout.next_piece = layout.next_piece.saturating_add(1);
            if !piece.is_empty()
                && let Some(node) = layout.run(piece, font)
            {
                nodes.push(node);
            }
        } else if child.tag == "tspan" {
            if property(child, "display").is_some_and(|v| v == "none") {
                // Its pieces are passed over, so the next run gets its own.
                layout.next_piece = layout.next_piece.saturating_add(pieces_in(child));
                continue;
            }
            let inner = font_of(child, font);
            let children = walk(child, b, inner, pieces, layout)?;
            if layout.shifts.is_some() {
                nodes.push(SvgNode::Group {
                    transform: Transform::IDENTITY,
                    style: parse_style_attrs(child, b)?,
                    children,
                });
            }
        }
    }
    Ok(nodes)
}

/// How many runs of characters are in `elem`, its `<tspan>`s' included.
fn pieces_in(elem: &XmlElement) -> usize {
    elem.children
        .iter()
        .map(|child| match child.tag.as_str() {
            TEXT_NODE => 1,
            "tspan" => pieces_in(child),
            _ => 0,
        })
        .sum()
}

/// Every run of characters in `elem`, in document order, its white space
/// handled as `preserve` says (see the module docs), and at most
/// [`MAX_CHARS`] characters in all.
fn collapsed(elem: &XmlElement, preserve: bool) -> Vec<String> {
    let mut raw = Vec::new();
    gather(elem, &mut raw);
    let mut out = Vec::with_capacity(raw.len());
    let mut budget = MAX_CHARS;
    let mut last_was_space = true;
    for text in raw {
        let mut piece = String::new();
        for c in text.chars() {
            if budget == 0 {
                break;
            }
            let c = match c {
                '\n' | '\r' if !preserve => continue,
                '\n' | '\r' | '\t' => ' ',
                c => c,
            };
            if c == ' ' && last_was_space && !preserve {
                continue;
            }
            last_was_space = c == ' ';
            piece.push(c);
            budget = budget.saturating_sub(1);
        }
        out.push(piece);
    }
    if !preserve {
        // None at the end: the last space laid down, wherever it fell.
        if let Some(last) = out.iter_mut().rev().find(|piece| !piece.is_empty())
            && last.ends_with(' ')
        {
            last.pop();
        }
    }
    out
}

/// The characters of every run in `elem`, in order.
fn gather<'e>(elem: &'e XmlElement, into: &mut Vec<&'e str>) {
    for child in &elem.children {
        match child.tag.as_str() {
            TEXT_NODE => into.push(&child.text),
            "tspan" => gather(child, into),
            _ => {}
        }
    }
}

/// The glyph outlines `outline` -- in pixels at the font's size, `y` down,
/// from a pen at the origin -- as a path from the pen at `(x, y)`.
///
/// Its own style says no markers: a marker inherited from a group belongs on
/// the group's paths, not on every point of every letter.
fn glyph_path(outline: &[PathCmd], x: f32, y: f32) -> SvgNode {
    let commands = outline
        .iter()
        .map(|command| match command {
            PathCmd::MoveTo(p) => PathCommand::MoveTo {
                x: x + p.x,
                y: y + p.y,
            },
            PathCmd::LineTo(p) => PathCommand::LineTo {
                x: x + p.x,
                y: y + p.y,
            },
            PathCmd::QuadTo(c, p) => PathCommand::QuadraticBezier {
                x1: x + c.x,
                y1: y + c.y,
                x: x + p.x,
                y: y + p.y,
            },
            PathCmd::CurveTo(c1, c2, p) => PathCommand::CubicBezier {
                x1: x + c1.x,
                y1: y + c1.y,
                x2: x + c2.x,
                y2: y + c2.y,
                x: x + p.x,
                y: y + p.y,
            },
            PathCmd::Close => PathCommand::Close,
        })
        .collect();
    SvgNode::Path {
        commands,
        transform: Transform::IDENTITY,
        style: SvgStyle {
            marker_start: Some(MarkerRef::None),
            marker_mid: Some(MarkerRef::None),
            marker_end: Some(MarkerRef::None),
            ..SvgStyle::default()
        },
    }
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
