//! `<linux/mount.h>` — fsmount() syscall constants.
//!
//! fsmount() creates a mount from a configured filesystem
//! context.  These constants define mount attribute flags
//! and fsmount-specific parameters.

// ---------------------------------------------------------------------------
// fsmount() flags (FSMOUNT_*)
// ---------------------------------------------------------------------------

/// Close-on-exec for the mount fd.
pub const FSMOUNT_CLOEXEC: u32 = 0x00000001;

// ---------------------------------------------------------------------------
// fsmount() attribute flags (MOUNT_ATTR_*)
// ---------------------------------------------------------------------------

/// Read only.
pub const FSMOUNT_ATTR_RDONLY: u32 = 0x00000001;
/// No setuid.
pub const FSMOUNT_ATTR_NOSUID: u32 = 0x00000002;
/// No device access.
pub const FSMOUNT_ATTR_NODEV: u32 = 0x00000004;
/// No exec.
pub const FSMOUNT_ATTR_NOEXEC: u32 = 0x00000008;

// ---------------------------------------------------------------------------
// fsmount() atime modes (MOUNT_ATTR_*)
// ---------------------------------------------------------------------------

/// Relative atime.
pub const FSMOUNT_ATTR_RELATIME: u32 = 0x00000000;
/// No atime.
pub const FSMOUNT_ATTR_NOATIME: u32 = 0x00000010;
/// Strict atime.
pub const FSMOUNT_ATTR_STRICTATIME: u32 = 0x00000020;
/// No directory atime.
pub const FSMOUNT_ATTR_NODIRATIME: u32 = 0x00000080;

// ---------------------------------------------------------------------------
// AT_* flags used with fsmount/move_mount
// ---------------------------------------------------------------------------

/// Empty path (operate on fd itself).
pub const AT_EMPTY_PATH_FSMOUNT: u32 = 0x1000;
/// No automount.
pub const AT_NO_AUTOMOUNT_FSMOUNT: u32 = 0x800;
/// Symlink no follow.
pub const AT_SYMLINK_NOFOLLOW_FSMOUNT: u32 = 0x100;
/// Recursive.
pub const AT_RECURSIVE_FSMOUNT: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloexec() {
        assert_eq!(FSMOUNT_CLOEXEC, 1);
    }

    #[test]
    fn test_attr_flags_distinct() {
        let flags = [
            FSMOUNT_ATTR_RDONLY, FSMOUNT_ATTR_NOSUID,
            FSMOUNT_ATTR_NODEV, FSMOUNT_ATTR_NOEXEC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_attr_flags_powers_of_two() {
        let flags = [
            FSMOUNT_ATTR_RDONLY, FSMOUNT_ATTR_NOSUID,
            FSMOUNT_ATTR_NODEV, FSMOUNT_ATTR_NOEXEC,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_atime_modes_distinct() {
        let modes = [
            FSMOUNT_ATTR_RELATIME, FSMOUNT_ATTR_NOATIME,
            FSMOUNT_ATTR_STRICTATIME, FSMOUNT_ATTR_NODIRATIME,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_at_flags_distinct() {
        let flags = [
            AT_EMPTY_PATH_FSMOUNT, AT_NO_AUTOMOUNT_FSMOUNT,
            AT_SYMLINK_NOFOLLOW_FSMOUNT, AT_RECURSIVE_FSMOUNT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_relatime_is_zero() {
        assert_eq!(FSMOUNT_ATTR_RELATIME, 0);
    }
}
