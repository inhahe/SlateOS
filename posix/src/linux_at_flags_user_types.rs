//! `<fcntl.h>` — `*at` syscall directory-fd flags and `AT_FDCWD`.
//!
//! Linux's `*at` syscalls (`openat`, `fstatat`, `linkat`, `renameat2`,
//! etc.) take a directory file-descriptor that anchors relative paths,
//! plus a flags word that selects symlink-follow behaviour, mountpoint
//! crossing, and other policy. This module collects those flags.

// ---------------------------------------------------------------------------
// Special dirfd value
// ---------------------------------------------------------------------------

/// "Use the current working directory" — passed as the dirfd argument.
pub const AT_FDCWD: i32 = -100;

// ---------------------------------------------------------------------------
// Flag bits accepted by various `*at` syscalls
// ---------------------------------------------------------------------------

pub const AT_SYMLINK_NOFOLLOW: i32 = 0x0100;
pub const AT_EACCESS: i32 = 0x0200;
pub const AT_REMOVEDIR: i32 = 0x0200;
pub const AT_SYMLINK_FOLLOW: i32 = 0x0400;
pub const AT_NO_AUTOMOUNT: i32 = 0x0800;
pub const AT_EMPTY_PATH: i32 = 0x1000;
pub const AT_STATX_SYNC_AS_STAT: i32 = 0x0000;
pub const AT_STATX_FORCE_SYNC: i32 = 0x2000;
pub const AT_STATX_DONT_SYNC: i32 = 0x4000;
pub const AT_STATX_SYNC_TYPE: i32 = 0x6000;
pub const AT_RECURSIVE: i32 = 0x8000;

// ---------------------------------------------------------------------------
// `renameat2(2)` second-flags argument (separate bit-space)
// ---------------------------------------------------------------------------

pub const RENAME_NOREPLACE: u32 = 1 << 0;
pub const RENAME_EXCHANGE: u32 = 1 << 1;
pub const RENAME_WHITEOUT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// `openat2(2)` resolve flags
// ---------------------------------------------------------------------------

pub const RESOLVE_NO_XDEV: u64 = 0x01;
pub const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
pub const RESOLVE_NO_SYMLINKS: u64 = 0x04;
pub const RESOLVE_BENEATH: u64 = 0x08;
pub const RESOLVE_IN_ROOT: u64 = 0x10;
pub const RESOLVE_CACHED: u64 = 0x20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atfdcwd_minus100() {
        // AT_FDCWD is the magic dirfd sentinel.
        assert_eq!(AT_FDCWD, -100);
    }

    #[test]
    fn test_eaccess_and_removedir_share_bit() {
        // The kernel reuses the 0x200 bit for two distinct syscall sets.
        // This is part of the ABI — verify the overlap is intentional.
        assert_eq!(AT_EACCESS, AT_REMOVEDIR);
        assert_eq!(AT_EACCESS, 0x200);
    }

    #[test]
    fn test_at_flag_bits_distinct_when_not_aliased() {
        // Aside from the EACCESS/REMOVEDIR overlap, the other flags are
        // pairwise distinct single bits in the 0x0100..=0x8000 range.
        let bits = [
            AT_SYMLINK_NOFOLLOW,
            AT_SYMLINK_FOLLOW,
            AT_NO_AUTOMOUNT,
            AT_EMPTY_PATH,
            AT_STATX_FORCE_SYNC,
            AT_STATX_DONT_SYNC,
            AT_RECURSIVE,
        ];
        for v in bits {
            assert!((v as u32).is_power_of_two());
            assert!(v >= 0x0100);
            assert!(v <= 0x8000);
        }
    }

    #[test]
    fn test_statx_sync_type_mask() {
        // STATX_SYNC_TYPE is the bitwise OR of the two force/dont-sync
        // bits — the mask that extracts the sync-mode field.
        assert_eq!(
            AT_STATX_SYNC_TYPE,
            AT_STATX_FORCE_SYNC | AT_STATX_DONT_SYNC
        );
        // SYNC_AS_STAT is the "default" — zero.
        assert_eq!(AT_STATX_SYNC_AS_STAT, 0);
    }

    #[test]
    fn test_renameat2_flags_each_power_of_two() {
        for v in [RENAME_NOREPLACE, RENAME_EXCHANGE, RENAME_WHITEOUT] {
            assert!(v.is_power_of_two());
        }
        // NOREPLACE + EXCHANGE is invalid (kernel rejects), but the bits
        // are disjoint as bitmasks.
        assert_eq!(RENAME_NOREPLACE & RENAME_EXCHANGE, 0);
    }

    #[test]
    fn test_resolve_flags_low_six_bits() {
        let r = [
            RESOLVE_NO_XDEV,
            RESOLVE_NO_MAGICLINKS,
            RESOLVE_NO_SYMLINKS,
            RESOLVE_BENEATH,
            RESOLVE_IN_ROOT,
            RESOLVE_CACHED,
        ];
        let mut or = 0u64;
        for v in r {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x3F);
    }
}
