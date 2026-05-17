//! `<linux/cred.h>` — Process credentials constants.
//!
//! Every process in Linux has a set of credentials that determine
//! its identity and access rights: real/effective/saved UIDs and GIDs,
//! filesystem UID/GID, supplementary groups, and capabilities. The
//! credential structure is copy-on-write (immutable once installed).
//! When a process needs new credentials (setuid, capability change),
//! a new cred struct is prepared, modified, and then atomically
//! committed, ensuring no half-updated credential state is visible.

// ---------------------------------------------------------------------------
// Credential flags
// ---------------------------------------------------------------------------

/// Credentials are being prepared (not yet committed).
pub const CRED_FLAG_PREPARING: u32 = 0x0000_0001;
/// Credentials have been committed (installed on task).
pub const CRED_FLAG_COMMITTED: u32 = 0x0000_0002;
/// Credentials are from exec (not setuid/setgid call).
pub const CRED_FLAG_EXEC: u32 = 0x0000_0004;
/// Credentials have been overridden (temporary override).
pub const CRED_FLAG_OVERRIDE: u32 = 0x0000_0008;

// ---------------------------------------------------------------------------
// UID/GID special values
// ---------------------------------------------------------------------------

/// Invalid/unset UID value.
pub const INVALID_UID: u32 = 0xFFFF_FFFF;
/// Invalid/unset GID value.
pub const INVALID_GID: u32 = 0xFFFF_FFFF;
/// Root UID.
pub const ROOT_UID: u32 = 0;
/// Root GID.
pub const ROOT_GID: u32 = 0;
/// Nobody UID (overflow UID for unmapped users).
pub const OVERFLOW_UID: u32 = 65534;
/// Nobody GID (overflow GID for unmapped groups).
pub const OVERFLOW_GID: u32 = 65534;

// ---------------------------------------------------------------------------
// Supplementary group limits
// ---------------------------------------------------------------------------

/// Maximum number of supplementary groups per process.
pub const NGROUPS_MAX: u32 = 65536;
/// Small group list threshold (below this, linear search is used).
pub const NGROUPS_SMALL: u32 = 32;

// ---------------------------------------------------------------------------
// setresuid/setresgid operation flags
// ---------------------------------------------------------------------------

/// Don't change this ID (pass as uid/gid argument to keep current).
pub const CRED_NO_CHANGE: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cred_flags_no_overlap() {
        let flags = [
            CRED_FLAG_PREPARING, CRED_FLAG_COMMITTED,
            CRED_FLAG_EXEC, CRED_FLAG_OVERRIDE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_special_uid_gid_values() {
        assert_eq!(ROOT_UID, 0);
        assert_eq!(ROOT_GID, 0);
        assert_eq!(OVERFLOW_UID, 65534);
        assert_eq!(OVERFLOW_GID, 65534);
        assert_eq!(INVALID_UID, u32::MAX);
        assert_eq!(INVALID_GID, u32::MAX);
    }

    #[test]
    fn test_groups_limits() {
        assert!(NGROUPS_SMALL < NGROUPS_MAX);
        assert!(NGROUPS_MAX > 0);
    }

    #[test]
    fn test_no_change_is_invalid() {
        assert_eq!(CRED_NO_CHANGE, INVALID_UID);
    }
}
