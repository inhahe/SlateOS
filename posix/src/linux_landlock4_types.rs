//! `<linux/landlock.h>` — Additional Landlock constants (part 4).
//!
//! Supplementary Landlock constants covering scope restrictions,
//! error codes, and compatibility flags.

// ---------------------------------------------------------------------------
// Landlock scope restrictions (LANDLOCK_SCOPE_*)
// ---------------------------------------------------------------------------

/// Abstract UNIX socket scope.
pub const LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET: u64 = 1 << 0;
/// Signal scope.
pub const LANDLOCK_SCOPE_SIGNAL: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Landlock handled access masks (combined)
// ---------------------------------------------------------------------------

/// All filesystem access rights (v1).
pub const LANDLOCK_ACCESS_FS_ALL_V1: u64 =
    (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) |
    (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) |
    (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11) |
    (1 << 12);
/// All filesystem access rights (v2: adds REFER).
pub const LANDLOCK_ACCESS_FS_ALL_V2: u64 =
    LANDLOCK_ACCESS_FS_ALL_V1 | (1 << 13);
/// All filesystem access rights (v3: adds TRUNCATE).
pub const LANDLOCK_ACCESS_FS_ALL_V3: u64 =
    LANDLOCK_ACCESS_FS_ALL_V2 | (1 << 14);
/// All filesystem access rights (v5: adds IOCTL_DEV).
pub const LANDLOCK_ACCESS_FS_ALL_V5: u64 =
    LANDLOCK_ACCESS_FS_ALL_V3 | (1 << 15);

// ---------------------------------------------------------------------------
// Landlock network access all
// ---------------------------------------------------------------------------

/// All network access rights.
pub const LANDLOCK_ACCESS_NET_ALL: u64 = (1 << 0) | (1 << 1);

// ---------------------------------------------------------------------------
// Landlock error constants
// ---------------------------------------------------------------------------

/// Not supported by kernel.
pub const LANDLOCK_E_NOSYS: i32 = -38;
/// Invalid argument.
pub const LANDLOCK_E_INVAL: i32 = -22;
/// Operation not permitted.
pub const LANDLOCK_E_PERM: i32 = -1;

// ---------------------------------------------------------------------------
// Landlock restrict_self flags
// ---------------------------------------------------------------------------

/// No flags.
pub const LANDLOCK_RESTRICT_SELF_FLAGS_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Landlock add_rule flags
// ---------------------------------------------------------------------------

/// No flags for add_rule.
pub const LANDLOCK_ADD_RULE_FLAGS_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_flags_power_of_two() {
        assert!(LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET.is_power_of_two());
        assert!(LANDLOCK_SCOPE_SIGNAL.is_power_of_two());
    }

    #[test]
    fn test_scope_flags_no_overlap() {
        assert_eq!(
            LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET & LANDLOCK_SCOPE_SIGNAL,
            0
        );
    }

    #[test]
    fn test_fs_all_versions_monotonic() {
        assert!(LANDLOCK_ACCESS_FS_ALL_V1 < LANDLOCK_ACCESS_FS_ALL_V2);
        assert!(LANDLOCK_ACCESS_FS_ALL_V2 < LANDLOCK_ACCESS_FS_ALL_V3);
        assert!(LANDLOCK_ACCESS_FS_ALL_V3 < LANDLOCK_ACCESS_FS_ALL_V5);
    }

    #[test]
    fn test_fs_v1_subset_v2() {
        assert_eq!(
            LANDLOCK_ACCESS_FS_ALL_V1 & LANDLOCK_ACCESS_FS_ALL_V2,
            LANDLOCK_ACCESS_FS_ALL_V1
        );
    }

    #[test]
    fn test_net_all() {
        assert_eq!(LANDLOCK_ACCESS_NET_ALL, 3);
    }

    #[test]
    fn test_error_constants_negative() {
        assert!(LANDLOCK_E_NOSYS < 0);
        assert!(LANDLOCK_E_INVAL < 0);
        assert!(LANDLOCK_E_PERM < 0);
    }

    #[test]
    fn test_error_constants_distinct() {
        let errs = [LANDLOCK_E_NOSYS, LANDLOCK_E_INVAL, LANDLOCK_E_PERM];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }
}
