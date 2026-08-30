//! `<linux/virtio_blk.h>` — Virtio block device constants.
//!
//! Virtio-blk provides virtual block device access in VMs.
//! It defines the protocol between guest drivers and the
//! hypervisor for disk I/O operations.

pub use crate::linux_virtio_types::VIRTIO_ID_BLOCK;

// ---------------------------------------------------------------------------
// Virtio-blk feature bits
// ---------------------------------------------------------------------------

/// Device supports max segment size.
pub const VIRTIO_BLK_F_SIZE_MAX: u32 = 1;
/// Device supports max number of segments.
pub const VIRTIO_BLK_F_SEG_MAX: u32 = 2;
/// Disk geometry available.
pub const VIRTIO_BLK_F_GEOMETRY: u32 = 4;
/// Read-only device.
pub const VIRTIO_BLK_F_RO: u32 = 5;
/// Block size available.
pub const VIRTIO_BLK_F_BLK_SIZE: u32 = 6;
/// Flush command supported.
pub const VIRTIO_BLK_F_FLUSH: u32 = 9;
/// Topology information available.
pub const VIRTIO_BLK_F_TOPOLOGY: u32 = 10;
/// Multiqueue supported.
pub const VIRTIO_BLK_F_MQ: u32 = 12;
/// Discard command supported.
pub const VIRTIO_BLK_F_DISCARD: u32 = 13;
/// Write zeroes command supported.
pub const VIRTIO_BLK_F_WRITE_ZEROES: u32 = 14;
/// Lifetime information available.
pub const VIRTIO_BLK_F_LIFETIME: u32 = 15;
/// Secure erase command supported.
pub const VIRTIO_BLK_F_SECURE_ERASE: u32 = 16;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Read.
pub const VIRTIO_BLK_T_IN: u32 = 0;
/// Write.
pub const VIRTIO_BLK_T_OUT: u32 = 1;
/// Flush.
pub const VIRTIO_BLK_T_FLUSH: u32 = 4;
/// Discard.
pub const VIRTIO_BLK_T_DISCARD: u32 = 11;
/// Write zeroes.
pub const VIRTIO_BLK_T_WRITE_ZEROES: u32 = 13;
/// Secure erase.
pub const VIRTIO_BLK_T_SECURE_ERASE: u32 = 14;
/// Get device ID.
pub const VIRTIO_BLK_T_GET_ID: u32 = 8;

// ---------------------------------------------------------------------------
// Status codes
// ---------------------------------------------------------------------------

/// Success.
pub const VIRTIO_BLK_S_OK: u8 = 0;
/// I/O error.
pub const VIRTIO_BLK_S_IOERR: u8 = 1;
/// Unsupported request.
pub const VIRTIO_BLK_S_UNSUPP: u8 = 2;

// ---------------------------------------------------------------------------
// Device ID length
// ---------------------------------------------------------------------------

/// Maximum serial number length.
pub const VIRTIO_BLK_ID_BYTES: usize = 20;

// ---------------------------------------------------------------------------
// Request header structure
// ---------------------------------------------------------------------------

/// Virtio block request header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioBlkReqHdr {
    /// Request type.
    pub req_type: u32,
    /// Reserved.
    pub reserved: u32,
    /// Sector (512-byte offset).
    pub sector: u64,
}

impl VirtioBlkReqHdr {
    /// Create a zeroed request header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_distinct() {
        let feats = [
            VIRTIO_BLK_F_SIZE_MAX,
            VIRTIO_BLK_F_SEG_MAX,
            VIRTIO_BLK_F_GEOMETRY,
            VIRTIO_BLK_F_RO,
            VIRTIO_BLK_F_BLK_SIZE,
            VIRTIO_BLK_F_FLUSH,
            VIRTIO_BLK_F_TOPOLOGY,
            VIRTIO_BLK_F_MQ,
            VIRTIO_BLK_F_DISCARD,
            VIRTIO_BLK_F_WRITE_ZEROES,
            VIRTIO_BLK_F_LIFETIME,
            VIRTIO_BLK_F_SECURE_ERASE,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_req_types_distinct() {
        let types = [
            VIRTIO_BLK_T_IN,
            VIRTIO_BLK_T_OUT,
            VIRTIO_BLK_T_FLUSH,
            VIRTIO_BLK_T_DISCARD,
            VIRTIO_BLK_T_WRITE_ZEROES,
            VIRTIO_BLK_T_SECURE_ERASE,
            VIRTIO_BLK_T_GET_ID,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [VIRTIO_BLK_S_OK, VIRTIO_BLK_S_IOERR, VIRTIO_BLK_S_UNSUPP];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_req_hdr_size() {
        assert_eq!(core::mem::size_of::<VirtioBlkReqHdr>(), 16);
    }

    #[test]
    fn test_req_hdr_zeroed() {
        let hdr = VirtioBlkReqHdr::zeroed();
        assert_eq!(hdr.req_type, 0);
        assert_eq!(hdr.sector, 0);
    }

    #[test]
    fn test_id_bytes() {
        assert_eq!(VIRTIO_BLK_ID_BYTES, 20);
    }

    #[test]
    fn test_virtio_id() {
        assert_eq!(VIRTIO_ID_BLOCK, 2);
    }
}
