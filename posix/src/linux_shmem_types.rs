//! `<linux/shm.h>` — Shared memory constants.
//!
//! System V shared memory constants covering flags,
//! commands, permission bits, and limits.

// ---------------------------------------------------------------------------
// SHM flags
// ---------------------------------------------------------------------------

/// Read-only attach.
pub const SHM_RDONLY: u32 = 0o10000;
/// Round attach address.
pub const SHM_RND: u32 = 0o20000;
/// Remap on attach.
pub const SHM_REMAP: u32 = 0o40000;
/// Executable attach.
pub const SHM_EXEC: u32 = 0o100000;
/// Hugetlb attach.
pub const SHM_HUGETLB: u32 = 0o4000;
/// Don't reserve swap.
pub const SHM_NORESERVE: u32 = 0o10000000;

// ---------------------------------------------------------------------------
// SHM commands (shmctl)
// ---------------------------------------------------------------------------

/// Lock pages in memory.
pub const SHM_LOCK: u32 = 11;
/// Unlock pages.
pub const SHM_UNLOCK: u32 = 12;
/// Get stats.
pub const SHM_STAT: u32 = 13;
/// Get info.
pub const SHM_INFO: u32 = 14;
/// New stat.
pub const SHM_STAT_ANY: u32 = 15;

// ---------------------------------------------------------------------------
// IPC commands (shared with msg/sem)
// ---------------------------------------------------------------------------

/// Remove.
pub const IPC_RMID: u32 = 0;
/// Set parameters.
pub const IPC_SET: u32 = 1;
/// Get info.
pub const IPC_STAT: u32 = 2;
/// Get info (any).
pub const IPC_INFO: u32 = 3;

// ---------------------------------------------------------------------------
// IPC flags
// ---------------------------------------------------------------------------

/// Create if key not found.
pub const IPC_CREAT: u32 = 0o1000;
/// Fail if key exists.
pub const IPC_EXCL: u32 = 0o2000;
/// Return error on wait.
pub const IPC_NOWAIT: u32 = 0o4000;
/// Private key.
pub const IPC_PRIVATE: u32 = 0;

// ---------------------------------------------------------------------------
// SHM page sizes (for SHM_HUGETLB)
// ---------------------------------------------------------------------------

/// Huge page shift for 2MB.
pub const SHM_HUGE_SHIFT: u32 = 26;
/// Huge page mask.
pub const SHM_HUGE_MASK: u32 = 0x3F;
/// 2MB huge pages.
pub const SHM_HUGE_2MB: u32 = 21 << SHM_HUGE_SHIFT;
/// 1GB huge pages.
pub const SHM_HUGE_1GB: u32 = 30 << SHM_HUGE_SHIFT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shm_flags_distinct() {
        let flags = [
            SHM_RDONLY, SHM_RND, SHM_REMAP,
            SHM_EXEC, SHM_HUGETLB, SHM_NORESERVE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_shm_commands_distinct() {
        let cmds = [SHM_LOCK, SHM_UNLOCK, SHM_STAT, SHM_INFO, SHM_STAT_ANY];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ipc_commands_distinct() {
        let cmds = [IPC_RMID, IPC_SET, IPC_STAT, IPC_INFO];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ipc_flags_distinct() {
        let flags = [IPC_CREAT, IPC_EXCL, IPC_NOWAIT, IPC_PRIVATE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_huge_pages() {
        assert_ne!(SHM_HUGE_2MB, SHM_HUGE_1GB);
    }
}
