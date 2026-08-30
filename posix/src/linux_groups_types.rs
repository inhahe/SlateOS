//! `<grp.h>` — Group database and supplementary group constants.
//!
//! These constants define limits and special values for the group
//! subsystem including supplementary group management, group
//! credential syscalls, and NSS (Name Service Switch) integration.

// ---------------------------------------------------------------------------
// Group syscall commands
// ---------------------------------------------------------------------------

/// getgroups: get supplementary group IDs.
pub const GETGROUPS_SYSCALL: u32 = 115;
/// setgroups: set supplementary group IDs.
pub const SETGROUPS_SYSCALL: u32 = 116;

// ---------------------------------------------------------------------------
// Group file field limits
// ---------------------------------------------------------------------------

/// Maximum group name length.
pub const GROUP_NAME_MAX: u32 = 32;
/// Maximum group password length (shadow).
pub const GROUP_PASSWD_MAX: u32 = 256;
/// Maximum number of members in a group line.
pub const GROUP_MEMBERS_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// setgroups() allow/deny (in user namespaces)
// ---------------------------------------------------------------------------

/// setgroups is allowed in this user namespace.
pub const SETGROUPS_ALLOW: u32 = 0;
/// setgroups is denied in this user namespace.
pub const SETGROUPS_DENY: u32 = 1;

// ---------------------------------------------------------------------------
// Well-known group IDs
// ---------------------------------------------------------------------------

/// Root group.
pub const GID_ROOT: u32 = 0;
/// Wheel group (sudo/admin).
pub const GID_WHEEL: u32 = 10;
/// TTY group (terminal devices).
pub const GID_TTY: u32 = 5;
/// Disk group (raw disk access).
pub const GID_DISK: u32 = 6;
/// Audio group.
pub const GID_AUDIO: u32 = 29;
/// Video group.
pub const GID_VIDEO: u32 = 44;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_numbers() {
        assert_eq!(GETGROUPS_SYSCALL, 115);
        assert_eq!(SETGROUPS_SYSCALL, 116);
    }

    #[test]
    fn test_limits() {
        assert_eq!(GROUP_NAME_MAX, 32);
        assert_eq!(GROUP_PASSWD_MAX, 256);
    }

    #[test]
    fn test_setgroups_allow_deny() {
        assert_eq!(SETGROUPS_ALLOW, 0);
        assert_eq!(SETGROUPS_DENY, 1);
        assert_ne!(SETGROUPS_ALLOW, SETGROUPS_DENY);
    }

    #[test]
    fn test_well_known_gids_distinct() {
        let gids = [GID_ROOT, GID_WHEEL, GID_TTY, GID_DISK, GID_AUDIO, GID_VIDEO];
        for i in 0..gids.len() {
            for j in (i + 1)..gids.len() {
                assert_ne!(gids[i], gids[j]);
            }
        }
    }

    #[test]
    fn test_root_gid() {
        assert_eq!(GID_ROOT, 0);
    }
}
