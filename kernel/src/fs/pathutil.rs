//! Path subtree matching utilities.
//!
//! A recurring footgun across the filesystem subsystem was the inline
//! "is `path` inside directory `prefix`" check written as:
//!
//! ```ignore
//! path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/')
//! ```
//!
//! That idiom is only correct when `prefix` has **no** trailing slash.
//! When callers register or build a prefix that already ends in `/`
//! (e.g. `"/protected/"` or `format!("{dir}/")`), `get(prefix.len())`
//! inspects the byte *after* the slash, so the check only matches
//! double-slash paths (`/protected//x`).  Real children never match,
//! which made deny handlers fail open (see `fs::intercept`) and made
//! "missing file" / column-discovery logic silently no-op (see
//! `fs::integrity`, `fs::findex`).
//!
//! The boundary rule itself now lives in [`Path::starts_with`], which
//! compares *components* rather than bytes and so cannot be fooled by a
//! trailing slash or by a shared byte prefix (`/ab` vs `/a`).  The two
//! predicates here remain as the canonical spelling of the two subtree
//! questions, and they add the one thing `Path::starts_with` deliberately
//! does not provide: the caller convenience that an **empty** directory
//! argument means "the whole tree".  `Path::starts_with` refuses to call a
//! relative prefix a prefix of an absolute path — correct for paths, wrong
//! for this "no filter configured" sentinel — so that case is spelled out
//! rather than delegated.

use crate::error::{KernelError, KernelResult};

use super::path::{Path, PathBuf};

/// Returns `true` if `path` lies within the directory subtree denoted by
/// `dir` — that is, `path` equals `dir` or is strictly underneath it.
///
/// `dir` may optionally carry a single trailing `/`; component matching
/// ignores it, so `"/a"` and `"/a/"` behave identically.  An empty `dir`
/// (or `"/"`) matches every path.  The match ends on a path-component
/// boundary, so `dir = "/a"` matches `"/a"` and `"/a/b"` but never `"/ab"`.
///
/// # Examples
/// ```ignore
/// assert!(path_in_subtree("/a/b", "/a"));
/// assert!(path_in_subtree("/a/b", "/a/"));   // trailing slash tolerated
/// assert!(path_in_subtree("/a", "/a"));      // the dir itself
/// assert!(!path_in_subtree("/ab", "/a"));    // not a component boundary
/// assert!(path_in_subtree("/anything", "")); // empty matches all
/// ```
#[must_use]
pub fn path_in_subtree<P: AsRef<Path>, D: AsRef<Path>>(path: P, dir: D) -> bool {
    let dir = dir.as_ref();
    if dir.components().next().is_none() {
        // Empty prefix, or `dir` was exactly "/": the whole tree.
        return true;
    }
    path.as_ref().starts_with(dir)
}

/// Returns `true` if `path` is *strictly* underneath `dir` (i.e. a
/// descendant), excluding `dir` itself.
///
/// Same trailing-slash tolerance and component-boundary semantics as
/// [`path_in_subtree`], but `path == dir` returns `false`.  Useful for
/// "list children" / "has descendants" checks where the directory node
/// itself must not be counted.
#[must_use]
pub fn path_strictly_under<P: AsRef<Path>, D: AsRef<Path>>(path: P, dir: D) -> bool {
    let (path, dir) = (path.as_ref(), dir.as_ref());
    if dir.components().next().is_none() {
        // Everything with at least one component is strictly under the root.
        return path.components().next().is_some();
    }
    path.starts_with(dir) && path.components().count() > dir.components().count()
}

/// Join `rel` underneath the directory `base`, refusing anything that could
/// escape it.
///
/// This is the guard every archive extractor and every "copy into a jail"
/// operation needs.  An archive member (or container `cp` argument) named
/// `../../etc/passwd` must not be able to write outside `base` — the "Zip
/// Slip" bug class.
///
/// A leading `/` on `rel` is **stripped**, not honoured: a tar member named
/// `/etc/passwd` denotes `etc/passwd` *inside the archive*, and treating it as
/// absolute is the same escape by another spelling.  `.` components are
/// dropped, and a trailing separator on either argument is ignored.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if `base` is empty, if `rel` contains a
///   NUL byte or any `..` component, or if `rel` has no real component (which
///   would name `base` itself rather than something inside it).
pub fn confine_under<B: AsRef<Path> + ?Sized, R: AsRef<Path> + ?Sized>(
    base: &B,
    rel: &R,
) -> KernelResult<PathBuf> {
    let (base, rel) = (base.as_ref(), rel.as_ref());
    if base.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if rel.as_bytes().contains(&0) {
        return Err(KernelError::InvalidArgument);
    }

    // Trim trailing separators off `base` so the join inserts exactly one.
    let base_bytes = base.as_bytes();
    let mut end = base_bytes.len();
    while end > 0 && base_bytes.get(end.wrapping_sub(1)) == Some(&b'/') {
        end = end.wrapping_sub(1);
    }
    let base_trimmed = base_bytes.get(..end).unwrap_or(base_bytes);

    let mut out =
        PathBuf::with_capacity(base.len().saturating_add(rel.len()).saturating_add(1));
    out.extend_bytes(base_trimmed);

    // `components()` already drops empty components, so a leading `/`, a
    // trailing `/` and any `//` run need no separate handling.
    let mut any_real = false;
    for comp in rel.components() {
        let bytes = comp.as_bytes();
        if bytes == b".." {
            return Err(KernelError::InvalidArgument);
        }
        if bytes == b"." {
            continue;
        }
        out.extend_bytes(b"/");
        out.extend_bytes(bytes);
        any_real = true;
    }
    if !any_real {
        return Err(KernelError::InvalidArgument);
    }
    Ok(out)
}

/// Returns `true` if `haystack` contains `needle` as a contiguous byte
/// subsequence.
///
/// `[u8]` has no `contains` for subslices (only for single elements), so this
/// is the byte analogue of `str::contains` — needed wherever a *substring*
/// filter is applied to a path or file name, which are byte strings and may
/// not be valid UTF-8.  An empty needle matches everything, matching
/// `str::contains("")`.
#[must_use]
pub fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtree_basic_boundary() {
        assert!(path_in_subtree("/a/b", "/a"));
        assert!(path_in_subtree("/a", "/a"));
        assert!(!path_in_subtree("/ab", "/a"));
        assert!(!path_in_subtree("/b/a", "/a"));
    }

    #[test]
    fn subtree_trailing_slash_tolerated() {
        // The exact bug class: a prefix with a trailing slash must still
        // match real children.
        assert!(path_in_subtree("/protected/secret.txt", "/protected/"));
        assert!(path_in_subtree("/protected", "/protected/"));
        assert!(!path_in_subtree("/protectedX/file", "/protected/"));
        // Equivalence between slashed and unslashed forms.
        for p in ["/d", "/d/x", "/d/x/y", "/dx", "/e"] {
            assert_eq!(
                path_in_subtree(p, "/d"),
                path_in_subtree(p, "/d/"),
                "slashed vs unslashed mismatch for {p}"
            );
        }
    }

    #[test]
    fn subtree_empty_and_root_match_all() {
        assert!(path_in_subtree("/anything/here", ""));
        assert!(path_in_subtree("/", ""));
        assert!(path_in_subtree("/anything/here", "/"));
    }

    #[test]
    fn strictly_under_excludes_self() {
        assert!(path_strictly_under("/a/b", "/a"));
        assert!(!path_strictly_under("/a", "/a"));
        assert!(!path_strictly_under("/a", "/a/"));
        assert!(path_strictly_under("/a/b", "/a/"));
        assert!(!path_strictly_under("/ab", "/a"));
    }

    #[test]
    fn strictly_under_root() {
        assert!(path_strictly_under("/a", "/"));
        assert!(path_strictly_under("/a/b", "/"));
        assert!(!path_strictly_under("/", "/"));
    }

    /// A name that is not UTF-8 must still match by bytes.  This is the
    /// property the whole byte-`Path` conversion exists for: before it, such
    /// a path could not even be *spelled*, so a subtree check involving one
    /// was unreachable code.
    #[test]
    fn subtree_matches_non_utf8_components() {
        let dir = Path::new(b"/data/\xff");
        assert!(path_in_subtree(Path::new(b"/data/\xff/file"), dir));
        assert!(path_strictly_under(Path::new(b"/data/\xff/file"), dir));
        // A different trailing byte is a different directory.
        assert!(!path_in_subtree(Path::new(b"/data/\xfe/file"), dir));
    }

    #[test]
    fn confine_under_normal_joins() {
        assert_eq!(confine_under("/dest", "a/b.txt").unwrap(), PathBuf::from("/dest/a/b.txt"));
        // Trailing separators on either side collapse to exactly one.
        assert_eq!(confine_under("/dest/", "a/").unwrap(), PathBuf::from("/dest/a"));
        // A leading slash on the member is stripped, not honoured.
        assert_eq!(confine_under("/dest", "/etc/passwd").unwrap(), PathBuf::from("/dest/etc/passwd"));
        // `.` components are dropped.
        assert_eq!(confine_under("/dest", "./a/./b").unwrap(), PathBuf::from("/dest/a/b"));
        // Bytes that are not UTF-8 survive intact.
        assert_eq!(
            confine_under("/dest", Path::new(b"re\xffport")).unwrap(),
            PathBuf::from(b"/dest/re\xffport".as_slice())
        );
    }

    #[test]
    fn confine_under_rejects_escapes() {
        // The Zip Slip case, in each of its spellings.
        assert!(confine_under("/dest", "../etc/passwd").is_err());
        assert!(confine_under("/dest", "a/../../etc/passwd").is_err());
        assert!(confine_under("/dest", "/../etc/passwd").is_err());
        assert!(confine_under("/dest", "a/..").is_err());
        // Naming the base itself is not "something inside it".
        assert!(confine_under("/dest", "").is_err());
        assert!(confine_under("/dest", "/").is_err());
        assert!(confine_under("/dest", ".").is_err());
        // A NUL byte cannot appear in a path.
        assert!(confine_under("/dest", Path::new(b"a\0b")).is_err());
        // An empty base names nothing.
        assert!(confine_under("", "a").is_err());
    }

    #[test]
    fn contains_bytes_basics() {
        assert!(contains_bytes(b"report.txt", b"port"));
        assert!(contains_bytes(b"report.txt", b"report.txt"));
        assert!(contains_bytes(b"report.txt", b""));
        assert!(!contains_bytes(b"report.txt", b"Port"));
        assert!(!contains_bytes(b"ab", b"abc"));
        // A needle that straddles a non-UTF-8 byte still matches by bytes.
        assert!(contains_bytes(b"re\xffport", b"\xffpo"));
    }
}
