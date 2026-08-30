//! `<linux/stat.h>` — extended stat definitions.
//!
//! Re-exports the `statx()` interface and `Statx`/`StatxTimestamp`
//! structures from the `file` module.

pub use crate::file::Statx;
pub use crate::file::StatxTimestamp;
pub use crate::file::statx;

// Mask constants.
pub use crate::file::STATX_ALL;
pub use crate::file::STATX_ATIME;
pub use crate::file::STATX_BASIC_STATS;
pub use crate::file::STATX_BLOCKS;
pub use crate::file::STATX_BTIME;
pub use crate::file::STATX_CTIME;
pub use crate::file::STATX_GID;
pub use crate::file::STATX_INO;
pub use crate::file::STATX_MODE;
pub use crate::file::STATX_MTIME;
pub use crate::file::STATX_NLINK;
pub use crate::file::STATX_SIZE;
pub use crate::file::STATX_TYPE;
pub use crate::file::STATX_UID;

// ---------------------------------------------------------------------------
// Statx attribute flags (stx_attributes)
// ---------------------------------------------------------------------------

/// File is compressed.
pub const STATX_ATTR_COMPRESSED: u64 = 0x0004;
/// File is immutable.
pub const STATX_ATTR_IMMUTABLE: u64 = 0x0010;
/// File is append-only.
pub const STATX_ATTR_APPEND: u64 = 0x0020;
/// File is not backed up.
pub const STATX_ATTR_NODUMP: u64 = 0x0040;
/// File is encrypted.
pub const STATX_ATTR_ENCRYPTED: u64 = 0x0800;
/// File is an automount point.
pub const STATX_ATTR_AUTOMOUNT: u64 = 0x1000;
/// File is a mount root.
pub const STATX_ATTR_MOUNT_ROOT: u64 = 0x2000;
/// File has verified data (fs-verity).
pub const STATX_ATTR_VERITY: u64 = 0x0010_0000;
/// File is DAX (direct access, bypassing page cache).
pub const STATX_ATTR_DAX: u64 = 0x0020_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statx_mask_values() {
        assert_eq!(STATX_TYPE, 0x0001);
        assert_eq!(STATX_ALL, 0x0FFF);
        assert_eq!(STATX_BASIC_STATS, 0x07FF);
    }

    #[test]
    fn test_statx_struct_size() {
        assert!(core::mem::size_of::<Statx>() >= 100);
    }

    #[test]
    fn test_statx_timestamp_size() {
        assert!(core::mem::size_of::<StatxTimestamp>() >= 12);
    }

    #[test]
    fn test_attr_flags_are_bits() {
        let attrs = [
            STATX_ATTR_COMPRESSED,
            STATX_ATTR_IMMUTABLE,
            STATX_ATTR_APPEND,
            STATX_ATTR_NODUMP,
            STATX_ATTR_ENCRYPTED,
            STATX_ATTR_AUTOMOUNT,
            STATX_ATTR_MOUNT_ROOT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0, "STATX_ATTR flags must not overlap");
            }
        }
    }

    #[test]
    fn test_mask_bits_are_bits() {
        let masks = [
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
        ];
        for i in 0..masks.len() {
            for j in (i + 1)..masks.len() {
                assert_eq!(masks[i] & masks[j], 0, "STATX_ mask bits must not overlap");
            }
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(STATX_TYPE, crate::file::STATX_TYPE);
        assert_eq!(STATX_ALL, crate::file::STATX_ALL);
    }
}
