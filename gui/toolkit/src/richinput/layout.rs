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
//! # Two writing directions on one line
//!
//! A line mixing right-to-left writing with left-to-right is laid out by
//! the Unicode bidirectional algorithm over its whole paragraph, as the
//! plain fields' lines are within one font: each character's level is
//! resolved once (`osfont::bidi`), a line's pieces are cut where the level
//! changes as well as where the format does, and the pieces are placed in
//! the order the levels put them on the screen (rule L2), the spaces that
//! end a line kept at the paragraph's level (rule L1). Within a piece --
//! one format, one direction -- the toolkit's own caret and click
//! arithmetic (`text::caret_x`, `text::cursor_at`) places the caret, so a
//! right-to-left piece's first character is at its right edge.
//!
//! # Pictures
//!
//! A picture is a word of its own -- a line may break before it and after
//! it -- and a piece of its own, as wide as it is shown: its own size, or
//! the box's width where it is wider, its shape kept. It stands on the
//! baseline, as a letter does, so a picture taller than the text lowers the
//! line's baseline and makes the line taller, with the text's descent kept
//! below it. A caret beside it is at one of its edges, and a click on it
//! goes to the nearer.

use osfont::bidi::{self, Base, Level};

use super::doc::{Format, OBJECT, RichDoc};
use crate::picture::Picture;
use crate::render::FontWeightHint;
use crate::text::TextCursor;

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

/// A stretch of a line in one format and one writing direction -- or one
/// picture: where it starts on the line and how wide it is.
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
    /// Whether it runs right to left: its first character at its right
    /// edge.
    pub rtl: bool,
    /// The picture it is, where it is one -- one character, its width the
    /// picture's as shown.
    pub picture: Option<Shown>,
}

/// A picture on a line, and how tall it is shown: its width is its
/// piece's.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// The picture.
    pub picture: Picture,
    /// How tall it is shown, standing on the line's baseline.
    pub height: f32,
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
    /// Its pieces, left to right on the screen -- which, on a line mixing
    /// writing directions, is not the order they are written in.
    pub pieces: Vec<Piece>,
    /// Whether its paragraph runs right to left: begins at the right.
    pub rtl: bool,
}

impl Line {
    /// Its width: where its rightmost piece ends.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.pieces
            .iter()
            .map(|p| p.x + p.width)
            .fold(0.0, f32::max)
    }

    /// Whether every piece runs left to right: a line of one direction, on
    /// which the screen's order is the text's.
    #[must_use]
    pub fn is_ltr(&self) -> bool {
        self.pieces.iter().all(|p| !p.rtl)
    }
}

/// Each character's bidirectional level in one paragraph, by its byte
/// offset in the document.
struct Levels {
    /// The paragraph's own level: 0 left to right, 1 right to left.
    base: Level,
    /// Each character's offset and level, in order; empty for a paragraph
    /// all of one direction at level 0, the common case.
    chars: Vec<(usize, Level)>,
}

impl Levels {
    /// The levels of the paragraph from `start` to `end` of `text`, its
    /// direction found from its first strong character (rules P2, P3).
    fn of(text: &str, start: usize, end: usize) -> Self {
        let para = text.get(start..end).unwrap_or("");
        if bidi::is_trivially_ltr(para) {
            return Self {
                base: 0,
                chars: Vec::new(),
            };
        }
        let chars: Vec<char> = para.chars().collect();
        let resolved = bidi::resolve(&chars, Base::Auto);
        let levels = resolved.render_levels();
        Self {
            base: resolved.level(),
            chars: para
                .char_indices()
                .zip(levels)
                .map(|((at, _), level)| (start.saturating_add(at), level))
                .collect(),
        }
    }

    /// The level of the character at byte `at`.
    fn at(&self, at: usize) -> Level {
        match self.chars.binary_search_by_key(&at, |&(offset, _)| offset) {
            Ok(i) => self.chars.get(i).map_or(self.base, |&(_, level)| level),
            Err(i) => i
                .checked_sub(1)
                .and_then(|i| self.chars.get(i))
                .map_or(self.base, |&(_, level)| level),
        }
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

/// The stretch of `doc` from `start` to `end` cut at its pictures, in
/// order: each part its start, its end, and the picture it is, where it is
/// one. A stretch with no picture is one part.
fn parts(doc: &RichDoc, start: usize, end: usize) -> Vec<(usize, usize, Option<&Picture>)> {
    let mut out = Vec::new();
    let mut from = start;
    for (at, picture) in doc
        .pictures()
        .iter()
        .filter(|&&(at, _)| at >= start && at < end)
    {
        if from < *at {
            out.push((from, *at, None));
        }
        let to = at.saturating_add(OBJECT.len_utf8()).min(end);
        out.push((*at, to, Some(picture)));
        from = to;
    }
    if from < end {
        out.push((from, end, None));
    }
    out
}

/// The width of the stretch of `doc` from `start` to `end`, which is in
/// `format`: its text measured in its font, its pictures as wide as they
/// are shown in a box `m.width` across.
fn stretch_width(doc: &RichDoc, start: usize, end: usize, format: &Format, m: &Metrics) -> f32 {
    parts(doc, start, end)
        .into_iter()
        .map(|(s, e, picture)| match picture {
            Some(picture) => picture.fitted(m.width).0,
            None => width_of(doc.text().get(s..e).unwrap_or(""), format, m.size),
        })
        .sum()
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
    let levels = Levels::of(doc.text(), start, end);
    // The line from `from` to `to`, laid out at the paragraph's levels.
    let laid = |from: usize, to: usize| line(doc, from, to, m, &levels);
    let mut lines = Vec::new();
    // The line being filled starts here; it holds a word once a word starts
    // after it.
    let mut line_start = start;
    let mut used = 0.0;
    for (ws, we) in words(doc.text(), start, end) {
        // Measured without its trailing spaces: they hang past the edge.
        let ink_end = trimmed_end(doc.text(), ws, we);
        let ink = span_width(doc, ws, ink_end, m);
        if m.wrap && line_start < ws && used + ink > m.width {
            lines.push(laid(line_start, ws));
            line_start = ws;
            used = 0.0;
        }
        // A word wider than the whole box, alone on its line: cut where it
        // stops fitting, the rest carried on to the next. (Never a picture,
        // which is shown no wider than the box.)
        if m.wrap && line_start == ws && ink > m.width {
            let mut from = ws;
            loop {
                let cut = fit_from(doc, from, ink_end, m);
                if cut >= ink_end {
                    break;
                }
                lines.push(laid(from, cut));
                from = cut;
            }
            line_start = from;
            used = span_width(doc, from, we, m);
            continue;
        }
        used += span_width(doc, ws, we, m);
    }
    lines.push(laid(line_start, end));
    lines
}

/// The words from `start` to `end`: each its letters and the spaces after
/// them, tiling the stretch. A picture's character is a word of its own, as
/// a picture in a word processor is: a line may break before it and after
/// it.
fn words(text: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let Some(stretch) = text.get(start..end) else {
        return out;
    };
    let mut word_start = 0;
    // Whether a word may start at the next letter: after a space, or after
    // a picture.
    let mut open = false;
    for (i, c) in stretch.char_indices() {
        let space = c == ' ' || c == '\t';
        let picture = c == OBJECT;
        if i > word_start && (picture || (!space && open)) {
            out.push((start.saturating_add(word_start), start.saturating_add(i)));
            word_start = i;
        }
        open = space || picture;
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

/// The width of the text from `start` to `end`, each run in its own font
/// and each picture as wide as it is shown.
fn span_width(doc: &RichDoc, start: usize, end: usize, m: &Metrics) -> f32 {
    doc.spans()
        .filter_map(|(s, e, format)| {
            let (s, e) = (s.max(start), e.min(end));
            (s < e).then(|| stretch_width(doc, s, e, &format, m))
        })
        .sum()
}

/// Where the text from `from` stops fitting in the box's width: the end of
/// the last character that fits, and never `from` itself -- a glyph wider
/// than the box still takes a line of its own.
fn fit_from(doc: &RichDoc, from: usize, end: usize, m: &Metrics) -> usize {
    let text = doc.text();
    let mut used = 0.0;
    let mut at = from;
    'runs: for (s, e, format) in doc.spans() {
        let (s, e) = (s.max(from), e.min(end));
        if s >= e {
            continue;
        }
        let (size, weight) = font_of(&format, m.size);
        for (ps, pe, picture) in parts(doc, s, e) {
            if let Some(picture) = picture {
                // Whole, or not at all.
                let width = picture.fitted(m.width).0;
                if used + width > m.width {
                    break 'runs;
                }
                used += width;
                at = pe;
                continue;
            }
            let piece = text.get(ps..pe).unwrap_or("");
            let fits = crate::text::fit(piece, m.width - used, size, weight);
            at = ps.saturating_add(fits);
            if fits < piece.len() {
                break 'runs;
            }
            used += crate::text::measure(piece, size, weight);
        }
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

/// The line from `start` to `end`, its pieces placed, at the top -- cut
/// where the format or the writing direction changes, and at each picture,
/// and placed in the order `levels` puts them on the screen.
fn line(doc: &RichDoc, start: usize, end: usize, m: &Metrics, levels: &Levels) -> Line {
    let base = m.size;
    let text = doc.text();
    // The spaces that end a line sit at the paragraph's level (rule L1), so
    // they hang past the line's end whichever way it runs.
    let trailing = trimmed_end(text, start, end);
    let level_at = |at: usize| {
        if at >= trailing {
            levels.base
        } else {
            levels.at(at)
        }
    };
    // In written order first: each stretch of one format and one level.
    let mut written: Vec<(Piece, Level)> = Vec::new();
    let mut height: f32 = 0.0;
    let mut ascent: f32 = 0.0;
    // Below the baseline: the deepest text's.
    let mut descent: f32 = 0.0;
    // The tallest picture, standing on the baseline.
    let mut tallest: Option<f32> = None;
    for (s, e, format) in doc.spans() {
        let (s, e) = (s.max(start), e.min(end));
        if s >= e {
            continue;
        }
        let (size, weight) = font_of(&format, base);
        let (line_height, text_ascent) = (
            crate::text::line_height(size, weight),
            crate::text::ascent(size, weight),
        );
        height = height.max(line_height);
        ascent = ascent.max(text_ascent);
        descent = descent.max(line_height - text_ascent);
        for (ps, pe, picture) in parts(doc, s, e) {
            if let Some(picture) = picture {
                let (width, shown) = picture.fitted(m.width);
                tallest = Some(tallest.map_or(shown, |t| t.max(shown)));
                let level = level_at(ps);
                written.push((
                    Piece {
                        start: ps,
                        end: pe,
                        x: 0.0,
                        width,
                        format,
                        rtl: level & 1 == 1,
                        picture: Some(Shown {
                            picture: picture.clone(),
                            height: shown,
                        }),
                    },
                    level,
                ));
                continue;
            }
            let mut from = ps;
            while from < pe {
                let level = level_at(from);
                let to = text
                    .get(from..pe)
                    .and_then(|rest| {
                        rest.char_indices()
                            .map(|(i, _)| from.saturating_add(i))
                            .find(|&at| level_at(at) != level)
                    })
                    .unwrap_or(pe);
                let width = crate::text::measure(text.get(from..to).unwrap_or(""), size, weight);
                written.push((
                    Piece {
                        start: from,
                        end: to,
                        x: 0.0,
                        width,
                        format,
                        // Odd levels run right to left.
                        rtl: level & 1 == 1,
                        picture: None,
                    },
                    level,
                ));
                from = to;
            }
        }
    }
    if let Some(tallest) = tallest {
        // A picture stands on the baseline: one taller than the text lowers
        // it, the text's descent still below.
        ascent = ascent.max(tallest);
        height = height.max(ascent + descent);
    }
    // Then in the order the screen shows them, left to right (rule L2).
    let order = bidi::visual_order(&written.iter().map(|(_, l)| *l).collect::<Vec<_>>());
    let mut pieces: Vec<Piece> = Vec::with_capacity(written.len());
    let mut x = 0.0;
    for i in order {
        if let Some((piece, _)) = written.get(i) {
            let mut piece = piece.clone();
            piece.x = x;
            x += piece.width;
            pieces.push(piece);
        }
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
        rtl: levels.base & 1 == 1,
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

/// The piece of `line` the caret at `at` is drawn in: the one whose
/// character it follows (`upstream`) -- where typing carries on -- or the
/// one whose character it comes before; either, where only one has it.
///
/// Where the direction changes, the two can be far apart on the screen: the
/// gap between an English word and the Hebrew after it is, at the Hebrew's
/// end, also its left edge. On a line of one direction they are the same
/// place.
fn piece_at(line: &Line, at: usize, upstream: bool) -> Option<&Piece> {
    let before = line.pieces.iter().find(|p| at > p.start && at <= p.end);
    let after = line.pieces.iter().find(|p| at >= p.start && at < p.end);
    if upstream {
        before.or(after)
    } else {
        after.or(before)
    }
}

/// How far across `piece` the caret at `at`, inside it, is drawn: from its
/// left edge, in its own direction.
fn x_in(doc: &RichDoc, piece: &Piece, at: usize, base: f32) -> f32 {
    if piece.picture.is_some() {
        // One character: before it, its leading edge; after it, its far one.
        let after = at > piece.start;
        return if after == piece.rtl { 0.0 } else { piece.width };
    }
    let text = doc.text().get(piece.start..piece.end).unwrap_or("");
    let (size, weight) = font_of(&piece.format, base);
    let into = at.saturating_sub(piece.start).min(text.len());
    if piece.rtl {
        crate::text::caret_x(text, TextCursor::from(into), size, weight)
    } else {
        // The width of what is before it: what `caret_x` gives a stretch in
        // one direction, without shaping it a second time.
        width_of(text.get(..into).unwrap_or(""), &piece.format, base)
    }
}

/// How far across its line `at` is, drawn after the character before it.
#[must_use]
pub fn x_of(doc: &RichDoc, line: &Line, at: usize, base: f32) -> f32 {
    x_at(doc, line, at, true, base)
}

/// How far across its line `at` is: after the character before it
/// (`upstream`) or before the character after it -- the same place but
/// where the writing direction changes (see `piece_at`).
#[must_use]
pub fn x_at(doc: &RichDoc, line: &Line, at: usize, upstream: bool, base: f32) -> f32 {
    match piece_at(line, at, upstream) {
        Some(piece) => piece.x + x_in(doc, piece, at, base),
        None if at <= line.start => 0.0,
        None => line.width(),
    }
}

/// Every place a caret can be on `line`, left to right: how far across it
/// is, the offset it stands for, and whether it is drawn after the
/// character before (`upstream`) -- see [`x_at`].
#[must_use]
pub fn caret_stops(doc: &RichDoc, line: &Line, base: f32) -> Vec<(f32, usize, bool)> {
    let text = doc.text();
    let mut stops: Vec<(f32, usize, bool)> = Vec::new();
    for piece in &line.pieces {
        let inside = text.get(piece.start..piece.end).unwrap_or("");
        let offsets = inside
            .char_indices()
            .map(|(i, _)| piece.start.saturating_add(i))
            .chain(core::iter::once(piece.end));
        for at in offsets {
            stops.push((piece.x + x_in(doc, piece, at, base), at, at > piece.start));
        }
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    stops
}

/// The offset nearest `x` across `line` -- the gap between characters a
/// click there means -- and whether the caret there is drawn after the
/// character before it (`upstream`). On a line of one direction that is
/// whether it is the line's end, which a wrap shares with the start of the
/// next line: a click past a line's end puts the caret at the end of that
/// line, not the start of the one below. On a line of two, it is also which
/// side of a change of direction the click was on.
#[must_use]
pub fn offset_at(doc: &RichDoc, line: &Line, x: f32, base: f32) -> (usize, bool) {
    if line.is_ltr() {
        if x <= 0.0 {
            return (line.start, false);
        }
        for piece in &line.pieces {
            if x <= piece.x + piece.width {
                let at = offset_in(doc, piece, x - piece.x, base);
                return (at, at == line.end);
            }
        }
        return (line.end, true);
    }
    // Two directions: the piece under the pointer -- or, off either end of
    // the line, the one there -- and the gap in it nearest the pointer, in
    // its own direction.
    let width = line.width();
    let x = x.clamp(0.0, width);
    let Some(piece) = line
        .pieces
        .iter()
        .find(|p| x <= p.x + p.width)
        .or_else(|| line.pieces.last())
    else {
        return (line.start, false);
    };
    let at = offset_in(doc, piece, x - piece.x, base);
    // Drawn in the piece clicked: after its character, unless the click was
    // at its very start.
    (at, at > piece.start)
}

/// The offset nearest `x` across `piece`, from its left edge: the gap
/// between its characters a click there means, in its own direction -- or,
/// on a picture, the nearer of its two edges.
fn offset_in(doc: &RichDoc, piece: &Piece, x: f32, base: f32) -> usize {
    if piece.picture.is_some() {
        let right_half = x > piece.width / 2.0;
        return if right_half == piece.rtl {
            piece.start
        } else {
            piece.end
        };
    }
    let text = doc.text().get(piece.start..piece.end).unwrap_or("");
    let (size, weight) = font_of(&piece.format, base);
    let at = crate::text::cursor_at(text, x, size, weight);
    piece.start.saturating_add(at.byte()).min(piece.end)
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
