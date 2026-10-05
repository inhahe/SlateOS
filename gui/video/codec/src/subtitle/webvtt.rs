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
//! Not yet read: a cue's settings (`line:0`, `align:start`), which place it.

use super::srt::{Op, Toggle};

/// The elements cue text nests, the specification's eight.
const ELEMENTS: [&str; 8] = ["c", "i", "b", "u", "ruby", "rt", "v", "lang"];

/// The cue's steps.
pub(crate) fn ops(text: &str) -> Vec<Op> {
    let mut out = Vec::new();
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
        let mut w = Writer::new();
        for op in ops(text) {
            w.op(op);
        }
        w.finish()
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
