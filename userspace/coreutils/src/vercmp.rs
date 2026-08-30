//! gnulib's `filevercmp`: one copy, for `sort -V` and `ls -v`.
//!
//! Two utilities offer "version sort" and they must mean the same thing by it,
//! because a user reaches for them in the same breath — `ls -v` to look at a
//! directory of releases, `sort -V` to feed the same names to something else.
//! A disagreement between them is not a cosmetic difference: it is `ls -v`
//! showing `1.9` above `1.10` while `sort -V` shows the reverse, from one
//! directory, in one terminal.
//!
//! It lives here rather than in `sort`'s private `order` module because that
//! module is a *binary's* module, unreachable from `bin/ls.rs`; the second
//! caller would otherwise have copied it, which is how the four disagreeing
//! `u+X` parsers that became `userspace/modechange` began.
//!
//! # It is a *file name* comparison, not a version-string one
//!
//! The name says `vercmp`, and three of its rules are not about versions at
//! all. See [`version`] for each of them; the one worth stating up front is
//! the suffix rule, because it is the reason the function exists in this shape:
//! a file's extension is cut off before the comparison and only put back if the
//! stems tie. Without that, a directory of source tarballs sorts by extension
//! before it sorts by version — `foo-1.10.tar.gz` above `foo-1.9.tar.bz2` — and
//! sorting by version is the entire point.

use std::cmp::Ordering;

/// `-V`/`-v`: order the way a human reads a release list — `1.9` below `1.10`.
///
/// This is GNU's `filevercmp`, and it is a *file name* comparison rather than
/// a version-string comparison, which is why three things happen here that the
/// name does not suggest:
///
/// - **The empty string sorts first**, ahead even of `~` — a special case
///   before the ordering proper, not a consequence of it.
/// - **Dot files sort before everything else**, with `.` first and `..`
///   second. `.a` therefore comes before `b` even though `.` outranks `b` in
///   the ordering below.
/// - **A file suffix is cut off before the comparison** and only restored if
///   the rest ties. That is what puts `a.b` before `a-b`: the comparison is
///   between `a` and `a-b`, and `a` is a prefix of it. Comparing the whole
///   strings would rank `-` (a separator) below `.` and answer the other way.
///
/// Without the suffix rule a list of source tarballs sorts by extension before
/// it sorts by version, which is the thing `-V` exists to avoid.
///
/// ```
/// use coreutils::vercmp::version;
/// use std::cmp::Ordering;
/// assert_eq!(version(b"1.9", b"1.10"), Ordering::Less);
/// assert_eq!(version(b"a.b", b"a-b"), Ordering::Less);
/// ```
#[must_use]
pub fn version(a: &[u8], b: &[u8]) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    // The special cases, in GNU's order. Each is "this exact name is smaller
    // than anything it has not already been compared against".
    for name in [b"".as_slice(), b".".as_slice(), b"..".as_slice()] {
        if a == name {
            return Ordering::Less;
        }
        if b == name {
            return Ordering::Greater;
        }
    }
    let (a, b) = match (a.first() == Some(&b'.'), b.first() == Some(&b'.')) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        // Both hidden: the leading dot is common to them and is dropped, so
        // `.a` and `.b` compare as `a` and `b` rather than through `.`'s rank.
        (true, true) => (
            a.get(1..).unwrap_or_default(),
            b.get(1..).unwrap_or_default(),
        ),
        (false, false) => (a, b),
    };

    let a_stem = a.get(..file_prefixlen(a)).unwrap_or_default();
    let b_stem = b.get(..file_prefixlen(b)).unwrap_or_default();
    match verrevcmp(a_stem, b_stem) {
        // The stems decide unless they tie, in which case the suffixes are put
        // back and the whole names compared. (When neither name had a suffix
        // the stem *is* the whole name, so this second pass is a no-op.)
        Ordering::Equal => verrevcmp(a, b),
        other => other,
    }
}

/// The length of the part of a name that is not its extension.
///
/// The extension is the longest tail matching `(\.[A-Za-z~][A-Za-z0-9~]*)*$` —
/// a run of dot-separated words. A `.` followed by a digit does not start one,
/// which is why `1.10` keeps its `.10` and is compared as a version rather
/// than as a name with an extension.
fn file_prefixlen(s: &[u8]) -> usize {
    let mut matched: Option<usize> = None;
    let mut after_dot = false;
    for (index, &c) in s.iter().enumerate() {
        if after_dot {
            after_dot = false;
            // A dot must be followed by a letter or `~` to begin an extension.
            if !(c.is_ascii_alphabetic() || c == b'~') {
                matched = None;
            }
        } else if c == b'.' {
            after_dot = true;
            if matched.is_none() {
                matched = Some(index);
            }
        } else if !(c.is_ascii_alphanumeric() || c == b'~') {
            // Anything else cannot appear in an extension, so no extension can
            // have started before here.
            matched = None;
        }
    }
    matched.unwrap_or(s.len())
}

/// The ordering proper: digit runs compare as numbers, everything else by rank.
fn verrevcmp(s1: &[u8], s2: &[u8]) -> Ordering {
    let (mut i, mut j) = (0usize, 0usize);
    while i < s1.len() || j < s2.len() {
        // The non-digit run. The end of a string ranks 0, the same as a digit,
        // so reaching the end of one while the other is at a digit leaves the
        // loop rather than deciding.
        let mut first_diff = 0i32;
        while (i < s1.len() && !s1.get(i).copied().is_some_and(|c| c.is_ascii_digit()))
            || (j < s2.len() && !s2.get(j).copied().is_some_and(|c| c.is_ascii_digit()))
        {
            let c1 = s1.get(i).map_or(0, |c| version_rank(*c));
            let c2 = s2.get(j).map_or(0, |c| version_rank(*c));
            if c1 != c2 {
                return c1.cmp(&c2);
            }
            i = i.saturating_add(1);
            j = j.saturating_add(1);
        }
        // The digit run, as a number: leading zeros go, then the longer run is
        // the larger number and only a tie in length lets the digits decide.
        while s1.get(i) == Some(&b'0') {
            i = i.saturating_add(1);
        }
        while s2.get(j) == Some(&b'0') {
            j = j.saturating_add(1);
        }
        while s1.get(i).copied().is_some_and(|c| c.is_ascii_digit())
            && s2.get(j).copied().is_some_and(|c| c.is_ascii_digit())
        {
            if first_diff == 0 {
                first_diff = i32::from(s1.get(i).copied().unwrap_or(0))
                    .saturating_sub(i32::from(s2.get(j).copied().unwrap_or(0)));
            }
            i = i.saturating_add(1);
            j = j.saturating_add(1);
        }
        if s1.get(i).copied().is_some_and(|c| c.is_ascii_digit()) {
            return Ordering::Greater;
        }
        if s2.get(j).copied().is_some_and(|c| c.is_ascii_digit()) {
            return Ordering::Less;
        }
        if first_diff != 0 {
            return first_diff.cmp(&0);
        }
    }
    Ordering::Equal
}

/// Where a character sits in the version ordering.
///
/// A digit ranks 0, the same as the end of the string, because digits are
/// handled by the numeric pass instead. `~` is below that at -1, a letter
/// keeps its byte value, and every other character is pushed above the letters
/// so that a separator never outranks a name.
fn version_rank(c: u8) -> i32 {
    if c.is_ascii_digit() {
        0
    } else if c.is_ascii_alphabetic() {
        i32::from(c)
    } else if c == b'~' {
        -1
    } else {
        i32::from(c).saturating_add(256)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn version_orders_releases_the_way_a_human_reads_them() {
        assert_eq!(version(b"1.9", b"1.10"), Ordering::Less);
        assert_eq!(version(b"1.10", b"1.9"), Ordering::Greater);
        assert_eq!(version(b"1.0", b"1.0"), Ordering::Equal);
        assert_eq!(version(b"1.0.1", b"1.0"), Ordering::Greater);
        // A tilde is below even the empty string: pre-releases come first.
        assert_eq!(version(b"1.0~rc1", b"1.0"), Ordering::Less);
        assert_eq!(version(b"1.0~rc1", b"1.0~rc2"), Ordering::Less);
    }

    /// `-V` is gnulib's `filevercmp`, which is a *file name* comparison and not
    /// a version-string one. Every case here was read off GNU sort 8.32:
    ///
    /// ```text
    /// printf '.b\nb\n.\n..\n\n~\na.b\na-b\na.b~\nx\nx.tar\nx.tar.gz\n' | sort -V
    ///   ->  ""  .  ..  .b  ~  a.b~  a.b  a-b  b  x  x.tar  x.tar.gz
    /// ```
    #[test]
    fn version_compares_file_names_not_just_version_numbers() {
        // The empty name, then `.`, then `..`, ahead of everything else.
        assert_eq!(version(b"", b"~"), Ordering::Less);
        assert_eq!(version(b".", b".."), Ordering::Less);
        assert_eq!(version(b"..", b".b"), Ordering::Less);
        // Any dot file sorts before any non-dot file, whatever the letters say.
        assert_eq!(version(b".b", b"a"), Ordering::Less);
        assert_eq!(version(b".z", b"a"), Ordering::Less);
        // The file suffix is cut off and the stems compared first, which is why
        // `a.b` beats `a-b` even though `-` (0x2d) is below `.` (0x2e) in bytes:
        // the stems are `a` and `a-b`, and `a` is the shorter prefix.
        assert_eq!(version(b"a.b", b"a-b"), Ordering::Less);
        // A `~` inside a suffix still ranks below the empty suffix.
        assert_eq!(version(b"a.b~", b"a.b"), Ordering::Less);
        // Equal stems fall back to comparing the whole names.
        assert_eq!(version(b"x", b"x.tar"), Ordering::Less);
        assert_eq!(version(b"x.tar", b"x.tar.gz"), Ordering::Less);
        // A suffix must start with a letter or `~`, so `.1` is not one and the
        // stem of `x.1` is the whole name — which is what keeps `x.9 < x.10`.
        assert_eq!(version(b"x.1", b"x.2"), Ordering::Less);
        assert_eq!(version(b"x.9", b"x.10"), Ordering::Less);
    }

    /// `ls -v` and `sort -V` must agree, which is the whole reason this is a
    /// library module. The digit-run rule is the one a hand-written second copy
    /// gets wrong: `f10` above `f9` is byte order, not version order.
    ///
    /// The last case is the one worth keeping. `f009` and `f9` are the *same
    /// version*, so this function answers `Equal` — and both callers then break
    /// the tie with a plain byte comparison of the whole names, which is why
    /// `sort -V` and `ls -v` still print `f009` first rather than in input
    /// order. A caller that forgets the tie-break gets an unstable listing out
    /// of a function that is behaving correctly.
    #[test]
    fn the_digits_in_a_file_name_compare_as_numbers() {
        assert_eq!(version(b"f9", b"f10"), Ordering::Less);
        assert_eq!(version(b"f2", b"f9"), Ordering::Less);
        // Measured, GNU sort 9.4: `printf 'f009\nf9\n' | sort -V` keeps `f009`
        // first either way round, which is `strcmp` deciding, not this.
        assert_eq!(version(b"f009", b"f9"), Ordering::Equal);
    }
}
