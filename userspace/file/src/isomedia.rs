//! What `file` says about an ISO base media file -- MP4, QuickTime, 3GP,
//! AVIF, HEIF and the rest of the `ftyp` family.
//!
//! The rules are file 5.45's own (`magic/Magdir/animation`), generated into
//! [`crate::isomedia_table`] by `scripts/file-isomedia-gen.py`; this module is
//! the part of libmagic's `softmagic.c` they need. Until 2026-10-01 this
//! branch of `file` was eight hand-written brands that called every other one
//! "ISO Media, MPEG-4 compatible", `video/mp4` -- so an AVIF picture was a
//! video (lane F's `requests/f-bce-avif-pictures-open-and-animate.md`) and an
//! unknown brand was asserted to be MP4, where GNU says only "ISO Media".
//!
//! How libmagic evaluates the rules, and so how this does:
//!
//! * Every rule at a level is tried, in order, and **every one that matches
//!   prints** -- `qt  ` matches two, and is "Apple QuickTime movie, Apple
//!   QuickTime (.MOV/QT)".
//! * A rule's children are tried only when it matched.
//! * A description is printed after a space, unless it begins with `\b`
//!   ([`Rule::backspace`]), which joins it to what came before. An empty one
//!   prints nothing.
//! * `%d` prints the value tested; `%.4s` the first four characters of the
//!   string tested, after `file_printable`'s escaping of what is not
//!   printable.
//! * The MIME type is the first one met, depth first, among the rules that
//!   matched; none at all is `application/octet-stream`, as `file -i` says
//!   for a brand it does not know.

use crate::isomedia_table::RULES;

/// One test of a rule against the bytes at its offset.
pub(crate) enum Test {
    /// `string VALUE`: the bytes there begin with VALUE.
    String(&'static [u8]),
    /// `string/W VALUE`: the same, with libmagic's compacting of white space
    /// -- which, for a value with no white space in it (the generator refuses
    /// any other), is the same test.
    StringW(&'static [u8]),
    /// `string x`: true at any offset up to and including the end of the
    /// file; the description prints the string there (empty at the end).
    AnyString,
    /// `byte N`: the byte there is N.
    Byte(u8),
    /// `beshort x`: true wherever two bytes are there; prints them as a
    /// signed big-endian number, as libmagic's `beshort` is.
    AnyBeShort,
}

/// One magic rule: a test at an offset, what to print when it holds, and the
/// rules under it.
pub(crate) struct Rule {
    /// Where the test looks, from the start of the file.
    pub(crate) offset: usize,
    /// What it looks for.
    pub(crate) test: Test,
    /// Whether the description began with `\b`: printed with no separator.
    pub(crate) backspace: bool,
    /// The description, after any `\b`.
    pub(crate) text: &'static str,
    /// The rule's `!:mime`, if it has one.
    pub(crate) mime: Option<&'static str>,
    /// The rules tried only when this one matched.
    pub(crate) children: &'static [Rule],
}

/// What a test found, for the description's format to print.
enum Value<'a> {
    /// Nothing to print (`string VALUE`, whose description has no format).
    None,
    /// A number, for `%d`.
    Number(i64),
    /// A string, for `%.4s`.
    Bytes(&'a [u8]),
}

/// The description and MIME type of an ISO base media file, whose `ftyp`
/// box the caller found at offset 4.
///
/// The description goes through `file_printable` last, as libmagic's whole
/// result does: file 5.45's rules hold one literal TAB (the `caqv` brand), and
/// `file` prints it as `\011`. A value already escaped by `%.4s` is not
/// escaped twice, since the escape is printable.
pub(crate) fn identify(buf: &[u8]) -> (String, &'static str) {
    let mut out = String::from("ISO Media");
    let mut mime = None;
    walk(RULES, buf, &mut out, &mut mime);
    (
        printable(out.as_bytes()),
        mime.unwrap_or("application/octet-stream"),
    )
}

fn walk(rules: &[Rule], buf: &[u8], out: &mut String, mime: &mut Option<&'static str>) {
    for rule in rules {
        let Some(value) = test(rule, buf) else {
            continue;
        };
        print(rule, &value, out);
        if mime.is_none() {
            *mime = rule.mime;
        }
        walk(rule.children, buf, out, mime);
    }
}

/// The rule's test against `buf`: what it found if it holds.
fn test<'a>(rule: &Rule, buf: &'a [u8]) -> Option<Value<'a>> {
    let at = buf.get(rule.offset..)?;
    match rule.test {
        Test::String(want) | Test::StringW(want) => at.starts_with(want).then_some(Value::None),
        // Matches even at the very end of the file, with the empty string --
        // a 96-byte XAVC file prints `Audio ""` -- but not beyond it, where
        // `buf.get` above has already said no.
        Test::AnyString => {
            let end = at.iter().position(|&b| b == 0).unwrap_or(at.len());
            Some(Value::Bytes(at.get(..end).unwrap_or_default()))
        }
        Test::Byte(want) => {
            let &b = at.first()?;
            // libmagic's `byte` is signed: a value is printed as `%d` of it.
            (b == want).then_some(Value::Number(i64::from(b.cast_signed())))
        }
        Test::AnyBeShort => {
            let &[hi, lo] = at.first_chunk::<2>()?;
            Some(Value::Number(i64::from(i16::from_be_bytes([hi, lo]))))
        }
    }
}

/// Print a matched rule's description, libmagic's way.
fn print(rule: &Rule, value: &Value<'_>, out: &mut String) {
    if rule.text.is_empty() {
        return;
    }
    if !rule.backspace {
        out.push(' ');
    }
    let mut rest = rule.text;
    while let Some(i) = rest.find('%') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let after = rest.get(i..).unwrap_or_default();
        if let Some(tail) = after.strip_prefix("%d") {
            if let Value::Number(n) = value {
                out.push_str(&n.to_string());
            }
            rest = tail;
        } else if let Some(tail) = after.strip_prefix("%.4s") {
            if let Value::Bytes(b) = value {
                out.extend(printable(b).chars().take(4));
            }
            rest = tail;
        } else {
            // The generator accepts no other format; a lone `%` is itself.
            out.push('%');
            rest = after.get(1..).unwrap_or_default();
        }
    }
    out.push_str(rest);
}

/// libmagic's `file_printable`: printable ASCII as itself, every other byte
/// as a three-digit octal escape.
fn printable(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        if b.is_ascii_graphic() || b == b' ' {
            s.push(char::from(b));
        } else {
            s.push_str(&format!("\\{b:03o}"));
        }
    }
    s
}

#[cfg(test)]
mod tests;
