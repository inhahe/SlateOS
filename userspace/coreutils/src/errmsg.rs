//! How a utility words an I/O failure.
//!
//! ## Why this is not `e.to_string()`
//!
//! Rust's `io::Error` displays the *host's* message, and on a Windows host that
//! is not POSIX's:
//!
//! ```text
//! grep: nope.txt: The system cannot find the file specified. (os error 2)
//! grep: nope.txt: No such file or directory
//! ```
//!
//! The first is what these utilities used to print. Lane C's report of the red
//! workspace test picked the same wording out of `cat` and used it to identify
//! whose `cat` had run — which is the point: the message is an interface. A
//! shell script that does `2>&1 | grep 'No such file'`, a test that asserts a
//! diagnostic, and a person reading a log all read it, and none of them is
//! reading the host's error table.
//!
//! On SlateOS the two agree, because the OS *is* POSIX here. That is exactly
//! why this must not be left to the host: the utilities would then be correct
//! on the target and wrong on the machine they are developed and tested on,
//! which is the arrangement most likely to let a wrong message ship.
//!
//! ## Why the message is chosen by `ErrorKind` and not by errno
//!
//! `io::Error::raw_os_error` gives a *Win32* code on a Windows host and an
//! errno on SlateOS — the same integer meaning different things — so mapping it
//! would have to know which host it was on. `ErrorKind` is std's own
//! normalisation of that difference and is the one thing that means the same on
//! both. A kind std does not recognise falls back to the host's text, which is
//! worse than a POSIX string and much better than a wrong one — but with std's
//! ` (os error N)` suffix removed first, because that parenthetical belongs to
//! no utility's output and is what would give the fallback away. See
//! [`host_text`].

use std::io::{Error, ErrorKind};

/// The POSIX `strerror` text for an I/O failure, for use in a diagnostic.
///
/// Falls back to the host's own message for a failure `ErrorKind` does not
/// name — an unrecognised error is still worth printing.
#[must_use]
pub fn strerror(e: &Error) -> String {
    // The strings are `strerror(3)`'s, verbatim, because that is what the
    // things reading them were written against.
    let s = match e.kind() {
        ErrorKind::NotFound => "No such file or directory",
        ErrorKind::PermissionDenied => "Permission denied",
        ErrorKind::AlreadyExists => "File exists",
        ErrorKind::IsADirectory => "Is a directory",
        ErrorKind::NotADirectory => "Not a directory",
        ErrorKind::DirectoryNotEmpty => "Directory not empty",
        ErrorKind::ReadOnlyFilesystem => "Read-only file system",
        ErrorKind::StorageFull => "No space left on device",
        ErrorKind::FileTooLarge => "File too large",
        ErrorKind::QuotaExceeded => "Disk quota exceeded",
        ErrorKind::CrossesDevices => "Invalid cross-device link",
        ErrorKind::TooManyLinks => "Too many links",
        ErrorKind::InvalidFilename => "File name too long",
        ErrorKind::ArgumentListTooLong => "Argument list too long",
        ErrorKind::ResourceBusy => "Device or resource busy",
        ErrorKind::ExecutableFileBusy => "Text file busy",
        ErrorKind::Deadlock => "Resource deadlock avoided",
        ErrorKind::NotSeekable => "Illegal seek",
        ErrorKind::BrokenPipe => "Broken pipe",
        ErrorKind::WouldBlock => "Resource temporarily unavailable",
        ErrorKind::Interrupted => "Interrupted system call",
        ErrorKind::InvalidInput => "Invalid argument",
        ErrorKind::Unsupported => "Operation not supported",
        ErrorKind::OutOfMemory => "Cannot allocate memory",
        ErrorKind::ConnectionRefused => "Connection refused",
        ErrorKind::ConnectionReset => "Connection reset by peer",
        ErrorKind::ConnectionAborted => "Software caused connection abort",
        ErrorKind::NotConnected => "Transport endpoint is not connected",
        ErrorKind::AddrInUse => "Address already in use",
        ErrorKind::AddrNotAvailable => "Cannot assign requested address",
        ErrorKind::NetworkDown => "Network is down",
        ErrorKind::NetworkUnreachable => "Network is unreachable",
        ErrorKind::HostUnreachable => "No route to host",
        ErrorKind::TimedOut => "Connection timed out",
        // `UnexpectedEof` has no errno: it is std's own condition, raised when a
        // read that needed more bytes got none. GNU words this as a truncation
        // rather than as a system error.
        ErrorKind::UnexpectedEof => "Unexpected end of file",
        // Not a kind std names *on stable*. One errno in that group has a
        // caller here and so is recovered from the raw code; see `errno_text`.
        // Otherwise the host's text is the only description of the failure that
        // exists, and printing it beats printing "error".
        _ => {
            return errno_text(e).map_or_else(|| host_text(e), ToString::to_string);
        }
    };
    s.to_string()
}

/// The host's own wording, with the error *number* taken back off.
///
/// `io::Error`'s `Display` for a raw OS error is the platform's `strerror`
/// text followed by ` (os error N)`. The text is the useful half; the
/// parenthetical is std's, not POSIX's, and no utility this crate imitates has
/// ever printed it — so a message that reached this fallback was recognisably
/// ours rather than `nice`'s or `cat`'s. Measured: `nice >&-` printed
/// `write error: Bad file descriptor (os error 9)` where GNU printed
/// `write error: Bad file descriptor`.
///
/// Stripping it is a suffix removal rather than a re-lookup because the text in
/// front of it is already the platform's `strerror` output — on SlateOS,
/// exactly the string the match above would have produced had `ErrorKind`
/// been able to name the errno. An error carrying no OS number (`Error::other`)
/// has no such suffix and is returned untouched.
fn host_text(e: &Error) -> String {
    let text = e.to_string();
    let Some(open) = text.rfind(" (os error ") else {
        return text;
    };
    let Some(inside) = text.get(open.saturating_add(11)..) else {
        return text;
    };
    let Some(digits) = inside.strip_suffix(')') else {
        return text;
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return text;
    }
    text.get(..open)
        .map_or_else(|| text.clone(), ToString::to_string)
}

/// The POSIX text for `ELOOP`.
///
/// A named constant rather than a literal because two things must produce
/// exactly this string and they reach it by different routes: [`strerror`],
/// when the kernel raises `ELOOP`, and [`filesystem_loop`], when this crate's
/// own canonicaliser decides a symlink chain does not terminate.
pub const FILESYSTEM_LOOP: &str = "Too many levels of symbolic links";

/// `ELOOP` as an [`Error`], for a caller that detects the loop itself.
///
/// `ErrorKind::FilesystemLoop` would be the obvious way to say this, and it is
/// still unstable (`io_error_more`, rust-lang/rust#86442) — every *other* kind
/// this module words was stabilised in 1.83 and this one was left behind. So a
/// loop that *we* detect, rather than the kernel, cannot be carried as a kind.
///
/// It is carried as its own message instead: `Error::other`'s `Display` is
/// verbatim the string handed to it, and [`strerror`]'s fallback for a kind it
/// cannot name is `to_string()`. The two therefore already agree, and no arm in
/// the match above is needed — which is the point of doing it this way rather
/// than inventing a private error type that every caller would have to know
/// about. Swap in `ErrorKind::FilesystemLoop` when it stabilises; nothing
/// outside this function has to change.
#[must_use]
pub fn filesystem_loop() -> Error {
    Error::other(FILESYSTEM_LOOP)
}

/// POSIX text for an errno that stable `ErrorKind` cannot name.
///
/// Compiled only where `raw_os_error` genuinely *is* an errno. That is the
/// whole reason the module header rejects errno mapping in general: the same
/// integer is a Win32 code on the development host. Under `cfg(unix)` — which
/// is SlateOS, per `toolchain/x86_64-slateos.json`'s `target-family` — the
/// ambiguity is gone, and the numbers are Linux's, per `posix::errno`.
#[cfg(unix)]
fn errno_text(e: &Error) -> Option<&'static str> {
    match e.raw_os_error()? {
        // ELOOP. Distinct from `TooManyLinks` (EMLINK), which is about how many
        // *hard* links one file may have; this is a symlink chain that does not
        // terminate.
        40 => Some(FILESYSTEM_LOOP),
        _ => None,
    }
}

#[cfg(not(unix))]
fn errno_text(_e: &Error) -> Option<&'static str> {
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_worded_as_posix_words_it() {
        // The case lane C found, and the one every script matches on.
        let e = std::fs::File::open("no_such_file_xyz123").unwrap_err();
        assert_eq!(strerror(&e), "No such file or directory");
        assert!(
            !strerror(&e).contains("os error"),
            "the host's error number is not part of the message"
        );
    }

    #[test]
    fn the_common_kinds_are_all_named() {
        for (kind, text) in [
            (ErrorKind::NotFound, "No such file or directory"),
            (ErrorKind::PermissionDenied, "Permission denied"),
            (ErrorKind::AlreadyExists, "File exists"),
            (ErrorKind::IsADirectory, "Is a directory"),
            (ErrorKind::NotADirectory, "Not a directory"),
            (ErrorKind::BrokenPipe, "Broken pipe"),
            (ErrorKind::StorageFull, "No space left on device"),
            (ErrorKind::TooManyLinks, "Too many links"),
        ] {
            assert_eq!(strerror(&Error::from(kind)), text, "{kind:?}");
        }
    }

    #[test]
    fn a_loop_is_worded_the_same_whoever_detected_it() {
        // The two routes to ELOOP have to arrive at one sentence, or a script
        // matching on it works against `cat` and not against `readlink`.
        assert_eq!(
            strerror(&filesystem_loop()),
            "Too many levels of symbolic links"
        );
        assert!(
            !strerror(&filesystem_loop()).contains("os error"),
            "the constructed form must not pick up a numeric suffix"
        );

        // The kernel's route. Only checkable where `raw_os_error` is an errno,
        // which is the target rather than the development host.
        #[cfg(unix)]
        assert_eq!(
            strerror(&Error::from_raw_os_error(40)),
            "Too many levels of symbolic links"
        );
    }

    #[test]
    fn an_errno_no_kind_names_still_loses_the_number() {
        // EBADF. `stdfd` raises it for every `prog >&-`, and stable `ErrorKind`
        // has no name for it, so it takes the fallback — which must still not
        // print std's ` (os error 9)`. Measured against GNU: `nice >&-` says
        // `nice: write error: Bad file descriptor` and nothing more.
        #[cfg(unix)]
        {
            let e = Error::from_raw_os_error(9);
            assert_eq!(strerror(&e), "Bad file descriptor");
        }
        // The suffix is stripped wherever it appears, host included, so the
        // rule can be checked on both arms even though the text differs.
        for code in [9, 22, 32] {
            let e = Error::from_raw_os_error(code);
            assert!(
                !strerror(&e).contains("(os error"),
                "raw {code} kept its number: {}",
                strerror(&e)
            );
            assert!(!strerror(&e).is_empty(), "raw {code} lost its text");
        }
    }

    #[test]
    fn a_message_that_merely_mentions_os_error_is_left_alone() {
        // The strip is a suffix removal with a shape, not a search: text that
        // happens to contain the words, or a parenthetical that is not a
        // number, is the failure's own wording and must survive.
        for text in [
            "no space (os error left)",
            "read (os error )",
            "(os error 5) came first",
        ] {
            assert_eq!(strerror(&Error::other(text)), text);
        }
        // And one that does have the shape is still cut, whoever built it.
        assert_eq!(
            strerror(&Error::other("Bad thing (os error 9)")),
            "Bad thing"
        );
    }

    #[test]
    fn an_unrecognised_failure_still_says_something() {
        // A kind this table does not name must not degrade to silence or to a
        // placeholder: the host's own text is the only account of it there is.
        let e = Error::other("something specific went wrong");
        assert!(strerror(&e).contains("something specific"));
    }
}
