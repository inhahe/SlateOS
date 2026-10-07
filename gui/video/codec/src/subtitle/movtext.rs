//! 3GPP timed text -- MP4's `tx3g`, QuickTime's `text`, what `ffmpeg -c:s
//! mov_text`, HandBrake and phones write -- said in SRT as ffmpeg's
//! `mov_text` decoder and `srt` encoder say it (`tests/data/movtext*.srt`).
//!
//! A sample is its text's length in two bytes, the text, and boxes that
//! modify it; the track's setup (its sample entry) gives the default style,
//! the justification and the font table:
//!
//! - The default style -- a font from the table, a size, a colour, bold,
//!   italic, underline -- is the cue's style, as an ASS line's is
//!   (`srt::Writer`'s `<font>` and tags), and the justification its place:
//!   left, centre or right across, top, middle or bottom down, as `{\anN}`.
//! - A `styl` box's runs of characters (code points, as ffmpeg counts them)
//!   switch on what differs from the default -- bold, italic, underline,
//!   then the size, the font and the colour, each a tag of its own -- and a
//!   run that ends before the text does goes back to the default style.
//!   Runs out of order, overlapping, or ending before they start drop the
//!   whole box, as ffmpeg drops it; a run past the text's end is cut at it;
//!   one of no characters is passed over; a font the table lacks is no font.
//! - Highlights (`hlit`, `hclr`), karaoke, blinking, wrapping and text
//!   boxes are not said: SRT has no words for them.
//! - A line break is one; spaces at the very start are dropped, before any
//!   style is switched on; an empty sample clears the screen and is no cue.
//!
//! **Where this departs from ffmpeg, on purpose:** the text is plain text,
//! so what SRT would read as markup in it (`<i>`, `{\an8}`, `\N`) is kept
//! from reading as it (`srt::escape`) -- ffmpeg passes `<i>` on as italic
//! and reads `\N` as a line break. Text in UTF-16 with its byte-order mark,
//! which the 3GPP specification allows, is read; ffmpeg drops the cue. An
//! empty sample with a style box is no cue; ffmpeg makes an empty one.
//! Sizes are taken as SRT's units, as ffmpeg takes them; a player reading
//! them as the text track's pixels would size them otherwise.

use super::srt::{self, Op, Toggle};

/// The fields of a `tx3g` sample entry before its boxes.
const SETUP_FIELDS: usize = 30;

/// A style record's length: two character offsets, a font, flags, a size
/// and a colour.
const STYLE_RECORD: usize = 12;

/// A run of characters' style, or the default's.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Style {
    font: u16,
    bold: bool,
    italic: bool,
    underline: bool,
    size: u8,
    /// `0xRRGGBB`, the alpha dropped.
    colour: u32,
}

impl Style {
    /// A style record's fields after its character offsets.
    fn read(bytes: &[u8]) -> Option<Self> {
        let [f0, f1, flags, size, r, g, b, _alpha] = *bytes.get(..8)? else {
            return None;
        };
        Some(Self {
            font: u16::from_be_bytes([f0, f1]),
            bold: flags & 1 != 0,
            italic: flags & 2 != 0,
            underline: flags & 4 != 0,
            size,
            colour: u32::from_be_bytes([0, r, g, b]),
        })
    }
}

/// A track's setup: its default style, place and fonts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Setup {
    default: Style,
    /// 1 to 9 as a numeric keypad lays them out.
    alignment: u8,
    /// The font table: each font's ID and name.
    fonts: Vec<(u16, String)>,
}

impl Setup {
    /// A `tx3g` sample entry's fields and boxes (MP4's `config` for the
    /// track). A setup too short to read is SRT's own: Arial, 16, white,
    /// bottom centre.
    pub(crate) fn parse(config: &[u8]) -> Self {
        let mut setup = Self {
            default: Style {
                font: 1,
                bold: false,
                italic: false,
                underline: false,
                size: 16,
                colour: srt::DEFAULT_COLOUR,
            },
            alignment: 2,
            fonts: Vec::new(),
        };
        if config.len() < SETUP_FIELDS {
            return setup;
        }
        // Display flags (4), justification across and down (2), background
        // colour (4), text box (8), then the default style record.
        let across = config.get(4).copied().map_or(1, |b| b as i8);
        let down = config.get(5).copied().map_or(-1, |b| b as i8);
        setup.alignment = keypad(across, down);
        if let Some(style) = config.get(22..SETUP_FIELDS).and_then(Style::read) {
            setup.default = style;
        }
        for (kind, body) in boxes(config.get(SETUP_FIELDS..).unwrap_or_default()) {
            if &kind == b"ftab" {
                setup.fonts = font_table(body);
            }
        }
        setup
    }

    /// The name a font ID is in the table, if it is.
    fn font_name(&self, id: u16) -> Option<&str> {
        self.fonts
            .iter()
            .find(|(f, _)| *f == id)
            .map(|(_, name)| name.as_str())
    }

    /// The default style as SRT says it.
    fn srt_style(&self) -> srt::Style {
        srt::Style {
            face: self
                .font_name(self.default.font)
                .unwrap_or(srt::DEFAULT_FACE)
                .to_owned(),
            size: u32::from(self.default.size),
            colour: self.default.colour,
            bold: self.default.bold,
            italic: self.default.italic,
            underline: self.default.underline,
            strike: false,
        }
    }
}

/// 3GPP's justification -- across 0 left, 1 centre, -1 right; down 0 top,
/// 1 middle, -1 bottom -- on a numeric keypad.
fn keypad(across: i8, down: i8) -> u8 {
    let row: u8 = match down {
        0 => 6,
        1 => 3,
        _ => 0,
    };
    let column = match across {
        0 => 1,
        -1 => 3,
        _ => 2,
    };
    row.saturating_add(column)
}

/// The boxes in `bytes`: each's type and body, until one does not fit.
fn boxes(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while let Some((head, _)) = rest.split_first_chunk::<8>() {
        let [s0, s1, s2, s3, k0, k1, k2, k3] = *head;
        let size = u32::from_be_bytes([s0, s1, s2, s3]) as usize;
        if size < 8 || size > rest.len() {
            break;
        }
        out.push(([k0, k1, k2, k3], rest.get(8..size).unwrap_or_default()));
        rest = rest.get(size..).unwrap_or_default();
    }
    out
}

/// `ftab`'s fonts: a count, then each's ID, its name's length and its name.
fn font_table(body: &[u8]) -> Vec<(u16, String)> {
    let mut fonts = Vec::new();
    let Some((count, mut rest)) = body.split_first_chunk::<2>() else {
        return fonts;
    };
    for _ in 0..u16::from_be_bytes(*count) {
        let Some((&[i0, i1, len], after)) = rest.split_first_chunk::<3>() else {
            break;
        };
        let Some((name, after)) = after.split_at_checked(usize::from(len)) else {
            break;
        };
        // A name that is not UTF-8 is no name: a font the table lacks.
        if let Ok(name) = core::str::from_utf8(name) {
            fonts.push((u16::from_be_bytes([i0, i1]), name.to_owned()));
        }
        rest = after;
    }
    fonts
}

/// A run: characters `start..end` in `style`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Run {
    start: usize,
    end: usize,
    style: Style,
}

/// A `styl` box's runs, as ffmpeg takes them (`decode_styl`): none at all if
/// the box is short, or a run ends before it starts or starts before the
/// one before it ends; runs of no characters passed over.
fn style_runs(body: &[u8]) -> Vec<Run> {
    let Some((count, records)) = body.split_first_chunk::<2>() else {
        return Vec::new();
    };
    let count = usize::from(u16::from_be_bytes(*count));
    if count.saturating_mul(STYLE_RECORD) > records.len() {
        return Vec::new();
    }
    let mut runs: Vec<Run> = Vec::new();
    for record in records.chunks_exact(STYLE_RECORD).take(count) {
        let [s0, s1, e0, e1, ..] = *record else {
            return Vec::new();
        };
        let start = usize::from(u16::from_be_bytes([s0, s1]));
        let end = usize::from(u16::from_be_bytes([e0, e1]));
        let Some(style) = record.get(4..).and_then(Style::read) else {
            return Vec::new();
        };
        if end < start || runs.last().is_some_and(|before| start < before.end) {
            return Vec::new();
        }
        if start < end {
            runs.push(Run { start, end, style });
        }
    }
    runs
}

/// A sample's text: UTF-8, or UTF-16 after its byte-order mark; `None` for
/// one that is neither.
fn text(bytes: &[u8]) -> Option<String> {
    let utf16 = |rest: &[u8], big: bool| {
        if !rest.len().is_multiple_of(2) {
            return None;
        }
        let units = rest.chunks_exact(2).map(|pair| {
            let pair = [
                pair.first().copied().unwrap_or(0),
                pair.get(1).copied().unwrap_or(0),
            ];
            if big {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            }
        });
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .ok()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, false);
    }
    core::str::from_utf8(bytes).ok().map(str::to_owned)
}

/// One sample as SRT markup; `None` for one too damaged to read (shorter
/// than its length, or text that is not UTF-8 or UTF-16), and an empty
/// string for one with no text, which shows nothing.
pub(crate) fn cue(setup: &Setup, sample: &[u8]) -> Option<String> {
    let (length, rest) = sample.split_first_chunk::<2>()?;
    let length = usize::from(u16::from_be_bytes(*length));
    // A length past the sample's end is cut at it, as ffmpeg cuts it.
    let (text_bytes, modifiers) = rest.split_at(length.min(rest.len()));
    let text = text(text_bytes)?;
    if text.is_empty() {
        return Some(String::new());
    }
    let mut runs = Vec::new();
    for (kind, body) in boxes(modifiers) {
        if &kind == b"styl" {
            runs = style_runs(body);
        }
    }
    let chars: Vec<char> = text.chars().collect();
    let mut ops = vec![Op::Reset(Some(setup.srt_style()))];
    if setup.alignment != 2 {
        ops.push(Op::Align(setup.alignment));
    }
    let mut plain = String::new();
    let flush = |plain: &mut String, ops: &mut Vec<Op>| {
        if !plain.is_empty() {
            ops.push(Op::Text(srt::escape(&core::mem::take(plain), true)));
        }
    };
    // Spaces at the very start are dropped, until a style or a character.
    let mut started = false;
    let mut chars = chars.iter().enumerate().peekable();
    while let Some((at, &c)) = chars.next() {
        // A run ending before the text does goes back to the default style.
        if runs.iter().any(|r| r.end == at) {
            flush(&mut plain, &mut ops);
            ops.push(Op::Reset(Some(setup.srt_style())));
        }
        if let Some(run) = runs.iter().find(|r| r.start == at) {
            let before = ops.len();
            flush(&mut plain, &mut ops);
            style_start(setup, &run.style, &mut ops);
            started |= ops.len() > before;
        }
        match c {
            ' ' if !started => {}
            '\n' => {
                flush(&mut plain, &mut ops);
                ops.push(Op::Break);
                started = true;
            }
            '\r' if chars.peek().is_some_and(|&(_, &n)| n == '\n') => {}
            _ => {
                plain.push(c);
                started = true;
            }
        }
    }
    flush(&mut plain, &mut ops);
    let mut w = srt::Writer::new();
    for op in ops {
        w.op(op);
    }
    Some(w.finish())
}

/// What a run switches on, where it differs from the default style, in
/// ffmpeg's order: bold, italic, underline, the size, the font, the colour.
fn style_start(setup: &Setup, style: &Style, ops: &mut Vec<Op>) {
    let default = &setup.default;
    for (on, was, t) in [
        (style.bold, default.bold, Toggle::Bold),
        (style.italic, default.italic, Toggle::Italic),
        (style.underline, default.underline, Toggle::Underline),
    ] {
        if on != was {
            ops.push(Op::Toggle(t, on));
        }
    }
    if style.size != default.size {
        ops.push(Op::Size(Some(u32::from(style.size))));
    }
    if style.font != default.font
        && let Some(name) = setup.font_name(style.font)
    {
        ops.push(Op::Face(Some(name.to_owned())));
    }
    if style.colour != default.colour {
        ops.push(Op::Colour(Some(style.colour)));
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, reason = "tests over fixed inputs")]
mod tests {
    use super::*;

    /// A setup: Arial (1) and Courier (2), centred at the bottom, the
    /// default style plain white 16.
    fn arial() -> Setup {
        let mut config = vec![0, 0, 0, 0, 1, 0xFF, 0, 0, 0, 0xFF];
        config.extend([0; 8]);
        config.extend([0, 0, 0, 0, 0, 1, 0, 16, 0xFF, 0xFF, 0xFF, 0xFF]);
        let ftab = [
            &[0, 0, 0, 28][..],
            b"ftab",
            &[0, 2, 0, 1, 5],
            b"Arial",
            &[0, 2, 7],
            b"Courier",
        ]
        .concat();
        config.extend(ftab);
        Setup::parse(&config)
    }

    fn sample(text: &[u8], modifiers: &[u8]) -> Vec<u8> {
        let mut s = u16::try_from(text.len())
            .unwrap_or(0)
            .to_be_bytes()
            .to_vec();
        s.extend_from_slice(text);
        s.extend_from_slice(modifiers);
        s
    }

    fn styl(runs: &[(u16, u16, u16, u8, u8, u32)]) -> Vec<u8> {
        let mut body = u16::try_from(runs.len())
            .unwrap_or(0)
            .to_be_bytes()
            .to_vec();
        for &(start, end, font, flags, size, rgba) in runs {
            body.extend(start.to_be_bytes());
            body.extend(end.to_be_bytes());
            body.extend(font.to_be_bytes());
            body.extend([flags, size]);
            body.extend(rgba.to_be_bytes());
        }
        let size = u32::try_from(body.len() + 8).unwrap_or(0);
        [&size.to_be_bytes()[..], b"styl", &body].concat()
    }

    #[test]
    fn the_setup_gives_the_fonts_the_place_and_the_default() {
        let s = arial();
        assert_eq!(
            s.fonts,
            [(1, "Arial".to_owned()), (2, "Courier".to_owned())]
        );
        assert_eq!(
            (s.alignment, s.default.size, s.default.colour),
            (2, 16, 0xFF_FFFF)
        );
        assert_eq!(keypad(0, 0), 7);
        assert_eq!(keypad(-1, 1), 6);
        assert_eq!(keypad(1, -1), 2);
        // Too short to read: SRT's own.
        assert_eq!(Setup::parse(&[0; 10]).alignment, 2);
    }

    #[test]
    fn runs_switch_on_what_differs_and_go_back_after() {
        let s = arial();
        let runs = styl(&[(0, 3, 2, 1, 30, 0xFF00_00FF), (4, 8, 1, 2, 16, 0xFFFF_FFFF)]);
        assert_eq!(
            cue(&s, &sample(b"all four differ", &runs)).as_deref(),
            Some(
                "<b><font size=\"30\"><font face=\"Courier\"><font color=\"#ff0000\">all</font></font></font></b> \
                 <i>four</i> differ"
            )
        );
    }

    #[test]
    fn a_bad_style_box_drops_every_run_and_keeps_the_text() {
        let s = arial();
        let overlapping = styl(&[(0, 6, 1, 1, 16, 0xFFFF_FFFF), (3, 9, 1, 2, 16, 0xFFFF_FFFF)]);
        assert_eq!(
            cue(&s, &sample(b"overlapping", &overlapping)).as_deref(),
            Some("overlapping")
        );
        let backwards = styl(&[(5, 2, 1, 1, 16, 0xFFFF_FFFF)]);
        assert_eq!(
            cue(&s, &sample(b"backwards", &backwards)).as_deref(),
            Some("backwards")
        );
        let past = styl(&[(0, 100, 1, 1, 16, 0xFFFF_FFFF)]);
        assert_eq!(
            cue(&s, &sample(b"short", &past)).as_deref(),
            Some("<b>short</b>")
        );
        let empty_run = styl(&[(2, 2, 1, 1, 16, 0xFFFF_FFFF), (3, 6, 1, 2, 16, 0xFFFF_FFFF)]);
        assert_eq!(
            cue(&s, &sample(b"zero length run", &empty_run)).as_deref(),
            Some("zer<i>o l</i>ength run")
        );
    }

    #[test]
    fn utf16_text_is_read_and_offsets_count_characters() {
        let s = arial();
        let text: Vec<u8> = [
            &[0xFE, 0xFF][..],
            &"été 日本"
                .encode_utf16()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>(),
        ]
        .concat();
        let runs = styl(&[(4, 6, 1, 1, 16, 0xFFFF_FFFF)]);
        assert_eq!(
            cue(&s, &sample(&text, &runs)).as_deref(),
            Some("été <b>日本</b>")
        );
        // Neither UTF-8 nor UTF-16: no cue.
        assert_eq!(cue(&s, &sample(&[0xC3, 0x28], &[])), None);
    }

    #[test]
    fn plain_text_stays_plain_and_empty_is_nothing() {
        let s = arial();
        assert_eq!(
            cue(&s, &sample(b"<i>x</i> {\\an8} a\\Nb", &[])).as_deref(),
            Some("<\u{2060}i>x<\u{2060}/i> {\u{2060}\\an8} a\\\u{2060}Nb")
        );
        assert_eq!(
            cue(&s, &sample(b"one\r\ntwo\nthree", &[])).as_deref(),
            Some("one\ntwo\nthree")
        );
        assert_eq!(
            cue(&s, &sample(b"  leading", &[])).as_deref(),
            Some("leading")
        );
        assert_eq!(
            cue(&s, &sample(b"", &styl(&[(0, 3, 1, 1, 16, 0)]))).as_deref(),
            Some("")
        );
        assert_eq!(cue(&s, &[0]), None);
        // A length past the sample is cut at it.
        assert_eq!(cue(&s, &[0, 40, b'h', b'i']).as_deref(), Some("hi"));
    }

    #[test]
    fn the_default_style_and_place_open_every_cue() {
        let mut config = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF];
        config.extend([0; 8]);
        config.extend([0, 0, 0, 0, 0, 1, 3, 24, 0xFF, 0xFF, 0, 0xFF]);
        config.extend([&[0, 0, 0, 20][..], b"ftab", &[0, 1, 0, 1, 7], b"Georgia"].concat());
        let s = Setup::parse(&config);
        assert_eq!(
            cue(&s, &sample(b"styled", &[])).as_deref(),
            Some(
                "<font face=\"Georgia\" size=\"24\" color=\"#ffff00\"><b><i>{\\an7}styled</i></b></font>"
            )
        );
    }
}
