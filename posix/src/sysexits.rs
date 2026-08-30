//! `<sysexits.h>` — preferred exit codes for programs.
//!
//! Standard exit status codes from BSD. Programs use these to
//! indicate the general category of failure. Values 64–78 are
//! reserved by this convention.

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

/// Successful termination.
pub const EX_OK: i32 = 0;

/// Base value for error codes.
pub const EX__BASE: i32 = 64;

/// Command line usage error.
///
/// The command was used incorrectly: wrong arguments, bad flag
/// combination, etc.
pub const EX_USAGE: i32 = 64;

/// Data format error.
///
/// Input data was incorrect in some way (not a system error).
pub const EX_DATAERR: i32 = 65;

/// Cannot open input.
///
/// An input file (not a system file) did not exist or was unreadable.
pub const EX_NOINPUT: i32 = 66;

/// Addressee unknown.
///
/// The user specified as recipient does not exist.
pub const EX_NOUSER: i32 = 67;

/// Host name unknown.
///
/// The destination host does not exist.
pub const EX_NOHOST: i32 = 68;

/// Service unavailable.
///
/// A service is unavailable (may be temporary; a future retry may
/// succeed).
pub const EX_UNAVAILABLE: i32 = 69;

/// Internal software error.
///
/// An internal software error was detected (bug or inconsistency).
pub const EX_SOFTWARE: i32 = 70;

/// System error (e.g., can't fork).
///
/// An operating system error was detected (e.g., cannot create a
/// process, file, etc.).
pub const EX_OSERR: i32 = 71;

/// Critical OS file missing.
///
/// A system file (e.g., `/etc/passwd`, `/var/run/utmp`) does not
/// exist, cannot be opened, or has a syntax error.
pub const EX_OSFILE: i32 = 72;

/// Can't create output file.
///
/// A (user-specified) output file cannot be created.
pub const EX_CANTCREAT: i32 = 73;

/// Input/output error.
///
/// An error occurred while doing I/O on some file.
pub const EX_IOERR: i32 = 74;

/// Temporary failure; user is invited to retry.
///
/// Temporary failure, indicating something that is not really an
/// error (e.g., sendmail returning a message to the queue for retry).
pub const EX_TEMPFAIL: i32 = 75;

/// Remote error in protocol.
///
/// The remote system returned something "not possible" during a
/// protocol exchange.
pub const EX_PROTOCOL: i32 = 76;

/// Permission denied.
///
/// The user does not have sufficient privilege to perform the
/// operation.
pub const EX_NOPERM: i32 = 77;

/// Configuration error.
///
/// Something was found in an unconfigured or misconfigured state.
pub const EX_CONFIG: i32 = 78;

/// Maximum value for error codes.
pub const EX__MAX: i32 = 78;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ex_ok() {
        assert_eq!(EX_OK, 0);
    }

    #[test]
    fn test_base_and_max() {
        assert_eq!(EX__BASE, 64);
        assert_eq!(EX__MAX, 78);
    }

    #[test]
    fn test_exit_codes_sequential_from_base() {
        assert_eq!(EX_USAGE, EX__BASE);
        assert_eq!(EX_DATAERR, EX__BASE + 1);
        assert_eq!(EX_NOINPUT, EX__BASE + 2);
        assert_eq!(EX_NOUSER, EX__BASE + 3);
        assert_eq!(EX_NOHOST, EX__BASE + 4);
        assert_eq!(EX_UNAVAILABLE, EX__BASE + 5);
        assert_eq!(EX_SOFTWARE, EX__BASE + 6);
        assert_eq!(EX_OSERR, EX__BASE + 7);
        assert_eq!(EX_OSFILE, EX__BASE + 8);
        assert_eq!(EX_CANTCREAT, EX__BASE + 9);
        assert_eq!(EX_IOERR, EX__BASE + 10);
        assert_eq!(EX_TEMPFAIL, EX__BASE + 11);
        assert_eq!(EX_PROTOCOL, EX__BASE + 12);
        assert_eq!(EX_NOPERM, EX__BASE + 13);
        assert_eq!(EX_CONFIG, EX__BASE + 14);
    }

    #[test]
    fn test_ex_config_is_max() {
        assert_eq!(EX_CONFIG, EX__MAX);
    }

    #[test]
    fn test_all_codes_in_range() {
        let codes = [
            EX_USAGE,
            EX_DATAERR,
            EX_NOINPUT,
            EX_NOUSER,
            EX_NOHOST,
            EX_UNAVAILABLE,
            EX_SOFTWARE,
            EX_OSERR,
            EX_OSFILE,
            EX_CANTCREAT,
            EX_IOERR,
            EX_TEMPFAIL,
            EX_PROTOCOL,
            EX_NOPERM,
            EX_CONFIG,
        ];
        for &code in &codes {
            assert!(
                code >= EX__BASE && code <= EX__MAX,
                "exit code {} should be in range [{}..{}]",
                code,
                EX__BASE,
                EX__MAX
            );
        }
    }

    #[test]
    fn test_codes_distinct() {
        let codes = [
            EX_OK,
            EX_USAGE,
            EX_DATAERR,
            EX_NOINPUT,
            EX_NOUSER,
            EX_NOHOST,
            EX_UNAVAILABLE,
            EX_SOFTWARE,
            EX_OSERR,
            EX_OSFILE,
            EX_CANTCREAT,
            EX_IOERR,
            EX_TEMPFAIL,
            EX_PROTOCOL,
            EX_NOPERM,
            EX_CONFIG,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j], "exit codes must be distinct");
            }
        }
    }

    #[test]
    fn test_ex_ok_not_in_error_range() {
        assert!(EX_OK < EX__BASE);
    }

    #[test]
    fn test_count_of_error_codes() {
        // 15 error codes: 64–78 inclusive.
        assert_eq!(EX__MAX - EX__BASE + 1, 15);
    }
}
