//! `_nc_trim_sgr0` (`trim_sgr0.c`): the `sgr0` that `tgetent` hands to
//! termcap callers, who ask for it as `me`.
//!
//! A terminfo `sgr0` often also switches the alternate character set off
//! (xterm's is `\E(B\E[m`), which a termcap program, knowing nothing of
//! `sgr`, does not expect of "attributes off". So `tgetent` works out what
//! `sgr` with every attribute off writes, compares it with `sgr0` and with
//! `sgr` that has only the alternate set on, and -- when they agree that
//! `sgr0` does more than reset attributes -- cuts the `rmacs` part (or an
//! SGR 10) out. `tgetstr ("me")` then answers with the cut string: on
//! xterm, `\E[0m`. Every step is upstream's, including the slip in its last
//! fallback, which cuts `sgr0` from the offset of the match to the *length*
//! of the match (see [`trim_sgr0`]).

use crate::Entry;
use crate::string as cap;
use crate::tparm::Tparm;

/// `CSI_CHR`.
const CSI: u8 = 0x9b;
/// `ESC_CHR`.
const ESC: u8 = 0x1b;

/// The byte at `i` of a C string, or its NUL.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `is_csi`: how long the control sequence introducer `s` starts with is.
fn is_csi(s: &[u8]) -> usize {
    if at(s, 0) == CSI {
        1
    } else if at(s, 0) == ESC && at(s, 1) == b'[' {
        2
    } else {
        0
    }
}

/// `skip_zero`: past a leading `0;`, or a `0` before a letter.
fn skip_zero(s: &[u8], i: usize) -> usize {
    if at(s, i) == b'0' {
        let next = at(s, i.saturating_add(1));
        if next == b';' {
            return i.saturating_add(2);
        } else if next.is_ascii_alphabetic() {
            return i.saturating_add(1);
        }
    }
    i
}

/// `skip_delay`: past a `$<...>` padding.
fn skip_delay(s: &[u8], mut i: usize) -> usize {
    if at(s, i) == b'$' && at(s, i.saturating_add(1)) == b'<' {
        i = i.saturating_add(2);
        while at(s, i).is_ascii_digit() || at(s, i) == b'/' {
            i = i.saturating_add(1);
        }
        if at(s, i) == b'>' {
            i = i.saturating_add(1);
        }
    }
    i
}

/// `rewrite_sgr`: `attr` moved from the front of `s` to its end.
fn rewrite_sgr(s: &mut [u8], attr: Option<&[u8]>) {
    if let Some(attr) = attr
        && s.len() > attr.len()
        && s.starts_with(attr)
    {
        s.rotate_left(attr.len());
    }
}

/// `similar_sgr`: whether one of `a` and `b` begins with the other, after
/// any common introducer and -- where they differ at once -- a leading zero
/// parameter.
fn similar_sgr(a: &[u8], b: &[u8]) -> bool {
    let (mut ia, mut ib) = (0usize, 0usize);
    let (csi_a, csi_b) = (is_csi(a), is_csi(b));
    if csi_a != 0 && csi_b != 0 && csi_a == csi_b {
        ia = csi_a;
        ib = csi_b;
        if at(a, ia) != at(b, ib) {
            ia = skip_zero(a, ia);
            ib = skip_zero(b, ib);
        }
    }
    let a = a.get(ia..).unwrap_or_default();
    let b = b.get(ib..).unwrap_or_default();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let n = a.len().min(b.len());
    a.get(..n) == b.get(..n)
}

/// `chop_out`: bytes `i` to `j` taken out. `None` where upstream's copy
/// would run past the end of its buffer -- `j` before `i`.
fn chop_out(s: &mut Vec<u8>, i: usize, j: usize) -> Option<()> {
    if j < i {
        return None;
    }
    if j <= s.len() {
        s.drain(i..j);
    } else {
        s.truncate(i);
    }
    Some(())
}

/// `compare_part`: how many bytes of `full` match `part`, its delays
/// matched loosely; 0 for a mismatch.
fn compare_part(part: &[u8], full: &[u8]) -> usize {
    let (mut p, mut f) = (0usize, 0usize);
    let mut used_full = 0usize;
    let mut used_delay = 0usize;
    while at(part, p) != 0 {
        if at(part, p) != at(full, f) {
            used_full = 0;
            break;
        }
        // "Adjust the return-value to allow the rare case of
        // string<delay>string to remove the whole piece."
        if used_delay != 0 {
            used_full = used_full.saturating_add(used_delay);
            used_delay = 0;
        }
        if at(part, p) == b'$' && at(full, f) == b'$' {
            let next_part = skip_delay(part, p);
            let next_full = skip_delay(full, f);
            if next_part != p && next_full != f {
                used_delay = used_delay.saturating_add(next_full.saturating_sub(f));
                f = next_full;
                p = next_part;
                continue;
            }
        }
        used_full = used_full.saturating_add(1);
        p = p.saturating_add(1);
        f = f.saturating_add(1);
    }
    used_full
}

/// `strstr (hay, needle)`: where `needle` first occurs.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `_nc_trim_sgr0`, and `tgetent`'s comparison of its answer with `sgr0`:
/// the `sgr0` termcap callers get when it differs from the terminal's own,
/// or `None`.
///
/// `tparm` is the terminal's (its static variables carry from the first
/// expansion of `sgr` to the second, as upstream's do).
///
/// Where upstream's last fallback would cut with an end before its start
/// -- `sgr(0)` found in `sgr0` further in than it is long -- its `chop_out`
/// copies past the end of its buffer; here the trimming is given up, and
/// `sgr0` is kept.
#[must_use]
pub fn trim_sgr0(entry: &Entry, tparm: &mut Tparm) -> Option<Vec<u8>> {
    let sgr0 = entry.string(cap::EXIT_ATTRIBUTE_MODE)?;
    let sgr = entry.string(cap::SET_ATTRIBUTES)?;
    let mut on = tparm.nc_tiparm(entry, 9, sgr, &[0, 0, 0, 0, 0, 0, 0, 0, 1])?;
    let mut off = tparm.nc_tiparm(entry, 9, sgr, &[0; 9])?;
    let mut end = sgr0.to_vec();
    let smacs = entry.string(cap::ENTER_ALT_CHARSET_MODE);
    let rmacs = entry.string(cap::EXIT_ALT_CHARSET_MODE);
    rewrite_sgr(&mut on, smacs);
    rewrite_sgr(&mut off, rmacs);
    rewrite_sgr(&mut end, rmacs);
    if !similar_sgr(&off, &end) || similar_sgr(&off, &on) {
        // "Either the sgr does not reference alternate character set, or
        // it is incorrect."
        return None;
    }
    let mut result = off.clone();
    let mut found = false;
    // "If rmacs is a substring of sgr(0), remove that chunk."
    if let Some(rmacs) = rmacs {
        let (j, k) = (result.len(), rmacs.len());
        if j > k {
            for i in 0..=j.saturating_sub(k) {
                let k2 = compare_part(rmacs, result.get(i..).unwrap_or_default());
                if k2 != 0 {
                    found = true;
                    chop_out(&mut result, i, i.saturating_add(k2))?;
                    break;
                }
            }
        }
    }
    // "SGR 10 would reset to normal font."
    if !found {
        let i = is_csi(&result);
        if i != 0 && result.last() == Some(&b'm') {
            let tmp = skip_zero(&result, i);
            if at(&result, tmp) == b'1'
                && skip_zero(&result, tmp.saturating_add(1)) != tmp.saturating_add(1)
            {
                let mut from = tmp;
                if tmp > 0 && at(&result, tmp.saturating_sub(1)) == b';' {
                    from = tmp.saturating_sub(1);
                }
                let to = skip_zero(&result, tmp.saturating_add(1));
                chop_out(&mut result, from, to)?;
                found = true;
            }
        }
    }
    if !found
        && let Some(i) = find(&end, &off)
        && end != off
    {
        let mut tmp = end.clone();
        chop_out(&mut tmp, i, off.len())?;
        result = tmp;
    }
    (result != sgr0).then_some(result)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use crate::entry::tests::compile;

    /// An entry with the given `smacs`, `rmacs`, `sgr0` and `sgr`.
    fn terminal(smacs: &[u8], rmacs: &[u8], sgr0: &[u8], sgr: &[u8]) -> Entry {
        let mut strs: Vec<Option<&[u8]>> = vec![None; cap::SET_ATTRIBUTES + 1];
        strs[cap::ENTER_ALT_CHARSET_MODE] = Some(smacs);
        strs[cap::EXIT_ALT_CHARSET_MODE] = Some(rmacs);
        strs[cap::EXIT_ATTRIBUTE_MODE] = Some(sgr0);
        strs[cap::SET_ATTRIBUTES] = Some(sgr);
        crate::entry::read_termtype(&compile(b"t", &[], &[], &strs, false)).unwrap()
    }

    #[test]
    fn xterm_and_vt100_lose_the_charset_reset_and_linux_keeps_it() {
        // Measured: `TERM=xterm-256color pstree -h` and `TERM=vt100` write
        // `\E[0m` for `me`; `TERM=linux` writes its own `\E[m\017`.
        let xterm = terminal(
            b"\x1b(0",
            b"\x1b(B",
            b"\x1b(B\x1b[m",
            b"%?%p9%t\x1b(0%e\x1b(B%;\x1b[0%?%p6%t;1%;%?%p5%t;2%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p7%t;8%;m",
        );
        assert_eq!(
            trim_sgr0(&xterm, &mut Tparm::new()),
            Some(b"\x1b[0m".to_vec())
        );
        let vt100 = terminal(
            b"\x0e",
            b"\x0f",
            b"\x1b[m\x0f$<2>",
            b"\x1b[0%?%p1%p6%|%t;1%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;m%?%p9%t\x0e%e\x0f%;$<2>",
        );
        // The padding stays; `tputs` drops it on the way out.
        assert_eq!(
            trim_sgr0(&vt100, &mut Tparm::new()),
            Some(b"\x1b[0m$<2>".to_vec())
        );
        let linux = terminal(
            b"\x0e",
            b"\x0f",
            b"\x1b[m\x0f",
            b"\x1b[0;10%?%p1%t;7%;%?%p2%t;4%;%?%p3%t;7%;%?%p4%t;5%;%?%p5%t;2%;%?%p6%t;1%;m%?%p9%t\x0e%e\x0f%;",
        );
        assert_eq!(trim_sgr0(&linux, &mut Tparm::new()), None);
    }

    #[test]
    fn no_sgr_or_no_sgr0_is_no_change() {
        let e = Entry::default();
        assert_eq!(trim_sgr0(&e, &mut Tparm::new()), None);
    }

    #[test]
    fn helpers_behave_as_upstream_s() {
        assert!(similar_sgr(b"\x1b[0m\x1b(B", b"\x1b[m\x1b(B"));
        assert!(!similar_sgr(b"\x1b[0m\x1b(B", b"\x1b[0m\x1b(0"));
        assert!(!similar_sgr(b"", b"x"));
        assert_eq!(compare_part(b"\x1b(B", b"\x1b(Bxyz"), 3);
        assert_eq!(compare_part(b"\x1b(B", b"\x1b(0"), 0);
        assert_eq!(compare_part(b"a$<2>", b"a$<5>b"), 1);
        let mut s = b"abcdef".to_vec();
        chop_out(&mut s, 1, 3).unwrap();
        assert_eq!(s, b"adef");
        assert!(chop_out(&mut s, 3, 1).is_none());
        let mut s = b"\x1b(0\x1b[0m".to_vec();
        rewrite_sgr(&mut s, Some(b"\x1b(0"));
        assert_eq!(s, b"\x1b[0m\x1b(0");
    }
}
