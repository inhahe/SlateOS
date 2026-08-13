//! Filesystem paths as byte strings.
//!
//! ## Why this exists
//!
//! `CLAUDE.md` is explicit: *"Never force UTF-8 on filesystem paths... Our
//! paths allow all bytes except `/` and `\0`."* The VFS nevertheless took
//! `&str` throughout, so every syscall handler had to run
//! `String::from_utf8` on the bytes it had just copied from userspace and
//! reject anything that failed. A file whose name contains a lone `0x80` —
//! legal under our own rules, and routinely produced by archives, foreign
//! filesystems and non-UTF-8 locales — could therefore be *created* by a lower
//! layer (ext4 does not care) and then be permanently unopenable, unlistable
//! by name and undeletable through the syscall API. See
//! `known-issues.md` § `D-VFS-PATHS-ARE-STR-NOT-BYTES`.
//!
//! [`Path`] and [`PathBuf`] are the byte-string types that replace `&str` and
//! `String` in the VFS. They are deliberately shaped like `std`'s so the code
//! reads the same, but they are `[u8]`/`Vec<u8>` underneath with **no UTF-8
//! invariant at all**.
//!
//! ## Lossy conversion is for display only
//!
//! [`Path::display`] renders undecodable bytes as U+FFFD. That is legitimate
//! *for a log line or a `/proc` file* and nowhere else: a lossy path can never
//! be fed back into a lookup, because the replacement character does not name
//! the file the bytes came from. Anything that reopens, compares or stores a
//! path must use [`Path::as_bytes`].
//!
//! ## Path syntax
//!
//! Per `design.txt`: forward slash separator, case-sensitive, every byte
//! allowed except `/` (the separator) and `\0` (the C-string terminator that
//! the POSIX layer must be able to round-trip through). There is no drive
//! letter, no backslash and no notion of a "verbatim" prefix, so the whole
//! `std::path::Component` taxonomy collapses to: an optional leading `/`,
//! then a sequence of non-empty byte components.

#![allow(dead_code)]

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

// ---------------------------------------------------------------------------
// Path — the borrowed form
// ---------------------------------------------------------------------------

/// A borrowed filesystem path: an arbitrary byte string.
///
/// Unsized, like `str` and `std::path::Path`, so `&Path` is a fat pointer and
/// a `Path` can be created by reinterpreting an existing `[u8]` without
/// copying. Construct one with [`Path::new`].
#[repr(transparent)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path([u8]);

impl Path {
    /// Wrap a byte string as a path, without copying.
    ///
    /// Accepts anything that is already bytes — `&[u8]`, `&str`, `&String`,
    /// `&Vec<u8>` — because a `&str` *is* a valid path, just one that happens
    /// to be UTF-8. The reverse is not true, which is the whole point of this
    /// module.
    #[must_use]
    pub fn new<S: AsRef<[u8]> + ?Sized>(s: &S) -> &Self {
        // SAFETY: `Path` is `#[repr(transparent)]` over `[u8]`, so the two
        // types have identical layout and metadata (the slice length). The
        // cast preserves the fat pointer's length, and the returned reference
        // borrows from `s`, so the lifetime is the input's. There is no
        // validity invariant on `Path` beyond `[u8]`'s — deliberately, since
        // "any bytes" is the property this type exists to provide.
        unsafe { &*(core::ptr::from_ref::<[u8]>(s.as_ref()) as *const Self) }
    }

    /// The path's bytes. This is the form to compare, hash, store or hand to
    /// a filesystem — never [`Self::display`].
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Length in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the path is the empty byte string. An empty path names nothing
    /// and every VFS entry point rejects it.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the path starts at the filesystem root.
    #[must_use]
    pub fn is_absolute(&self) -> bool {
        self.0.first() == Some(&b'/')
    }

    /// The path as UTF-8, or `None` if it is not valid UTF-8.
    ///
    /// For interoperating with the parts of the kernel that still legitimately
    /// want text (mount type names, `/proc` node names that we ourselves
    /// chose). Never use it to decide whether a *user-supplied* path is
    /// acceptable — that is exactly the bug this module removes.
    #[must_use]
    pub fn to_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.0).ok()
    }

    /// A `Display` adapter that renders undecodable bytes as U+FFFD.
    ///
    /// **For human-readable output only.** The result cannot be turned back
    /// into a path: U+FFFD does not name the byte it replaced, so a lossily
    /// rendered path reopens a *different* file, or none.
    #[must_use]
    pub const fn display(&self) -> Display<'_> {
        Display(&self.0)
    }

    /// Iterate the non-empty components, skipping the leading `/` and any
    /// repeated or trailing separators.
    ///
    /// `"/a//b/"` and `"a/b"` both yield `["a", "b"]`. `.` and `..` are
    /// yielded verbatim — resolving them is the VFS's job, not the lexer's,
    /// because whether `..` may escape depends on the caller's root.
    /// Returns a `DoubleEndedIterator` — [`Self::file_name`] and
    /// [`Self::parent`] both want the *last* component, and a forward-only
    /// iterator would force them to walk the whole path to find it.
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &Path> {
        self.0
            .split(|&b| b == b'/')
            .filter(|c| !c.is_empty())
            .map(Path::new)
    }

    /// The final component, or `None` for `/`, the empty path, or a path
    /// consisting only of separators.
    #[must_use]
    pub fn file_name(&self) -> Option<&Path> {
        self.components().next_back()
    }

    /// Everything before the final component, keeping a leading `/`.
    ///
    /// `/a/b` → `/a`; `/a` → `/`; `/` → `None`; `a` → `None`. A trailing
    /// separator is ignored, so `/a/b/` also yields `/a`.
    #[must_use]
    pub fn parent(&self) -> Option<&Path> {
        // Ignore trailing separators first: the parent of `/a/b/` is `/a`,
        // not `/a/b`.
        let mut end = self.0.len();
        while end > 0 && self.0.get(end.wrapping_sub(1)) == Some(&b'/') {
            end = end.wrapping_sub(1);
        }
        let trimmed = self.0.get(..end)?;
        // Find the separator that starts the final component.
        let sep = trimmed.iter().rposition(|&b| b == b'/')?;
        if sep == 0 {
            // The only separator is the root one: the parent is `/` itself,
            // not the empty path, which would name nothing.
            return Some(Path::new(b"/"));
        }
        trimmed.get(..sep).map(Path::new)
    }

    /// The extension of [`Self::file_name`] — the bytes after the last `.`,
    /// or `None` when there is no `.`, when the name *starts* with the only
    /// `.` (a dotfile is not an extension), or when nothing follows it.
    #[must_use]
    pub fn extension(&self) -> Option<&Path> {
        let name = self.file_name()?.as_bytes();
        let dot = name.iter().rposition(|&b| b == b'.')?;
        if dot == 0 || dot.wrapping_add(1) >= name.len() {
            return None;
        }
        name.get(dot.wrapping_add(1)..).map(Path::new)
    }

    /// Whether `prefix` is a leading *component-aligned* prefix of `self`.
    ///
    /// Component-aligned is the whole point: `/ab` does not start with `/a`,
    /// even though the bytes do. Getting this wrong is how the subtree checks
    /// documented in [`super::pathutil`] failed open.
    #[must_use]
    pub fn starts_with<P: AsRef<Self>>(&self, prefix: P) -> bool {
        let mut ours = self.components();
        for want in prefix.as_ref().components() {
            if ours.next() != Some(want) {
                return false;
            }
        }
        // An absolute prefix cannot match a relative path or vice versa;
        // a prefix of no components (`/`, ``) matches only the same kind.
        self.is_absolute() == prefix.as_ref().is_absolute()
    }

    /// `self` with a component-aligned `prefix` removed, or `None` if
    /// [`Self::starts_with`] is false.
    ///
    /// The result is always relative and never has a leading separator.
    #[must_use]
    pub fn strip_prefix<P: AsRef<Self>>(&self, prefix: P) -> Option<PathBuf> {
        if !self.starts_with(&prefix) {
            return None;
        }
        let skip = prefix.as_ref().components().count();
        let mut out = PathBuf::new();
        for c in self.components().skip(skip) {
            out.push(c);
        }
        Some(out)
    }

    /// `self` with `other` appended as a child component. An absolute `other`
    /// replaces `self` entirely, matching `std`.
    #[must_use]
    pub fn join<P: AsRef<Self>>(&self, other: P) -> PathBuf {
        let mut out = self.to_path_buf();
        out.push(other);
        out
    }

    /// Copy into an owned [`PathBuf`].
    #[must_use]
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf(self.0.to_owned())
    }

    /// Whether every byte is legal in a path — i.e. no NUL.
    ///
    /// `/` is *not* checked here: it is the separator, so it is legal in a
    /// path and illegal only inside a single component. NUL is rejected
    /// because the POSIX layer must be able to hand the name back as a C
    /// string, and a name it cannot round-trip is a name it cannot reopen.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.0.contains(&0)
    }

    /// Whether every component is a legal file name — no NUL, and not `.` or
    /// `..`, which are directory references rather than names.
    #[must_use]
    pub fn has_no_dot_components(&self) -> bool {
        self.components().all(|c| {
            let b = c.as_bytes();
            b != b"." && b != b".."
        })
    }
}

// Only the *unsized* / owned forms get an impl. Reference forms (`&Path`,
// `&str`, `&[u8]`, `&PathBuf`) are covered by core's blanket
// `impl<T: ?Sized, U: ?Sized> AsRef<U> for &T where T: AsRef<U>`; writing them
// out by hand collides with it (E0119).
impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}
impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}
impl AsRef<Path> for String {
    fn as_ref(&self) -> &Path {
        Path::new(self.as_str())
    }
}
impl AsRef<Path> for [u8] {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}
// Deliberately *no* `impl<const N: usize> AsRef<Path> for [u8; N]`, even
// though it would let byte-string literals (`b"/proc"`, which is `&[u8; 5]`,
// not `&[u8]`) be passed directly. Adding it makes every `.as_ref()` call on a
// byte array anywhere in the crate ambiguous — core already supplies
// `[u8; N]: AsRef<[u8]>`, so a second candidate breaks inference at unrelated
// call sites (it broke 8 in `fs::encrypt` and `fs::fcompress` on first
// attempt). `std::path::Path` avoids the same trap by not implementing
// `AsRef<Path>` for byte types at all. Byte literals are rare in path code —
// nearly every literal path is a `&str` — so the few that exist say
// `Path::new(b"/proc")` explicitly.
impl AsRef<Path> for Vec<u8> {
    fn as_ref(&self) -> &Path {
        Path::new(self.as_slice())
    }
}
impl AsRef<[u8]> for Path {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Path {
    /// Debug output is the lossy rendering in quotes. A `Path` in a panic
    /// message or a `{:?}` log line is being *read by a human*, so the same
    /// display-only caveat applies as to [`Path::display`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\"", self.display())
    }
}

// ---------------------------------------------------------------------------
// PathBuf — the owned form
// ---------------------------------------------------------------------------

/// An owned filesystem path. The [`Path`] to `PathBuf` relation is `str` to
/// `String`.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathBuf(Vec<u8>);

impl PathBuf {
    /// An empty path.
    #[must_use]
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// An empty path with room for `cap` bytes.
    #[must_use]
    pub fn with_capacity(cap: usize) -> Self {
        Self(Vec::with_capacity(cap))
    }

    /// Take ownership of a byte vector as a path, without copying.
    #[must_use]
    pub const fn from_vec(v: Vec<u8>) -> Self {
        Self(v)
    }

    /// Give up the bytes.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }

    /// Borrow as a [`Path`].
    #[must_use]
    pub fn as_path(&self) -> &Path {
        Path::new(self.0.as_slice())
    }

    /// Append a component, inserting a separator if one is needed.
    ///
    /// An absolute `other` *replaces* the whole path, matching `std`: joining
    /// a caller-supplied absolute path onto a base must not silently produce
    /// a path under the base, because the caller asked for the root.
    pub fn push<P: AsRef<Path>>(&mut self, other: P) {
        let other = other.as_ref().as_bytes();
        if other.first() == Some(&b'/') {
            self.0.clear();
            self.0.extend_from_slice(other);
            return;
        }
        if other.is_empty() {
            return;
        }
        if !self.0.is_empty() && self.0.last() != Some(&b'/') {
            self.0.push(b'/');
        }
        self.0.extend_from_slice(other);
    }

    /// Remove the final component, returning whether there was one to remove.
    pub fn pop(&mut self) -> bool {
        match self.as_path().parent() {
            Some(p) => {
                let bytes = p.as_bytes().to_owned();
                self.0 = bytes;
                true
            }
            None => false,
        }
    }

    /// Append raw bytes with no separator handling — for building a name a
    /// component at a time (e.g. adding an extension).
    pub fn extend_bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// Drop all components.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl core::ops::Deref for PathBuf {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.as_path()
    }
}

/// Lets a `BTreeMap<PathBuf, _>` or `BTreeSet<PathBuf>` be looked up by
/// `&Path` without allocating an owned key for every probe — the same reason
/// `String: Borrow<str>` exists. `Ord`/`Eq`/`Hash` must agree between the two
/// types for this to be sound, and they do: both derive from the same byte
/// slice, since `PathBuf`'s impls delegate to `Vec<u8>` and `Path`'s to `[u8]`.
impl core::borrow::Borrow<Path> for PathBuf {
    fn borrow(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<Path> for PathBuf {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}
impl AsRef<[u8]> for PathBuf {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<&Path> for PathBuf {
    fn from(p: &Path) -> Self {
        p.to_path_buf()
    }
}
impl From<&str> for PathBuf {
    fn from(s: &str) -> Self {
        Self(s.as_bytes().to_owned())
    }
}
impl From<String> for PathBuf {
    fn from(s: String) -> Self {
        Self(s.into_bytes())
    }
}
impl From<Vec<u8>> for PathBuf {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl fmt::Debug for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_path())
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

/// Lossy rendering of a path for human consumption. See [`Path::display`].
pub struct Display<'a>(&'a [u8]);

impl fmt::Display for Display<'_> {
    /// Decodes as UTF-8 where possible and substitutes U+FFFD for each
    /// maximal invalid subsequence, the same policy `String::from_utf8_lossy`
    /// uses. Written out rather than delegated because the kernel builds
    /// `no_std` and this avoids materialising an intermediate allocation on a
    /// path that is frequently a log line in a fault handler.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut rest = self.0;
        loop {
            match core::str::from_utf8(rest) {
                Ok(s) => return f.write_str(s),
                Err(e) => {
                    let good = e.valid_up_to();
                    // SAFETY-free: `valid_up_to` is by definition a valid
                    // UTF-8 boundary, so the `get` cannot fail; the `?` on a
                    // `None` would be a bug, so fall back to stopping rather
                    // than indexing.
                    let Some(head) = rest.get(..good) else { return Ok(()) };
                    let Ok(head) = core::str::from_utf8(head) else { return Ok(()) };
                    f.write_str(head)?;
                    f.write_str("\u{FFFD}")?;
                    // Skip the whole invalid subsequence, or the rest of the
                    // input if the error was a truncated final sequence.
                    let skip = good.saturating_add(e.error_len().unwrap_or(rest.len()));
                    match rest.get(skip..) {
                        Some(r) if !r.is_empty() => rest = r,
                        _ => return Ok(()),
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exercise the lexical rules. Wired into the boot battery in `main.rs`,
/// because the kernel binary sets `test = false` and so has no host
/// `cargo test` to run these under.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::serial_println;
    use alloc::format;

    serial_println!("  path::self_test 1: construction and bytes");
    // The case the whole module exists for: a lone 0x80 is a legal file name
    // under our rules and is not UTF-8.
    let weird = Path::new(b"/data/\x80\xfename");
    assert!(weird.to_str().is_none());
    // `/data/` (6) + 0x80 + 0xfe + `name` (4) = 12. Spelled out because the
    // `\xfe` escape ends at two hex digits and it is easy to miscount the
    // literal by eye.
    assert_eq!(weird.as_bytes().len(), 12);
    assert_eq!(weird.file_name().map(Path::as_bytes), Some(&b"\x80\xfename"[..]));
    assert!(weird.is_valid());
    assert!(!Path::new(b"/a\0b").is_valid());
    assert!(Path::new("/a/b").is_absolute());
    assert!(!Path::new("a/b").is_absolute());
    assert!(Path::new("").is_empty());

    serial_println!("  path::self_test 2: components");
    let comps: Vec<&[u8]> = Path::new("/a//b/c/").components().map(Path::as_bytes).collect();
    assert_eq!(comps, alloc::vec![&b"a"[..], &b"b"[..], &b"c"[..]]);
    assert_eq!(Path::new("/").components().count(), 0);
    assert_eq!(Path::new("").components().count(), 0);
    // A non-UTF-8 component survives the split intact.
    let c: Vec<&[u8]> = Path::new(b"/\x80/\xff").components().map(Path::as_bytes).collect();
    assert_eq!(c, alloc::vec![&b"\x80"[..], &b"\xff"[..]]);

    serial_println!("  path::self_test 3: parent and file_name");
    assert_eq!(Path::new("/a/b").parent().map(Path::as_bytes), Some(&b"/a"[..]));
    assert_eq!(Path::new("/a/b/").parent().map(Path::as_bytes), Some(&b"/a"[..]));
    assert_eq!(Path::new("/a").parent().map(Path::as_bytes), Some(&b"/"[..]));
    assert_eq!(Path::new("/").parent().map(Path::as_bytes), None);
    assert_eq!(Path::new("a").parent().map(Path::as_bytes), None);
    assert_eq!(Path::new("/a/b").file_name().map(Path::as_bytes), Some(&b"b"[..]));
    assert_eq!(Path::new("/a/b/").file_name().map(Path::as_bytes), Some(&b"b"[..]));
    assert_eq!(Path::new("/").file_name().map(Path::as_bytes), None);

    serial_println!("  path::self_test 4: extension");
    assert_eq!(Path::new("/a/b.txt").extension().map(Path::as_bytes), Some(&b"txt"[..]));
    assert_eq!(Path::new("/a/.hidden").extension().map(Path::as_bytes), None);
    assert_eq!(Path::new("/a/b.").extension().map(Path::as_bytes), None);
    assert_eq!(Path::new("/a/b").extension().map(Path::as_bytes), None);
    assert_eq!(Path::new("/a/b.tar.gz").extension().map(Path::as_bytes), Some(&b"gz"[..]));

    serial_println!("  path::self_test 5: prefixes are component-aligned");
    assert!(Path::new("/a/b").starts_with("/a"));
    assert!(Path::new("/a/b").starts_with("/a/"));
    assert!(Path::new("/a").starts_with("/a"));
    // The bytes match but the components do not — the failure mode that made
    // the old inline subtree checks fail open.
    assert!(!Path::new("/ab").starts_with("/a"));
    // Absolute and relative never match each other.
    assert!(!Path::new("a/b").starts_with("/a"));
    assert!(!Path::new("/a/b").starts_with("a"));
    assert_eq!(
        Path::new("/a/b/c").strip_prefix("/a").map(PathBuf::into_vec),
        Some(b"b/c".to_vec())
    );
    assert_eq!(Path::new("/ab/c").strip_prefix("/a"), None);
    // A prefix longer than the path is not a prefix (the loop must run out of
    // *our* components, not of the prefix's).
    assert!(!Path::new("/a").starts_with("/a/b"));
    assert_eq!(Path::new("/a").strip_prefix("/a/b"), None);
    // Every path is under the root; the root is under only itself.
    assert!(Path::new("/a/b").starts_with("/"));
    assert!(Path::new("/").starts_with("/"));
    assert!(!Path::new("/").starts_with("/a"));
    // Stripping a prefix equal to the whole path yields the empty relative
    // path, not `None` and not `/` — callers append to it.
    assert_eq!(Path::new("/a/b").strip_prefix("/a/b").map(PathBuf::into_vec), Some(Vec::new()));
    // Repeated and trailing separators are noise, on either side. This is
    // exactly what the old `starts_with(prefix) && bytes[len] == b'/'` idiom
    // got wrong when `prefix` carried a trailing slash.
    assert!(Path::new("/a//b").starts_with("/a/"));
    assert!(Path::new("//a/b/").starts_with("/a"));
    // Containment is lexical, so `..` is just a component here — a caller that
    // has not resolved `..` first cannot use this as a sandbox check.
    assert!(Path::new("/a/../etc").starts_with("/a"));
    assert!(!Path::new("/a/../etc").has_no_dot_components());
    // Non-UTF-8 components compare by bytes like any other.
    assert!(Path::new(b"/\x80/b").starts_with(Path::new(b"/\x80")));
    assert!(!Path::new(b"/\x80/b").starts_with(Path::new(b"/\x81")));

    serial_println!("  path::self_test 6: join and push");
    assert_eq!(Path::new("/a").join("b").into_vec(), b"/a/b".to_vec());
    assert_eq!(Path::new("/a/").join("b").into_vec(), b"/a/b".to_vec());
    // An absolute component replaces rather than nests — a caller that asked
    // for the root must not silently get a path under the base.
    assert_eq!(Path::new("/a").join("/b").into_vec(), b"/b".to_vec());
    assert_eq!(Path::new("").join("b").into_vec(), b"b".to_vec());
    let mut p = PathBuf::from("/a/b");
    assert!(p.pop());
    assert_eq!(p.as_path().as_bytes(), b"/a");
    assert!(p.pop());
    assert_eq!(p.as_path().as_bytes(), b"/");
    assert!(!p.pop());
    // Joining a non-UTF-8 name onto a UTF-8 base is the ordinary case, not an
    // error.
    assert_eq!(Path::new("/d").join(Path::new(b"\x80")).into_vec(), b"/d/\x80".to_vec());

    serial_println!("  path::self_test 7: display is lossy and only for humans");
    assert_eq!(format!("{}", Path::new("/a/b").display()), "/a/b");
    assert_eq!(format!("{}", Path::new(b"/a/\x80b").display()), "/a/\u{FFFD}b");
    // A truncated multi-byte sequence at the very end must not loop forever
    // or panic.
    assert_eq!(format!("{}", Path::new(b"/a/\xe2\x82").display()), "/a/\u{FFFD}");
    assert_eq!(format!("{}", Path::new(b"\xff\xff").display()), "\u{FFFD}\u{FFFD}");
    // Two different byte strings can render identically — which is exactly
    // why a rendered path must never be used to reopen a file.
    assert_eq!(
        format!("{}", Path::new(b"/\x80").display()),
        format!("{}", Path::new(b"/\xff").display())
    );
    assert_ne!(Path::new(b"/\x80"), Path::new(b"/\xff"));

    serial_println!("  path::self_test 8: dot components");
    assert!(Path::new("/a/b").has_no_dot_components());
    assert!(!Path::new("/a/../b").has_no_dot_components());
    assert!(!Path::new("/a/./b").has_no_dot_components());
    // A name that merely begins with a dot is a normal name.
    assert!(Path::new("/a/.b").has_no_dot_components());
    assert!(Path::new("/a/...").has_no_dot_components());

    serial_println!("  path: all tests passed");
    Ok(())
}
