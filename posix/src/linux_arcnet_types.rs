//! `<linux/if_arcnet.h>` — ARCNET constants.
//!
//! ARCNET is a LAN protocol using token-bus access.  These
//! constants define ARCNET protocol IDs, hardware addresses,
//! frame types, and MTU values.

// ---------------------------------------------------------------------------
// ARCNET protocol IDs
// ---------------------------------------------------------------------------

/// IP over ARCNET.
pub const ARC_P_IP: u8 = 212;
/// IPv6 over ARCNET.
pub const ARC_P_IPV6: u8 = 196;
/// ARP over ARCNET.
pub const ARC_P_ARP: u8 = 213;
/// RARP over ARCNET.
pub const ARC_P_RARP: u8 = 214;
/// IPX over ARCNET.
pub const ARC_P_IPX: u8 = 250;
/// Novell EC.
pub const ARC_P_NOVELL_EC: u8 = 236;
/// Datapoint boot.
pub const ARC_P_DATAPOINT_BOOT: u8 = 0;
/// Datapoint mount.
pub const ARC_P_DATAPOINT_MOUNT: u8 = 1;
/// ATALK (AppleTalk).
pub const ARC_P_ATALK: u8 = 221;
/// LANsoft.
pub const ARC_P_LANSOFT: u8 = 251;

// ---------------------------------------------------------------------------
// ARCNET frame sizes
// ---------------------------------------------------------------------------

/// ARCNET header length.
pub const ARC_HLEN: u32 = 4;
/// ARCNET hardware address length.
pub const ARC_ALEN: u32 = 1;
/// ARCNET MTU (short frame).
pub const ARC_MTU_SHORT: u32 = 253;
/// ARCNET MTU (long frame).
pub const ARC_MTU_LONG: u32 = 504;
/// ARCNET MTU default.
pub const ARC_MTU: u32 = ARC_MTU_SHORT;

// ---------------------------------------------------------------------------
// ARCNET special addresses
// ---------------------------------------------------------------------------

/// Broadcast address.
pub const ARC_BROADCAST: u8 = 0x00;

// ---------------------------------------------------------------------------
// ARCNET frame types
// ---------------------------------------------------------------------------

/// Short frame (256 bytes max).
pub const ARC_FRAME_SHORT: u8 = 0;
/// Long frame (512 bytes max).
pub const ARC_FRAME_LONG: u8 = 1;
/// Exception frame.
pub const ARC_FRAME_EXCEPTION: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            ARC_P_IP, ARC_P_IPV6, ARC_P_ARP, ARC_P_RARP,
            ARC_P_IPX, ARC_P_NOVELL_EC, ARC_P_DATAPOINT_BOOT,
            ARC_P_DATAPOINT_MOUNT, ARC_P_ATALK, ARC_P_LANSOFT,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_ip_protocol() {
        assert_eq!(ARC_P_IP, 212);
    }

    #[test]
    fn test_mtu_values() {
        assert!(ARC_MTU_SHORT < ARC_MTU_LONG);
        assert_eq!(ARC_MTU, ARC_MTU_SHORT);
    }

    #[test]
    fn test_hlen() {
        assert_eq!(ARC_HLEN, 4);
    }

    #[test]
    fn test_alen() {
        assert_eq!(ARC_ALEN, 1);
    }

    #[test]
    fn test_broadcast() {
        assert_eq!(ARC_BROADCAST, 0);
    }

    #[test]
    fn test_frame_types_distinct() {
        let types = [ARC_FRAME_SHORT, ARC_FRAME_LONG, ARC_FRAME_EXCEPTION];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
