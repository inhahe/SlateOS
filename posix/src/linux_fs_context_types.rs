//! `<linux/fs_context.h>` — Filesystem context (new mount API) constants.
//!
//! The filesystem context API (added in Linux 5.2) replaces the old
//! single-string mount options with a structured interface. A
//! filesystem context (fs_context) is created with fsopen(), configured
//! with fsconfig() calls, and then used to create a superblock or
//! reconfigure an existing mount. This allows proper error reporting,
//! type-safe options, and step-by-step mount configuration.

// ---------------------------------------------------------------------------
// fs_context purpose (why was this context created?)
// ---------------------------------------------------------------------------

/// Context created for a new mount (fsopen + fsmount).
pub const FS_CONTEXT_FOR_MOUNT: u32 = 0;
/// Context created for submount (automount triggered).
pub const FS_CONTEXT_FOR_SUBMOUNT: u32 = 1;
/// Context created for reconfiguration (remount).
pub const FS_CONTEXT_FOR_RECONFIGURE: u32 = 2;

// ---------------------------------------------------------------------------
// fsconfig() command types
// ---------------------------------------------------------------------------

/// Set a flag (boolean option with no value).
pub const FSCONFIG_SET_FLAG: u32 = 0;
/// Set a string option.
pub const FSCONFIG_SET_STRING: u32 = 1;
/// Set a binary blob option.
pub const FSCONFIG_SET_BINARY: u32 = 2;
/// Set a path option (fd-relative).
pub const FSCONFIG_SET_PATH: u32 = 3;
/// Set a path option (relative to empty root).
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
/// Set an fd option (pass an open file descriptor).
pub const FSCONFIG_SET_FD: u32 = 5;
/// Trigger creation of the superblock.
pub const FSCONFIG_CMD_CREATE: u32 = 6;
/// Trigger reconfiguration of the superblock.
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;
/// Trigger creation with exclusive superblock.
pub const FSCONFIG_CMD_CREATE_EXCL: u32 = 8;

// ---------------------------------------------------------------------------
// fs_context phase (lifecycle stage)
// ---------------------------------------------------------------------------

/// Free form (initial configuration phase).
pub const FS_CONTEXT_PHASE_FREE: u32 = 0;
/// Creating superblock.
pub const FS_CONTEXT_PHASE_CREATE: u32 = 1;
/// Superblock created, mounting.
pub const FS_CONTEXT_PHASE_MOUNT: u32 = 2;
/// Reconfiguring existing mount.
pub const FS_CONTEXT_PHASE_RECONF: u32 = 3;
/// Context failed (error occurred).
pub const FS_CONTEXT_PHASE_FAILED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_purpose_values_distinct() {
        let vals = [
            FS_CONTEXT_FOR_MOUNT,
            FS_CONTEXT_FOR_SUBMOUNT,
            FS_CONTEXT_FOR_RECONFIGURE,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_fsconfig_cmds_distinct() {
        let cmds = [
            FSCONFIG_SET_FLAG,
            FSCONFIG_SET_STRING,
            FSCONFIG_SET_BINARY,
            FSCONFIG_SET_PATH,
            FSCONFIG_SET_PATH_EMPTY,
            FSCONFIG_SET_FD,
            FSCONFIG_CMD_CREATE,
            FSCONFIG_CMD_RECONFIGURE,
            FSCONFIG_CMD_CREATE_EXCL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_fsconfig_cmds_sequential() {
        assert_eq!(FSCONFIG_SET_FLAG, 0);
        assert_eq!(FSCONFIG_SET_STRING, 1);
        assert_eq!(FSCONFIG_SET_BINARY, 2);
        assert_eq!(FSCONFIG_CMD_CREATE, 6);
        assert_eq!(FSCONFIG_CMD_RECONFIGURE, 7);
    }

    #[test]
    fn test_phases_distinct() {
        let phases = [
            FS_CONTEXT_PHASE_FREE,
            FS_CONTEXT_PHASE_CREATE,
            FS_CONTEXT_PHASE_MOUNT,
            FS_CONTEXT_PHASE_RECONF,
            FS_CONTEXT_PHASE_FAILED,
        ];
        for i in 0..phases.len() {
            for j in (i + 1)..phases.len() {
                assert_ne!(phases[i], phases[j]);
            }
        }
    }
}
