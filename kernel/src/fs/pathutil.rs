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

    let mut out = PathBuf::with_capacity(base.len().saturating_add(rel.len()).saturating_add(1));
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

/// Returns `true` if `name` is one of the two self/parent directory entries
/// that `readdir` yields, `.` or `..`.
///
/// Every directory walk has to skip these — recursing into `.` never
/// terminates and recursing into `..` escapes upward — so the check is
/// spelled once here rather than open-coded per walk.  It takes a *name*
/// (a single component), not a path: `"a/.."` is not a dot entry.
#[must_use]
pub fn is_dot_entry<N: AsRef<Path>>(name: N) -> bool {
    let bytes = name.as_ref().as_bytes();
    bytes == b"." || bytes == b".."
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

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------
//
// These checks were written as a `#[cfg(test)] mod tests` and therefore never
// ran: `kernel/Cargo.toml` sets `test = false` on the kernel binary and there
// is no `lib.rs`, so `cargo test -p kernel` builds no test target at all.  That
// is the correct call for a `#![no_std]` binary supplying its own `panic_impl`
// — it genuinely cannot link a host harness — but it left ten checks on a trust
// boundary that had never once executed.  They are boot self-tests now, which
// is the mechanism this kernel actually runs.  See `known-issues.md`
// → `A-KERNEL-UNIT-TESTS-NEVER-RUN`.
//
// Note the shape difference from a unit test: a boot self-test must not
// `assert!`.  A failed assertion is a panic, and a panic during boot is a dead
// machine rather than a failed test — so every check reports and returns `Err`,
// and the caller logs and carries on.

/// Compare one predicate result against its expectation.
fn expect_pred(
    got: bool,
    want: bool,
    what: &str,
    path: &str,
    dir: &str,
) -> crate::error::KernelResult<()> {
    if got == want {
        return Ok(());
    }
    crate::serial_println!("[pathutil]   FAIL: {what}({path:?}, {dir:?}) = {got}, expected {want}");
    Err(crate::error::KernelError::InternalError)
}

/// [`path_in_subtree`] — component boundaries, trailing slashes, empty dir.
fn check_in_subtree() -> crate::error::KernelResult<()> {
    // (path, dir, expected)
    const CASES: &[(&str, &str, bool)] = &[
        // The match ends on a component boundary, so a shared *byte* prefix
        // is not a subtree.
        ("/a/b", "/a", true),
        ("/a", "/a", true),
        ("/ab", "/a", false),
        ("/b/a", "/a", false),
        // The bug class this module exists to kill: a dir that already ends
        // in `/` must still match its real children.
        ("/protected/secret.txt", "/protected/", true),
        ("/protected", "/protected/", true),
        ("/protectedX/file", "/protected/", false),
        // An empty dir, and `/`, both mean "the whole tree".
        ("/anything/here", "", true),
        ("/", "", true),
        ("/anything/here", "/", true),
    ];
    for &(path, dir, want) in CASES {
        expect_pred(
            path_in_subtree(path, dir),
            want,
            "path_in_subtree",
            path,
            dir,
        )?;
    }

    // Slashed and unslashed spellings of the same dir must be
    // indistinguishable — the property, rather than a list of cases.
    for p in ["/d", "/d/x", "/d/x/y", "/dx", "/e"] {
        if path_in_subtree(p, "/d") != path_in_subtree(p, "/d/") {
            crate::serial_println!("[pathutil]   FAIL: '/d' and '/d/' disagree for {p:?}");
            return Err(crate::error::KernelError::InternalError);
        }
    }

    crate::serial_println!(
        "[pathutil]   path_in_subtree: {} cases + slash equivalence OK",
        CASES.len()
    );
    Ok(())
}

/// [`path_strictly_under`] — same boundaries, but excluding the dir itself.
fn check_strictly_under() -> crate::error::KernelResult<()> {
    const CASES: &[(&str, &str, bool)] = &[
        ("/a/b", "/a", true),
        ("/a", "/a", false),
        ("/a", "/a/", false),
        ("/a/b", "/a/", true),
        ("/ab", "/a", false),
        // Everything with a component is strictly under the root; the root
        // is not strictly under itself.
        ("/a", "/", true),
        ("/a/b", "/", true),
        ("/", "/", false),
    ];
    for &(path, dir, want) in CASES {
        expect_pred(
            path_strictly_under(path, dir),
            want,
            "path_strictly_under",
            path,
            dir,
        )?;
    }
    crate::serial_println!("[pathutil]   path_strictly_under: {} cases OK", CASES.len());
    Ok(())
}

/// Subtree matching on components that are not valid UTF-8.
///
/// This is the property the whole byte-`Path` conversion exists for: before it
/// such a path could not even be *spelled*, so a subtree check involving one
/// was unreachable code.
fn check_non_utf8() -> crate::error::KernelResult<()> {
    let dir = Path::new(b"/data/\xff");
    let inside = Path::new(b"/data/\xff/file");
    let other = Path::new(b"/data/\xfe/file");

    if !path_in_subtree(inside, dir)
        || !path_strictly_under(inside, dir)
        // A different trailing byte is a different directory.
        || path_in_subtree(other, dir)
    {
        crate::serial_println!("[pathutil]   FAIL: non-UTF-8 component matching");
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[pathutil]   non-UTF-8 components match by bytes: OK");
    Ok(())
}

/// [`confine_under`] — the joins that must succeed.
fn check_confine_joins() -> crate::error::KernelResult<()> {
    // (base, rel, expected result)
    const CASES: &[(&str, &str, &str)] = &[
        ("/dest", "a/b.txt", "/dest/a/b.txt"),
        // Trailing separators on either side collapse to exactly one.
        ("/dest/", "a/", "/dest/a"),
        // A leading `/` on the member is stripped, not honoured: a tar member
        // named `/etc/passwd` denotes `etc/passwd` *inside* the archive, and
        // treating it as absolute is the same escape by another spelling.
        ("/dest", "/etc/passwd", "/dest/etc/passwd"),
        // `.` components drop out.
        ("/dest", "./a/./b", "/dest/a/b"),
    ];
    for &(base, rel, want) in CASES {
        match confine_under(base, rel) {
            Ok(got) if got == PathBuf::from(want) => {}
            Ok(got) => {
                crate::serial_println!(
                    "[pathutil]   FAIL: confine_under({base:?}, {rel:?}) = {:?}, expected {want:?}",
                    got.as_bytes()
                );
                return Err(crate::error::KernelError::InternalError);
            }
            Err(e) => {
                crate::serial_println!(
                    "[pathutil]   FAIL: confine_under({base:?}, {rel:?}) errored: {e:?}"
                );
                return Err(crate::error::KernelError::InternalError);
            }
        }
    }

    // Bytes that are not UTF-8 survive the join intact.
    match confine_under("/dest", Path::new(b"re\xffport")) {
        Ok(got) if got == PathBuf::from(b"/dest/re\xffport".as_slice()) => {}
        _ => {
            crate::serial_println!("[pathutil]   FAIL: confine_under lost non-UTF-8 bytes");
            return Err(crate::error::KernelError::InternalError);
        }
    }

    crate::serial_println!(
        "[pathutil]   confine_under joins: {} cases + non-UTF-8 OK",
        CASES.len()
    );
    Ok(())
}

/// [`confine_under`] — the inputs that must be refused.
///
/// This is the security half: every one of these is an attempt to write
/// outside `base` (the "Zip Slip" class), or an input that names `base`
/// itself rather than something inside it.
fn check_confine_escapes() -> crate::error::KernelResult<()> {
    // (base, rel, why it must be refused)
    const CASES: &[(&str, &str, &str)] = &[
        ("/dest", "../etc/passwd", "leading .."),
        ("/dest", "a/../../etc/passwd", ".. after a real component"),
        (
            "/dest",
            "/../etc/passwd",
            ".. behind a stripped leading slash",
        ),
        ("/dest", "a/..", "trailing .."),
        ("/dest", "", "empty names the base itself"),
        ("/dest", "/", "root names the base itself"),
        ("/dest", ".", "dot names the base itself"),
        ("", "a", "empty base names nothing"),
    ];
    for &(base, rel, why) in CASES {
        if let Ok(got) = confine_under(base, rel) {
            crate::serial_println!(
                "[pathutil]   FAIL: confine_under({base:?}, {rel:?}) accepted ({why}) -> {:?}",
                got.as_bytes()
            );
            return Err(crate::error::KernelError::InternalError);
        }
    }

    // A NUL byte cannot appear in a path at all.
    if confine_under("/dest", Path::new(b"a\0b")).is_ok() {
        crate::serial_println!("[pathutil]   FAIL: confine_under accepted an embedded NUL");
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!(
        "[pathutil]   confine_under refuses escapes: {} cases + NUL OK",
        CASES.len()
    );
    Ok(())
}

/// [`is_dot_entry`] and [`contains_bytes`].
fn check_dot_and_substring() -> crate::error::KernelResult<()> {
    // (name, expected)
    const DOTS: &[(&str, bool)] = &[
        (".", true),
        ("..", true),
        ("...", false),
        (".a", false),
        ("", false),
        // A *path* ending in `..` is not a dot *entry*: the check is on a
        // single component, so this must not be mistaken for one.
        ("a/..", false),
    ];
    for &(name, want) in DOTS {
        expect_pred(
            is_dot_entry(Path::new(name)),
            want,
            "is_dot_entry",
            name,
            "",
        )?;
    }

    // (haystack, needle, expected)
    const SUBS: &[(&[u8], &[u8], bool)] = &[
        (b"report.txt", b"port", true),
        (b"report.txt", b"report.txt", true),
        // An empty needle matches everything, as `str::contains("")` does.
        (b"report.txt", b"", true),
        // Case matters — these are bytes, not a locale-aware comparison.
        (b"report.txt", b"Port", false),
        // A needle longer than the haystack cannot match.
        (b"ab", b"abc", false),
        // A needle straddling a non-UTF-8 byte still matches by bytes.
        (b"re\xffport", b"\xffpo", true),
    ];
    for &(haystack, needle, want) in SUBS {
        if contains_bytes(haystack, needle) != want {
            crate::serial_println!(
                "[pathutil]   FAIL: contains_bytes({haystack:?}, {needle:?}) != {want}"
            );
            return Err(crate::error::KernelError::InternalError);
        }
    }

    crate::serial_println!(
        "[pathutil]   is_dot_entry ({}) and contains_bytes ({}): OK",
        DOTS.len(),
        SUBS.len()
    );
    Ok(())
}

/// Boot self-test for the path predicates.
///
/// Runs entirely on constants — no disk, no allocator pressure beyond a few
/// short `PathBuf`s — so it can run early in boot, before any filesystem is
/// mounted.
///
/// # Errors
/// [`KernelError::InternalError`] if any predicate disagrees with its
/// expectation.  The specific failing case is logged to the serial console
/// first.
pub fn self_test() -> crate::error::KernelResult<()> {
    crate::serial_println!("[pathutil] Running self-test...");
    check_in_subtree()?;
    check_strictly_under()?;
    check_non_utf8()?;
    check_confine_joins()?;
    check_confine_escapes()?;
    check_dot_and_substring()?;
    crate::serial_println!("[pathutil] Self-test PASSED");
    Ok(())
}
