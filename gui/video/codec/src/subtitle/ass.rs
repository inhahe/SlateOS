//! ASS and SSA -- Matroska's `S_TEXT/ASS` and `S_TEXT/SSA`, what fansubs and
//! signs come in -- said in SRT as far as SRT can say them.
//!
//! **What a line says** is what an ASS renderer shows of it: libass's reading
//! (`libass/ass_parse.c`, `ass_render.c`, `ass.c`; ISC licence), which
//! follows VSFilter's. **How it is written** is how ffmpeg's `srt` encoder
//! writes it (`srt.rs`), and where ffmpeg's conversion says what the renderer
//! shows -- most lines -- the cue is ffmpeg's own, `tests/data/ass.srt` and
//! `ssa.srt` holding the answers. Where it does not, this follows the
//! renderer, and `tests/data/generate_subtitle_fixtures.py`'s `DEPARTURES`
//! lists the cues that differ:
//!
//! - Every `{…}` is an override block, its tags read and anything else in it
//!   hidden: `{a comment}` and `{}` are not text, and a tag that does not
//!   read (`{\i0 }` reads; `{\b100}` is a light weight) does not end the
//!   line. ffmpeg prints the first two and stops reading a line at a block
//!   it cannot parse.
//! - A drawing (`{\p1}m 0 0 l 100 0{\p0}`) is a shape, not text.
//! - `\h` is a no-break space (U+00A0); `\n` is a space unless the wrap style
//!   -- the script's `WrapStyle`, or `\q` -- is 2; a tab is a space.
//! - `{\r}`, and `{\rName}` for a style the script lacks, go back to the
//!   line's own style; `{\c}`, `{\fn}`, `{\fs}`, `{\i}` and the rest without
//!   a value go back to the style in force. A line in a style the script
//!   lacks is in its "Default".
//! - `\b` takes a weight (`\b700` is bold, from 600 up as a font's weights
//!   are matched); `\fs` takes fractions, and `+`/`-` steps of a tenth of
//!   the size; a colour reads as far as its digits go (`\c&H00FF00` is
//!   green).
//! - A line's alignment is its first `\an` or `\a`, wherever in the line it
//!   stands, else its style's: an explicit `{\an2}` in a top style is bottom
//!   centre, and a reset does not move the line. It is written once, with
//!   the style's opening tags. A style's strike-out is `<s>`.
//! - A size is in SRT's units, 1/288 of the picture's height -- the height
//!   of the script ffmpeg's SubRip decoder, and every player built on it,
//!   reads SRT into. ASS's are in the script's `PlayResY` lines, and are
//!   scaled, rounded to whole units. ffmpeg writes the script's numbers
//!   unscaled, so a script laid out for 1080 lines comes out giant.
//!
//! What SRT cannot say is dropped: positions, margins, rotation, scaling,
//! spacing, borders, shadows, blur, transparency, fades, karaoke, clipping,
//! animation (`\t`), the colours other than the text's own.

use super::srt::{self, Op, Toggle};

/// The height SRT's sizes count in: ffmpeg's SubRip decoder's script.
const SRT_LINES: f64 = 288.0;

/// A script's `PlayResY` when it gives neither it nor `PlayResX`.
const DEFAULT_PLAY_RES_Y: u32 = 288;

/// libass's own "Default" style, which a script's shadows: its
/// `lookup_style_strict` finds it, and `\rDefault` resets to it, when the
/// script has none of that name.
const BUILT_IN_DEFAULT: &str = "Default";

/// The columns of a `[V4+ Styles]` section without a `Format:` line.
const ASS_STYLE_FORMAT: &[&str] = &[
    "name",
    "fontname",
    "fontsize",
    "primarycolour",
    "secondarycolour",
    "outlinecolour",
    "backcolour",
    "bold",
    "italic",
    "underline",
    "strikeout",
    "scalex",
    "scaley",
    "spacing",
    "angle",
    "borderstyle",
    "outline",
    "shadow",
    "alignment",
    "marginl",
    "marginr",
    "marginv",
    "encoding",
];

/// The columns of a `[V4 Styles]` section without a `Format:` line.
const SSA_STYLE_FORMAT: &[&str] = &[
    "name",
    "fontname",
    "fontsize",
    "primarycolour",
    "secondarycolour",
    "tertiarycolour",
    "backcolour",
    "bold",
    "italic",
    "borderstyle",
    "outline",
    "shadow",
    "alignment",
    "marginl",
    "marginr",
    "marginv",
    "alphalevel",
    "encoding",
];

/// Which numbering a header's alignments are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flavour {
    /// ASS's: a numeric keypad's.
    Ass,
    /// SSA's: 1 to 3 along the bottom, 5 to 7 along the top, 9 to 11 in the
    /// middle.
    Ssa,
}

/// What a track's header (its `CodecPrivate`) says that SRT can use.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Script {
    styles: Vec<Style>,
    /// The height the script's sizes are in.
    play_res_y: u32,
    /// `WrapStyle`: at 2, `\n` breaks the line.
    wrap_style: i32,
}

/// A style from the header.
#[derive(Clone, Debug, PartialEq)]
struct Style {
    name: String,
    face: String,
    /// In the script's lines.
    size: f64,
    /// `0xRRGGBB`.
    colour: u32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    /// 1 to 9 as a numeric keypad lays them out.
    alignment: u8,
}

impl Script {
    /// Read a header: its `[Script Info]`, and its styles.
    ///
    /// A line that is not UTF-8 is passed over rather than guessed at: a
    /// Matroska text track's header is UTF-8 by the specification.
    pub(crate) fn parse(header: &[u8]) -> Self {
        let header = header.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(header);
        let mut styles = Vec::new();
        let (mut play_res_x, mut play_res_y, mut wrap_style) = (0i32, 0i32, 0i32);
        let mut flavour = Flavour::Ass;
        let mut format: Option<Vec<String>> = None;
        let mut section = Section::Other;
        for line in header.split(|&b| b == b'\n') {
            let Ok(line) = core::str::from_utf8(line) else {
                continue;
            };
            let line = line.strip_suffix('\r').unwrap_or(line);
            let line = line.trim_start_matches([' ', '\t']);
            if starts_with_ignoring_case(line, "[Script Info]") {
                section = Section::Info;
            } else if starts_with_ignoring_case(line, "[V4 Styles]") {
                section = Section::Styles;
                flavour = Flavour::Ssa;
            } else if starts_with_ignoring_case(line, "[V4+ Styles]") {
                section = Section::Styles;
                flavour = Flavour::Ass;
            } else if line.starts_with('[') {
                section = Section::Other;
            } else if section == Section::Info {
                if let Some(v) = line.strip_prefix("PlayResX:") {
                    play_res_x = header_int(v) as i32;
                } else if let Some(v) = line.strip_prefix("PlayResY:") {
                    play_res_y = header_int(v) as i32;
                } else if let Some(v) = line.strip_prefix("WrapStyle:") {
                    wrap_style = header_int(v) as i32;
                } else if let Some(v) = line.strip_prefix("ScriptType:") {
                    if let Some(f) = script_type(v) {
                        flavour = f;
                    }
                }
            } else if section == Section::Styles {
                if let Some(columns) = line.strip_prefix("Format:") {
                    format = Some(
                        columns
                            .split(',')
                            .map(|c| c.trim_matches([' ', '\t']).to_ascii_lowercase())
                            .collect(),
                    );
                } else if let Some(values) = line.strip_prefix("Style:") {
                    let defaults = match flavour {
                        Flavour::Ass => ASS_STYLE_FORMAT,
                        Flavour::Ssa => SSA_STYLE_FORMAT,
                    };
                    let columns: Vec<&str> = match &format {
                        Some(f) => f.iter().map(String::as_str).collect(),
                        None => defaults.to_vec(),
                    };
                    styles.push(style(&columns, values, flavour));
                }
            }
        }
        Self {
            styles,
            play_res_y: resolve_play_res_y(play_res_x, play_res_y),
            wrap_style,
        }
    }

    /// The style a line names, as libass finds it (`ass_lookup_style`):
    /// leading `*`s dropped, "default" in any case read as "Default", the
    /// last style of the name; else the script's "Default"; else none --
    /// libass's own default, SRT's.
    fn line_style(&self, name: &str) -> Option<&Style> {
        let name = name.trim_start_matches('*');
        let name = if name.eq_ignore_ascii_case("Default") {
            "Default"
        } else {
            name
        };
        self.styles
            .iter()
            .rev()
            .find(|s| s.name == name)
            .or_else(|| self.styles.iter().rev().find(|s| s.name == "Default"))
    }

    /// A size in the script's lines, in SRT's: scaled, rounded, at least 1.
    fn srt_size(&self, size: f64) -> u32 {
        let scaled = (size * SRT_LINES / f64::from(self.play_res_y.max(1))).round();
        if scaled.is_nan() || scaled < 1.0 {
            1
        } else if scaled > f64::from(u16::MAX) {
            u32::from(u16::MAX)
        } else {
            scaled as u32
        }
    }

    /// SRT's default size, in the script's lines.
    fn default_size(&self) -> f64 {
        f64::from(srt::DEFAULT_SIZE) * f64::from(self.play_res_y.max(1)) / SRT_LINES
    }

    /// `style` as SRT says it.
    fn srt_style(&self, style: &Style) -> srt::Style {
        srt::Style {
            face: style.face.clone(),
            size: if style.size > 0.0 {
                self.srt_size(style.size)
            } else {
                srt::DEFAULT_SIZE
            },
            colour: style.colour,
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            strike: style.strike,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Info,
    Styles,
    Other,
}

fn starts_with_ignoring_case(line: &str, prefix: &str) -> bool {
    line.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// `ScriptType`'s value: `v4.00` for SSA, `v4.00+` for ASS, as libass reads
/// it (from the end, the `v` not required).
fn script_type(value: &str) -> Option<Flavour> {
    let value = value.trim_end_matches([' ', '\t']);
    let (value, flavour) = match value.strip_suffix('+') {
        Some(v) => (v, Flavour::Ass),
        None => (value, Flavour::Ssa),
    };
    value.ends_with("4.00").then_some(flavour)
}

/// libass's `ass_lazy_track_init`: a `PlayResY` the script does not give is
/// worked out from its `PlayResX`, or is 288 without either.
fn resolve_play_res_y(x: i32, y: i32) -> u32 {
    if y > 0 {
        return y.unsigned_abs();
    }
    if x <= 0 {
        return DEFAULT_PLAY_RES_Y;
    }
    if x == 1280 {
        return 1024;
    }
    let x = x.unsigned_abs().saturating_sub(1);
    x.saturating_sub(x / 4).max(1)
}

/// One `Style:` line's values, by `columns`: each value's leading spaces
/// dropped, the last running to the end of the line (libass's
/// `process_style`).
fn style(columns: &[&str], values: &str, flavour: Flavour) -> Style {
    let mut style = Style {
        name: String::new(),
        face: String::new(),
        size: 0.0,
        colour: 0,
        bold: false,
        italic: false,
        underline: false,
        strike: false,
        alignment: 2,
    };
    let mut rest = values;
    for column in columns {
        let value = rest.trim_start_matches([' ', '\t']);
        if value.is_empty() {
            break;
        }
        let (token, after) = value.split_once(',').unwrap_or((value, ""));
        rest = after;
        let token = token.trim_end_matches([' ', '\t']);
        match *column {
            "name" => style.name = token.trim_start_matches('*').to_owned(),
            "fontname" => style.face = token.to_owned(),
            "fontsize" => style.size = strtod(token),
            "primarycolour" => style.colour = rgb(header_int(token)),
            "bold" => style.bold = header_int(token) != 0,
            "italic" => style.italic = header_int(token) != 0,
            "underline" => style.underline = header_int(token) != 0,
            "strikeout" => style.strike = header_int(token) != 0,
            "alignment" => {
                let n = header_int(token) as i32;
                style.alignment = match flavour {
                    Flavour::Ass => numpad(n),
                    // VSFilter's reading of SSA's 8 and 4, which have no place.
                    Flavour::Ssa => keypad_of_ssa(match n {
                        8 => 3,
                        4 => 11,
                        n => n,
                    }),
                };
            }
            _ => {}
        }
    }
    if style.name.is_empty() {
        style.name = "Default".to_owned();
    }
    // A vertical font (`@MS Gothic`) is its face on its side, which SRT
    // cannot do: the face is named, upright.
    if let Some(face) = style.face.strip_prefix('@') {
        style.face = face.to_owned();
    }
    if style.face.is_empty() {
        style.face = srt::DEFAULT_FACE.to_owned();
    }
    style
}

/// An ASS style's alignment as libass's `numpad2align` reads one out of
/// range (`10` is top left), on a numeric keypad.
fn numpad(n: i32) -> u8 {
    let n = if n < -i32::MAX { 2 } else { n.saturating_abs() };
    if (1..=9).contains(&n) {
        return u8::try_from(n).unwrap_or(2);
    }
    // ((n - 1) % 3) + 1 across, then the row: what libass makes of the rest.
    let across = n
        .saturating_sub(1)
        .checked_rem(3)
        .map_or(0, |r| r.saturating_add(1));
    if across < 1 {
        return 2;
    }
    let row = if n <= 3 {
        0
    } else if n <= 6 {
        3
    } else {
        6
    };
    u8::try_from(across.saturating_add(row)).unwrap_or(2)
}

/// SSA's numbering on a numeric keypad: the low two bits across (1 left,
/// 2 centre, 3 right), then 4 for the top row or 8 for the middle.
fn keypad_of_ssa(n: i32) -> u8 {
    let Ok(n) = u8::try_from(n) else {
        return 2;
    };
    let across = n & 3;
    if across == 0 {
        return 2;
    }
    let row = if n & 4 != 0 {
        6
    } else if n & 8 != 0 {
        3
    } else {
        0
    };
    across.saturating_add(row)
}

/// ASS's colour, `0xAABBGGRR` as written, as `0xRRGGBB`.
fn rgb(abgr: u32) -> u32 {
    let [r, g, b, _] = abgr.to_le_bytes();
    u32::from_be_bytes([0, r, g, b])
}

/// libass's `parse_int_header`: `&H` or `0x` and hexadecimal, else decimal,
/// a sign allowed, wrapping as VSFilter's `scanf` wraps; 0 for nothing.
fn header_int(s: &str) -> u32 {
    let hex = s
        .get(..2)
        .is_some_and(|p| p.eq_ignore_ascii_case("&h") || p.eq_ignore_ascii_case("0x"));
    let (s, base) = if hex {
        (s.get(2..).unwrap_or(""), 16)
    } else {
        (s, 10)
    };
    let s = s.trim_start_matches([' ', '\t']);
    let (negative, s) = split_sign(s);
    let s = if base == 16 { strip_0x(s) } else { s };
    let mut value = 0u32;
    for c in s.chars() {
        let Some(d) = c.to_digit(base) else {
            break;
        };
        value = value.wrapping_mul(base).wrapping_add(d);
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// C's `strtoll`, clamped to `i32`, as libass's `mystrtoi32` calls it:
/// white space, a sign, `0x` in base 16, digits as far as they go; 0 for
/// none.
fn strtoi32(s: &str, base: u32) -> i32 {
    let s = s.trim_start_matches(is_c_space);
    let (negative, s) = split_sign(s);
    let s = if base == 16 { strip_0x(s) } else { s };
    let mut value = 0i64;
    for c in s.chars() {
        let Some(d) = c.to_digit(base) else {
            break;
        };
        value = value
            .saturating_mul(i64::from(base))
            .saturating_add(i64::from(d));
    }
    let value = if negative {
        value.saturating_neg()
    } else {
        value
    };
    i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or(0)
}

/// libass's `ass_strtod`: white space, a sign, digits with a fraction, an
/// exponent; 0 for no digits.
fn strtod(s: &str) -> f64 {
    let s = s.trim_start_matches(is_c_space);
    let bytes = s.as_bytes();
    let digits_from = |from: usize| {
        bytes
            .get(from..)
            .map_or(0, |b| b.iter().take_while(|c| c.is_ascii_digit()).count())
    };
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let whole = digits_from(end);
    end = end.saturating_add(whole);
    let mut digits = whole;
    if bytes.get(end) == Some(&b'.') {
        let fraction = digits_from(end.saturating_add(1));
        if whole > 0 || fraction > 0 {
            end = end.saturating_add(1).saturating_add(fraction);
            digits = digits.saturating_add(fraction);
        }
    }
    if digits == 0 {
        return 0.0;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(
            bytes.get(end.saturating_add(1)),
            Some(b'+' | b'-')
        ));
        let exponent = digits_from(end.saturating_add(1).saturating_add(sign));
        if exponent > 0 {
            end = end
                .saturating_add(1)
                .saturating_add(sign)
                .saturating_add(exponent);
        }
    }
    s.get(..end)
        .and_then(|n| n.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn split_sign(s: &str) -> (bool, &str) {
    match s.as_bytes().first() {
        Some(b'-') => (true, s.get(1..).unwrap_or("")),
        Some(b'+') => (false, s.get(1..).unwrap_or("")),
        _ => (false, s),
    }
}

fn strip_0x(s: &str) -> &str {
    s.strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s)
}

const fn is_c_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// An event's style and text, from its Matroska packet: `ReadOrder, Layer,
/// Style, Name, MarginL, MarginR, MarginV, Effect, Text`, the text taking
/// the rest, commas and all.
pub(crate) fn event(packet: &str) -> Option<(&str, &str)> {
    let mut fields = packet.splitn(9, ',');
    let _read_order = fields.next()?;
    let _layer = fields.next()?;
    let style = fields.next()?;
    for _ in 0..5 {
        fields.next()?;
    }
    let text = fields.next()?;
    Some((
        style.trim_matches([' ', '\t']),
        text.trim_end_matches(['\r', '\n']),
    ))
}

/// One event's text as SRT markup; `None` for a packet that is not an
/// event.
pub(crate) fn cue(script: &Script, packet: &str) -> Option<String> {
    let (style, text) = event(packet)?;
    let mut line = Line::new(script, script.line_style(style));
    line.text(text);
    Some(line.finish())
}

/// What a tag is, among those that begin alike: matched in libass's order,
/// so that `\fsp` is spacing, not a size, and `\be` a blur, not bold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Size,
    Face,
    Align,
    LegacyAlign,
    Colour,
    Reset,
    Bold,
    Italic,
    Strike,
    Underline,
    Drawing,
    Wrap,
    Other,
}

/// `ass_parse_tags`'s tags, in its order.
const TAGS: &[(&str, Tag)] = &[
    ("xbord", Tag::Other),
    ("ybord", Tag::Other),
    ("xshad", Tag::Other),
    ("yshad", Tag::Other),
    ("fax", Tag::Other),
    ("fay", Tag::Other),
    ("iclip", Tag::Other),
    ("blur", Tag::Other),
    ("fscx", Tag::Other),
    ("fscy", Tag::Other),
    ("fsc", Tag::Other),
    ("fsp", Tag::Other),
    ("fs", Tag::Size),
    ("bord", Tag::Other),
    ("move", Tag::Other),
    ("frx", Tag::Other),
    ("fry", Tag::Other),
    ("frz", Tag::Other),
    ("fr", Tag::Other),
    ("fn", Tag::Face),
    ("alpha", Tag::Other),
    ("an", Tag::Align),
    ("a", Tag::LegacyAlign),
    ("pos", Tag::Other),
    ("fade", Tag::Other),
    ("fad", Tag::Other),
    ("org", Tag::Other),
    ("t", Tag::Other),
    ("clip", Tag::Other),
    ("c", Tag::Colour),
    ("1c", Tag::Colour),
    ("2c", Tag::Other),
    ("3c", Tag::Other),
    ("4c", Tag::Other),
    ("1a", Tag::Other),
    ("2a", Tag::Other),
    ("3a", Tag::Other),
    ("4a", Tag::Other),
    ("r", Tag::Reset),
    ("be", Tag::Other),
    ("b", Tag::Bold),
    ("i", Tag::Italic),
    ("kt", Tag::Other),
    ("kf", Tag::Other),
    ("K", Tag::Other),
    ("ko", Tag::Other),
    ("k", Tag::Other),
    ("shad", Tag::Other),
    ("s", Tag::Strike),
    ("u", Tag::Underline),
    ("pbo", Tag::Other),
    ("p", Tag::Drawing),
    ("q", Tag::Wrap),
    ("fe", Tag::Other),
];

/// One event being read: what is in force, and the steps so far.
struct Line<'a> {
    script: &'a Script,
    /// The line's own style; `None` for SRT's default.
    own: Option<&'a Style>,
    /// The style in force: the line's own, or the one `\r` last named.
    base: Option<&'a Style>,
    italic: bool,
    bold: bool,
    underline: bool,
    strike: bool,
    /// The size in force, in the script's lines.
    size: f64,
    wrap_style: i32,
    drawing: bool,
    aligned: Aligned,
    ops: Vec<Op>,
    /// Text not yet a step.
    plain: String,
}

/// What has decided a line's alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aligned {
    /// No `\an` or `\a` yet: the line's own style.
    ByStyle,
    /// The first `\an` or `\a`, as this.
    ByTag(u8),
    /// The first `\an` or `\a`, as bottom centre: SRT's own, not written.
    Unwritten,
}

impl<'a> Line<'a> {
    fn new(script: &'a Script, own: Option<&'a Style>) -> Self {
        let mut line = Self {
            script,
            own,
            base: None,
            italic: false,
            bold: false,
            underline: false,
            strike: false,
            size: 0.0,
            wrap_style: script.wrap_style,
            drawing: false,
            aligned: Aligned::ByStyle,
            ops: Vec::new(),
            plain: String::new(),
        };
        line.reset(own);
        line
    }

    /// Everything back to `style`'s.
    fn reset(&mut self, style: Option<&'a Style>) {
        self.base = style;
        self.italic = style.is_some_and(|s| s.italic);
        self.bold = style.is_some_and(|s| s.bold);
        self.underline = style.is_some_and(|s| s.underline);
        self.strike = style.is_some_and(|s| s.strike);
        self.size = self.base_size();
        self.ops
            .push(Op::Reset(style.map(|s| self.script.srt_style(s))));
    }

    /// The style in force's size, in the script's lines.
    fn base_size(&self) -> f64 {
        match self.base {
            Some(s) if s.size > 0.0 => s.size,
            _ => self.script.default_size(),
        }
    }

    /// The text's steps, as libass's `parse_events` walks it.
    fn text(&mut self, text: &str) {
        // ffmpeg drops the spaces a line starts with; libass drops them
        // when it lays the line out.
        let mut rest = text.trim_start_matches([' ', '\t']);
        while let Some(c) = rest.chars().next() {
            let after = rest.get(c.len_utf8()..).unwrap_or("");
            if c == '{'
                && let Some(end) = after.find('}')
            {
                self.flush();
                self.block(after.get(..end).unwrap_or(""));
                rest = after.get(end.saturating_add(1)..).unwrap_or("");
                continue;
            }
            if self.drawing {
                // A drawing's commands run to the next block.
                rest = after
                    .get(after.find('{').unwrap_or(after.len())..)
                    .unwrap_or("");
                continue;
            }
            match c {
                '\\' => {
                    let escaped = match after.chars().next() {
                        Some('N') => Some(None),
                        Some('n') if self.wrap_style == 2 => Some(None),
                        Some('n') => Some(Some(' ')),
                        Some('h') => Some(Some('\u{a0}')),
                        Some('{') => Some(Some('{')),
                        Some('}') => Some(Some('}')),
                        _ => None,
                    };
                    match escaped {
                        Some(Some(ch)) => self.plain.push(ch),
                        Some(None) => self.line_break(),
                        None => self.plain.push('\\'),
                    }
                    if escaped.is_some() {
                        rest = after.get(1..).unwrap_or("");
                        continue;
                    }
                }
                '\t' => self.plain.push(' '),
                '\n' => self.line_break(),
                // Never seen in an event; a line's end, not text.
                '\r' => {}
                _ => self.plain.push(c),
            }
            rest = after;
        }
    }

    fn line_break(&mut self) {
        self.flush();
        self.ops.push(Op::Break);
    }

    /// Text so far, kept from reading as SRT's override blocks (`\{\i1\}`
    /// is text). HTML's tags in a line are left as SRT reads them, as ffmpeg
    /// leaves them: an ASS renderer shows `<i>` as text, but a line holding
    /// one is a converted SRT's slip far more often than a line meant to
    /// show it.
    fn flush(&mut self) {
        if !self.plain.is_empty() {
            let text = srt::escape(&core::mem::take(&mut self.plain), false);
            self.ops.push(Op::Text(text));
        }
    }

    /// One override block's tags, as libass's `ass_parse_tags` finds them:
    /// each from a backslash (spaces after it skipped) to the next `(` or
    /// backslash, its parenthesised arguments after it; what is not a tag is
    /// passed over.
    fn block(&mut self, content: &str) {
        let bytes = content.as_bytes();
        let end = bytes.len();
        let mut p = 0usize;
        while p < end {
            let Some(at) = content.get(p..).and_then(|s| s.find('\\')) else {
                break;
            };
            p = p.saturating_add(at).saturating_add(1);
            while matches!(bytes.get(p), Some(b' ' | b'\t')) {
                p = p.saturating_add(1);
            }
            let name_end = content
                .get(p..)
                .and_then(|s| s.find(['(', '\\']))
                .map_or(end, |i| p.saturating_add(i));
            if name_end == p {
                continue;
            }
            let mut q = name_end;
            let mut first_argument: Option<&str> = None;
            if bytes.get(q) == Some(&b'(') {
                q = q.saturating_add(1);
                loop {
                    while matches!(bytes.get(q), Some(b' ' | b'\t')) {
                        q = q.saturating_add(1);
                    }
                    let mut r = content
                        .get(q..)
                        .and_then(|s| s.find([',', '\\', ')']))
                        .map_or(end, |i| q.saturating_add(i));
                    let comma = bytes.get(r) == Some(&b',');
                    if bytes.get(r) == Some(&b'\\') {
                        // An argument holding tags (`\t(\fs20)`) runs to the
                        // next `)`.
                        r = content
                            .get(r..)
                            .and_then(|s| s.find(')'))
                            .map_or(end, |i| r.saturating_add(i));
                    }
                    let argument = content
                        .get(q..r)
                        .unwrap_or("")
                        .trim_end_matches([' ', '\t']);
                    if first_argument.is_none() && !argument.is_empty() {
                        first_argument = Some(argument);
                    }
                    q = if r < end { r.saturating_add(1) } else { end };
                    if !comma {
                        break;
                    }
                }
            }
            self.tag(content.get(p..name_end).unwrap_or(""), first_argument);
            p = q;
        }
    }

    /// One tag: its name and what follows it to the next `(` or backslash,
    /// and its first parenthesised argument if it has one, which libass
    /// reads before the other.
    fn tag(&mut self, tag: &str, parenthesised: Option<&str>) {
        let Some(&(name, kind)) = TAGS.iter().find(|(name, _)| tag.starts_with(name)) else {
            return;
        };
        let trailing = tag
            .get(name.len()..)
            .unwrap_or("")
            .trim_end_matches([' ', '\t']);
        let argument = parenthesised.or((!trailing.is_empty()).then_some(trailing));
        let base = self.base;
        match kind {
            Tag::Size => self.size_tag(argument),
            Tag::Face => {
                let face = argument
                    .filter(|&a| a != "0")
                    .map(|a| a.trim_start_matches([' ', '\t']))
                    .map(|a| a.strip_prefix('@').unwrap_or(a).to_owned());
                self.ops.push(Op::Face(face));
            }
            Tag::Colour => {
                self.ops.push(Op::Colour(argument.map(colour_tag)));
            }
            Tag::Align => {
                let n = strtoi32(argument.unwrap_or(""), 10);
                let n = (1..=9).contains(&n).then(|| u8::try_from(n).unwrap_or(2));
                self.align(n);
            }
            Tag::LegacyAlign => {
                let n = strtoi32(argument.unwrap_or(""), 10);
                // VSFilter's: SSA's 4 and 8, which have no place, are 5.
                let n = (1..=11)
                    .contains(&n)
                    .then(|| keypad_of_ssa(if n & 3 == 0 { 5 } else { n }));
                self.align(n);
            }
            Tag::Reset => {
                let to = match argument {
                    None => self.own,
                    Some(name) => match self.script.styles.iter().rev().find(|s| s.name == name) {
                        Some(s) => Some(s),
                        None if name == BUILT_IN_DEFAULT => None,
                        None => self.own,
                    },
                };
                self.flush();
                self.reset(to);
            }
            Tag::Bold => {
                let on = match argument.map(|a| strtoi32(a, 10)) {
                    Some(0) => false,
                    Some(1) => true,
                    Some(weight) if weight >= 100 => weight >= 600,
                    _ => base.is_some_and(|s| s.bold),
                };
                self.toggle(Toggle::Bold, on);
            }
            Tag::Italic | Tag::Underline | Tag::Strike => {
                let (t, style_has) = match kind {
                    Tag::Italic => (Toggle::Italic, base.is_some_and(|s| s.italic)),
                    Tag::Underline => (Toggle::Underline, base.is_some_and(|s| s.underline)),
                    _ => (Toggle::Strike, base.is_some_and(|s| s.strike)),
                };
                let on = match argument.map(|a| strtoi32(a, 10)) {
                    Some(0) => false,
                    Some(1) => true,
                    _ => style_has,
                };
                self.toggle(t, on);
            }
            Tag::Drawing => {
                self.drawing = strtoi32(argument.unwrap_or(""), 10) > 0;
            }
            Tag::Wrap => {
                self.wrap_style = match argument.map(|a| strtoi32(a, 10)) {
                    Some(n @ 0..=3) => n,
                    _ => self.script.wrap_style,
                };
            }
            Tag::Other => {}
        }
    }

    /// `\fs`: a size, `+`/`-` a step of a tenth of the one in force, or --
    /// without one, or for none above 0 -- the style's.
    fn size_tag(&mut self, argument: Option<&str>) {
        let size = argument.map(|a| {
            let v = strtod(a);
            if a.starts_with(['+', '-']) {
                self.size * (1.0 + v / 10.0)
            } else {
                v
            }
        });
        match size {
            Some(size) if size > 0.0 && size.is_finite() => {
                self.size = size;
                self.ops.push(Op::Size(Some(self.script.srt_size(size))));
            }
            _ => {
                self.size = self.base_size();
                self.ops.push(Op::Size(None));
            }
        }
    }

    /// The line's first `\an` or `\a`: `n`, or -- for a value that is not
    /// one -- the style in force's. Any after it are passed over.
    fn align(&mut self, n: Option<u8>) {
        if self.aligned == Aligned::ByStyle {
            let style = self.base.map(|s| s.alignment).filter(|&a| a != 2);
            self.aligned = n.or(style).map_or(Aligned::Unwritten, Aligned::ByTag);
        }
    }

    fn toggle(&mut self, t: Toggle, on: bool) {
        let now = match t {
            Toggle::Italic => &mut self.italic,
            Toggle::Bold => &mut self.bold,
            Toggle::Underline => &mut self.underline,
            Toggle::Strike => &mut self.strike,
        };
        if *now != on {
            *now = on;
            self.ops.push(Op::Toggle(t, on));
        }
    }

    /// The markup. The line's alignment -- a tag's, else its own style's --
    /// is the line's wherever the tag stands, and is written with the
    /// style's opening tags, where ffmpeg writes a style's.
    fn finish(mut self) -> String {
        self.flush();
        let alignment = match self.aligned {
            Aligned::ByStyle => self.own.map(|s| s.alignment).filter(|&a| a != 2),
            Aligned::ByTag(n) => Some(n),
            Aligned::Unwritten => None,
        };
        if let Some(n) = alignment {
            let at = self.ops.len().min(1);
            self.ops.insert(at, Op::Align(n));
        }
        let mut w = srt::Writer::new();
        for op in self.ops {
            w.op(op);
        }
        w.finish()
    }
}

/// libass's `parse_color_tag`: `&` and `H` skipped, then hexadecimal as far
/// as it goes (clamped as `strtol` clamps), as `0xRRGGBB`.
fn colour_tag(argument: &str) -> u32 {
    let digits = argument.trim_start_matches(['&', 'H']);
    rgb(strtoi32(digits, 16) as u32)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "tests over fixed inputs")]
mod tests {
    use super::*;

    const HEADER: &str = "[Script Info]\nScriptType: v4.00+\nPlayResY: 288\n\n[V4+ Styles]\n\
        Format: Name, Fontname, Fontsize, PrimaryColour, Bold, Italic, Underline, StrikeOut, Alignment\n\
        Style: Default,Arial,16,&H00FFFFFF,0,0,0,0,2\n\
        Style: Lean,Georgia,24.5,&H0000FF00,0,-1,1,0,5\n\
        Style: Top,Arial,16,&H00FFFFFF,1,0,0,0,8\n";

    fn srt(style: &str, text: &str) -> String {
        let script = Script::parse(HEADER.as_bytes());
        cue(&script, &format!("1,0,{style},,0,0,0,,{text}")).unwrap_or_default()
    }

    #[test]
    fn every_brace_pair_is_a_block_and_hides_what_is_not_a_tag() {
        assert_eq!(srt("Default", "{a comment}x{}y{\\}z"), "xyz");
        assert_eq!(srt("Default", "{ \\i1}a{\\i0 } b"), "<i>a</i> b");
        assert_eq!(srt("Default", "{a{\\i1}b}c"), "<i>b}c</i>");
        assert_eq!(
            srt("Default", "{\\i1}open {unclosed"),
            "<i>open {unclosed</i>"
        );
    }

    #[test]
    fn a_drawing_is_not_text() {
        assert_eq!(srt("Default", "{\\p1}m 0 0 l 100 0{\\p0}after"), "after");
        assert_eq!(
            srt("Default", "{\\p1}m 0 0 {unclosed l 1 1{\\p0}after"),
            "after"
        );
    }

    #[test]
    fn escapes_are_what_libass_shows() {
        assert_eq!(srt("Default", "a\\Nb\\nc\\hd\te"), "a\nb c\u{a0}d e");
        assert_eq!(srt("Default", "{\\q2}a\\nb"), "a\nb");
        assert_eq!(srt("Default", "\\{x\\} \\i"), "{x} \\i");
    }

    #[test]
    fn a_value_left_out_goes_back_to_the_style_in_force() {
        assert_eq!(srt("Lean", "{\\i0}a{\\i}b").matches("<i>").count(), 2);
        assert_eq!(
            srt(
                "Default",
                "{\\c&H0000FF&}red {\\c&H00FF00&}green {\\c}white"
            ),
            "<font color=\"#ff0000\">red <font color=\"#00ff00\">green </font></font>white"
        );
    }

    #[test]
    fn a_reset_goes_back_to_the_lines_own_style() {
        assert_eq!(
            srt("Lean", "{\\rDefault}d{\\r}l"),
            "<font face=\"Georgia\" size=\"25\" color=\"#00ff00\"><i><u>{\\an5}</u></i></font>\
             d<font face=\"Georgia\" size=\"25\" color=\"#00ff00\"><i><u>l</u></i></font>"
        );
        assert_eq!(srt("Default", "{\\i1}a{\\rNope}b"), "<i>a</i>b");
    }

    #[test]
    fn weights_and_sizes_read_as_libass_reads_them() {
        assert_eq!(srt("Default", "{\\b700}a{\\b400}b{\\b2}c"), "<b>a</b>bc");
        assert_eq!(
            srt("Default", "{\\fs20}a{\\fs+5}b{\\fs-20}c"),
            "<font size=\"20\">a<font size=\"30\">b</font></font>c"
        );
        assert_eq!(srt("Default", "{\\fs12.5}a"), "<font size=\"13\">a</font>");
    }

    #[test]
    fn the_first_alignment_in_the_line_is_its_alignment() {
        assert_eq!(srt("Top", "{\\an2}x"), "<b>{\\an2}x</b>");
        assert_eq!(srt("Top", "x"), "<b>{\\an8}x</b>");
        // Written with the style's tags, wherever in the line it stands.
        assert_eq!(
            srt("Top", "middle {\\an1}late"),
            "<b>{\\an1}middle late</b>"
        );
        assert_eq!(srt("Top", "{\\an0}x"), "<b>{\\an8}x</b>");
        assert_eq!(srt("Default", "{\\an0}a{\\an8}b"), "ab");
        assert_eq!(srt("Default", "{\\a6}a"), "{\\an8}a");
        assert_eq!(srt("Default", "{\\a4}a"), "{\\an7}a");
    }

    #[test]
    fn colours_read_as_far_as_their_digits_go() {
        assert_eq!(colour_tag("&H0000FF&"), 0xFF_0000);
        assert_eq!(colour_tag("&H00FF00"), 0x00_FF00);
        assert_eq!(colour_tag("H0000FF&"), 0xFF_0000);
        assert_eq!(colour_tag("junk"), 0);
        // Past i32, clamped as strtol clamps: 0x7FFFFFFF, white.
        assert_eq!(colour_tag("&H80FF0000&"), 0xFF_FFFF);
    }

    #[test]
    fn styles_come_from_either_header() {
        let s = Script::parse(HEADER.as_bytes());
        let lean = &s.styles[1];
        assert_eq!(
            (
                lean.face.as_str(),
                s.srt_size(lean.size),
                lean.colour,
                lean.italic,
                lean.alignment
            ),
            ("Georgia", 25, 0x00_FF00, true, 5)
        );
        let ssa = "[V4 Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, Bold, Italic, Alignment\n\
                   Style: Old,Verdana,28,255,-1,0,6\nStyle: Odd,Arial,16,255,0,0,8\n";
        let s = Script::parse(ssa.as_bytes());
        assert_eq!(
            (s.styles[0].colour, s.styles[0].bold, s.styles[0].alignment),
            (0xFF_0000, true, 8)
        );
        assert_eq!(s.styles[1].alignment, 3);
    }

    #[test]
    fn sizes_are_scaled_from_the_scripts_height() {
        let header = "[Script Info]\nPlayResX: 1280\nPlayResY: 720\n";
        let s = Script::parse(header.as_bytes());
        assert_eq!(
            (s.srt_size(40.0), s.srt_size(60.0), s.srt_size(41.0)),
            (16, 24, 16)
        );
        assert_eq!(resolve_play_res_y(0, 0), 288);
        assert_eq!(resolve_play_res_y(1280, 0), 1024);
        assert_eq!(resolve_play_res_y(640, 0), 480);
    }

    #[test]
    fn an_unknown_style_is_the_scripts_default() {
        let s = Script::parse(HEADER.as_bytes());
        assert_eq!(
            s.line_style("Nope").map(|s| s.name.as_str()),
            Some("Default")
        );
        assert_eq!(
            s.line_style("*default").map(|s| s.name.as_str()),
            Some("Default")
        );
    }

    #[test]
    fn an_event_is_its_packets_last_field() {
        assert_eq!(
            event("3,0,Shout,Actor,0,0,0,,Text, with commas"),
            Some(("Shout", "Text, with commas"))
        );
        assert_eq!(event("3,0,Shout"), None);
    }

    #[test]
    fn numbers_read_as_c_reads_them() {
        assert_eq!(strtoi32(" -12x", 10), -12);
        assert_eq!(strtoi32("99999999999", 10), i32::MAX);
        assert_eq!(strtoi32("0x1F", 16), 31);
        assert!((strtod(" 12.5e1x") - 125.0).abs() < f64::EPSILON);
        assert!(strtod(".").abs() < f64::EPSILON && strtod("abc").abs() < f64::EPSILON);
        assert_eq!(header_int("&H00FF00FF"), 0x00FF_00FF);
        assert_eq!(header_int("-1"), u32::MAX);
    }
}
