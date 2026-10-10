//! SubRip's markup -- Matroska's `S_TEXT/UTF8`, the commonest subtitle in a
//! film -- read as ffmpeg's `subrip` decoder reads it, its output shown by
//! ffmpeg's `srt` encoder (`tests/data/subrip.srt` holds every rule below).
//!
//! - `<i>`, `<b>`, `<u>`, `<s>`, in any case and with spaces about the name
//!   (`< b >`), each on and off; a tag closed that was never opened is
//!   dropped, and one never closed is closed at the end.
//! - `<font>`'s `color`, `face` and `size`, in the order written, quoted with
//!   `"` or bare: a colour as `#RRGGBB`, `RRGGBB`, the first six digits of
//!   `#RRGGBBAA`, or one of the names in `colours.rs`; a size as digits. An
//!   attribute that does not read is dropped. `</font>` puts back what the
//!   font before had set -- as a font of its own, the way ffmpeg puts it back.
//! - `<br>` is a line break; any other tag (`<span>`, `<p>`, `<i/>`) is
//!   dropped, its text kept.
//! - `{\an1}` to `{\an9}`, exactly so, is an alignment (the first one counts);
//!   `{\an0}` stays as text; any other `{\...}` is dropped; a `{` that does
//!   not start one is text.
//! - A line break, `\N` and `\n` are line breaks; any other backslash is text.
//! - Spaces at the very start are dropped. Entities (`&amp;`) are text.

use super::colours;
use super::srt::{Op, Toggle};

/// A font's attribute, as `</font>` has to put it back.
#[derive(Clone, Debug)]
enum Set {
    Colour(Option<u32>),
    Face(Option<String>),
    Size(Option<u32>),
}

/// What is in force, so that `</font>` knows what the font before had set.
#[derive(Default)]
struct Fonts {
    colour: Option<u32>,
    face: Option<String>,
    size: Option<u32>,
    /// Each open `<font>`'s attributes, with what each replaced.
    frames: Vec<Vec<Set>>,
}

/// The cue's steps.
pub(crate) fn ops(text: &str) -> Vec<Op> {
    let text = text.trim_start_matches([' ', '\t']);
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut fonts = Fonts::default();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let next = rest.get(c.len_utf8()..).unwrap_or("");
        match c {
            '<' => {
                if let Some((tag, after)) = parse_tag(rest) {
                    flush(&mut plain, &mut out);
                    tag_ops(&tag, &mut fonts, &mut out);
                    rest = after;
                    continue;
                }
                plain.push('<');
            }
            '{' if next.starts_with('\\') => {
                if let Some(end) = next.find('}') {
                    let block = next.get(..end).unwrap_or("");
                    let after = next.get(end.saturating_add(1)..).unwrap_or("");
                    match alignment(block) {
                        Some(0) => plain.push_str("{\\an0}"),
                        Some(n) => {
                            flush(&mut plain, &mut out);
                            out.push(Op::Align(n));
                        }
                        // Any other override block: dropped.
                        None => {}
                    }
                    rest = after;
                    continue;
                }
                plain.push('{');
            }
            '\\' if next.starts_with(['N', 'n']) => {
                flush(&mut plain, &mut out);
                out.push(Op::Break);
                rest = next.get(1..).unwrap_or("");
                continue;
            }
            '\n' => {
                flush(&mut plain, &mut out);
                out.push(Op::Break);
            }
            '\r' if next.starts_with('\n') => {}
            _ => plain.push(c),
        }
        rest = next;
    }
    flush(&mut plain, &mut out);
    out
}

fn flush(plain: &mut String, out: &mut Vec<Op>) {
    if !plain.is_empty() {
        out.push(Op::Text(core::mem::take(plain)));
    }
}

/// `\an` and one digit, exactly: its digit.
fn alignment(block: &str) -> Option<u8> {
    let digits = block.strip_prefix("\\an")?;
    let mut chars = digits.chars();
    let d = chars.next()?.to_digit(10)?;
    if chars.next().is_some() {
        return None;
    }
    u8::try_from(d).ok()
}

/// A tag: its name in lower case, whether it closes, whether it closes
/// itself, and its attributes as written.
struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    attributes: Vec<(String, String)>,
}

/// The tag `s` starts with, and what follows it; `None` where `<` starts no
/// tag (a name must begin with a letter, and the tag must end).
fn parse_tag(s: &str) -> Option<(Tag, &str)> {
    let inner_end = s.find('>')?;
    let inner = s.get(1..inner_end)?;
    let after = s.get(inner_end.saturating_add(1)..)?;
    let mut rest = inner.trim_start_matches(' ');
    let closing = rest.starts_with('/');
    if closing {
        rest = rest.get(1..)?.trim_start_matches(' ');
    }
    let name_len = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    if name_len == 0 {
        return None;
    }
    let name = rest.get(..name_len)?.to_ascii_lowercase();
    let mut rest = rest.get(name_len..)?;
    let self_closing = rest.trim_end_matches(' ').ends_with('/');
    if self_closing {
        rest = rest.trim_end_matches(' ');
        rest = rest.get(..rest.len().saturating_sub(1))?;
    }
    Some((
        Tag {
            name,
            closing,
            self_closing,
            attributes: attributes(rest),
        },
        after,
    ))
}

/// `name=value` pairs, the value quoted with `"` or bare; names in lower case.
fn attributes(mut s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start_matches(' ');
        let name_len = s.find(['=', ' ']).unwrap_or(s.len());
        if name_len == 0 {
            break;
        }
        let name = s.get(..name_len).unwrap_or("").to_ascii_lowercase();
        s = s.get(name_len..).unwrap_or("");
        let Some(value_part) = s.strip_prefix('=') else {
            continue;
        };
        let (value, after) = if let Some(quoted) = value_part.strip_prefix('"') {
            match quoted.find('"') {
                Some(end) => (
                    quoted.get(..end).unwrap_or(""),
                    quoted.get(end.saturating_add(1)..).unwrap_or(""),
                ),
                None => (quoted, ""),
            }
        } else {
            let end = value_part.find(' ').unwrap_or(value_part.len());
            (
                value_part.get(..end).unwrap_or(""),
                value_part.get(end..).unwrap_or(""),
            )
        };
        out.push((name, value.to_owned()));
        s = after;
        if s.is_empty() {
            break;
        }
    }
    out
}

fn tag_ops(tag: &Tag, fonts: &mut Fonts, out: &mut Vec<Op>) {
    let toggle = match tag.name.as_str() {
        "i" => Some(Toggle::Italic),
        "b" => Some(Toggle::Bold),
        "u" => Some(Toggle::Underline),
        "s" => Some(Toggle::Strike),
        _ => None,
    };
    if let Some(t) = toggle {
        if !tag.self_closing {
            out.push(Op::Toggle(t, !tag.closing));
        }
        return;
    }
    match tag.name.as_str() {
        "br" if !tag.closing => out.push(Op::Break),
        "font" if tag.closing => {
            let Some(frame) = fonts.frames.pop() else {
                return;
            };
            for set in frame.into_iter().rev() {
                match set {
                    Set::Colour(before) => {
                        fonts.colour = before;
                        out.push(Op::Colour(before));
                    }
                    Set::Face(before) => {
                        fonts.face.clone_from(&before);
                        out.push(Op::Face(before));
                    }
                    Set::Size(before) => {
                        fonts.size = before;
                        out.push(Op::Size(before));
                    }
                }
            }
        }
        "font" if !tag.self_closing => {
            let mut frame = Vec::new();
            for (name, value) in &tag.attributes {
                match name.as_str() {
                    "color" => {
                        if let Some(rgb) = colour(value) {
                            frame.push(Set::Colour(fonts.colour.replace(rgb)));
                            out.push(Op::Colour(Some(rgb)));
                        }
                    }
                    "face" if !value.is_empty() => {
                        frame.push(Set::Face(fonts.face.replace(value.clone())));
                        out.push(Op::Face(Some(value.clone())));
                    }
                    "size" => {
                        if let Some(size) = size(value) {
                            frame.push(Set::Size(fonts.size.replace(size)));
                            out.push(Op::Size(Some(size)));
                        }
                    }
                    _ => {}
                }
            }
            fonts.frames.push(frame);
        }
        _ => {}
    }
}

/// A colour as SubRip writes one: `#RRGGBB`, `RRGGBB`, `#RRGGBBAA` (its
/// alpha dropped), or a name.
fn colour(value: &str) -> Option<u32> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if (hex.len() == 6 || hex.len() == 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return u32::from_str_radix(hex.get(..6)?, 16).ok();
    }
    if value.starts_with('#') {
        return None;
    }
    colours::by_name(value)
}

/// A size: digits.
fn size(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "tests over fixed inputs")]
mod tests {
    use super::super::srt::Writer;
    use super::*;

    fn srt(text: &str) -> String {
        let mut w = Writer::new();
        for op in ops(text) {
            w.op(op);
        }
        w.finish()
    }

    #[test]
    fn tags_are_read_in_any_case_and_closed_at_the_end() {
        assert_eq!(
            srt("<I>Upper</I> <i>unclosed"),
            "<i>Upper</i> <i>unclosed</i>"
        );
        assert_eq!(srt("< i >spaced</ i >"), "<i>spaced</i>");
        assert_eq!(srt("</i>never opened"), "never opened");
    }

    #[test]
    fn a_font_closed_puts_back_the_one_before_as_a_font_of_its_own() {
        assert_eq!(
            srt("<font color=\"#ff0000\"><font color=\"#00ff00\">inner</font> outer</font>"),
            "<font color=\"#ff0000\"><font color=\"#00ff00\">inner<font color=\"#ff0000\"> outer</font></font></font>"
        );
        // ...and the last one closed leaves no colour at all.
        assert_eq!(
            srt("<font color=\"#ff0000\">a<font color=\"#00ff00\">b</font>c</font>after"),
            "<font color=\"#ff0000\">a<font color=\"#00ff00\">b<font color=\"#ff0000\">c</font></font></font>after"
        );
    }

    #[test]
    fn a_tag_closed_out_of_order_keeps_the_others_as_html_does() {
        assert_eq!(srt("<b><i>x</b>y</i>z"), "<b><i>x</i></b><i>y</i>z");
    }

    #[test]
    fn colours_are_read_as_ffmpeg_reads_them() {
        assert_eq!(colour("#80ff0000"), Some(0x80_FF00));
        assert_eq!(colour("00ff00"), Some(0x00_FF00));
        assert_eq!(colour("Navy"), Some(0x00_0080));
        assert_eq!(colour("#abc"), None);
        assert_eq!(colour(" #ff0000 "), None);
        assert_eq!(colour("'#0000ff'"), None);
    }

    #[test]
    fn only_an_exact_alignment_survives_its_braces() {
        assert_eq!(srt("{\\an8}{\\an2}two"), "{\\an8}two");
        assert_eq!(srt("{\\an0}zero {\\an10}ten {\\i1}x"), "{\\an0}zero ten x");
        assert_eq!(srt("{comment} {unclosed"), "{comment} {unclosed");
    }

    #[test]
    fn breaks_and_leading_spaces() {
        assert_eq!(srt("   a\\Nb \\n c \\h d"), "a\nb \n c \\h d");
        assert_eq!(srt("<br>x<br/>y"), "\nx\ny");
    }
}
