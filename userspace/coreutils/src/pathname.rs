//! Splitting a file name into a directory part and a last component.
//!
//! This is gnulib's `dirname.h` family — `last_component`, `base_len`,
//! `dir_len`, and the two answers `basename` and `dirname` print — transcribed
//! over bytes.
//!
//! # Why this is a module and not two functions in two files
//!
//! Sixteen of this crate's binaries split a path, and almost all of them did it
//! with [`std::path::Path`]'s `file_name` and `parent`. Those are *host*
//! operations: on the build host they treat `\` as a separator and understand
//! drive letters, so `dirname 'a\b'` answered `a` where GNU answers `.`, and a
//! name containing a backslash — legal on our filesystem, which forbids only
//! `/` and NUL — was silently cut in a place the target would never cut it.
//! They are also lossy at the type level, since a path is bytes and `Path`'s
//! display side is text.
//!
//! The rules themselves are also not guessable, which is the other half of the
//! argument. Every one of these was measured against GNU 9.4 and contradicts
//! the obvious implementation:
//!
//! | Name | `dirname` | `basename` | Naive "split at the last `/`" |
//! |---|---|---|---|
//! | `/` | `/` | `/` | `` and `` |
//! | `//` | `/` | `/` | `/` and `` |
//! | `a/` | `.` | `a` | `a` and `` |
//! | `a//b//` | `a` | `b` | `a//b` and `` |
//! | `//a//b//` | `//a` | `b` | … and `` |
//! | `////a////b////` | `////a` | `b` | … and `` |
//! | `` | `.` | `` | `` and `` |
//!
//! Two patterns are worth naming because they are what a hand-written version
//! gets wrong. Trailing slashes are *ignored*, not stripped, so `a/` has no
//! directory part at all rather than a directory part of `a`. And a run of
//! slashes is collapsed only where it separates: `////a////b////` keeps its
//! leading four, because the walk stops at the start of the last component and
//! never rewrites what precedes it.
//!
//! # The three constants
//!
//! gnulib's originals are written for hosts we are not, and the conditional
//! code is compiled out by three macros. Rather than carry dead branches, each
//! is fixed here at the value our target gives it, and named where it applied:
//!
//! | gnulib macro | Here | Because |
//! |---|---|---|
//! | `ISSLASH(c)` | `c == b'/'` | `/` is the only separator; `\` is an ordinary byte in a name. |
//! | `FILE_SYSTEM_PREFIX_LEN` | `0` | No drive letters, so no name has a prefix before its root. |
//! | `DOUBLE_SLASH_IS_DISTINCT_ROOT` | `0` | `//` is the same directory as `/`, as on Linux. |
//!
//! The last is the one with a visible consequence, and it is why `dirname //`
//! answers `/` here. Note it does *not* make `//` disappear everywhere:
//! `dirname //a//b//` still answers `//a`, because those two slashes survive as
//! ordinary leading bytes rather than as a distinct root. Were the constant
//! ever flipped, `base_len` and `dir_len` would each gain the one branch marked
//! below — they are the only two places it is observable.

/// Is this byte a path separator?
///
/// gnulib's `ISSLASH`. Only `/`: our filesystem forbids `/` and NUL in a name
/// and permits every other byte, so `\` is part of a name and never a
/// separator.
#[must_use]
const fn is_slash(c: u8) -> bool {
    c == b'/'
}

/// Where the last component of `name` begins.
///
/// gnulib's `last_component`, as an offset rather than a pointer. Returns the
/// length of `name` when there is no last component — that is, for the empty
/// name and for one made entirely of slashes.
#[must_use]
pub fn last_component_offset(name: &[u8]) -> usize {
    // Skip the leading slashes: they are the root, not a component.
    let mut base = 0;
    while name.get(base).is_some_and(|&c| is_slash(c)) {
        base = base.saturating_add(1);
    }

    // Then take the start of every run of non-slashes after a slash. The last
    // one to be recorded is the answer, so a trailing run of slashes — which
    // records nothing — leaves `base` at the component before it.
    let mut last_was_slash = false;
    for (p, &c) in name.iter().enumerate().skip(base) {
        if is_slash(c) {
            last_was_slash = true;
        } else if last_was_slash {
            base = p;
            last_was_slash = false;
        }
    }
    base
}

/// The last component of `name`, with any trailing slashes still attached.
///
/// gnulib's `last_component`. `basename`'s answer is this passed through
/// [`base_len`]; use [`base_name`] for that in one step.
#[must_use]
pub fn last_component(name: &[u8]) -> &[u8] {
    name.get(last_component_offset(name)..).unwrap_or_default()
}

/// The length of `name` with its trailing slashes ignored.
///
/// gnulib's `base_len`. The `1 < len` guard is what keeps `/` as `/` rather
/// than reducing it to nothing: a name that is *only* slashes keeps its first
/// one.
#[must_use]
pub fn base_len(name: &[u8]) -> usize {
    let mut len = name.len();
    while 1 < len
        && name
            .get(len.saturating_sub(1))
            .is_some_and(|&c| is_slash(c))
    {
        len = len.saturating_sub(1);
    }
    // Here gnulib returns 2 for the name `//` when DOUBLE_SLASH_IS_DISTINCT_ROOT.
    len
}

/// The length of the directory part of `file` — `0` when there is none.
///
/// gnulib's `dir_len`. Note `0` rather than `"."`: the caller decides how to
/// render "no directory part", and [`dir_name`] is the one that chooses `.`.
///
/// Trailing slashes are ignored rather than stripped, so `a/` has *no*
/// directory part — the last component is `a`, and nothing precedes it.
#[must_use]
pub fn dir_len(file: &[u8]) -> usize {
    // A leading slash is the root and is never stripped, which is what makes
    // `dirname /etc` answer `/` instead of the empty name.
    // Here gnulib uses 2 instead of 1 for a `//` root when
    // DOUBLE_SLASH_IS_DISTINCT_ROOT.
    let prefix_length = usize::from(file.first().is_some_and(|&c| is_slash(c)));

    // Strip the last component, then the redundant slashes before it — but
    // never into the root.
    let mut length = last_component_offset(file);
    while prefix_length < length
        && file
            .get(length.saturating_sub(1))
            .is_some_and(|&c| is_slash(c))
    {
        length = length.saturating_sub(1);
    }
    length
}

/// What `dirname` prints for `file`.
///
/// gnulib's `mdir_name`, minus the allocation: the answer is always either a
/// prefix of `file` or the literal `.`, so it can be borrowed. `.` is the
/// answer whenever there is no directory part, which includes both `foo` and
/// the empty name.
#[must_use]
pub fn dir_name(file: &[u8]) -> &[u8] {
    let length = dir_len(file);
    if length == 0 {
        return b".";
    }
    file.get(..length).unwrap_or_default()
}

/// What `basename` prints for `name`, before any suffix removal.
///
/// gnulib's `base_name` followed by `strip_trailing_slashes`, which is how
/// coreutils' `perform_basename` calls them. The pair is transcribed as one
/// function because the first half of the work cancels: `base_name` collapses a
/// run of trailing slashes down to one, and `strip_trailing_slashes` then
/// removes that one. What survives is just "the last component, without its
/// trailing slashes".
///
/// The empty answer is real and is not an error: `basename ''` prints an empty
/// line, and this returns an empty slice for it.
#[must_use]
pub fn base_name(name: &[u8]) -> &[u8] {
    let base = last_component(name);
    // `last_component` is empty for a filesystem root and for the empty name.
    // Falling back to the whole name is what turns `///` into `/` — and it is
    // gnulib's own comment on `strip_trailing_slashes` that says so.
    let base = if base.is_empty() { name } else { base };
    base.get(..base_len(base)).unwrap_or_default()
}

/// `file` with any trailing slashes removed.
///
/// gnulib's `strip_trailing_slashes`, borrowing rather than truncating in
/// place. A filesystem root keeps one slash, so `///` becomes `/` and not the
/// empty name.
#[must_use]
pub fn strip_trailing_slashes(file: &[u8]) -> &[u8] {
    let base = last_component(file);
    let keep = if base.is_empty() { file } else { base };
    // The kept part ends `base_len` bytes into wherever it starts, and that
    // start is an offset into `file`, so the truncation point is measured from
    // the front of `file`.
    let start = file.len().saturating_sub(keep.len());
    let end = start.saturating_add(base_len(keep));
    file.get(..end).unwrap_or_default()
}

/// Is `name` a relative file name?
///
/// gnulib's `IS_RELATIVE_FILE_NAME`. `basename` uses it to decide whether a
/// `SUFFIX` may be removed at all: the answer for a filesystem root is the root
/// itself, and stripping a suffix off `/` would leave nothing.
#[must_use]
pub fn is_relative(name: &[u8]) -> bool {
    !name.first().is_some_and(|&c| is_slash(c))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Every row measured from GNU coreutils 9.4 on Linux, by printing
    /// `dirname X` and `basename X` for each name.
    ///
    /// The rows are the interesting shapes rather than a sample: each root
    /// form, each way of placing a slash run (leading, interior, trailing), the
    /// empty name, a name that looks like an option, and a name containing a
    /// backslash — which is one ordinary byte here and a separator on the
    /// build host, and so is the row that catches a `std::path` regression.
    const GNU: &[(&[u8], &[u8], &[u8])] = &[
        // name, dirname, basename
        (b"foo.txt", b".", b"foo.txt"),
        (b"/usr/bin/cat", b"/usr/bin", b"cat"),
        (b"dir/sub/file", b"dir/sub", b"file"),
        (b"/", b"/", b"/"),
        (b"", b".", b""),
        (b"/usr/bin/", b"/usr", b"bin"),
        (b"/etc", b"/", b"etc"),
        (b"foo/bar", b"foo", b"bar"),
        (b".", b".", b"."),
        (b"..", b".", b".."),
        (b"...", b".", b"..."),
        (b"/a/b/c/d/e/f", b"/a/b/c/d/e", b"f"),
        (b"//", b"/", b"/"),
        (b"///", b"/", b"/"),
        (b"//a", b"/", b"a"),
        (b"//a//b//", b"//a", b"b"),
        (b"a//b", b"a", b"b"),
        (b"a/", b".", b"a"),
        (b"a//", b".", b"a"),
        (b"////a////b////", b"////a", b"b"),
        (b"-", b".", b"-"),
        (b"a\\b", b".", b"a\\b"),
        (b"d/.", b"d", b"."),
        (b"d/..", b"d", b".."),
    ];

    #[test]
    fn dir_name_matches_gnu_on_every_measured_row() {
        for (name, want, _) in GNU {
            assert_eq!(
                dir_name(name),
                *want,
                "dirname {:?}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn base_name_matches_gnu_on_every_measured_row() {
        for (name, _, want) in GNU {
            assert_eq!(
                base_name(name),
                *want,
                "basename {:?}",
                String::from_utf8_lossy(name)
            );
        }
    }

    /// The property `dirname`'s documentation states: the directory part is a
    /// prefix of the name as typed, never a rewriting of it. This is what keeps
    /// the leading slashes of `////a////b////`.
    #[test]
    fn the_directory_part_is_always_a_prefix_of_the_name() {
        for (name, want, _) in GNU {
            if *want != b"." {
                assert!(
                    name.starts_with(want),
                    "{:?} is not a prefix of {:?}",
                    String::from_utf8_lossy(want),
                    String::from_utf8_lossy(name)
                );
            }
        }
    }

    #[test]
    fn a_backslash_is_an_ordinary_byte() {
        // The host would split these; the target must not.
        assert_eq!(dir_name(b"a\\b\\c"), b".");
        assert_eq!(base_name(b"a\\b\\c"), b"a\\b\\c");
        assert_eq!(dir_name(b"d/a\\b"), b"d");
        assert_eq!(base_name(b"d/a\\b"), b"a\\b");
    }

    #[test]
    fn a_name_need_not_be_utf8() {
        // A lone 0x80 is not valid UTF-8 and is a perfectly good file name.
        assert_eq!(base_name(b"/d/\x80\xff"), b"\x80\xff");
        assert_eq!(dir_name(b"/d/\x80\xff"), b"/d");
    }

    #[test]
    fn last_component_keeps_the_trailing_slashes_base_name_drops() {
        assert_eq!(last_component(b"a//b//"), b"b//");
        assert_eq!(base_name(b"a//b//"), b"b");
        // No last component at all: a root, or nothing.
        assert_eq!(last_component(b"///"), b"");
        assert_eq!(last_component(b""), b"");
    }

    #[test]
    fn base_len_keeps_one_slash_for_a_root() {
        assert_eq!(base_len(b"/"), 1);
        assert_eq!(base_len(b"//"), 1);
        assert_eq!(base_len(b"///"), 1);
        assert_eq!(base_len(b"a//"), 1);
        assert_eq!(base_len(b""), 0);
    }

    #[test]
    fn strip_trailing_slashes_leaves_a_root_alone() {
        assert_eq!(strip_trailing_slashes(b"a/b//"), b"a/b");
        assert_eq!(strip_trailing_slashes(b"a/b"), b"a/b");
        assert_eq!(strip_trailing_slashes(b"///"), b"/");
        assert_eq!(strip_trailing_slashes(b"/"), b"/");
        assert_eq!(strip_trailing_slashes(b""), b"");
        assert_eq!(strip_trailing_slashes(b"//a//"), b"//a");
    }

    #[test]
    fn dir_len_is_zero_where_dir_name_says_dot() {
        for (name, want, _) in GNU {
            assert_eq!(
                dir_len(name) == 0,
                *want == b".",
                "dir_len {:?}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn only_a_leading_slash_makes_a_name_absolute() {
        assert!(is_relative(b"a/b"));
        assert!(is_relative(b""));
        assert!(is_relative(b"a\\b"));
        assert!(!is_relative(b"/"));
        assert!(!is_relative(b"//a"));
    }
}
