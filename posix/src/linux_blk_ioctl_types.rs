//! `<linux/fs.h>` — Block device ioctl command constants.
//!
//! These ioctl commands operate on block device file descriptors
//! to query geometry, set read-ahead, flush caches, and perform
//! other device-level operations.

// ---------------------------------------------------------------------------
// Block device ioctl commands
// ---------------------------------------------------------------------------

/// Get device size in sectors (512-byte).
pub const BLKGETSIZE: u32 = 0x1260;
/// Flush buffer cache.
pub const BLKFLSBUF: u32 = 0x1261;
/// Set read-ahead (sectors).
pub const BLKRASET: u32 = 0x1262;
/// Get read-ahead (sectors).
pub const BLKRAGET: u32 = 0x1263;
/// Set block device read-only.
pub const BLKROSET: u32 = 0x125D;
/// Get block device read-only flag.
pub const BLKROGET: u32 = 0x125E;
/// Re-read partition table.
pub const BLKRRPART: u32 = 0x125F;
/// Get device size in bytes (u64).
pub const BLKGETSIZE64: u32 = 0x80081272;
/// Get sector size (logical block).
pub const BLKSSZGET: u32 = 0x1268;
/// Get physical block size.
pub const BLKPBSZGET: u32 = 0x127B;
/// Get alignment offset.
pub const BLKALIGNOFF: u32 = 0x127A;
/// Get minimum I/O size.
pub const BLKIOMIN: u32 = 0x1278;
/// Get optimal I/O size.
pub const BLKIOOPT: u32 = 0x1279;
/// Discard sectors.
pub const BLKDISCARD: u32 = 0x1277;
/// Zero-fill sectors.
pub const BLKZEROOUT: u32 = 0x127F;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            BLKGETSIZE,
            BLKFLSBUF,
            BLKRASET,
            BLKRAGET,
            BLKROSET,
            BLKROGET,
            BLKRRPART,
            BLKGETSIZE64,
            BLKSSZGET,
            BLKPBSZGET,
            BLKALIGNOFF,
            BLKIOMIN,
            BLKIOOPT,
            BLKDISCARD,
            BLKZEROOUT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_blkgetsize() {
        assert_eq!(BLKGETSIZE, 0x1260);
    }

    #[test]
    fn test_blksszget() {
        assert_eq!(BLKSSZGET, 0x1268);
    }

    #[test]
    fn test_blkgetsize64() {
        assert_eq!(BLKGETSIZE64, 0x80081272);
    }
}
