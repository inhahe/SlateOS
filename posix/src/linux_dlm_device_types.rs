//! `<linux/dlm_device.h>` — DLM character-device userspace protocol.
//!
//! Constants exchanged between userspace cluster managers and the
//! kernel DLM via `/dev/misc/dlm-control` — used to create, list,
//! and destroy lockspaces.

// ---------------------------------------------------------------------------
// dlm_device_command — control commands
// ---------------------------------------------------------------------------

/// Create a new lockspace.
pub const DLM_USER_LSFLG_AUTOFREE: u32 = 0x0001;
/// Forced create — destroy any existing one of the same name.
pub const DLM_USER_LSFLG_FORCEFREE: u32 = 0x0002;
/// Lockspace already exists — caller is recovering.
pub const DLM_USER_LSFLG_TIMEWARN: u32 = 0x0004;

// ---------------------------------------------------------------------------
// dlm_write_request.cmd — write-side request types
// ---------------------------------------------------------------------------

/// Lock acquire request.
pub const DLM_USER_LOCK: u32 = 1;
/// Unlock request.
pub const DLM_USER_UNLOCK: u32 = 2;
/// Query — list locks in this lockspace.
pub const DLM_USER_QUERY: u32 = 3;
/// Create lockspace.
pub const DLM_USER_CREATE_LOCKSPACE: u32 = 4;
/// Remove lockspace.
pub const DLM_USER_REMOVE_LOCKSPACE: u32 = 5;
/// Mark a pending request "purge" for the given owner.
pub const DLM_USER_PURGE: u32 = 6;
/// Deadlock cancellation hint.
pub const DLM_USER_DEADLOCK: u32 = 7;

// ---------------------------------------------------------------------------
// dlm_lock_result.user_lksb status (kernel→user)
// ---------------------------------------------------------------------------

/// Result is a CAST (completion AST).
pub const DLM_USER_LRH_CALLBACK: u32 = 0x01;
/// Result is a BAST (blocking AST).
pub const DLM_USER_LRH_BLOCKING: u32 = 0x02;
/// Lock was cancelled.
pub const DLM_USER_LRH_CANCEL: u32 = 0x04;

// ---------------------------------------------------------------------------
// Versioning
// ---------------------------------------------------------------------------

/// Major version of the dlm_device API.
pub const DLM_DEVICE_VERSION_MAJOR: u32 = 6;
/// Minor version.
pub const DLM_DEVICE_VERSION_MINOR: u32 = 0;
/// Patch version.
pub const DLM_DEVICE_VERSION_PATCH: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsflg_single_bits() {
        for &f in &[
            DLM_USER_LSFLG_AUTOFREE,
            DLM_USER_LSFLG_FORCEFREE,
            DLM_USER_LSFLG_TIMEWARN,
        ] {
            assert!(f.is_power_of_two(), "{f:#x} not single-bit");
        }
    }

    #[test]
    fn test_request_cmds_distinct() {
        let cmds = [
            DLM_USER_LOCK,
            DLM_USER_UNLOCK,
            DLM_USER_QUERY,
            DLM_USER_CREATE_LOCKSPACE,
            DLM_USER_REMOVE_LOCKSPACE,
            DLM_USER_PURGE,
            DLM_USER_DEADLOCK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_lrh_single_bits() {
        for &f in &[
            DLM_USER_LRH_CALLBACK,
            DLM_USER_LRH_BLOCKING,
            DLM_USER_LRH_CANCEL,
        ] {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_version_sane() {
        // dlm_device is at protocol version 6 and has been for many
        // kernel releases — guard against accidental downgrade.
        assert!(DLM_DEVICE_VERSION_MAJOR >= 6);
    }
}
