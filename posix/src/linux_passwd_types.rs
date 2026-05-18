//! `<pwd.h>` — Password file field and limit constants.
//!
//! These constants define the fields and limits for the
//! `/etc/passwd` file format, including username/UID/GID
//! length limits and well-known system UIDs.

// ---------------------------------------------------------------------------
// Well-known UIDs
// ---------------------------------------------------------------------------

/// Root user UID.
pub const ROOT_UID: u32 = 0;
/// Nobody user UID (overflow/unmapped).
pub const NOBODY_UID: u32 = 65534;
/// First UID for regular (non-system) users.
pub const USER_UID_MIN: u32 = 1000;
/// Last UID for regular users (default).
pub const USER_UID_MAX: u32 = 60000;
/// First system user UID.
pub const SYS_UID_MIN: u32 = 100;
/// Last system user UID (default).
pub const SYS_UID_MAX: u32 = 999;

// ---------------------------------------------------------------------------
// Well-known GIDs
// ---------------------------------------------------------------------------

/// Root group GID.
pub const ROOT_GID: u32 = 0;
/// Nobody group GID.
pub const NOBODY_GID: u32 = 65534;
/// First GID for regular groups.
pub const USER_GID_MIN: u32 = 1000;
/// Last GID for regular groups (default).
pub const USER_GID_MAX: u32 = 60000;

// ---------------------------------------------------------------------------
// Field limits
// ---------------------------------------------------------------------------

/// Maximum username length (LOGIN_NAME_MAX).
pub const LOGIN_NAME_MAX: u32 = 256;
/// Maximum password field length.
pub const PASSWD_FIELD_MAX: u32 = 256;
/// Maximum GECOS (comment) field length.
pub const GECOS_FIELD_MAX: u32 = 1024;
/// Maximum home directory path length.
pub const HOME_DIR_MAX: u32 = 4096;
/// Maximum shell path length.
pub const SHELL_MAX: u32 = 4096;

// ---------------------------------------------------------------------------
// passwd file separator
// ---------------------------------------------------------------------------

/// Field separator in /etc/passwd (colon).
pub const PASSWD_SEPARATOR: u8 = b':';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_uid() {
        assert_eq!(ROOT_UID, 0);
    }

    #[test]
    fn test_root_gid() {
        assert_eq!(ROOT_GID, 0);
    }

    #[test]
    fn test_nobody_uid() {
        assert_eq!(NOBODY_UID, 65534);
    }

    #[test]
    fn test_user_uid_range() {
        assert!(USER_UID_MIN < USER_UID_MAX);
        assert!(SYS_UID_MAX < USER_UID_MIN);
    }

    #[test]
    fn test_system_uid_range() {
        assert!(SYS_UID_MIN < SYS_UID_MAX);
    }

    #[test]
    fn test_login_name_max() {
        assert_eq!(LOGIN_NAME_MAX, 256);
    }

    #[test]
    fn test_separator() {
        assert_eq!(PASSWD_SEPARATOR, b':');
    }
}
