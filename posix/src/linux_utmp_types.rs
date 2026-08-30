//! `<utmpx.h>` — Extended utmp (utmpx) constants.
//!
//! utmpx is the POSIX-standardized version of utmp. These constants
//! define the record structure sizes, exit status encoding, and
//! database access modes.

// ---------------------------------------------------------------------------
// utmpx structure sizes (for on-disk layout)
// ---------------------------------------------------------------------------

/// Size of utmpx record on Linux (384 bytes).
pub const UTMPX_RECORD_SIZE: u32 = 384;
/// Offset of ut_type in utmpx.
pub const UTMPX_OFF_TYPE: u32 = 0;
/// Offset of ut_pid in utmpx.
pub const UTMPX_OFF_PID: u32 = 4;
/// Offset of ut_line in utmpx.
pub const UTMPX_OFF_LINE: u32 = 8;
/// Offset of ut_id in utmpx.
pub const UTMPX_OFF_ID: u32 = 40;
/// Offset of ut_user in utmpx.
pub const UTMPX_OFF_USER: u32 = 44;
/// Offset of ut_host in utmpx.
pub const UTMPX_OFF_HOST: u32 = 76;

// ---------------------------------------------------------------------------
// Exit status encoding
// ---------------------------------------------------------------------------

/// Process terminated normally (exit).
pub const UTMPX_EXIT_NORMAL: u16 = 0;
/// Process terminated by signal.
pub const UTMPX_EXIT_SIGNAL: u16 = 1;

// ---------------------------------------------------------------------------
// Database access modes (for utmpxname/setutxent)
// ---------------------------------------------------------------------------

/// Read mode.
pub const UTMPX_DB_READ: u32 = 0;
/// Read-write mode.
pub const UTMPX_DB_READWRITE: u32 = 1;

// ---------------------------------------------------------------------------
// Maximum concurrent logins (for tracking)
// ---------------------------------------------------------------------------

/// Default max sessions per user.
pub const UTMPX_MAX_SESSIONS_DEFAULT: u32 = 256;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_size() {
        assert_eq!(UTMPX_RECORD_SIZE, 384);
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            UTMPX_OFF_TYPE,
            UTMPX_OFF_PID,
            UTMPX_OFF_LINE,
            UTMPX_OFF_ID,
            UTMPX_OFF_USER,
            UTMPX_OFF_HOST,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_offsets_within_record() {
        assert!(UTMPX_OFF_HOST < UTMPX_RECORD_SIZE);
    }

    #[test]
    fn test_exit_types_distinct() {
        assert_ne!(UTMPX_EXIT_NORMAL, UTMPX_EXIT_SIGNAL);
    }

    #[test]
    fn test_db_modes_distinct() {
        assert_ne!(UTMPX_DB_READ, UTMPX_DB_READWRITE);
    }

    #[test]
    fn test_max_sessions() {
        assert_eq!(UTMPX_MAX_SESSIONS_DEFAULT, 256);
    }
}
