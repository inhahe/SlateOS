//! `<linux/qrtr.h>` — Qualcomm IPC Router socket constants.
//!
//! Constants for `AF_QIPCRTR` sockets, used by Qualcomm baseband
//! firmware on phones/laptops/SoCs to route messages between
//! co-processors. Userspace daemons (`qrtr-ns`, modem managers) use
//! these to advertise services and locate peers.

// ---------------------------------------------------------------------------
// Special node / port identifiers
// ---------------------------------------------------------------------------

/// Match any node when binding/listening.
pub const QRTR_NODE_BCAST: u32 = 0xffff_ffff;
/// Match any port when binding/listening.
pub const QRTR_PORT_CTRL: u32 = 0xffff_fffe;

// ---------------------------------------------------------------------------
// Control-message types (struct qrtr_ctrl_pkt.cmd)
// ---------------------------------------------------------------------------

/// No-op / sentinel.
pub const QRTR_TYPE_DATA: u32 = 1;
/// Service registered.
pub const QRTR_TYPE_HELLO: u32 = 2;
/// Service deregistered.
pub const QRTR_TYPE_BYE: u32 = 3;
/// New server registered.
pub const QRTR_TYPE_NEW_SERVER: u32 = 4;
/// Server removed.
pub const QRTR_TYPE_DEL_SERVER: u32 = 5;
/// Client removed.
pub const QRTR_TYPE_DEL_CLIENT: u32 = 6;
/// Resume transmission (flow control).
pub const QRTR_TYPE_RESUME_TX: u32 = 7;
/// Exit / cleanup.
pub const QRTR_TYPE_EXIT: u32 = 8;
/// Ping.
pub const QRTR_TYPE_PING: u32 = 9;
/// New lookup request.
pub const QRTR_TYPE_NEW_LOOKUP: u32 = 10;
/// Lookup removed.
pub const QRTR_TYPE_DEL_LOOKUP: u32 = 11;

// ---------------------------------------------------------------------------
// Service-instance encoding (within a 32-bit instance field)
// ---------------------------------------------------------------------------

/// Mask covering the instance-id sub-field.
pub const QRTR_INSTANCE_MASK: u32 = 0x00ff_ffff;
/// Bit shift to recover the version sub-field.
pub const QRTR_VERSION_SHIFT: u32 = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_ids_high_bits_set() {
        // BCAST/CTRL use the top of the 32-bit space so they cannot
        // collide with a real allocated id.
        assert!(QRTR_NODE_BCAST > 0xffff_0000);
        assert!(QRTR_PORT_CTRL > 0xffff_0000);
        assert_ne!(QRTR_NODE_BCAST, QRTR_PORT_CTRL);
    }

    #[test]
    fn test_ctrl_types_distinct() {
        let types = [
            QRTR_TYPE_DATA,
            QRTR_TYPE_HELLO,
            QRTR_TYPE_BYE,
            QRTR_TYPE_NEW_SERVER,
            QRTR_TYPE_DEL_SERVER,
            QRTR_TYPE_DEL_CLIENT,
            QRTR_TYPE_RESUME_TX,
            QRTR_TYPE_EXIT,
            QRTR_TYPE_PING,
            QRTR_TYPE_NEW_LOOKUP,
            QRTR_TYPE_DEL_LOOKUP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_instance_encoding() {
        // Instance field splits into a 24-bit instance and 8-bit
        // version: mask + (1 << shift) must tile the full u32.
        assert_eq!(QRTR_INSTANCE_MASK, (1u32 << QRTR_VERSION_SHIFT) - 1);
        // Round-trip: encode v=3, inst=0x1234 and decode it.
        let encoded = (3u32 << QRTR_VERSION_SHIFT) | 0x1234;
        assert_eq!(encoded & QRTR_INSTANCE_MASK, 0x1234);
        assert_eq!(encoded >> QRTR_VERSION_SHIFT, 3);
    }
}
