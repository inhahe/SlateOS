//! `getttynam (tty)->ty_type`: the terminal type `/etc/ttys` gives a line,
//! read as glibc's `getttyent` reads the file -- one line a terminal, its
//! name, getty and type the first three fields, a field `"quoted"` to hold
//! blanks, `#` beginning a comment. Neither Ubuntu nor SlateOS ships the
//! file, so this is nearly always `None`, but agetty asks.

use std::path::Path;

/// `getttyent`'s line buffer: a line of this many bytes or more is skipped.
const LINE_MAX: usize = 100;

/// glibc's `skip (p)`: the field at the start of `s` -- quotes taken out, a
/// `\"` inside them a quote -- and what follows its end and the blanks
/// after it. A `#` outside quotes ends the field and the line.
fn skip(s: &[u8]) -> (Vec<u8>, &[u8]) {
    let mut field = Vec::new();
    let mut quoted = false;
    let mut p = 0usize;
    while let Some(&c) = s.get(p) {
        if c == b'"' {
            quoted = !quoted;
            p = p.saturating_add(1);
            continue;
        }
        let mut copied = c;
        if quoted && c == b'\\' && s.get(p.saturating_add(1)) == Some(&b'"') {
            p = p.saturating_add(1);
            copied = b'"';
        }
        if quoted {
            field.push(copied);
            p = p.saturating_add(1);
            continue;
        }
        if c == b'#' {
            return (field, &[]);
        }
        if matches!(c, b'\t' | b' ' | b'\n') {
            p = p.saturating_add(1);
            while matches!(s.get(p), Some(b'\t' | b' ' | b'\n')) {
                p = p.saturating_add(1);
            }
            return (field, s.get(p..).unwrap_or_default());
        }
        field.push(copied);
        p = p.saturating_add(1);
    }
    // The end of the string: upstream ends the field one byte short,
    // over the last byte copied.
    field.pop();
    (field, &[])
}

/// The type the file `data` gives terminal `tty`, if a line names it and
/// gives one.
#[must_use]
pub fn tty_type(data: &[u8], tty: &[u8]) -> Option<Vec<u8>> {
    let mut rest = data;
    while !rest.is_empty() {
        let line_len = rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(rest.len(), |nl| nl.saturating_add(1));
        let (whole, after) = rest.split_at(line_len);
        rest = after;
        // `fgets` into a hundred bytes, and a line it could not end is
        // skipped.
        if whole.len() >= LINE_MAX || whole.last() != Some(&b'\n') {
            continue;
        }
        // The buffer is a C string: a NUL in the line ends it.
        let line = whole.split(|&b| b == 0).next().unwrap_or_default();
        let start = line
            .iter()
            .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
            .unwrap_or(line.len());
        let line = line.get(start..).unwrap_or_default();
        if line.is_empty() || line.first() == Some(&b'#') {
            continue;
        }
        let (name, after_name) = skip(line);
        if name != tty {
            continue;
        }
        if after_name.is_empty() {
            return None;
        }
        let (_getty, after_getty) = skip(after_name);
        if after_getty.is_empty() {
            return None;
        }
        let (kind, _) = skip(after_getty);
        return Some(kind);
    }
    None
}

/// [`tty_type`] of `/etc/ttys` (`_PATH_TTYS`), `None` when it does not
/// open.
#[must_use]
pub fn getttynam_type(path: &Path, tty: &[u8]) -> Option<Vec<u8>> {
    tty_type(&std::fs::read(path).ok()?, tty)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const TTYS: &[u8] = b"# name getty type status\n\
        console \"/usr/libexec/getty Pc\" cons25 on secure\n\
        ttyv0   \"/usr/libexec/getty Pc\"  xterm  on  secure # main\n\
        ttyd0   none  \"vt100 \\\"x\\\"\"  off\n\
        ttyd1   none\n\
        ttyd2\n";

    #[test]
    fn the_third_field_is_the_type() {
        assert_eq!(tty_type(TTYS, b"console").unwrap(), b"cons25");
        assert_eq!(tty_type(TTYS, b"ttyv0").unwrap(), b"xterm");
        assert_eq!(tty_type(TTYS, b"ttyd0").unwrap(), b"vt100 \"x\"");
        assert_eq!(tty_type(TTYS, b"ttyd1"), None);
        assert_eq!(tty_type(TTYS, b"ttyd2"), None);
        assert_eq!(tty_type(TTYS, b"tty1"), None);
    }

    #[test]
    fn a_line_too_long_or_unended_is_skipped() {
        let mut long = b"tty9 none ".to_vec();
        long.extend_from_slice(&[b'v'; 95]);
        long.push(b'\n');
        assert_eq!(tty_type(&long, b"tty9"), None);
        assert_eq!(tty_type(b"tty9 none vt100", b"tty9"), None);
        assert_eq!(tty_type(b"tty9 none vt100\n", b"tty9").unwrap(), b"vt100");
    }

    #[test]
    fn a_field_at_the_string_s_end_loses_its_last_byte() {
        // Upstream's own arithmetic, met only by a line its caller already
        // refused: a field with no blank after it.
        assert_eq!(skip(b"abc").0, b"ab");
        assert_eq!(skip(b"abc def").0, b"abc");
        assert_eq!(skip(b"a#b").0, b"a");
    }
}
