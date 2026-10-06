//! Laying a rich input's text out in lines: each paragraph's words wrapped
//! to the box's width, measured in the size and weight each run is drawn
//! in, and each line as tall as the tallest text on it.
//!
//! A line breaks between words, never inside one: a word whose letters are
//! in two formats -- half of it bold -- is measured whole and kept whole,
//! as a word processor keeps it. A word wider than the box is cut where it
//! stops fitting. The spaces after a word stay on its line, past its end,
//! as the plain field's do (`text::wrap_ranges`).
//!
//! Lines run left to right: a rich input lays its runs out in the order
//! they are written, so a line mixing writing directions shows each run in
//! its own direction but the runs in written order
//! (`known-issues/TD-C-A-RICH-INPUT-LAYS-ITS-RUNS-OUT-LEFT-TO-RIGHT.md`).

use super::doc::{Format, RichDoc};
use crate::render::FontWeightHint;

/// What a rich input is laid out for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The box's width, for wrapping.
    pub width: f32,
    /// The size of text whose format gives none.
    pub size: f32,
    /// Whether long lines wrap; unwrapped, a paragraph is one line.
    pub wrap: bool,
}

/// A stretch of a line in one format: where it starts on the line and how
/// wide it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// Its first byte.
    pub start: usize,
    /// One past its last.
    pub end: usize,
    /// Its left edge, from the line's start.
    pub x: f32,
    /// Its width.
    pub width: f32,
    /// Its format.
    pub format: Format,
}

/// One line on the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Its first byte.
    pub start: usize,
    /// One past its last -- its newline, where it ends a paragraph, is not
    /// in it.
    pub end: usize,
    /// Its top, from the text's.
    pub y: f32,
    /// How tall it is: its tallest text's line.
    pub height: f32,
    /// From its top to its baseline: its tallest text's.
    pub ascent: f32,
    /// Its pieces, left to right.
    pub pieces: Vec<Piece>,
}

impl Line {
    /// Its width: where its last piece ends.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.pieces.last().map_or(0.0, |p| p.x + p.width)
    }
}

/// The size and weight text in `format` is drawn at, in a field whose size
/// is `base`.
#[must_use]
pub fn font_of(format: &Format, base: f32) -> (f32, FontWeightHint) {
    let size = format
        .size
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(base);
    let weight = if format.bold {
        FontWeightHint::Bold
    } else {
        FontWeightHint::Regular
    };
    (size, weight)
}

/// The width of `text` in `format`.
fn width_of(text: &str, format: &Format, base: f32) -> f32 {
    let (size, weight) = font_of(format, base);
    crate::text::measure(text, size, weight)
}

/// `doc` laid out in lines for `m`.
#[must_use]
pub fn lay_out(doc: &RichDoc, m: &Metrics) -> Vec<Line> {
    let text = doc.text();
    let mut lines = Vec::new();
    let mut y = 0.0;
    let mut start = 0;
    loop {
        let end = text
            .get(start..)
            .and_then(|rest| rest.find('\n'))
            .map_or(text.len(), |i| start.saturating_add(i));
        for mut line in paragraph(doc, start, end, m) {
            line.y = y;
            y += line.height;
            lines.push(line);
        }
        if end >= text.len() {
            break;
        }
        // Past the newline.
        start = end.saturating_add(1);
    }
    lines
}

/// The paragraph from `start` to `end` in lines, at the top.
fn paragraph(doc: &RichDoc, start: usize, end: usize, m: &Metrics) -> Vec<Line> {
    let mut lines = Vec::new();
    // The line being filled starts here; it holds a word once a word starts
    // after it.
    let mut line_start = start;
    let mut used = 0.0;
    for (ws, we) in words(doc.text(), start, end) {
        // Measured without its trailing spaces: they hang past the edge.
        let ink_end = trimmed_end(doc.text(), ws, we);
        let ink = span_width(doc, ws, ink_end, m.size);
        if m.wrap && line_start < ws && used + ink > m.width {
            lines.push(line(doc, line_start, ws, m.size));
            line_start = ws;
            used = 0.0;
        }
        // A word wider than the whole box, alone on its line: cut where it
        // stops fitting, the rest carried on to the next.
        if m.wrap && line_start == ws && ink > m.width {
            let mut from = ws;
            loop {
                let cut = fit_from(doc, from, ink_end, m.width, m.size);
                if cut >= ink_end {
                    break;
                }
                lines.push(line(doc, from, cut, m.size));
                from = cut;
            }
            line_start = from;
            used = span_width(doc, from, we, m.size);
            continue;
        }
        used += span_width(doc, ws, we, m.size);
    }
    lines.push(line(doc, line_start, end, m.size));
    lines
}

/// The words from `start` to `end`: each its letters and the spaces after
/// them, tiling the stretch.
fn words(text: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let Some(stretch) = text.get(start..end) else {
        return out;
    };
    let mut word_start = 0;
    let mut in_space = false;
    for (i, c) in stretch.char_indices() {
        let space = c == ' ' || c == '\t';
        if !space && in_space {
            out.push((start.saturating_add(word_start), start.saturating_add(i)));
            word_start = i;
        }
        in_space = space;
    }
    if word_start < stretch.len() {
        out.push((start.saturating_add(word_start), end));
    }
    out
}

/// `end`, moved back over the spaces before it, no further than `start`.
fn trimmed_end(text: &str, start: usize, end: usize) -> usize {
    let word = text.get(start..end).unwrap_or("");
    start.saturating_add(word.trim_end_matches([' ', '\t']).len())
}

/// The width of the text from `start` to `end`, each run in its own font.
fn span_width(doc: &RichDoc, start: usize, end: usize, base: f32) -> f32 {
    doc.spans()
        .filter_map(|(s, e, format)| {
            let (s, e) = (s.max(start), e.min(end));
            (s < e).then(|| width_of(doc.text().get(s..e).unwrap_or(""), &format, base))
        })
        .sum()
}

/// Where the text from `from` stops fitting in `width`: the end of the
/// last character that fits, and never `from` itself -- a glyph wider than
/// the box still takes a line of its own.
fn fit_from(doc: &RichDoc, from: usize, end: usize, width: f32, base: f32) -> usize {
    let text = doc.text();
    let mut used = 0.0;
    let mut at = from;
    for (s, e, format) in doc.spans() {
        let (s, e) = (s.max(from), e.min(end));
        if s >= e {
            continue;
        }
        let piece = text.get(s..e).unwrap_or("");
        let (size, weight) = font_of(&format, base);
        let fits = crate::text::fit(piece, width - used, size, weight);
        at = s.saturating_add(fits);
        if fits < piece.len() {
            break;
        }
        used += crate::text::measure(piece, size, weight);
    }
    if at <= from {
        // Not even one character: take one.
        text.get(from..)
            .and_then(|rest| rest.chars().next())
            .map_or(end, |c| from.saturating_add(c.len_utf8()).min(end))
    } else {
        at
    }
}

/// The line from `start` to `end`, its pieces placed, at the top.
fn line(doc: &RichDoc, start: usize, end: usize, base: f32) -> Line {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut x = 0.0;
    let mut height: f32 = 0.0;
    let mut ascent: f32 = 0.0;
    for (s, e, format) in doc.spans() {
        let (s, e) = (s.max(start), e.min(end));
        if s >= e {
            continue;
        }
        let (size, weight) = font_of(&format, base);
        let width = crate::text::measure(doc.text().get(s..e).unwrap_or(""), size, weight);
        height = height.max(crate::text::line_height(size, weight));
        ascent = ascent.max(crate::text::ascent(size, weight));
        pieces.push(Piece {
            start: s,
            end: e,
            x,
            width,
            format,
        });
        x += width;
    }
    if pieces.is_empty() {
        // An empty line is as tall as text typed into it would be.
        let format = doc.format_before(start);
        let (size, weight) = font_of(&format, base);
        height = crate::text::line_height(size, weight);
        ascent = crate::text::ascent(size, weight);
    }
    Line {
        start,
        end,
        y: 0.0,
        height,
        ascent,
        pieces,
    }
}

/// The line `at` is on, by index. Where a wrap makes one offset both the
/// end of a line and the start of the next, it is the next's -- where
/// typing carries on -- unless `upstream` says the caret is at the end of
/// the line above, as End or a click past a line's end puts it.
#[must_use]
pub fn line_of(lines: &[Line], at: usize, upstream: bool) -> usize {
    let below = lines.iter().rposition(|l| l.start <= at).unwrap_or(0);
    let wrapped = below > 0
        && lines.get(below).is_some_and(|l| l.start == at)
        && lines
            .get(below.saturating_sub(1))
            .is_some_and(|above| above.end == at);
    if upstream && wrapped {
        below.saturating_sub(1)
    } else {
        below
    }
}

/// How far across its line `at` is.
#[must_use]
pub fn x_of(doc: &RichDoc, line: &Line, at: usize, base: f32) -> f32 {
    for piece in &line.pieces {
        if at >= piece.start && at <= piece.end {
            let before = doc.text().get(piece.start..at).unwrap_or("");
            return piece.x + width_of(before, &piece.format, base);
        }
    }
    if at <= line.start { 0.0 } else { line.width() }
}

/// The offset nearest `x` across `line` -- the gap between characters a
/// click there means -- and whether it is the line's end, which a wrap
/// shares with the start of the next line: a click past a line's end puts
/// the caret at the end of that line, not the start of the one below.
#[must_use]
pub fn offset_at(doc: &RichDoc, line: &Line, x: f32, base: f32) -> (usize, bool) {
    if x <= 0.0 {
        return (line.start, false);
    }
    for piece in &line.pieces {
        if x <= piece.x + piece.width {
            let text = doc.text().get(piece.start..piece.end).unwrap_or("");
            let (size, weight) = font_of(&piece.format, base);
            let at = crate::text::cursor_at(text, x - piece.x, size, weight);
            let at = piece.start.saturating_add(at.byte()).min(piece.end);
            return (at, at == line.end);
        }
    }
    (line.end, true)
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
