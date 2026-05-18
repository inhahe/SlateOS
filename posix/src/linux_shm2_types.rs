//! `<sys/shm.h>` — System V shared memory constants.
//!
//! System V shared memory (`shmget`, `shmat`, `shmdt`, `shmctl`)
//! provides shared memory segments between processes.  These
//! constants define flags, permission bits, and ctl commands.

// ---------------------------------------------------------------------------
// shmget() flags
// ---------------------------------------------------------------------------

/// Create a new shared memory segment.
pub const IPC_CREAT_SHM: u32 = 0o1000;
/// Fail if segment exists (with IPC_CREAT).
pub const IPC_EXCL_SHM: u32 = 0o2000;

// ---------------------------------------------------------------------------
// shmat() flags
// ---------------------------------------------------------------------------

/// Attach for read-only access.
pub const SHM_RDONLY: u32 = 0o10000;
/// Round attach address down to SHMLBA.
pub const SHM_RND: u32 = 0o20000;
/// Allow remap of existing mappings.
pub const SHM_REMAP: u32 = 0o40000;
/// Attach without reserving swap (Linux extension).
pub const SHM_EXEC: u32 = 0o100000;

// ---------------------------------------------------------------------------
// shmctl() commands
// ---------------------------------------------------------------------------

/// Get shared memory segment info.
pub const IPC_STAT_SHM: u32 = 2;
/// Set shared memory segment info.
pub const IPC_SET_SHM: u32 = 1;
/// Remove shared memory segment.
pub const IPC_RMID_SHM: u32 = 0;
/// Get info (Linux extension, returns index).
pub const IPC_INFO_SHM: u32 = 3;
/// Get segment info by index (Linux extension).
pub const SHM_INFO: u32 = 14;
/// Get statistics (Linux extension).
pub const SHM_STAT: u32 = 13;
/// Get statistics, newer version (Linux extension).
pub const SHM_STAT_ANY: u32 = 15;

// ---------------------------------------------------------------------------
// Shared memory permission bits
// ---------------------------------------------------------------------------

/// Owner read.
pub const SHM_R: u32 = 0o400;
/// Owner write.
pub const SHM_W: u32 = 0o200;

// ---------------------------------------------------------------------------
// Shared memory limits
// ---------------------------------------------------------------------------

/// Maximum shared memory segment size (bytes, default).
pub const SHMMAX_DEFAULT: u64 = 0x2000000000; // 128 GiB on modern Linux
/// Minimum shared memory segment size (bytes).
pub const SHMMIN: u32 = 1;
/// Maximum number of shared memory segments system-wide.
pub const SHMMNI_DEFAULT: u32 = 4096;
/// Shared memory low boundary address alignment.
pub const SHMLBA: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flags_no_overlap() {
        assert_eq!(IPC_CREAT_SHM & IPC_EXCL_SHM, 0);
    }

    #[test]
    fn test_attach_flags_distinct() {
        let flags = [SHM_RDONLY, SHM_RND, SHM_REMAP, SHM_EXEC];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_ctl_commands_distinct() {
        let cmds = [
            IPC_STAT_SHM, IPC_SET_SHM, IPC_RMID_SHM,
            IPC_INFO_SHM, SHM_INFO, SHM_STAT, SHM_STAT_ANY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rmid_is_zero() {
        assert_eq!(IPC_RMID_SHM, 0);
    }

    #[test]
    fn test_permission_bits() {
        assert_eq!(SHM_R, 0o400);
        assert_eq!(SHM_W, 0o200);
        assert_eq!(SHM_R & SHM_W, 0);
    }

    #[test]
    fn test_shmmin() {
        assert_eq!(SHMMIN, 1);
    }

    #[test]
    fn test_shmlba() {
        assert_eq!(SHMLBA, 4096);
        assert!(SHMLBA.is_power_of_two());
    }

    #[test]
    fn test_shmmni_default() {
        assert_eq!(SHMMNI_DEFAULT, 4096);
    }
}
