//! How an application shows a path, or any other name the system handed it.
//!
//! # Why not `Path::display`
//!
//! A SlateOS name may hold any byte but `/` and NUL, so a name need not be
//! text. `Path::display` (and `OsStr::display`, and `to_string_lossy`) replace
//! each byte that is not part of a character with U+FFFD, the replacement
//! character. Two files that differ only in such bytes then look the same on
//! screen, and a message about one reads as a message about the other --
//! nothing on disk is harmed, but the user cannot tell the files apart, which
//! is what a name is for.
//!
//! [`ShowPath::shown`] is the drop-in replacement: `path.shown()` wherever
//! `path.display()` was, in a `format!` or anywhere else that takes a
//! [`Display`](fmt::Display). It renders through
//! [`quoting::escape_unprintable`], the tree's one renderer for untrusted
//! text: every printable character as itself, and every other byte as a
//! three-digit octal escape, so `caf\xe9.txt` is shown `caf\351.txt`. A
//! control character in a name is escaped the same way, so a name cannot put
//! a line break, or anything else that is not a mark, into a status line.
//!
//! # The ratchet
//!
//! `apps/clippy.toml` names `Path::display` and `OsStr::display` in
//! `disallowed-methods`, and the workspace denies `clippy::all`, so a new
//! `.display()` anywhere under `apps/` fails the boot test's clippy gate
//! rather than quietly shipping.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

/// A name as it is shown to a person; see the crate docs.
///
/// Made by [`ShowPath::shown`]. It borrows the name, so building one costs
/// nothing; the rendering is done each time it is formatted.
#[derive(Clone, Copy, Debug)]
pub struct Shown<'a>(&'a OsStr);

impl fmt::Display for Shown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `as_encoded_bytes` is the name's own bytes on SlateOS (and every
        // Unix). On a Windows development host it is WTF-8, whose only bytes
        // that are not UTF-8 are those of an unpaired surrogate -- which come
        // out escaped, as a name that is not text should.
        f.pad(&quoting::escape_unprintable(self.0.as_encoded_bytes()))
    }
}

/// `.shown()` on a path or a name: [`Path::display`] without the lossy
/// decode. Implemented for [`Path`] and [`OsStr`], and so reachable from a
/// `PathBuf`, an `OsString` and a reference to any of them.
pub trait ShowPath {
    /// This name as it is shown to a person; see the crate docs.
    fn shown(&self) -> Shown<'_>;

    /// This name's own text when it is text, and [`shown`](Self::shown)
    /// when it is not.
    ///
    /// For a name that goes on to be *used* rather than only read -- a file
    /// name suggested for saving, the name an attached file carries -- where
    /// it must stay exactly what it is whenever it can be, a control
    /// character included. A name that is not text has no exact text to
    /// give, and its shown form is the honest stand-in. Never for a key, a
    /// record or anything read back as a path: two names can share a shown
    /// form (`pathcodec` is for those).
    fn text_or_shown(&self) -> Cow<'_, str>;
}

impl ShowPath for Path {
    fn shown(&self) -> Shown<'_> {
        Shown(self.as_os_str())
    }

    fn text_or_shown(&self) -> Cow<'_, str> {
        self.as_os_str().text_or_shown()
    }
}

impl ShowPath for OsStr {
    fn shown(&self) -> Shown<'_> {
        Shown(self)
    }

    fn text_or_shown(&self) -> Cow<'_, str> {
        self.to_str()
            .map_or_else(|| Cow::Owned(self.shown().to_string()), Cow::Borrowed)
    }
}

#[cfg(test)]
mod tests {
    // A test that cannot read its fixture should fail loudly at that line.
    #![allow(clippy::expect_used)]

    use super::ShowPath;
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};

    /// A name of text is shown as it is -- spaces, accents and all.
    #[test]
    fn a_name_of_text_is_shown_as_it_is() {
        assert_eq!(
            Path::new("/home/me/my notes.txt").shown().to_string(),
            "/home/me/my notes.txt"
        );
        assert_eq!(
            Path::new("café/日記.md").shown().to_string(),
            "café/日記.md"
        );
        assert_eq!(OsStr::new("").shown().to_string(), "");
    }

    /// Every owner and reference of a name reaches `shown`, as each reaches
    /// `display`.
    #[test]
    fn every_form_of_a_name_can_be_shown() {
        let buf = PathBuf::from("/a/b");
        let owned = OsString::from("c d");
        assert_eq!(buf.shown().to_string(), "/a/b");
        assert_eq!(buf.as_path().shown().to_string(), "/a/b");
        assert_eq!(owned.shown().to_string(), "c d");
        assert_eq!(format!("[{}]", buf.shown()), "[/a/b]");
    }

    /// A control character is escaped, so a name cannot break a line of the
    /// message it is shown in.
    #[test]
    fn a_control_character_is_escaped() {
        assert_eq!(Path::new("two\nlines").shown().to_string(), r"two\012lines");
        assert_eq!(Path::new("tab\there").shown().to_string(), r"tab\011here");
        assert_eq!(Path::new("bell\u{7}").shown().to_string(), r"bell\007");
    }

    /// A name that is text is used exactly -- a control character included,
    /// unlike `shown` -- and borrowed, not copied.
    #[test]
    fn a_name_of_text_is_used_exactly() {
        let tab = Path::new("a\tb.wav");
        assert_eq!(tab.text_or_shown(), "a\tb.wav");
        assert!(matches!(tab.text_or_shown(), std::borrow::Cow::Borrowed(_)));
        assert_eq!(OsStr::new("x").text_or_shown(), "x");
    }

    /// Width, fill and alignment apply to the shown text, as they do to any
    /// string -- a table column of names lines up.
    #[test]
    fn formatting_flags_apply_to_the_shown_text() {
        assert_eq!(format!("{:>8}|", Path::new("abc").shown()), "     abc|");
        assert_eq!(format!("{:-<6}|", Path::new("a\nb").shown()), r"a\012b|");
        assert_eq!(format!("{:.3}", Path::new("abcdef").shown()), "abc");
    }

    /// A byte that is not part of a character is shown by its value, not
    /// replaced -- so two names that differ only there look different.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_text_is_escaped_not_replaced() {
        use std::os::unix::ffi::OsStrExt;
        let latin1 = Path::new(OsStr::from_bytes(b"caf\xe9.txt"));
        let other = Path::new(OsStr::from_bytes(b"caf\xe8.txt"));
        assert_eq!(latin1.shown().to_string(), r"caf\351.txt");
        assert_ne!(latin1.shown().to_string(), other.shown().to_string());
        assert!(!latin1.shown().to_string().contains('\u{fffd}'));
    }

    /// A name that is not text has no exact text, so it is used as shown.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_text_is_used_as_shown() {
        use std::os::unix::ffi::OsStrExt;
        let odd = OsStr::from_bytes(b"caf\xe9");
        assert_eq!(odd.text_or_shown(), r"caf\351");
    }

    /// `apps/clippy.toml` replaces the workspace's `clippy.toml` for every
    /// crate under `apps/` -- clippy reads the nearest one and merges nothing
    /// -- so each setting there must be repeated here, or the applications
    /// silently lose it. Compared line by line, which is exact for the flat
    /// files these are: every line of the workspace's that is not a comment
    /// or blank must appear in the applications'.
    #[test]
    fn the_apps_clippy_config_repeats_the_workspaces() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |p: &std::path::Path| std::fs::read_to_string(p).expect("readable");
        let workspace = read(&dir.join("../../clippy.toml"));
        let apps = read(&dir.join("../clippy.toml"));
        let apps_lines: Vec<&str> = apps.lines().map(str::trim).collect();
        let missing: Vec<&str> = workspace
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter(|l| !apps_lines.contains(l))
            .collect();
        assert!(
            missing.is_empty(),
            "apps/clippy.toml lacks the workspace's {missing:?}"
        );
        assert!(
            apps.contains("\"std::path::Path::display\"")
                && apps.contains("\"std::ffi::OsStr::display\""),
            "the ratchet itself is gone from apps/clippy.toml"
        );
    }

    /// On a Windows host a name can hold an unpaired surrogate, which is not
    /// text either; it is escaped, not replaced.
    #[cfg(windows)]
    #[test]
    fn an_unpaired_surrogate_is_escaped_not_replaced() {
        use std::os::windows::ffi::OsStringExt;
        let name = OsString::from_wide(&[u16::from(b'a'), 0xD800, u16::from(b'b')]);
        let shown = name.shown().to_string();
        assert_eq!(shown, r"a\355\240\200b");
        assert!(!shown.contains('\u{fffd}'));
    }
}
