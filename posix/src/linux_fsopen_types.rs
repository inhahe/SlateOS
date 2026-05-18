//! `<linux/fsopen.h>` — fsopen()/fsconfig() syscall constants.
//!
//! fsopen() creates a filesystem configuration context.
//! fsconfig() configures it before mounting.  These constants
//! define fsconfig commands and open_tree/move_mount flags.

// ---------------------------------------------------------------------------
// fsconfig() commands (FSCONFIG_CMD_*)
// ---------------------------------------------------------------------------

/// Set flag (boolean key, no value).
pub const FSCONFIG_SET_FLAG: u32 = 0;
/// Set string value.
pub const FSCONFIG_SET_STRING: u32 = 1;
/// Set binary data.
pub const FSCONFIG_SET_BINARY: u32 = 2;
/// Set path (file descriptor reference).
pub const FSCONFIG_SET_PATH: u32 = 3;
/// Set path (empty path).
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
/// Set file descriptor.
pub const FSCONFIG_SET_FD: u32 = 5;
/// Create (finalize configuration).
pub const FSCONFIG_CMD_CREATE: u32 = 6;
/// Reconfigure.
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;
/// Create superblock excl.
pub const FSCONFIG_CMD_CREATE_EXCL: u32 = 8;

// ---------------------------------------------------------------------------
// fsopen() flags
// ---------------------------------------------------------------------------

/// Close-on-exec for the context fd.
pub const FSOPEN_CLOEXEC: u32 = 0x00000001;

// ---------------------------------------------------------------------------
// fspick() flags
// ---------------------------------------------------------------------------

/// Close-on-exec.
pub const FSPICK_CLOEXEC: u32 = 0x00000001;
/// Symlink no follow.
pub const FSPICK_SYMLINK_NOFOLLOW: u32 = 0x00000002;
/// No automount.
pub const FSPICK_NO_AUTOMOUNT: u32 = 0x00000004;
/// Empty path.
pub const FSPICK_EMPTY_PATH: u32 = 0x00000008;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fsconfig_cmds_distinct() {
        let cmds = [
            FSCONFIG_SET_FLAG, FSCONFIG_SET_STRING,
            FSCONFIG_SET_BINARY, FSCONFIG_SET_PATH,
            FSCONFIG_SET_PATH_EMPTY, FSCONFIG_SET_FD,
            FSCONFIG_CMD_CREATE, FSCONFIG_CMD_RECONFIGURE,
            FSCONFIG_CMD_CREATE_EXCL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_set_flag_is_zero() {
        assert_eq!(FSCONFIG_SET_FLAG, 0);
    }

    #[test]
    fn test_fsopen_cloexec() {
        assert_eq!(FSOPEN_CLOEXEC, 1);
    }

    #[test]
    fn test_fspick_flags_no_overlap() {
        let flags = [
            FSPICK_CLOEXEC, FSPICK_SYMLINK_NOFOLLOW,
            FSPICK_NO_AUTOMOUNT, FSPICK_EMPTY_PATH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fspick_flags_powers_of_two() {
        let flags = [
            FSPICK_CLOEXEC, FSPICK_SYMLINK_NOFOLLOW,
            FSPICK_NO_AUTOMOUNT, FSPICK_EMPTY_PATH,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }
}
