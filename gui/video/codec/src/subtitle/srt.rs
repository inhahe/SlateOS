//! SRT markup, written in the form ffmpeg's `srt` encoder writes it: the one
//! text form every subtitle format here comes out in.
//!
//! Each format's reader (`subrip.rs`, `ass.rs`, `webvtt.rs`) turns a cue into
//! [`Op`]s -- text, line breaks, styles switched on and off, an alignment, a
//! reset to a style -- and [`Writer`] makes the markup of them:
//!
//! - A style switched on is a tag of its own: `<i>`, `<b>`, `<u>`, `<s>`,
//!   and a `<font>` for each colour, face or size, its attribute written as
//!   ffmpeg writes it (`color="#rrggbb"`, `face="…"`, `size="16"`).
//! - A style an ASS line starts in, or is reset to, is one `<font>` holding
//!   the face, size and colour that differ from SRT's default (Arial, 16,
//!   white), then `<b>`, `<i>`, `<u>` and `<s>` as the style has them.
//! - Switching a toggle off closes the most recent tag of its kind; switching
//!   a colour, face or size off -- back to the style's -- closes every tag of
//!   its kind. Tags having to nest, every tag opened after it closes too, and
//!   is opened again before the next text: `{\i1}a{\u1}b{\i0}c` is
//!   `<i>a<u>b</u></i><u>c</u>`. Here ffmpeg reopens nothing, and its `c`
//!   loses the underline that every ASS renderer shows.
//! - The first alignment is the one written, as `{\anN}`.
//! - At the end everything still open is closed, last first.

/// `text` as SRT text: with U+2060 WORD JOINER, which shows nothing, after
/// whatever SRT's readers would otherwise read as markup -- a `{` a
/// backslash follows (an override block), a backslash `N`, `n` or `h`
/// follows (a line break, a hard space), and with `tags` every `<` (a tag).
///
/// For the text of a format whose own markup is not SRT's: WebVTT's `&lt;i>`
/// is the three characters, not italic. (ffmpeg puts the joiner after every
/// backslash, and writes `{` as ASS's `\{{}`, which only ASS renderers read.)
pub(crate) fn escape(text: &str, tags: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        let next = chars.peek().copied();
        let joins = match c {
            '{' => next == Some('\\'),
            '\\' => matches!(next, Some('N' | 'n' | 'h')),
            '<' => tags,
            _ => false,
        };
        if joins {
            out.push('\u{2060}');
        }
    }
    out
}

/// A style a tag of its own switches: `<i>`, `<b>`, `<u>`, `<s>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Toggle {
    Italic,
    Bold,
    Underline,
    Strike,
}

impl Toggle {
    const fn letter(self) -> char {
        match self {
            Self::Italic => 'i',
            Self::Bold => 'b',
            Self::Underline => 'u',
            Self::Strike => 's',
        }
    }
}

/// One step of a cue, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    /// Text, written as it is.
    Text(String),
    /// A line break.
    Break,
    /// A tag of its own on (`true`), or the most recent one off.
    Toggle(Toggle, bool),
    /// A colour, `0xRRGGBB`; or, with `None`, back to the style's.
    Colour(Option<u32>),
    /// A face; or, with `None`, back to the style's.
    Face(Option<String>),
    /// A size, in SRT's units; or, with `None`, back to the style's.
    Size(Option<u32>),
    /// An alignment, 1 to 9 as a numeric keypad lays them out.
    Align(u8),
    /// Everything off, then the style on -- or none, SRT's default.
    Reset(Option<Style>),
}

/// A style as SRT can say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Style {
    pub face: String,
    /// In SRT's units: 1/288 of the picture's height, SRT's default being 16
    /// (`DEFAULT_SIZE`).
    pub size: u32,
    /// `0xRRGGBB`.
    pub colour: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

/// SRT's own default face, size and colour, which ffmpeg's SubRip decoder
/// gives a cue: a style that has them says nothing about them.
pub(crate) const DEFAULT_FACE: &str = "Arial";
pub(crate) const DEFAULT_SIZE: u32 = 16;
pub(crate) const DEFAULT_COLOUR: u32 = 0x00FF_FFFF;

/// A tag open in the markup being written, with what it says, so that it can
/// be opened again.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Open {
    Toggle(Toggle),
    Colour(u32),
    Face(String),
    Size(u32),
    /// A style's `<font>`, its attributes as written.
    Style(String),
}

/// Which tags an [`Op`] switching something off closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Toggle(Toggle),
    Colour,
    Face,
    Size,
}

impl Open {
    fn is(&self, kind: Kind) -> bool {
        match (self, kind) {
            (Self::Toggle(a), Kind::Toggle(b)) => *a == b,
            (Self::Colour(_), Kind::Colour)
            | (Self::Face(_), Kind::Face)
            | (Self::Size(_), Kind::Size) => true,
            _ => false,
        }
    }
}

/// Makes one cue's markup of its [`Op`]s.
pub(crate) struct Writer {
    out: String,
    open: Vec<Open>,
    /// Tags closed only because a tag opened before them was, in the order
    /// they were opened: opened again before the next text.
    reopen: Vec<Open>,
    aligned: bool,
}

impl Writer {
    /// A cue with nothing open.
    pub(crate) const fn new() -> Self {
        Self {
            out: String::new(),
            open: Vec::new(),
            reopen: Vec::new(),
            aligned: false,
        }
    }

    /// Take one step.
    pub(crate) fn op(&mut self, op: Op) {
        match op {
            Op::Text(text) => {
                if !text.is_empty() {
                    self.flush_reopen();
                    self.out.push_str(&text);
                }
            }
            Op::Break => self.out.push('\n'),
            Op::Toggle(t, true) => self.push(Open::Toggle(t)),
            Op::Toggle(t, false) => self.close_newest(Kind::Toggle(t)),
            Op::Colour(Some(rgb)) => self.push(Open::Colour(rgb)),
            Op::Colour(None) => self.close_every(Kind::Colour),
            Op::Face(Some(face)) => self.push(Open::Face(face)),
            Op::Face(None) => self.close_every(Kind::Face),
            Op::Size(Some(size)) => self.push(Open::Size(size)),
            Op::Size(None) => self.close_every(Kind::Size),
            Op::Align(n) => {
                if !self.aligned && (1..=9).contains(&n) {
                    self.out.push_str(&format!("{{\\an{n}}}"));
                    self.aligned = true;
                }
            }
            Op::Reset(style) => {
                self.reopen.clear();
                self.unwind(0, |_| true);
                if let Some(style) = style {
                    self.apply(&style);
                }
            }
        }
    }

    /// The markup, everything still open closed.
    pub(crate) fn finish(mut self) -> String {
        self.reopen.clear();
        self.unwind(0, |_| true);
        self.out
    }

    fn push(&mut self, tag: Open) {
        self.write_open(&tag);
        self.open.push(tag);
    }

    /// Close the most recent tag of `kind`: one waiting to be opened again is
    /// simply not; else the newest open one, with every tag above it.
    fn close_newest(&mut self, kind: Kind) {
        if let Some(i) = self.reopen.iter().rposition(|o| o.is(kind)) {
            self.reopen.remove(i);
            return;
        }
        if let Some(at) = self.open.iter().rposition(|o| o.is(kind)) {
            self.unwind(at, |o| o.is(kind));
        }
    }

    /// Close every tag of `kind`, with every tag above the oldest of them.
    fn close_every(&mut self, kind: Kind) {
        self.reopen.retain(|o| !o.is(kind));
        if let Some(at) = self.open.iter().position(|o| o.is(kind)) {
            self.unwind(at, |o| o.is(kind));
        }
    }

    /// Close every open tag from `at` up; those `closing` does not take are
    /// opened again before the next text, ahead of any already waiting.
    ///
    /// The tag at `at` is always taken: it is the one being closed.
    fn unwind(&mut self, at: usize, closing: impl Fn(&Open) -> bool) {
        let mut kept = Vec::new();
        while self.open.len() > at {
            let Some(tag) = self.open.pop() else {
                break;
            };
            self.write_close(&tag);
            if self.open.len() > at && !closing(&tag) {
                kept.push(tag);
            }
        }
        kept.reverse();
        kept.append(&mut self.reopen);
        self.reopen = kept;
    }

    fn flush_reopen(&mut self) {
        for tag in core::mem::take(&mut self.reopen) {
            self.push(tag);
        }
    }

    fn write_open(&mut self, tag: &Open) {
        let text = match tag {
            Open::Toggle(t) => format!("<{}>", t.letter()),
            Open::Colour(rgb) => format!("<font color=\"#{rgb:06x}\">"),
            Open::Face(face) => format!("<font face=\"{face}\">"),
            Open::Size(size) => format!("<font size=\"{size}\">"),
            Open::Style(attributes) => format!("<font{attributes}>"),
        };
        self.out.push_str(&text);
    }

    fn write_close(&mut self, tag: &Open) {
        match tag {
            Open::Toggle(t) => self.out.push_str(&format!("</{}>", t.letter())),
            Open::Colour(_) | Open::Face(_) | Open::Size(_) | Open::Style(_) => {
                self.out.push_str("</font>");
            }
        }
    }

    /// Open `style`'s tags.
    fn apply(&mut self, style: &Style) {
        let mut attributes = String::new();
        if style.face != DEFAULT_FACE {
            attributes.push_str(&format!(" face=\"{}\"", style.face));
        }
        if style.size != DEFAULT_SIZE {
            attributes.push_str(&format!(" size=\"{}\"", style.size));
        }
        if style.colour != DEFAULT_COLOUR {
            attributes.push_str(&format!(" color=\"#{:06x}\"", style.colour));
        }
        if !attributes.is_empty() {
            self.push(Open::Style(attributes));
        }
        for (on, t) in [
            (style.bold, Toggle::Bold),
            (style.italic, Toggle::Italic),
            (style.underline, Toggle::Underline),
            (style.strike, Toggle::Strike),
        ] {
            if on {
                self.push(Open::Toggle(t));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(ops: Vec<Op>) -> String {
        let mut w = Writer::new();
        for op in ops {
            w.op(op);
        }
        w.finish()
    }

    fn text(s: &str) -> Op {
        Op::Text(s.to_string())
    }

    fn style() -> Style {
        Style {
            face: "Impact".to_string(),
            size: 32,
            colour: 0xFF_0000,
            bold: true,
            italic: false,
            underline: false,
            strike: true,
        }
    }

    #[test]
    fn a_tag_closed_under_another_is_opened_again_before_the_next_text() {
        let ops = vec![
            Op::Toggle(Toggle::Italic, true),
            text("open "),
            Op::Toggle(Toggle::Underline, true),
            text("both "),
            Op::Toggle(Toggle::Italic, false),
            text("closes i "),
            Op::Toggle(Toggle::Underline, false),
            text("then u"),
        ];
        assert_eq!(write(ops), "<i>open <u>both </u></i><u>closes i </u>then u");
    }

    #[test]
    fn a_tag_waiting_to_be_opened_again_that_is_switched_off_is_not() {
        // `<b><i>both</b></i>`: the italic `</b>` closed is never reopened.
        let ops = vec![
            Op::Toggle(Toggle::Bold, true),
            Op::Toggle(Toggle::Italic, true),
            text("both"),
            Op::Toggle(Toggle::Bold, false),
            Op::Toggle(Toggle::Italic, false),
            text(" after"),
        ];
        assert_eq!(write(ops), "<b><i>both</i></b> after");
    }

    #[test]
    fn a_colour_switched_off_goes_back_to_the_styles_closing_every_override() {
        let ops = vec![
            Op::Colour(Some(0x00_FF00)),
            text("green "),
            Op::Size(Some(30)),
            Op::Colour(Some(0xFF_0000)),
            text("red "),
            Op::Colour(None),
            text("reset"),
        ];
        assert_eq!(
            write(ops),
            "<font color=\"#00ff00\">green <font size=\"30\"><font color=\"#ff0000\">red \
             </font></font></font><font size=\"30\">reset</font>"
        );
    }

    #[test]
    fn switching_off_what_was_never_on_writes_nothing() {
        let ops = vec![
            Op::Toggle(Toggle::Bold, false),
            Op::Colour(None),
            Op::Face(None),
            Op::Size(None),
            text("x"),
        ];
        assert_eq!(write(ops), "x");
    }

    #[test]
    fn the_first_alignment_is_the_one_written() {
        let ops = vec![
            Op::Align(8),
            text("a"),
            Op::Align(2),
            Op::Align(0),
            text("b"),
        ];
        assert_eq!(write(ops), "{\\an8}ab");
        assert_eq!(
            write(vec![Op::Align(0), Op::Align(3), text("c")]),
            "{\\an3}c"
        );
    }

    #[test]
    fn a_style_says_only_what_differs_from_srts_default() {
        assert_eq!(
            write(vec![Op::Reset(Some(style())), text("x")]),
            "<font face=\"Impact\" size=\"32\" color=\"#ff0000\"><b><s>x</s></b></font>"
        );
        let plain = Style {
            face: DEFAULT_FACE.to_string(),
            size: DEFAULT_SIZE,
            colour: DEFAULT_COLOUR,
            bold: false,
            italic: true,
            underline: false,
            strike: false,
        };
        assert_eq!(write(vec![Op::Reset(Some(plain)), text("x")]), "<i>x</i>");
    }

    #[test]
    fn a_reset_closes_everything_and_forgets_what_waited_to_reopen() {
        let ops = vec![
            Op::Reset(Some(style())),
            Op::Toggle(Toggle::Italic, true),
            text("a"),
            Op::Toggle(Toggle::Bold, false),
            Op::Reset(None),
            text("b"),
        ];
        assert_eq!(
            write(ops),
            "<font face=\"Impact\" size=\"32\" color=\"#ff0000\"><b><s><i>a</i></s></b></font>b"
        );
    }

    #[test]
    fn text_that_reads_as_markup_is_kept_from_reading_as_it() {
        assert_eq!(
            escape("{\\an8} \\N \\h \\n \\x {x <i> a<b", true),
            "{\u{2060}\\an8} \\\u{2060}N \\\u{2060}h \\\u{2060}n \\x {x <\u{2060}i> a<\u{2060}b"
        );
        assert_eq!(escape("<i>{\\i1}", false), "<i>{\u{2060}\\i1}");
    }

    #[test]
    fn text_after_the_last_tag_closed_does_not_reopen_anything_twice() {
        // Waiting tags are opened once, then are ordinary open tags.
        let ops = vec![
            Op::Toggle(Toggle::Bold, true),
            Op::Toggle(Toggle::Italic, true),
            text("a"),
            Op::Toggle(Toggle::Bold, false),
            text("b"),
            text("c"),
        ];
        assert_eq!(write(ops), "<b><i>a</i></b><i>bc</i>");
    }
}
