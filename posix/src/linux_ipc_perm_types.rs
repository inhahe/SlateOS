//! `<sys/ipc.h>` — System V IPC permission and key constants.
//!
//! System V IPC (shared memory, semaphores, message queues)
//! shares a common permission/key infrastructure.  These
//! constants define the key generation, permission modes,
//! and control commands common to all SysV IPC mechanisms.

// ---------------------------------------------------------------------------
// IPC key constants
// ---------------------------------------------------------------------------

/// Private key (create a new unique IPC object).
pub const IPC_PRIVATE: u32 = 0;

// ---------------------------------------------------------------------------
// IPC common flags
// ---------------------------------------------------------------------------

/// Create IPC object if it does not exist.
pub const IPC_CREAT: u32 = 0o1000;
/// Fail if IPC object exists (with IPC_CREAT).
pub const IPC_EXCL: u32 = 0o2000;
/// Do not block on IPC operations.
pub const IPC_NOWAIT: u32 = 0o4000;

// ---------------------------------------------------------------------------
// IPC control commands (for shmctl/semctl/msgctl)
// ---------------------------------------------------------------------------

/// Remove IPC object.
pub const IPC_RMID: u32 = 0;
/// Set IPC object parameters.
pub const IPC_SET: u32 = 1;
/// Get IPC object parameters.
pub const IPC_STAT: u32 = 2;
/// Get system-wide IPC info.
pub const IPC_INFO: u32 = 3;

// ---------------------------------------------------------------------------
// struct ipc_perm field offsets (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of __key in struct ipc_perm.
pub const IPC_PERM_OFF_KEY: u32 = 0;
/// Offset of uid in struct ipc_perm.
pub const IPC_PERM_OFF_UID: u32 = 4;
/// Offset of gid in struct ipc_perm.
pub const IPC_PERM_OFF_GID: u32 = 8;
/// Offset of cuid (creator uid) in struct ipc_perm.
pub const IPC_PERM_OFF_CUID: u32 = 12;
/// Offset of cgid (creator gid) in struct ipc_perm.
pub const IPC_PERM_OFF_CGID: u32 = 16;
/// Offset of mode in struct ipc_perm.
pub const IPC_PERM_OFF_MODE: u32 = 20;
/// Offset of __seq in struct ipc_perm.
pub const IPC_PERM_OFF_SEQ: u32 = 24;

/// Size of struct ipc_perm on Linux x86_64 (bytes).
pub const IPC_PERM_SIZE: u32 = 48;

// ---------------------------------------------------------------------------
// IPC permission bits
// ---------------------------------------------------------------------------

/// Owner read.
pub const IPC_PERM_UREAD: u32 = 0o400;
/// Owner write.
pub const IPC_PERM_UWRITE: u32 = 0o200;
/// Group read.
pub const IPC_PERM_GREAD: u32 = 0o040;
/// Group write.
pub const IPC_PERM_GWRITE: u32 = 0o020;
/// Other read.
pub const IPC_PERM_OREAD: u32 = 0o004;
/// Other write.
pub const IPC_PERM_OWRITE: u32 = 0o002;

// ---------------------------------------------------------------------------
// ftok() project ID limits
// ---------------------------------------------------------------------------

/// Minimum valid project ID for ftok().
pub const FTOK_PROJ_MIN: u8 = 1;
/// Maximum valid project ID for ftok() (non-zero byte).
pub const FTOK_PROJ_MAX: u8 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_private_is_zero() {
        assert_eq!(IPC_PRIVATE, 0);
    }

    #[test]
    fn test_common_flags_no_overlap() {
        assert_eq!(IPC_CREAT & IPC_EXCL, 0);
        assert_eq!(IPC_CREAT & IPC_NOWAIT, 0);
        assert_eq!(IPC_EXCL & IPC_NOWAIT, 0);
    }

    #[test]
    fn test_ctl_commands_distinct() {
        let cmds = [IPC_RMID, IPC_SET, IPC_STAT, IPC_INFO];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rmid_is_zero() {
        assert_eq!(IPC_RMID, 0);
    }

    #[test]
    fn test_perm_offsets_ascending() {
        let offsets = [
            IPC_PERM_OFF_KEY, IPC_PERM_OFF_UID, IPC_PERM_OFF_GID,
            IPC_PERM_OFF_CUID, IPC_PERM_OFF_CGID, IPC_PERM_OFF_MODE,
            IPC_PERM_OFF_SEQ,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_perm_offsets_within_struct() {
        assert!(IPC_PERM_OFF_SEQ < IPC_PERM_SIZE);
    }

    #[test]
    fn test_permission_bits_no_overlap() {
        let bits = [
            IPC_PERM_UREAD, IPC_PERM_UWRITE,
            IPC_PERM_GREAD, IPC_PERM_GWRITE,
            IPC_PERM_OREAD, IPC_PERM_OWRITE,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_ftok_proj_range() {
        assert!(FTOK_PROJ_MIN > 0);
        assert!(FTOK_PROJ_MAX >= FTOK_PROJ_MIN);
    }
}
