//! `<linux/shm.h>` — System V shared memory constants.
//!
//! System V shared memory (shmget/shmat/shmdt/shmctl) allows processes
//! to share memory regions. A process creates a shared memory segment
//! with shmget(), attaches it to its address space with shmat(), and
//! detaches with shmdt(). The segment persists in kernel memory until
//! explicitly removed with shmctl(IPC_RMID). Multiple processes can
//! attach the same segment. Largely superseded by mmap(MAP_SHARED)
//! and POSIX shm_open(), but still used in databases and legacy apps.

// ---------------------------------------------------------------------------
// shmat() flags
// ---------------------------------------------------------------------------

/// Attach for read-only access.
pub const SHM_RDONLY: u32 = 0o10000;
/// Attach at a rounded-down address.
pub const SHM_RND: u32 = 0o20000;
/// Remap existing mappings (like MAP_FIXED for shmat).
pub const SHM_REMAP: u32 = 0o40000;
/// Use huge pages for this segment.
pub const SHM_HUGETLB: u32 = 0o4000;
/// Don't reserve swap space (Linux extension).
pub const SHM_NORESERVE: u32 = 0o100_000_000;

// ---------------------------------------------------------------------------
// shmctl() commands (in addition to IPC_RMID, IPC_SET, IPC_STAT)
// ---------------------------------------------------------------------------

/// Lock shared memory segment in RAM (prevent swapping).
pub const SHM_LOCK: u32 = 11;
/// Unlock shared memory segment (allow swapping).
pub const SHM_UNLOCK: u32 = 12;
/// Get system-wide shared memory info.
pub const SHM_INFO: u32 = 14;
/// Get shared memory status by index.
pub const SHM_STAT: u32 = 13;
/// Like SHM_STAT but respects permissions.
pub const SHM_STAT_ANY: u32 = 15;

// ---------------------------------------------------------------------------
// Shared memory limits
// ---------------------------------------------------------------------------

/// Default maximum shared memory segment size (SHMMAX, very large).
pub const SHMMAX_DEFAULT: u64 = 0xFFFF_FFFF_FFFF_FFFF;
/// Default minimum shared memory segment size (1 byte).
pub const SHMMIN: u32 = 1;
/// Default maximum number of shared memory segments system-wide.
pub const SHMMNI: u32 = 4096;
/// Default maximum number of shared memory segments per process.
pub const SHMSEG: u32 = 4096;
/// Maximum total shared memory pages.
pub const SHMALL_DEFAULT: u64 = 0xFFFF_FFFF_FFFF_FFFF;

// ---------------------------------------------------------------------------
// Huge page size flags (for SHM_HUGETLB)
// ---------------------------------------------------------------------------

/// Use 2 MiB huge pages.
pub const SHM_HUGE_2MB: u32 = 21 << 26;
/// Use 1 GiB huge pages.
pub const SHM_HUGE_1GB: u32 = 30 << 26;
/// Shift for huge page size encoding.
pub const SHM_HUGE_SHIFT: u32 = 26;
/// Mask for huge page size encoding.
pub const SHM_HUGE_MASK: u32 = 0x3F;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shmat_flags_distinct() {
        let flags = [SHM_RDONLY, SHM_RND, SHM_REMAP, SHM_HUGETLB];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_ctl_commands_distinct() {
        let cmds = [SHM_LOCK, SHM_UNLOCK, SHM_INFO, SHM_STAT, SHM_STAT_ANY];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_limits_positive() {
        assert!(SHMMIN > 0);
        assert!(SHMMNI > 0);
        assert!(SHMSEG > 0);
    }

    #[test]
    fn test_huge_page_encoding() {
        // 2MB = 2^21, so the flag stores 21 in the size field
        assert_eq!(SHM_HUGE_2MB >> SHM_HUGE_SHIFT, 21);
        // 1GB = 2^30, so the flag stores 30 in the size field
        assert_eq!(SHM_HUGE_1GB >> SHM_HUGE_SHIFT, 30);
    }
}
