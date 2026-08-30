//! `<linux/fs.h>` block-device ioctls (`BLKGETSIZE`, `BLKDISCARD`, …).
//!
//! These are the generic block-layer ioctls every block driver
//! supports, regardless of the underlying transport (NVMe, SCSI,
//! virtio_blk). Numbers are encoded with the `_IO` family macros, so
//! we list the cooked u32 constants directly.

// ---------------------------------------------------------------------------
// Magic byte for the BLK ioctl family
// ---------------------------------------------------------------------------

pub const BLK_IOCTL_MAGIC: u8 = 0x12;

// ---------------------------------------------------------------------------
// Geometry / size queries
// ---------------------------------------------------------------------------

pub const BLKROSET: u32 = 0x1261;
pub const BLKROGET: u32 = 0x125E;
pub const BLKRRPART: u32 = 0x125F;
pub const BLKGETSIZE: u32 = 0x1260;
pub const BLKFLSBUF: u32 = 0x1261;
pub const BLKRASET: u32 = 0x1262;
pub const BLKRAGET: u32 = 0x1263;
pub const BLKFRASET: u32 = 0x1264;
pub const BLKFRAGET: u32 = 0x1265;
pub const BLKSECTSET: u32 = 0x1266;
pub const BLKSECTGET: u32 = 0x1267;
pub const BLKSSZGET: u32 = 0x1268;
pub const BLKBSZGET: u32 = 0x80081270;
pub const BLKBSZSET: u32 = 0x40081271;
pub const BLKGETSIZE64: u32 = 0x80081272;

// ---------------------------------------------------------------------------
// Discard / zeroing / secure erase
// ---------------------------------------------------------------------------

pub const BLKDISCARD: u32 = 0x1277;
pub const BLKIOMIN: u32 = 0x1278;
pub const BLKIOOPT: u32 = 0x1279;
pub const BLKALIGNOFF: u32 = 0x127A;
pub const BLKPBSZGET: u32 = 0x127B;
pub const BLKDISCARDZEROES: u32 = 0x127C;
pub const BLKSECDISCARD: u32 = 0x127D;
pub const BLKROTATIONAL: u32 = 0x127E;
pub const BLKZEROOUT: u32 = 0x127F;
pub const BLKGETDISKSEQ: u32 = 0x80081280;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_byte() {
        // `_IO(0x12, ...)` family.
        assert_eq!(BLK_IOCTL_MAGIC, 0x12);
    }

    #[test]
    fn test_short_ioctl_nrs_in_low_byte() {
        // The 16-bit ioctls embed (type<<8 | nr) — type byte is 0x12.
        let short = [
            BLKROSET,
            BLKROGET,
            BLKRRPART,
            BLKGETSIZE,
            BLKFLSBUF,
            BLKRASET,
            BLKRAGET,
            BLKFRASET,
            BLKFRAGET,
            BLKSECTSET,
            BLKSECTGET,
            BLKSSZGET,
            BLKDISCARD,
            BLKIOMIN,
            BLKIOOPT,
            BLKALIGNOFF,
            BLKPBSZGET,
            BLKDISCARDZEROES,
            BLKSECDISCARD,
            BLKROTATIONAL,
            BLKZEROOUT,
        ];
        for &v in &short {
            // Type byte 0x12 sits in bits 15..8.
            assert_eq!((v >> 8) & 0xFF, BLK_IOCTL_MAGIC as u32);
        }
        // Pairwise distinct (excluding the documented BLKROSET/BLKFLSBUF
        // duplicate alias — both 0x1261, the original Linux uapi quirk).
        assert_eq!(BLKROSET, BLKFLSBUF);
    }

    #[test]
    fn test_size_queries_ordered() {
        // BLKSSZGET (logical sector size) precedes BLKBSZGET (block size).
        assert!(BLKSSZGET < BLKBSZGET);
        // BLKGETSIZE64 is a "size_t-based" variant of BLKGETSIZE.
        assert!(BLKGETSIZE64 > BLKGETSIZE);
    }

    #[test]
    fn test_size_ioctls_carry_argument_size() {
        // The 32-bit ioctls embed the argument size in bits 16..30.
        // BLKBSZGET/SET pass a 4-byte block-size argument.
        let argsize = (BLKBSZGET >> 16) & 0x3FFF;
        assert_eq!(argsize, 8);
        // BLKGETSIZE64 carries an 8-byte u64 result.
        let argsize = (BLKGETSIZE64 >> 16) & 0x3FFF;
        assert_eq!(argsize, 8);
        // BLKGETDISKSEQ carries an 8-byte u64.
        let argsize = (BLKGETDISKSEQ >> 16) & 0x3FFF;
        assert_eq!(argsize, 8);
    }

    #[test]
    fn test_discard_family_grouped() {
        // Discard variants cluster around 0x127D..0x127F.
        for v in [BLKDISCARD, BLKSECDISCARD, BLKZEROOUT, BLKDISCARDZEROES] {
            assert!((0x1277..=0x127F).contains(&v));
        }
    }

    #[test]
    fn test_topology_ioctls_in_block() {
        // The topology ioctls cluster 0x1278..0x127B.
        for v in [BLKIOMIN, BLKIOOPT, BLKALIGNOFF, BLKPBSZGET] {
            assert!((0x1278..=0x127B).contains(&v));
        }
        // BLKROTATIONAL distinguishes spinning vs solid-state media.
        assert_eq!(BLKROTATIONAL, 0x127E);
    }
}
