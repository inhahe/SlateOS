//! The text of a rich input and its formatting: the characters, and runs of
//! them alike in weight, slant, lines and colour.
//!
//! A [`RichDoc`] is a string and a list of [`Run`]s that tile it -- each run
//! ends where the next begins, the last at the text's end -- so every byte has
//! exactly one [`Format`]. Neighbouring runs alike are always merged, so two
//! documents that read the same are the same value, and a run never ends
//! inside a character.
//!
//! # Pictures
//!
//! A picture in the text is one character, [`OBJECT`] -- the object
//! replacement character every rich text format marks an object with -- and
//! the picture itself is kept beside the text, at that character's offset.
//! So a picture is cut, copied, pasted, deleted and undone as any other
//! character is, by the same replacement of one stretch by another, and goes
//! with the stretch it is in. An [`OBJECT`] typed or pasted as text is a
//! character like any other, with no picture.

use crate::color::Color;
use crate::picture::Picture;

/// The character a picture is in the text: the object replacement
/// character.
pub const OBJECT: char = '\u{FFFC}';

/// [`OBJECT`]'s length in the text, in bytes.
const OBJECT_LEN: usize = OBJECT.len_utf8();

/// How a stretch of text is drawn: what its user set on it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Format {
    /// Bold.
    pub bold: bool,
    /// Slanted. Kept and saved; drawn upright until the font cache draws a
    /// face slanted (`requests/c-f-text-at-any-weight-and-in-italic.md`).
    pub italic: bool,
    /// A line under it.
    pub underline: bool,
    /// A line through it.
    pub strike: bool,
    /// Its colour; `None` is the field's text colour, which follows the
    /// user's theme.
    pub color: Option<Color>,
    /// Its size in pixels; `None` is the field's size.
    pub size: Option<f32>,
}

/// A stretch of text in one [`Format`], up to `end` -- a byte offset one
/// past its last byte -- from where the run before it ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    /// One past its last byte.
    pub end: usize,
    /// How it is drawn.
    pub format: Format,
}

/// A text, its formatting and its pictures.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichDoc {
    text: String,
    /// Tiling `text`, ends ascending, the last at its length; empty for an
    /// empty text.
    runs: Vec<Run>,
    /// The pictures in it, each at the offset of the [`OBJECT`] it is,
    /// offsets ascending.
    pictures: Vec<(usize, Picture)>,
}

impl RichDoc {
    /// `text` in one format. An [`OBJECT`] in it is a character, not a
    /// picture.
    #[must_use]
    pub fn plain(text: &str, format: Format) -> Self {
        let mut doc = Self {
            text: text.to_string(),
            runs: Vec::new(),
            pictures: Vec::new(),
        };
        if !text.is_empty() {
            doc.runs.push(Run {
                end: text.len(),
                format,
            });
        }
        doc
    }

    /// `picture` alone, its character in `format` -- what typing beside it
    /// carries on in.
    #[must_use]
    pub fn picture(picture: Picture, format: Format) -> Self {
        let mut doc = Self::plain(OBJECT.encode_utf8(&mut [0; 4]), format);
        doc.pictures.push((0, picture));
        doc
    }

    /// The text: each picture in it an [`OBJECT`].
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text with each picture's character taken out: what is left of it
    /// where only text can go.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let mut out = String::with_capacity(self.text.len());
        let mut from = 0;
        for &(at, _) in &self.pictures {
            out.push_str(self.text.get(from..at).unwrap_or(""));
            from = at.saturating_add(OBJECT_LEN);
        }
        out.push_str(self.text.get(from..).unwrap_or(""));
        out
    }

    /// Its pictures, each with the offset of the character it is, in
    /// order.
    #[must_use]
    pub fn pictures(&self) -> &[(usize, Picture)] {
        &self.pictures
    }

    /// The picture the character at `at` is, if it is one.
    #[must_use]
    pub fn picture_at(&self, at: usize) -> Option<&Picture> {
        self.pictures
            .binary_search_by_key(&at, |&(offset, _)| offset)
            .ok()
            .and_then(|i| self.pictures.get(i))
            .map(|(_, picture)| picture)
    }

    /// Its length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether it holds no text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The runs, in order: each from the end of the one before it.
    #[must_use]
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// Each run as its start, its end and its format, in order.
    pub fn spans(&self) -> impl Iterator<Item = (usize, usize, Format)> + '_ {
        let mut start = 0;
        self.runs.iter().map(move |run| {
            let span = (start, run.end, run.format);
            start = run.end;
            span
        })
    }

    /// The format of the character at `at` -- or, at the end, of the last
    /// one; the default for an empty text.
    #[must_use]
    pub fn format_at(&self, at: usize) -> Format {
        self.runs
            .iter()
            .find(|run| at < run.end)
            .or_else(|| self.runs.last())
            .map(|run| run.format)
            .unwrap_or_default()
    }

    /// The format text typed at `at` takes: the character's before it, as a
    /// word processor carries on in the formatting before the caret; at the
    /// start, the first character's.
    #[must_use]
    pub fn format_before(&self, at: usize) -> Format {
        let before = self
            .text
            .get(..at)
            .and_then(|s| s.chars().next_back())
            .map_or(0, |c| at.saturating_sub(c.len_utf8()));
        self.format_at(before)
    }

    /// `at`, moved back to the character boundary at or before it, and held
    /// to the text.
    fn boundary(&self, at: usize) -> usize {
        let mut at = at.min(self.text.len());
        while at > 0 && !self.text.is_char_boundary(at) {
            at = at.saturating_sub(1);
        }
        at
    }

    /// The part of it from `start` to `end`, formatting, pictures and all.
    /// Each bound is held to the text and moved back to a character
    /// boundary.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> Self {
        let (start, end) = (self.boundary(start), self.boundary(end));
        let (start, end) = (start.min(end), start.max(end));
        let mut out = Self {
            text: self.text.get(start..end).unwrap_or("").to_string(),
            runs: Vec::new(),
            // A picture's character starts on a boundary, so one starting in
            // the stretch ends in it too.
            pictures: self
                .pictures
                .iter()
                .filter(|&&(at, _)| at >= start && at < end)
                .map(|(at, picture)| (at.saturating_sub(start), picture.clone()))
                .collect(),
        };
        for (s, e, format) in self.spans() {
            let (s, e) = (s.max(start), e.min(end));
            if s < e {
                out.push_run(e.saturating_sub(start), format);
            }
        }
        out
    }

    /// Put `with` in place of the text from `start` to `end` -- formatting
    /// and all -- and answer what was there. Each bound is held to the text
    /// and moved back to a character boundary.
    pub fn replace(&mut self, start: usize, end: usize, with: &Self) -> Self {
        let (start, end) = (self.boundary(start), self.boundary(end));
        let (start, end) = (start.min(end), start.max(end));
        let removed = self.slice(start, end);
        let head = self.slice(0, start);
        let tail = self.slice(end, self.text.len());
        let mut out = head;
        out.append(with);
        out.append(&tail);
        *self = out;
        removed
    }

    /// `other` added at its end.
    pub fn append(&mut self, other: &Self) {
        let base = self.text.len();
        self.text.push_str(&other.text);
        for (_, e, format) in other.spans() {
            self.push_run(base.saturating_add(e), format);
        }
        self.pictures.extend(
            other
                .pictures
                .iter()
                .map(|(at, picture)| (base.saturating_add(*at), picture.clone())),
        );
    }

    /// Insert `text` at `at` in `format`.
    pub fn insert(&mut self, at: usize, text: &str, format: Format) {
        let at = self.boundary(at);
        self.replace(at, at, &Self::plain(text, format));
    }

    /// Change the format of the text from `start` to `end` by `change`,
    /// each run in it alike or not. Answers whether anything changed.
    pub fn apply(&mut self, start: usize, end: usize, change: impl Fn(&mut Format)) -> bool {
        let (start, end) = (self.boundary(start), self.boundary(end));
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            return false;
        }
        let before = self.runs.clone();
        let mut runs = Vec::new();
        let old = std::mem::take(&mut self.runs);
        let mut from = 0;
        for run in old {
            // Each run in up to three parts: before the range, in it, after.
            for (s, e, inside) in [
                (from, run.end.min(start), false),
                (from.max(start), run.end.min(end), true),
                (from.max(end), run.end, false),
            ] {
                if s < e {
                    let mut format = run.format;
                    if inside {
                        change(&mut format);
                    }
                    runs.push(Run { end: e, format });
                }
            }
            from = run.end;
        }
        self.runs = Vec::new();
        for run in runs {
            self.push_run(run.end, run.format);
        }
        self.runs != before
    }

    /// Whether every character from `start` to `end` passes `test` -- what
    /// a toolbar's button shows as pressed. An empty range asks the format
    /// before it.
    #[must_use]
    pub fn all(&self, start: usize, end: usize, test: impl Fn(&Format) -> bool) -> bool {
        let (start, end) = (start.min(end), start.max(end));
        if start == end {
            return test(&self.format_before(start));
        }
        self.spans()
            .filter(|&(s, e, _)| s < end && e > start)
            .all(|(_, _, f)| test(&f))
    }

    /// It as HTML -- what a mail program sends a formatted body as, and what
    /// any program that shows formatted text reads: each run in `<b>`, `<i>`,
    /// `<u>` and `<s>` as it has them, and a `<span>` for its colour and
    /// size; line breaks as `<br>`; each picture an `<img>` holding the
    /// picture itself, as a PNG (a `data:` address). The text is escaped, so
    /// nothing in it is read as markup.
    #[must_use]
    pub fn to_html(&self) -> String {
        self.to_html_with(data_url)
    }

    /// It as HTML, as [`to_html`](Self::to_html), each picture's `<img>`
    /// naming the address `src` gives it -- a mail program's `cid:` for a
    /// picture it sends as a part of the message, say. A picture `src` gives
    /// no address is left out.
    #[must_use]
    pub fn to_html_with(&self, src: impl Fn(&Picture) -> Option<String>) -> String {
        let mut out = String::new();
        for (s, e, format) in self.spans() {
            let text = self.text.get(s..e).unwrap_or("");
            let mut close = Vec::new();
            let mut style = Vec::new();
            if let Some(c) = format.color {
                style.push(format!("color:#{:02x}{:02x}{:02x}", c.r, c.g, c.b));
            }
            if let Some(size) = format.size {
                style.push(format!("font-size:{size}px"));
            }
            if !style.is_empty() {
                out.push_str(&format!("<span style=\"{}\">", style.join(";")));
                close.push("</span>");
            }
            for (on, open, end) in [
                (format.bold, "<b>", "</b>"),
                (format.italic, "<i>", "</i>"),
                (format.underline, "<u>", "</u>"),
                (format.strike, "<s>", "</s>"),
            ] {
                if on {
                    out.push_str(open);
                    close.push(end);
                }
            }
            for (i, c) in text.char_indices() {
                if let Some(picture) = self.picture_at(s.saturating_add(i)) {
                    if let Some(address) = src(picture) {
                        out.push_str(&format!(
                            "<img src=\"{}\" width=\"{}\" height=\"{}\" alt=\"\">",
                            escape_attribute(&address),
                            picture.width(),
                            picture.height()
                        ));
                    }
                    continue;
                }
                match c {
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    '&' => out.push_str("&amp;"),
                    '"' => out.push_str("&quot;"),
                    '\n' => out.push_str("<br>"),
                    c => out.push(c),
                }
            }
            for end in close.iter().rev() {
                out.push_str(end);
            }
        }
        out
    }

    /// End the formatting at `end` in `format`, merged with the run before
    /// if that is alike.
    fn push_run(&mut self, end: usize, format: Format) {
        match self.runs.last_mut() {
            Some(last) if last.format == format => last.end = end,
            Some(last) if last.end >= end => {}
            _ => self.runs.push(Run { end, format }),
        }
    }
}

/// `picture` as an address holding it -- `data:image/png;base64,` and the
/// picture as a PNG file -- or `None` for one too large for PNG.
fn data_url(picture: &Picture) -> Option<String> {
    picture
        .to_png()
        .ok()
        .map(|png| format!("data:image/png;base64,{}", base64(&png)))
}

/// `bytes` in base64 (RFC 4648's alphabet, padded with `=`).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digit = |six: u32| {
        // Six bits: an index into the 64 letters.
        let at = usize::try_from(six & 0x3F).unwrap_or(0);
        ALPHABET.get(at).map_or('A', |&b| char::from(b))
    };
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let (b0, b1, b2) = match *chunk {
            [b0, b1, b2] => (b0, Some(b1), Some(b2)),
            [b0, b1] => (b0, Some(b1), None),
            [b0] => (b0, None, None),
            _ => continue,
        };
        let n =
            (u32::from(b0) << 16) | (u32::from(b1.unwrap_or(0)) << 8) | u32::from(b2.unwrap_or(0));
        out.push(digit(n >> 18));
        out.push(digit(n >> 12));
        out.push(if b1.is_some() { digit(n >> 6) } else { '=' });
        out.push(if b2.is_some() { digit(n) } else { '=' });
    }
    out
}

/// `value` made safe inside a double-quoted HTML attribute.
fn escape_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[path = "doc_tests.rs"]
mod tests;
