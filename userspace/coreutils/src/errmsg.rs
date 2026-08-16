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
//! worse than a POSIX string and much better than a wrong one.

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
        // Not a kind std names. The host's text is the only description of the
        // failure that exists, and printing it beats printing "error".
        _ => return e.to_string(),
    };
    s.to_string()
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
        ] {
            assert_eq!(strerror(&Error::from(kind)), text, "{kind:?}");
        }
    }

    #[test]
    fn an_unrecognised_failure_still_says_something() {
        // A kind this table does not name must not degrade to silence or to a
        // placeholder: the host's own text is the only account of it there is.
        let e = Error::other("something specific went wrong");
        assert!(strerror(&e).contains("something specific"));
    }
}
