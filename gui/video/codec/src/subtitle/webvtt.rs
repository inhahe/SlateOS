//! WebVTT cue text -- WebM's `D_WEBVTT/SUBTITLES`, Matroska's
//! `S_TEXT/WEBVTT` -- as the WebVTT specification reads it (its cue text
//! tokenizer and the tree built from it), said in SRT. Where ffmpeg's
//! conversion agrees, the cue is ffmpeg's own (`tests/data/webvtt.srt`):
//!
//! - `<i>`, `<b>` and `<u>` are italic, bold and underline, classes and all
//!   (`<i.loud>`). `<c>`, `<v>` (a voice: whose line it is) and `<lang>`
//!   only mark text up, and are dropped with it kept. A timestamp tag
//!   (`<00:00:07.500>`, karaoke's) is dropped.
//! - An end tag closes the element it names only when that is the innermost
//!   one open, as the specification has it; one never closed is closed at
//!   the end.
//! - A `<` always starts a tag, to the next `>` or the cue's end: text meant
//!   to show one is written `&lt;`.
//! - `&amp;`, `&lt;`, `&gt;`, `&lrm;`, `&rlm;`, `&nbsp;` (U+00A0), `&quot;`,
//!   `&apos;` and numeric references (`&#39;`, `&#x27;`) are the characters
//!   they name; any other `&` is itself.
//! - Spaces at the very start are dropped; a line break is one.
//!
//! **Where this departs from ffmpeg, on purpose:** ffmpeg writes `&nbsp;` as
//! `\h`, ASS's no-break space, which SRT readers other than ffmpeg's show as
//! two characters, and leaves `&quot;`, `&apos;` and numeric references as
//! written -- YouTube's captions write every apostrophe `&#39;`. Ruby (a
//! reading set above the text) cannot be set above in SRT; it is put in
//! parentheses after its text, `漢(kan)`, as HTML's own fallback for it
//! shows, where ffmpeg runs the two together. An end tag that does not close
//! the innermost element is passed over, as the specification has it, where
//! ffmpeg closes whatever it names. And text SRT would read as markup -- a
//! `<` written `&lt;`, `{\`, `\N` -- is kept from reading as it
//! (`srt::escape`); ffmpeg writes `{` as ASS's `\{{}`, which only an ASS
//! renderer reads.
//!
//! **A cue's settings place it** (`line:0`, `align:start position:0%`),
//! which ffmpeg ignores: as `{\anN}` at the cue's start, the one of SRT's
//! nine places its text's anchor falls in ([`placement`]).

use super::srt::{Op, Toggle};

/// The elements cue text nests, the specification's eight.
const ELEMENTS: [&str; 8] = ["c", "i", "b", "u", "ruby", "rt", "v", "lang"];

/// A line's height, as a share of the picture's: what a cue's line number
/// counts in. Browsers set cue text 5% of the picture high, a line about a
/// sixteenth more.
const LINE_HEIGHT: f64 = 5.33;

/// How a cue's text is aligned: the `align` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
    Start,
    Center,
    End,
    Left,
    Right,
}

/// Which point of the cue's box `position` names: its alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    LineLeft,
    Center,
    LineRight,
}

/// Where the cue's `settings` put it, as SRT's `{\anN}` puts a cue: the
/// place on a numeric keypad; `None` for bottom centre, SRT's own.
///
/// SRT has nine places and WebVTT a continuum: this is the one the text's
/// anchor falls in, the picture cut in thirds each way. The anchor is where
/// the specification lays a horizontal cue's text ("apply WebVTT cue
/// settings"): across, its box -- `position` and `size`, the position its
/// left edge, middle or right edge as `position`'s alignment or else `align`
/// says, the size no wider than fits -- and in the box its left edge for
/// `start` and `left`, its right for `end` and `right`, its middle for
/// `center`; down, `line`, a percentage, or a line number counted from the
/// top when it is not negative and from the bottom when it is. A setting
/// the specification would pass over (`line:abc`, `position:50`) is passed
/// over. A vertical cue (`vertical:rl`) has no place in SRT, and is put
/// where a cue without settings goes.
pub(crate) fn placement(settings: &str) -> Option<u8> {
    let mut vertical = false;
    let mut line: Option<f64> = None;
    let mut position: Option<(f64, Option<Anchor>)> = None;
    let mut size = 100.0;
    let mut align = Align::Center;
    let separators = [' ', '\t', '\n', '\r', '\u{c}'];
    for setting in settings.split(separators).filter(|s| !s.is_empty()) {
        let Some((name, value)) = setting.split_once(':') else {
            continue;
        };
        match name {
            "vertical" if matches!(value, "rl" | "lr") => vertical = true,
            "line" => line = line_setting(value).or(line),
            "position" => position = position_setting(value).or(position),
            "size" => size = percentage(value).unwrap_or(size),
            "align" => {
                align = match value {
                    "start" => Align::Start,
                    "center" => Align::Center,
                    "end" => Align::End,
                    "left" => Align::Left,
                    "right" => Align::Right,
                    _ => align,
                };
            }
            _ => {}
        }
    }
    if vertical {
        return None;
    }
    let (start, end) = (
        matches!(align, Align::Start | Align::Left),
        matches!(align, Align::End | Align::Right),
    );
    let (at, anchor) = position.unwrap_or((
        if start {
            0.0
        } else if end {
            100.0
        } else {
            50.0
        },
        None,
    ));
    let anchor = anchor.unwrap_or(if start {
        Anchor::LineLeft
    } else if end {
        Anchor::LineRight
    } else {
        Anchor::Center
    });
    let widest = match anchor {
        Anchor::LineLeft => 100.0 - at,
        Anchor::LineRight => at,
        Anchor::Center => 2.0 * at.min(100.0 - at),
    };
    let width = size.min(widest);
    let left = match anchor {
        Anchor::LineLeft => at,
        Anchor::LineRight => at - width,
        Anchor::Center => at - width / 2.0,
    };
    let x = if start {
        left
    } else if end {
        left + width
    } else {
        left + width / 2.0
    };
    let y = line.unwrap_or(100.0);
    let column = if x < 100.0 / 3.0 {
        0
    } else if x > 200.0 / 3.0 {
        2
    } else {
        1
    };
    let row: u8 = if y < 100.0 / 3.0 {
        7
    } else if y < 200.0 / 3.0 {
        4
    } else {
        1
    };
    let place = row.saturating_add(column);
    (place != 2).then_some(place)
}

/// `line`'s value -- a line number, or a percentage, then an alignment --
/// as how far down the picture its text's anchor is, as a percentage.
fn line_setting(value: &str) -> Option<f64> {
    let (at, alignment) = value
        .split_once(',')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    if alignment.is_some_and(|a| !matches!(a, "start" | "center" | "end")) {
        return None;
    }
    if at.ends_with('%') {
        return percentage(at);
    }
    let digits = at.strip_prefix('-').unwrap_or(at);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: f64 = at.parse().ok()?;
    let lines = n * LINE_HEIGHT;
    Some(if n < 0.0 { 100.0 + lines } else { lines }.clamp(0.0, 100.0))
}

/// `position`'s value: a percentage, then an alignment.
fn position_setting(value: &str) -> Option<(f64, Option<Anchor>)> {
    let (at, alignment) = value
        .split_once(',')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let anchor = match alignment {
        None => None,
        Some("line-left") => Some(Anchor::LineLeft),
        Some("center") => Some(Anchor::Center),
        Some("line-right") => Some(Anchor::LineRight),
        Some(_) => return None,
    };
    Some((percentage(at)?, anchor))
}

/// A WebVTT percentage: digits, a fraction if any, `%`; 0 to 100.
fn percentage(value: &str) -> Option<f64> {
    let number = value.strip_suffix('%')?;
    let (whole, fraction) = number
        .split_once('.')
        .map_or((number, None), |(w, f)| (w, Some(f)));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || fraction.is_some_and(|f| !digits(f)) {
        return None;
    }
    number
        .parse::<f64>()
        .ok()
        .filter(|p| (0.0..=100.0).contains(p))
}

/// The cue's steps: its place, if its `settings` give one other than bottom
/// centre, then its text.
pub(crate) fn ops(text: &str, settings: &str) -> Vec<Op> {
    let mut out: Vec<Op> = placement(settings).map(Op::Align).into_iter().collect();
    let mut plain = String::new();
    // The elements open, innermost last.
    let mut open: Vec<&str> = Vec::new();
    let mut rest = text.trim_start_matches([' ', '\t']);
    while let Some(c) = rest.chars().next() {
        let after = rest.get(c.len_utf8()..).unwrap_or("");
        match c {
            '<' => {
                let (tag, next) = match after.find('>') {
                    Some(end) => (
                        after.get(..end).unwrap_or(""),
                        after.get(end.saturating_add(1)..).unwrap_or(""),
                    ),
                    None => (after, ""),
                };
                flush(&mut plain, &mut out);
                tag_ops(tag, &mut open, &mut out);
                rest = next;
                continue;
            }
            '&' => {
                if let Some((ch, next)) = reference(after) {
                    plain.push(ch);
                    rest = next;
                    continue;
                }
                plain.push('&');
            }
            '\n' => {
                flush(&mut plain, &mut out);
                out.push(Op::Break);
            }
            // A cue's lines end in `\n`; a `\r` before one is the file's.
            '\r' if after.starts_with('\n') => {}
            _ => plain.push(c),
        }
        rest = after;
    }
    flush(&mut plain, &mut out);
    // A reading never closed still ends in its parenthesis.
    for _ in open.iter().filter(|&&name| name == "rt") {
        out.push(Op::Text(")".to_owned()));
    }
    out
}

/// Text so far, kept from reading as SRT markup: a `<` in it was `&lt;`.
fn flush(plain: &mut String, out: &mut Vec<Op>) {
    if !plain.is_empty() {
        out.push(Op::Text(super::srt::escape(&core::mem::take(plain), true)));
    }
}

const fn toggle(name: &str) -> Option<Toggle> {
    match name.as_bytes() {
        b"i" => Some(Toggle::Italic),
        b"b" => Some(Toggle::Bold),
        b"u" => Some(Toggle::Underline),
        _ => None,
    }
}

/// One tag's steps: what is between its `<` and `>`.
fn tag_ops<'t>(tag: &'t str, open: &mut Vec<&'t str>, out: &mut Vec<Op>) {
    if let Some(name) = tag.strip_prefix('/') {
        // `</ruby>` closes a reading left open inside it.
        if name == "ruby" && open.last() == Some(&"rt") {
            close(open, out);
        }
        if open.last() == Some(&name) {
            close(open, out);
        }
        return;
    }
    // A timestamp: karaoke's, which SRT cannot do.
    if tag.starts_with(|c: char| c.is_ascii_digit()) {
        return;
    }
    let name_end = tag
        .find(['.', ' ', '\t', '\n', '\u{c}'])
        .unwrap_or(tag.len());
    let name = tag.get(..name_end).unwrap_or("");
    if !ELEMENTS.contains(&name) || (name == "rt" && open.last() != Some(&"ruby")) {
        return;
    }
    open.push(name);
    if let Some(t) = toggle(name) {
        out.push(Op::Toggle(t, true));
    }
    if name == "rt" {
        out.push(Op::Text("(".to_owned()));
    }
}

/// Close the innermost element open.
fn close(open: &mut Vec<&str>, out: &mut Vec<Op>) {
    let Some(name) = open.pop() else {
        return;
    };
    if let Some(t) = toggle(name) {
        out.push(Op::Toggle(t, false));
    }
    if name == "rt" {
        out.push(Op::Text(")".to_owned()));
    }
}

/// The character a reference names, and what follows its `;`: `s` is what
/// follows the `&`.
fn reference(s: &str) -> Option<(char, &str)> {
    let end = s.find(';')?;
    let name = s.get(..end)?;
    let next = s.get(end.saturating_add(1)..)?;
    let ch = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "lrm" => '\u{200e}',
        "rlm" => '\u{200f}',
        "nbsp" => '\u{a0}',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let number = name.strip_prefix('#')?;
            let (digits, radix) = match number.strip_prefix(['x', 'X']) {
                Some(hex) => (hex, 16),
                None => (number, 10),
            };
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return None;
            }
            // As HTML reads one: nothing, or nothing a character can be, is
            // the replacement character.
            u32::from_str_radix(digits, radix)
                .ok()
                .and_then(char::from_u32)
                .filter(|&c| c != '\0')
                .unwrap_or('\u{fffd}')
        }
    };
    Some((ch, next))
}

#[cfg(test)]
mod tests {
    use super::super::srt::Writer;
    use super::*;

    fn srt(text: &str) -> String {
        placed(text, "")
    }

    fn placed(text: &str, settings: &str) -> String {
        let mut w = Writer::new();
        for op in ops(text, settings) {
            w.op(op);
        }
        w.finish()
    }

    #[test]
    fn settings_place_a_cue_in_the_third_its_anchor_falls_in() {
        let cases = [
            ("", None),
            ("line:0", Some(8)),
            ("line:-1", None),
            ("line:7", Some(5)),
            ("line:-7", Some(5)),
            ("line:50%", Some(5)),
            ("line:10%,end", Some(8)),
            // YouTube's: left-aligned at the left edge.
            ("align:start position:0%", Some(1)),
            ("align:end", Some(3)),
            ("align:left line:10%", Some(7)),
            ("line:0 position:90% align:end", Some(9)),
            ("align:center position:20%", Some(1)),
            ("size:50% align:start", Some(1)),
            // A box from 0 to 80%, its text centred at 40%: bottom centre.
            ("position:80%,line-right", None),
            // Settings the specification passes over.
            ("line:6.5 position:50 size:120% align:middle", None),
            ("line:0,bottom", None),
            // No place in SRT for a vertical cue.
            ("vertical:rl line:0", None),
        ];
        for (settings, place) in cases {
            assert_eq!(placement(settings), place, "{settings:?}");
        }
        assert_eq!(placed("x", "line:0"), "{\\an8}x");
        assert_eq!(placed("<i>x</i>", "align:end"), "{\\an3}<i>x</i>");
    }

    #[test]
    fn italic_bold_and_underline_are_kept_and_the_rest_dropped() {
        assert_eq!(
            srt("<i>a</i> <b.loud>b</b> <u>c</u>"),
            "<i>a</i> <b>b</b> <u>c</u>"
        );
        assert_eq!(
            srt("<c.yellow>c</c> <v Roger>v</v> <lang en>l</lang>"),
            "c v l"
        );
        assert_eq!(srt("<00:00:07.500>timed <unknown>x</unknown>"), "timed x");
    }

    #[test]
    fn an_end_tag_closes_only_the_innermost_element() {
        // `</b>` with `<i>` innermost is passed over: `y` is bold and italic.
        assert_eq!(srt("<b><i>x</b>y</i>z"), "<b><i>xy</i>z</b>");
        assert_eq!(srt("<i><c>x</i>y</c>z"), "<i>xyz</i>");
    }

    #[test]
    fn a_less_than_sign_always_starts_a_tag() {
        assert_eq!(srt("a < b and c > d"), "a  d");
        // `&lt;` is a less-than sign SRT's readers do not take for a tag.
        assert_eq!(srt("a &lt;i> b"), "a <\u{2060}i> b");
        // A tag the cue ends inside is still a tag.
        assert_eq!(srt("cut <i"), "cut <i></i>");
    }

    #[test]
    fn references_are_the_characters_they_name() {
        assert_eq!(
            srt("&amp; &lt;&gt; x&nbsp;y &quot;q&quot; it&#39;s &#x41; &#0; & &bogus;"),
            "& <\u{2060}> x\u{a0}y \"q\" it's A \u{fffd} & &bogus;"
        );
        assert_eq!(srt("{\\an8} \\N"), "{\u{2060}\\an8} \\\u{2060}N");
        assert_eq!(srt("&lrm;l&rlm;r"), "\u{200e}l\u{200f}r");
        assert_eq!(srt("&#+41; &#x;"), "&#+41; &#x;");
    }

    #[test]
    fn a_reading_follows_its_text_in_parentheses() {
        assert_eq!(srt("<ruby>漢<rt>kan</rt></ruby>字"), "漢(kan)字");
        assert_eq!(srt("<ruby>漢<rt>kan</ruby>"), "漢(kan)");
        assert_eq!(srt("<ruby>漢<rt>kan"), "漢(kan)");
        // Outside ruby, rt is no element at all.
        assert_eq!(srt("<rt>x</rt>"), "x");
    }

    #[test]
    fn leading_spaces_and_line_breaks() {
        assert_eq!(srt("   a\nb\r\nc"), "a\nb\nc");
    }
}
