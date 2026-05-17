//! `<linux/shmem_fs.h>` — tmpfs/shmem filesystem constants.
//!
//! tmpfs (implemented via shmem) is a RAM-backed filesystem that
//! stores files in virtual memory (page cache + swap). It's used for
//! /tmp, /run, /dev/shm, and POSIX shared memory objects. Files
//! disappear on unmount or reboot.

// ---------------------------------------------------------------------------
// tmpfs mount option flags
// ---------------------------------------------------------------------------

/// Huge pages disabled.
pub const SHMEM_HUGE_NEVER: u8 = 0;
/// Huge pages always attempted.
pub const SHMEM_HUGE_ALWAYS: u8 = 1;
/// Huge pages if free and aligned.
pub const SHMEM_HUGE_WITHIN_SIZE: u8 = 2;
/// Huge pages advised (madvise).
pub const SHMEM_HUGE_ADVISE: u8 = 3;

// ---------------------------------------------------------------------------
// tmpfs inode flags
// ---------------------------------------------------------------------------

/// Inode is pinned in memory (never swap).
pub const SHMEM_FL_PINNED: u32 = 1 << 0;
/// Inode has been truncated.
pub const SHMEM_FL_TRUNCATED: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Shared memory flags (shmget/shmctl)
// ---------------------------------------------------------------------------

/// Create segment (IPC_CREAT).
pub const SHM_CREAT: u32 = 0o001000;
/// Exclusive create (IPC_EXCL).
pub const SHM_EXCL: u32 = 0o002000;
/// Segment is removable.
pub const SHM_DEST: u32 = 0o010000;
/// Segment is locked in memory.
pub const SHM_LOCKED: u32 = 0o020000;
/// Use huge pages.
pub const SHM_HUGETLB: u32 = 0o040000;
/// Don't check for segment removal.
pub const SHM_NORESERVE: u32 = 0o100000;

// ---------------------------------------------------------------------------
// Shared memory limits
// ---------------------------------------------------------------------------

/// Default max shared memory segment size (usually overridden by sysctl).
pub const SHMMAX_DEFAULT: u64 = 32 * 1024 * 1024;
/// Default max total shared memory.
pub const SHMALL_DEFAULT: u64 = 8 * 1024 * 1024 * 1024;
/// Default max segments system-wide.
pub const SHMMNI_DEFAULT: u32 = 4096;

// ---------------------------------------------------------------------------
// POSIX shared memory
// ---------------------------------------------------------------------------

/// Maximum POSIX shm name length (including null).
pub const SHM_NAME_MAX: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huge_page_modes_distinct() {
        let modes = [
            SHMEM_HUGE_NEVER, SHMEM_HUGE_ALWAYS,
            SHMEM_HUGE_WITHIN_SIZE, SHMEM_HUGE_ADVISE,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_shm_flags_distinct() {
        let flags = [SHM_CREAT, SHM_EXCL, SHM_DEST, SHM_LOCKED, SHM_HUGETLB, SHM_NORESERVE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert!(SHMMAX_DEFAULT > 0);
        assert!(SHMALL_DEFAULT > SHMMAX_DEFAULT);
        assert!(SHMMNI_DEFAULT > 0);
    }

    #[test]
    fn test_name_max() {
        assert_eq!(SHM_NAME_MAX, 255);
    }
}
