//! The one question util-linux asks of terminfo: how many colours a terminal
//! has -- `setupterm (NULL, STDOUT_FILENO, &ret) == 0 && ret == 1` and then
//! `tigetnum ("colors")`, from ncurses on the reference.
//!
//! An entry is a compiled terminfo file, found as ncurses finds it:
//!
//! 1. `$TERMINFO`, if set;
//! 2. `$HOME/.terminfo`;
//! 3. each directory of `$TERMINFO_DIRS`, an empty one meaning
//!    `/usr/share/terminfo`;
//! 4. `/etc/terminfo`, then `/usr/share/terminfo` -- the reference's
//!    compiled-in list (`infocmp -D`).
//!
//! In each, the file is `DIR/<first byte>/<name>`, and failing that
//! `DIR/<first byte in hex>/<name>`, the spelling ncurses uses where file
//! names fold case. A name that is empty, `.`, `..` or holds a `/` is no
//! terminal at all, as ncurses refuses it.
//!
//! The file's numbers are 16-bit (magic 0432) or 32-bit (01036, "extended
//! numbers"), and `colors` is number 13. Its absence (-1) or cancellation
//! (-2) is no colours, as any count below 2 is to util-linux.

use std::path::{Path, PathBuf};

/// Magic of the classic format, numbers in 16 bits.
const MAGIC_16: u16 = 0o432;
/// Magic of the extended-numbers format, numbers in 32 bits.
const MAGIC_32: u16 = 0o1036;
/// `colors`' index among the numeric capabilities.
const COLORS: usize = 13;

/// The colour count `term`'s entry gives, or `None` when there is no entry
/// to read (no `TERM`, no file, a file that is not terminfo) -- `setupterm`
/// failing.
#[must_use]
pub fn colors(
    term: &[u8],
    terminfo: Option<&[u8]>,
    home: Option<&[u8]>,
    terminfo_dirs: Option<&[u8]>,
) -> Option<i32> {
    if term.is_empty() || term == b"." || term == b".." || term.contains(&b'/') {
        return None;
    }
    for dir in search_dirs(terminfo, home, terminfo_dirs) {
        if let Some(text) = read_entry(&dir, term) {
            return parse_colors(&text);
        }
    }
    None
}

/// The directories, in the order ncurses tries them.
fn search_dirs(
    terminfo: Option<&[u8]>,
    home: Option<&[u8]>,
    terminfo_dirs: Option<&[u8]>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(t) = terminfo {
        if !t.is_empty() {
            dirs.push(super::path_of(t));
        }
    }
    if let Some(h) = home {
        if !h.is_empty() {
            dirs.push(super::path_of(h).join(".terminfo"));
        }
    }
    if let Some(list) = terminfo_dirs {
        for part in list.split(|&c| c == b':') {
            if part.is_empty() {
                dirs.push(PathBuf::from("/usr/share/terminfo"));
            } else {
                dirs.push(super::path_of(part));
            }
        }
    }
    dirs.push(PathBuf::from("/etc/terminfo"));
    dirs.push(PathBuf::from("/usr/share/terminfo"));
    dirs
}

/// The bytes of `DIR/<first>/<name>`, or of the hexadecimal spelling.
fn read_entry(dir: &Path, term: &[u8]) -> Option<Vec<u8>> {
    let first = *term.first()?;
    let name = super::path_of(term);
    let plain = dir.join(super::path_of(&[first])).join(&name);
    if let Ok(text) = std::fs::read(&plain) {
        return Some(text);
    }
    let hex = dir.join(format!("{first:02x}")).join(&name);
    std::fs::read(hex).ok()
}

/// A little-endian 16-bit word at `at`.
fn word(text: &[u8], at: usize) -> Option<u16> {
    let lo = *text.get(at)?;
    let hi = *text.get(at.checked_add(1)?)?;
    Some(u16::from_le_bytes([lo, hi]))
}

/// `colors` from a compiled entry: -1 when the entry has no such number.
fn parse_colors(text: &[u8]) -> Option<i32> {
    let magic = word(text, 0)?;
    let width = match magic {
        MAGIC_16 => 2usize,
        MAGIC_32 => 4usize,
        _ => return None,
    };
    let name_size = usize::from(word(text, 2)?);
    let bool_count = usize::from(word(text, 4)?);
    let num_count = usize::from(word(text, 6)?);
    if num_count <= COLORS {
        return Some(-1);
    }
    let mut at = 12usize.checked_add(name_size)?.checked_add(bool_count)?;
    // The numbers start on an even byte.
    if at % 2 == 1 {
        at = at.checked_add(1)?;
    }
    let at = at.checked_add(COLORS.checked_mul(width)?)?;
    let value = if width == 2 {
        i32::from(i16::from_le_bytes(word(text, at)?.to_le_bytes()))
    } else {
        let b = text.get(at..at.checked_add(4)?)?;
        i32::from_le_bytes([*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?])
    };
    Some(value)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A compiled entry with `n` numbers, `colors` among them set to `c`.
    fn entry(magic: u16, names: &[u8], bools: usize, colors: i32, width: usize) -> Vec<u8> {
        let mut t = Vec::new();
        for w in [magic, names.len() as u16, bools as u16, 15, 0, 0] {
            t.extend_from_slice(&w.to_le_bytes());
        }
        t.extend_from_slice(names);
        t.extend(std::iter::repeat_n(0u8, bools));
        if t.len() % 2 == 1 {
            t.push(0);
        }
        for k in 0..15 {
            let v: i32 = if k == COLORS { colors } else { -1 };
            if width == 2 {
                t.extend_from_slice(&(v as i16).to_le_bytes());
            } else {
                t.extend_from_slice(&v.to_le_bytes());
            }
        }
        t
    }

    #[test]
    fn both_formats_and_the_padding() {
        assert_eq!(
            parse_colors(&entry(MAGIC_32, b"xterm-256color|x\0", 38, 256, 4)),
            Some(256)
        );
        assert_eq!(
            parse_colors(&entry(MAGIC_16, b"vt100|v\0", 3, -1, 2)),
            Some(-1)
        );
        assert_eq!(parse_colors(&entry(MAGIC_16, b"ansi\0", 4, 8, 2)), Some(8));
        assert_eq!(parse_colors(b"not terminfo"), None);
    }

    #[test]
    fn names_ncurses_refuses() {
        assert_eq!(colors(b"", None, None, None), None);
        assert_eq!(colors(b"../x", None, None, None), None);
        assert_eq!(colors(b"..", None, None, None), None);
    }
}
