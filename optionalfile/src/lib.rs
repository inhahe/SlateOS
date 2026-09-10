//! Reading a file whose *absence* is normal but whose *unreadability* is not.
//!
//! # Why this is a crate
//!
//! Four programs in this tree read a text file that may legitimately not
//! exist, and all four wrote the same thing:
//!
//! ```ignore
//! let text = fs::read_to_string(path).unwrap_or_default();
//! ```
//!
//! which collapses three different situations into one empty string:
//!
//! | Situation | What it means | What `unwrap_or_default` says |
//! |---|---|---|
//! | the file is there | its contents | its contents |
//! | the file is absent | nothing configured yet | empty |
//! | the file cannot be read | **we do not know** | empty |
//!
//! The third row is the defect, and in three of the four callers it destroyed
//! data, because they rebuild the file from what they parsed and write it
//! back:
//!
//! * `userspace/sudo`'s **visudo** opened an empty editor over `/etc/sudoers`.
//!   The user adds a rule to what looks like a blank file, saves, and every
//!   existing rule is replaced by the one line they typed.
//! * `userspace/xdg` rewrote `~/.config/mimeapps.list` holding only the
//!   association just set, dropping every other default handler.
//! * `userspace/hostnamectl` rewrote `/etc/machine-info` holding only the
//!   field just set, dropping `PRETTY_HOSTNAME`, `ICON_NAME`, `CHASSIS`,
//!   `DEPLOYMENT` and `LOCATION`.
//! * `userspace/ntpd` did not rewrite anything, but fell back to
//!   `pool.ntp.org`, `time.google.com` and `time.cloudflare.com` — so an
//!   administrator who had restricted time sync to internal servers silently
//!   took the system clock from three hosts outside their network.
//!
//! This is lane A's rule, from `mkfs`/`fsck`'s `is_mounted`: for a check that
//! guards a decision, "I do not know" and "there is nothing there" must not be
//! the same value. It was written four times independently, each time by
//! someone thinking about the happy path, which is an argument that the
//! convenient default is the wrong one rather than that four authors were
//! careless.
//!
//! # Why not `String`
//!
//! [`read_or_empty`] refuses a file that is not valid UTF-8 rather than
//! converting it. Every caller here works in text and writes its result back,
//! so accepting the bytes would rewrite them as something else on the next
//! save — a quieter version of the same destruction. A caller that genuinely
//! wants bytes should use [`read_bytes_or_empty`] and keep them as bytes.
//!
//! This matters more than it looks: `read_to_string` fails for the *whole
//! file* if any single byte in it is not UTF-8, so one accented name in a
//! comment was enough to reach every defect above, with no unusual permissions
//! involved.

use std::fs;
use std::io;
use std::path::Path;

/// The bytes of `path`, or empty if it does not exist.
///
/// # Errors
///
/// Any failure other than "not found" — a permission problem, a directory
/// where a file was expected, an I/O error. Those mean *we could not look*,
/// which the caller must not treat as *there is nothing there*.
pub fn read_bytes_or_empty(path: &Path) -> io::Result<Vec<u8>> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// The text of `path`, or empty if it does not exist.
///
/// # Errors
///
/// As [`read_bytes_or_empty`], plus [`io::ErrorKind::InvalidData`] when the
/// file is not valid UTF-8 — refused rather than converted, because a caller
/// that rewrites what it read would otherwise replace those bytes.
pub fn read_or_empty(path: &Path) -> io::Result<String> {
    let bytes = read_bytes_or_empty(path)?;
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "the file holds bytes that are not valid UTF-8; refusing to read \
             it as text, because rewriting it would replace them",
        )
    })
}

#[cfg(test)]
// CLAUDE.md: the defensive lints are allowed inside `#[cfg(test)]`, where
// panicking on bad data is the intended reaction rather than a defect.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::{read_bytes_or_empty, read_or_empty};

    #[test]
    fn an_existing_file_reads_back_as_itself() {
        let scratch = scratchdir::ScratchDir::new("optfile-read");
        let p = scratch.path("f");
        std::fs::write(&p, b"KEY=\"value\"\n").expect("write");
        assert_eq!(read_or_empty(&p).expect("read"), "KEY=\"value\"\n");
    }

    /// The one failure that legitimately means "empty".
    #[test]
    fn a_missing_file_is_empty_rather_than_an_error() {
        let scratch = scratchdir::ScratchDir::new("optfile-absent");
        assert_eq!(
            read_or_empty(&scratch.path("nope")).ok().as_deref(),
            Some("")
        );
        assert_eq!(
            read_bytes_or_empty(&scratch.path("nope")).ok(),
            Some(Vec::new())
        );
    }

    /// THE CASE THIS CRATE EXISTS FOR. A file we were prevented from reading
    /// must not answer "empty", because three of the four callers rebuild the
    /// file from this value and write it back.
    ///
    /// A directory standing where the file belongs produces a read error that
    /// is not `NotFound`, on every platform, without touching permissions.
    #[test]
    fn an_unreadable_file_is_an_error_and_not_an_empty_answer() {
        let scratch = scratchdir::ScratchDir::new("optfile-blocked");
        let p = scratch.path("f");
        std::fs::create_dir(&p).expect("a directory where the file goes");
        assert!(
            read_or_empty(&p).is_err(),
            "an unreadable file must not read as empty"
        );
        assert!(
            read_bytes_or_empty(&p).is_err(),
            "the byte reader must refuse it too"
        );
    }

    /// One byte is enough, and it needs no unusual permissions -- which is
    /// what made the original `read_to_string(..).unwrap_or_default()`
    /// reachable in ordinary use rather than only under a broken filesystem.
    #[test]
    fn a_single_non_utf8_byte_is_refused_as_text_but_kept_as_bytes() {
        let scratch = scratchdir::ScratchDir::new("optfile-bytes");
        let p = scratch.path("f");
        std::fs::write(&p, b"NAME=\"Jos\xe9\"\nOTHER=\"keep\"\n").expect("write");

        assert!(
            read_or_empty(&p).is_err(),
            "as text it must be refused, not lossily converted"
        );
        // The bytes are still available to a caller that wants them; refusing
        // is about the text conversion, not about the file.
        assert_eq!(
            read_bytes_or_empty(&p).expect("bytes").len(),
            b"NAME=\"Jos\xe9\"\nOTHER=\"keep\"\n".len()
        );
    }

    #[test]
    fn an_empty_file_and_a_missing_one_read_alike_and_that_is_intended() {
        let scratch = scratchdir::ScratchDir::new("optfile-empty");
        let p = scratch.path("f");
        std::fs::write(&p, b"").expect("write");
        assert_eq!(read_or_empty(&p).ok().as_deref(), Some(""));
        assert_eq!(
            read_or_empty(&scratch.path("nope")).ok().as_deref(),
            Some("")
        );
    }
}
