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
//! [`path_in_subtree`] is the single canonical predicate: it normalises
//! away an optional trailing slash and applies a path-component boundary
//! check, so it is correct whether or not the prefix carries a trailing
//! slash.  All subtree checks should route through it rather than
//! re-deriving the boundary logic.

/// Returns `true` if `path` lies within the directory subtree denoted by
/// `dir` — that is, `path` equals `dir` or is strictly underneath it.
///
/// `dir` may optionally carry a single trailing `/`; it is normalised
/// away before the boundary check, so `"/a"` and `"/a/"` behave
/// identically.  An empty `dir` (or `"/"`, which normalises to empty)
/// matches every path.  The match must end on a path-component boundary,
/// so `dir = "/a"` matches `"/a"` and `"/a/b"` but never `"/ab"`.
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
pub fn path_in_subtree(path: &str, dir: &str) -> bool {
    // Normalise away a single trailing slash so the boundary check is
    // uniform whether or not the caller supplied one.
    let d = dir.strip_suffix('/').unwrap_or(dir);
    if d.is_empty() {
        // Empty prefix, or `dir` was exactly "/": the whole tree.
        return true;
    }
    path == d || (path.starts_with(d) && path.as_bytes().get(d.len()) == Some(&b'/'))
}

/// Returns `true` if `path` is *strictly* underneath `dir` (i.e. a
/// descendant), excluding `dir` itself.
///
/// Same trailing-slash normalisation and component-boundary semantics as
/// [`path_in_subtree`], but `path == dir` returns `false`.  Useful for
/// "list children" / "has descendants" checks where the directory node
/// itself must not be counted.
#[must_use]
pub fn path_strictly_under(path: &str, dir: &str) -> bool {
    let d = dir.strip_suffix('/').unwrap_or(dir);
    if d.is_empty() {
        // Everything except the root itself is strictly under the root.
        return !path.is_empty() && path != "/";
    }
    path.len() > d.len()
        && path.starts_with(d)
        && path.as_bytes().get(d.len()) == Some(&b'/')
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
}
