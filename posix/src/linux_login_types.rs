//! `<utmp.h>` — Login session type and record constants.
//!
//! These constants define login record types used in utmp/wtmp/btmp
//! files that track user login sessions, system events, and
//! failed login attempts.

// ---------------------------------------------------------------------------
// utmp record types (ut_type field)
// ---------------------------------------------------------------------------

/// Empty record.
pub const EMPTY: u16 = 0;
/// Run level change.
pub const RUN_LVL: u16 = 1;
/// System boot time.
pub const BOOT_TIME: u16 = 2;
/// Time changed (new time follows old time).
pub const NEW_TIME: u16 = 3;
/// Old time (before time change).
pub const OLD_TIME: u16 = 4;
/// Init spawned process.
pub const INIT_PROCESS: u16 = 5;
/// Login process (getty/login).
pub const LOGIN_PROCESS: u16 = 6;
/// Normal user login.
pub const USER_PROCESS: u16 = 7;
/// Process terminated.
pub const DEAD_PROCESS: u16 = 8;
/// Accounting info.
pub const ACCOUNTING: u16 = 9;

// ---------------------------------------------------------------------------
// utmp field sizes
// ---------------------------------------------------------------------------

/// Max length of ut_user (login name).
pub const UT_NAMESIZE: u32 = 32;
/// Max length of ut_line (tty device).
pub const UT_LINESIZE: u32 = 32;
/// Max length of ut_host (remote host).
pub const UT_HOSTSIZE: u32 = 256;
/// Max length of ut_id (init id).
pub const UT_IDSIZE: u32 = 4;

// ---------------------------------------------------------------------------
// utmp/wtmp/btmp file paths (as byte slices)
// ---------------------------------------------------------------------------

/// Path to utmp file.
pub const UTMP_FILE: &[u8] = b"/var/run/utmp";
/// Path to wtmp file.
pub const WTMP_FILE: &[u8] = b"/var/log/wtmp";
/// Path to btmp file (bad logins).
pub const BTMP_FILE: &[u8] = b"/var/log/btmp";
/// Path to lastlog file.
pub const LASTLOG_FILE: &[u8] = b"/var/log/lastlog";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_types_distinct() {
        let types = [
            EMPTY, RUN_LVL, BOOT_TIME, NEW_TIME, OLD_TIME,
            INIT_PROCESS, LOGIN_PROCESS, USER_PROCESS,
            DEAD_PROCESS, ACCOUNTING,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_empty_is_zero() {
        assert_eq!(EMPTY, 0);
    }

    #[test]
    fn test_user_process() {
        assert_eq!(USER_PROCESS, 7);
    }

    #[test]
    fn test_field_sizes() {
        assert_eq!(UT_NAMESIZE, 32);
        assert_eq!(UT_LINESIZE, 32);
        assert_eq!(UT_HOSTSIZE, 256);
        assert_eq!(UT_IDSIZE, 4);
    }

    #[test]
    fn test_file_paths_distinct() {
        let paths: [&[u8]; 4] = [UTMP_FILE, WTMP_FILE, BTMP_FILE, LASTLOG_FILE];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }
}
