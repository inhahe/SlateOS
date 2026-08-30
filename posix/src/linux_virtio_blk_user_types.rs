//! `<linux/virtio_blk.h>` — virtio-blk device-side wire format.
//!
//! Guest drivers (and userspace block-device backends in vhost-user
//! or rust-vmm) use these constants when reading/writing virtio
//! block-device descriptors. virtio-blk is the most common storage
//! device in cloud VMs.

// ---------------------------------------------------------------------------
// Feature bits (negotiated in virtio_blk_config)
// ---------------------------------------------------------------------------

/// `VIRTIO_BLK_F_SIZE_MAX` — host advertises max segment size.
pub const VIRTIO_BLK_F_SIZE_MAX: u32 = 1;
/// `VIRTIO_BLK_F_SEG_MAX` — host advertises max segment count.
pub const VIRTIO_BLK_F_SEG_MAX: u32 = 2;
/// `VIRTIO_BLK_F_GEOMETRY` — disk geometry in config.
pub const VIRTIO_BLK_F_GEOMETRY: u32 = 4;
/// `VIRTIO_BLK_F_RO` — device is read-only.
pub const VIRTIO_BLK_F_RO: u32 = 5;
/// `VIRTIO_BLK_F_BLK_SIZE` — block size in config (vs assumed 512).
pub const VIRTIO_BLK_F_BLK_SIZE: u32 = 6;
/// `VIRTIO_BLK_F_FLUSH` — FLUSH command supported.
pub const VIRTIO_BLK_F_FLUSH: u32 = 9;
/// `VIRTIO_BLK_F_TOPOLOGY` — physical/optimal block size in config.
pub const VIRTIO_BLK_F_TOPOLOGY: u32 = 10;
/// `VIRTIO_BLK_F_CONFIG_WCE` — writeback cache control in config.
pub const VIRTIO_BLK_F_CONFIG_WCE: u32 = 11;
/// `VIRTIO_BLK_F_MQ` — multi-queue.
pub const VIRTIO_BLK_F_MQ: u32 = 12;
/// `VIRTIO_BLK_F_DISCARD` — DISCARD command.
pub const VIRTIO_BLK_F_DISCARD: u32 = 13;
/// `VIRTIO_BLK_F_WRITE_ZEROES` — WRITE_ZEROES command.
pub const VIRTIO_BLK_F_WRITE_ZEROES: u32 = 14;
/// `VIRTIO_BLK_F_SECURE_ERASE` — SECURE_ERASE command.
pub const VIRTIO_BLK_F_SECURE_ERASE: u32 = 16;

// ---------------------------------------------------------------------------
// Request types (virtio_blk_outhdr.type)
// ---------------------------------------------------------------------------

/// `VIRTIO_BLK_T_IN` — read request.
pub const VIRTIO_BLK_T_IN: u32 = 0;
/// `VIRTIO_BLK_T_OUT` — write request.
pub const VIRTIO_BLK_T_OUT: u32 = 1;
/// `VIRTIO_BLK_T_FLUSH` — flush cache.
pub const VIRTIO_BLK_T_FLUSH: u32 = 4;
/// `VIRTIO_BLK_T_GET_ID` — return device id string.
pub const VIRTIO_BLK_T_GET_ID: u32 = 8;
/// `VIRTIO_BLK_T_GET_LIFETIME` — return lifetime info.
pub const VIRTIO_BLK_T_GET_LIFETIME: u32 = 10;
/// `VIRTIO_BLK_T_DISCARD`.
pub const VIRTIO_BLK_T_DISCARD: u32 = 11;
/// `VIRTIO_BLK_T_WRITE_ZEROES`.
pub const VIRTIO_BLK_T_WRITE_ZEROES: u32 = 13;
/// `VIRTIO_BLK_T_SECURE_ERASE`.
pub const VIRTIO_BLK_T_SECURE_ERASE: u32 = 14;

// ---------------------------------------------------------------------------
// Status (virtio_blk_inhdr.status)
// ---------------------------------------------------------------------------

/// OK.
pub const VIRTIO_BLK_S_OK: u8 = 0;
/// Generic device error.
pub const VIRTIO_BLK_S_IOERR: u8 = 1;
/// Unsupported request.
pub const VIRTIO_BLK_S_UNSUPP: u8 = 2;

// ---------------------------------------------------------------------------
// Discard / write-zeroes flags
// ---------------------------------------------------------------------------

/// `VIRTIO_BLK_WRITE_ZEROES_FLAG_UNMAP` — also unmap the region.
pub const VIRTIO_BLK_WRITE_ZEROES_FLAG_UNMAP: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Sector size assumed by virtio-blk requests (LBA unit).
pub const VIRTIO_BLK_SECTOR_SIZE: u32 = 512;
/// Maximum device-id string length (returned by VIRTIO_BLK_T_GET_ID).
pub const VIRTIO_BLK_ID_BYTES: u32 = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_distinct_and_in_low_word() {
        let f = [
            VIRTIO_BLK_F_SIZE_MAX,
            VIRTIO_BLK_F_SEG_MAX,
            VIRTIO_BLK_F_GEOMETRY,
            VIRTIO_BLK_F_RO,
            VIRTIO_BLK_F_BLK_SIZE,
            VIRTIO_BLK_F_FLUSH,
            VIRTIO_BLK_F_TOPOLOGY,
            VIRTIO_BLK_F_CONFIG_WCE,
            VIRTIO_BLK_F_MQ,
            VIRTIO_BLK_F_DISCARD,
            VIRTIO_BLK_F_WRITE_ZEROES,
            VIRTIO_BLK_F_SECURE_ERASE,
        ];
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
            // These are bit numbers (not masks); virtio uses a u64
            // feature word so they fit comfortably below 64.
            assert!(f[i] < 64);
        }
    }

    #[test]
    fn test_request_types_distinct() {
        let r = [
            VIRTIO_BLK_T_IN,
            VIRTIO_BLK_T_OUT,
            VIRTIO_BLK_T_FLUSH,
            VIRTIO_BLK_T_GET_ID,
            VIRTIO_BLK_T_GET_LIFETIME,
            VIRTIO_BLK_T_DISCARD,
            VIRTIO_BLK_T_WRITE_ZEROES,
            VIRTIO_BLK_T_SECURE_ERASE,
        ];
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
        // T_IN = 0 means read; userspace must check IN vs OUT before
        // touching the descriptor chain.
        assert_eq!(VIRTIO_BLK_T_IN, 0);
        assert_eq!(VIRTIO_BLK_T_OUT, 1);
    }

    #[test]
    fn test_status_codes_dense() {
        assert_eq!(VIRTIO_BLK_S_OK, 0);
        assert_eq!(VIRTIO_BLK_S_IOERR, 1);
        assert_eq!(VIRTIO_BLK_S_UNSUPP, 2);
    }

    #[test]
    fn test_sizes() {
        // LBA size is always 512 in the virtio-blk wire format, even
        // when the underlying disk is 4K.
        assert_eq!(VIRTIO_BLK_SECTOR_SIZE, 512);
        assert_eq!(VIRTIO_BLK_ID_BYTES, 20);
    }

    #[test]
    fn test_write_zeroes_flag_pow2() {
        assert!(VIRTIO_BLK_WRITE_ZEROES_FLAG_UNMAP.is_power_of_two());
    }
}
