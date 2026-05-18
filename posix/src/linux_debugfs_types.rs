//! `<linux/debugfs.h>` — debugfs attribute type and flag constants.
//!
//! debugfs is a virtual filesystem for kernel debugging that provides
//! a simple interface for exporting kernel state to userspace. It is
//! typically mounted at `/sys/kernel/debug` and available only to
//! root. These constants define entry types and access modes.

// ---------------------------------------------------------------------------
// debugfs file types
// ---------------------------------------------------------------------------

/// Regular file entry.
pub const DEBUGFS_TYPE_FILE: u32 = 0;
/// Directory entry.
pub const DEBUGFS_TYPE_DIR: u32 = 1;
/// Symbolic link entry.
pub const DEBUGFS_TYPE_SYMLINK: u32 = 2;
/// Blob (binary large object) entry.
pub const DEBUGFS_TYPE_BLOB: u32 = 3;
/// Boolean value entry (0/1).
pub const DEBUGFS_TYPE_BOOL: u32 = 4;
/// U8 value entry.
pub const DEBUGFS_TYPE_U8: u32 = 5;
/// U16 value entry.
pub const DEBUGFS_TYPE_U16: u32 = 6;
/// U32 value entry.
pub const DEBUGFS_TYPE_U32: u32 = 7;
/// U64 value entry.
pub const DEBUGFS_TYPE_U64: u32 = 8;
/// Size_t value entry.
pub const DEBUGFS_TYPE_SIZE_T: u32 = 9;
/// Atomic_t value entry.
pub const DEBUGFS_TYPE_ATOMIC: u32 = 10;

// ---------------------------------------------------------------------------
// debugfs access modes
// ---------------------------------------------------------------------------

/// Read-only access.
pub const DEBUGFS_MODE_RO: u32 = 0o444;
/// Write-only access.
pub const DEBUGFS_MODE_WO: u32 = 0o200;
/// Read-write access.
pub const DEBUGFS_MODE_RW: u32 = 0o644;

// ---------------------------------------------------------------------------
// debugfs filesystem magic
// ---------------------------------------------------------------------------

/// debugfs filesystem magic number.
pub const DEBUGFS_MAGIC: u64 = 0x64626720;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_types_distinct() {
        let types = [
            DEBUGFS_TYPE_FILE, DEBUGFS_TYPE_DIR, DEBUGFS_TYPE_SYMLINK,
            DEBUGFS_TYPE_BLOB, DEBUGFS_TYPE_BOOL,
            DEBUGFS_TYPE_U8, DEBUGFS_TYPE_U16,
            DEBUGFS_TYPE_U32, DEBUGFS_TYPE_U64,
            DEBUGFS_TYPE_SIZE_T, DEBUGFS_TYPE_ATOMIC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_file_type_is_zero() {
        assert_eq!(DEBUGFS_TYPE_FILE, 0);
    }

    #[test]
    fn test_access_modes_distinct() {
        let modes = [DEBUGFS_MODE_RO, DEBUGFS_MODE_WO, DEBUGFS_MODE_RW];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_magic() {
        assert_eq!(DEBUGFS_MAGIC, 0x64626720);
    }
}
