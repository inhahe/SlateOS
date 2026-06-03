//! `<linux/stat.h>` — `statx(2)` ABI.
//!
//! `statx(2)` (Linux 4.11+) is the modern stat: a request mask lets
//! the caller specify exactly which fields it wants (so the kernel
//! can skip expensive ones), and the response carries extra fields
//! (btime, attributes, mount id). glibc's `statx` shim and Rust's
//! `Metadata::created()` use it.

// ---------------------------------------------------------------------------
// Request mask bits (`STATX_*`)
// ---------------------------------------------------------------------------

pub const STATX_TYPE: u32 = 1 << 0;
pub const STATX_MODE: u32 = 1 << 1;
pub const STATX_NLINK: u32 = 1 << 2;
pub const STATX_UID: u32 = 1 << 3;
pub const STATX_GID: u32 = 1 << 4;
pub const STATX_ATIME: u32 = 1 << 5;
pub const STATX_MTIME: u32 = 1 << 6;
pub const STATX_CTIME: u32 = 1 << 7;
pub const STATX_INO: u32 = 1 << 8;
pub const STATX_SIZE: u32 = 1 << 9;
pub const STATX_BLOCKS: u32 = 1 << 10;
pub const STATX_BTIME: u32 = 1 << 11;
pub const STATX_MNT_ID: u32 = 1 << 12;
pub const STATX_DIOALIGN: u32 = 1 << 13;
pub const STATX_MNT_ID_UNIQUE: u32 = 1 << 14;
pub const STATX_SUBVOL: u32 = 1 << 15;

/// `STATX_BASIC_STATS` — the fields a plain `stat(2)` already returns.
pub const STATX_BASIC_STATS: u32 = STATX_TYPE
    | STATX_MODE
    | STATX_NLINK
    | STATX_UID
    | STATX_GID
    | STATX_ATIME
    | STATX_MTIME
    | STATX_CTIME
    | STATX_INO
    | STATX_SIZE
    | STATX_BLOCKS;

/// All flags currently defined.
pub const STATX_ALL: u32 = STATX_BASIC_STATS | STATX_BTIME;

// ---------------------------------------------------------------------------
// `stx_attributes` flag bits (`STATX_ATTR_*`)
// ---------------------------------------------------------------------------

pub const STATX_ATTR_COMPRESSED: u64 = 1 << 2;
pub const STATX_ATTR_IMMUTABLE: u64 = 1 << 4;
pub const STATX_ATTR_APPEND: u64 = 1 << 5;
pub const STATX_ATTR_NODUMP: u64 = 1 << 6;
pub const STATX_ATTR_ENCRYPTED: u64 = 1 << 11;
pub const STATX_ATTR_AUTOMOUNT: u64 = 1 << 12;
pub const STATX_ATTR_MOUNT_ROOT: u64 = 1 << 13;
pub const STATX_ATTR_VERITY: u64 = 1 << 20;
pub const STATX_ATTR_DAX: u64 = 1 << 21;

// ---------------------------------------------------------------------------
// `AT_*` flags reusable for statx + openat + linkat etc.
// ---------------------------------------------------------------------------

pub const AT_FDCWD: i32 = -100;
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
pub const AT_NO_AUTOMOUNT: u32 = 0x800;
pub const AT_EMPTY_PATH: u32 = 0x1000;
pub const AT_STATX_SYNC_AS_STAT: u32 = 0x0000;
pub const AT_STATX_FORCE_SYNC: u32 = 0x2000;
pub const AT_STATX_DONT_SYNC: u32 = 0x4000;
pub const AT_STATX_SYNC_TYPE: u32 = 0x6000;

// ---------------------------------------------------------------------------
// Syscall number
// ---------------------------------------------------------------------------

pub const NR_STATX: u32 = 332;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_bits_dense_0_to_15() {
        let r = [
            STATX_TYPE,
            STATX_MODE,
            STATX_NLINK,
            STATX_UID,
            STATX_GID,
            STATX_ATIME,
            STATX_MTIME,
            STATX_CTIME,
            STATX_INO,
            STATX_SIZE,
            STATX_BLOCKS,
            STATX_BTIME,
            STATX_MNT_ID,
            STATX_DIOALIGN,
            STATX_MNT_ID_UNIQUE,
            STATX_SUBVOL,
        ];
        let mut or = 0u32;
        for (i, v) in r.iter().enumerate() {
            assert_eq!(*v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0xFFFF);
    }

    #[test]
    fn test_basic_stats_covers_first_11_bits() {
        // STATX_BASIC_STATS covers all the fields legacy stat(2)
        // already filled in: bits 0..10.
        assert_eq!(STATX_BASIC_STATS, 0x07FF);
        // STATX_ALL adds BTIME (bit 11) on top.
        assert_eq!(STATX_ALL, 0x0FFF);
        // And BTIME is exactly the difference.
        assert_eq!(STATX_ALL & !STATX_BASIC_STATS, STATX_BTIME);
    }

    #[test]
    fn test_attr_flags_single_bit() {
        let a = [
            STATX_ATTR_COMPRESSED,
            STATX_ATTR_IMMUTABLE,
            STATX_ATTR_APPEND,
            STATX_ATTR_NODUMP,
            STATX_ATTR_ENCRYPTED,
            STATX_ATTR_AUTOMOUNT,
            STATX_ATTR_MOUNT_ROOT,
            STATX_ATTR_VERITY,
            STATX_ATTR_DAX,
        ];
        for v in a {
            assert!(v.is_power_of_two());
        }
        // VERITY/DAX live up at bits 20/21 to share encoding with chattr +i.
        assert_eq!(STATX_ATTR_VERITY, 1 << 20);
        assert_eq!(STATX_ATTR_DAX, 1 << 21);
    }

    #[test]
    fn test_at_fdcwd_is_minus_100() {
        assert_eq!(AT_FDCWD, -100);
    }

    #[test]
    fn test_statx_sync_type_mask_covers_both_force_and_dont() {
        // SYNC_TYPE is the 2-bit mask covering FORCE_SYNC and DONT_SYNC.
        assert_eq!(AT_STATX_SYNC_TYPE, AT_STATX_FORCE_SYNC | AT_STATX_DONT_SYNC);
        // SYNC_AS_STAT is the zero (default) value within that mask.
        assert_eq!(AT_STATX_SYNC_AS_STAT & AT_STATX_SYNC_TYPE, 0);
    }

    #[test]
    fn test_syscall_number_x86_64() {
        // statx is syscall 332 on x86_64.
        assert_eq!(NR_STATX, 332);
    }
}
