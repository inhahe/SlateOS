//! The text of a rich input and its formatting: the characters, and runs of
//! them alike in weight, slant, lines and colour.
//!
//! A [`RichDoc`] is a string and a list of [`Run`]s that tile it -- each run
//! ends where the next begins, the last at the text's end -- so every byte has
//! exactly one [`Format`]. Neighbouring runs alike are always merged, so two
//! documents that read the same are the same value, and a run never ends
//! inside a character.

use crate::color::Color;

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

/// A text and its formatting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichDoc {
    text: String,
    /// Tiling `text`, ends ascending, the last at its length; empty for an
    /// empty text.
    runs: Vec<Run>,
}

impl RichDoc {
    /// `text` in one format.
    #[must_use]
    pub fn plain(text: &str, format: Format) -> Self {
        let mut doc = Self {
            text: text.to_string(),
            runs: Vec::new(),
        };
        if !text.is_empty() {
            doc.runs.push(Run {
                end: text.len(),
                format,
            });
        }
        doc
    }

    /// The text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
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

    /// The part of it from `start` to `end`, formatting and all. Each bound
    /// is held to the text and moved back to a character boundary.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> Self {
        let (start, end) = (self.boundary(start), self.boundary(end));
        let (start, end) = (start.min(end), start.max(end));
        let mut out = Self {
            text: self.text.get(start..end).unwrap_or("").to_string(),
            runs: Vec::new(),
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
    /// size; line breaks as `<br>`. The text is escaped, so nothing in it is
    /// read as markup.
    #[must_use]
    pub fn to_html(&self) -> String {
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
            for c in text.chars() {
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

#[cfg(test)]
#[path = "doc_tests.rs"]
mod tests;
