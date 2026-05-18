//! `<unistd.h>` — UID/GID manipulation constants.
//!
//! `setuid()`, `setgid()`, `setreuid()`, `setresuid()`, and
//! related functions change process credentials.  These constants
//! define special UID/GID values, limits, and error codes.

// ---------------------------------------------------------------------------
// Special UID/GID values
// ---------------------------------------------------------------------------

/// Root (superuser) UID.
pub const ROOT_UID: u32 = 0;
/// Root (superuser) GID.
pub const ROOT_GID: u32 = 0;
/// Nobody UID (overflow, used by NFS and user namespaces).
pub const NOBODY_UID: u32 = 65534;
/// Nogroup GID (overflow).
pub const NOGROUP_GID: u32 = 65534;
/// Invalid/unused UID sentinel (for setreuid "don't change" semantics).
pub const UID_INVALID: u32 = 0xFFFFFFFF;
/// Invalid/unused GID sentinel.
pub const GID_INVALID: u32 = 0xFFFFFFFF;

// ---------------------------------------------------------------------------
// UID/GID range limits
// ---------------------------------------------------------------------------

/// Maximum UID value (32-bit unsigned).
pub const UID_MAX: u32 = 0xFFFFFFFE; // 2^32 - 2 (0xFFFFFFFF is reserved)
/// Maximum GID value.
pub const GID_MAX: u32 = 0xFFFFFFFE;
/// Minimum system UID (convention for system users).
pub const SYS_UID_MIN: u32 = 100;
/// Maximum system UID (convention, everything below this is system).
pub const SYS_UID_MAX: u32 = 999;
/// Minimum regular user UID.
pub const UID_MIN: u32 = 1000;

// ---------------------------------------------------------------------------
// Supplementary groups
// ---------------------------------------------------------------------------

/// Maximum number of supplementary groups per process (Linux).
pub const NGROUPS_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// Credential change error
// ---------------------------------------------------------------------------

/// Error return from setuid/setgid etc.
pub const SETUID_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// Saved set-user-ID / set-group-ID
// ---------------------------------------------------------------------------

/// setresuid/setresgid "no change" sentinel.
pub const SETRES_NOCHANGE: u32 = 0xFFFFFFFF;

// ---------------------------------------------------------------------------
// Capability-related UID constants
// ---------------------------------------------------------------------------

/// UID that triggers capability checks (non-root UID threshold).
pub const CAP_CHECK_UID_THRESHOLD: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_uid_is_zero() {
        assert_eq!(ROOT_UID, 0);
    }

    #[test]
    fn test_root_gid_is_zero() {
        assert_eq!(ROOT_GID, 0);
    }

    #[test]
    fn test_nobody_uid() {
        assert_eq!(NOBODY_UID, 65534);
    }

    #[test]
    fn test_nogroup_gid() {
        assert_eq!(NOGROUP_GID, 65534);
    }

    #[test]
    fn test_invalid_sentinels() {
        assert_eq!(UID_INVALID, 0xFFFFFFFF);
        assert_eq!(GID_INVALID, 0xFFFFFFFF);
        assert_eq!(SETRES_NOCHANGE, 0xFFFFFFFF);
    }

    #[test]
    fn test_uid_max() {
        assert_eq!(UID_MAX, 0xFFFFFFFE);
        assert!(UID_MAX < UID_INVALID);
    }

    #[test]
    fn test_sys_uid_range() {
        assert!(SYS_UID_MIN < SYS_UID_MAX);
        assert!(SYS_UID_MAX < UID_MIN);
    }

    #[test]
    fn test_ngroups_max() {
        assert_eq!(NGROUPS_MAX, 65536);
    }

    #[test]
    fn test_setuid_error() {
        assert_eq!(SETUID_ERROR, -1);
    }

    #[test]
    fn test_cap_threshold() {
        assert_eq!(CAP_CHECK_UID_THRESHOLD, 1);
    }
}
