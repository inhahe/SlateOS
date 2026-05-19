//! `<linux/mctp.h>` — Additional MCTP constants.
//!
//! Supplementary MCTP (Management Component Transport Protocol) constants
//! covering message types, tag flags, and address types.

// ---------------------------------------------------------------------------
// MCTP message types
// ---------------------------------------------------------------------------

/// Control message.
pub const MCTP_MSG_TYPE_CONTROL: u8 = 0x00;
/// PLDM message.
pub const MCTP_MSG_TYPE_PLDM: u8 = 0x01;
/// NCSI message.
pub const MCTP_MSG_TYPE_NCSI: u8 = 0x02;
/// Ethernet message.
pub const MCTP_MSG_TYPE_ETHERNET: u8 = 0x03;
/// NVMe-MI message.
pub const MCTP_MSG_TYPE_NVME_MI: u8 = 0x04;
/// SPDM message.
pub const MCTP_MSG_TYPE_SPDM: u8 = 0x05;
/// Secured MCTP message.
pub const MCTP_MSG_TYPE_SECURED: u8 = 0x06;
/// CXL FM-API message.
pub const MCTP_MSG_TYPE_CXL_FM: u8 = 0x07;
/// CXL CCI message.
pub const MCTP_MSG_TYPE_CXL_CCI: u8 = 0x08;
/// Vendor defined (PCI).
pub const MCTP_MSG_TYPE_VENDOR_PCI: u8 = 0x7E;
/// Vendor defined (IANA).
pub const MCTP_MSG_TYPE_VENDOR_IANA: u8 = 0x7F;

// ---------------------------------------------------------------------------
// MCTP tag flags
// ---------------------------------------------------------------------------

/// Tag owner bit.
pub const MCTP_TAG_OWNER: u8 = 0x08;
/// Tag value mask (bits 0-2).
pub const MCTP_TAG_MASK: u8 = 0x07;
/// Prealloc tag flag.
pub const MCTP_TAG_PREALLOC: u8 = 0x10;

// ---------------------------------------------------------------------------
// MCTP address constants
// ---------------------------------------------------------------------------

/// Null EID (Endpoint ID).
pub const MCTP_ADDR_NULL: u8 = 0x00;
/// Broadcast EID.
pub const MCTP_ADDR_BROADCAST: u8 = 0xFF;
/// Any EID (wildcard for binding).
pub const MCTP_ADDR_ANY: u8 = 0xFF;

/// Minimum valid EID.
pub const MCTP_EID_MIN: u8 = 0x08;
/// Maximum valid EID.
pub const MCTP_EID_MAX: u8 = 0xFE;

// ---------------------------------------------------------------------------
// MCTP network constants
// ---------------------------------------------------------------------------

/// Default MCTP network.
pub const MCTP_NET_ANY: u32 = 0;
/// Initial MCTP network.
pub const MCTP_NET_DEFAULT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_distinct() {
        let types = [
            MCTP_MSG_TYPE_CONTROL, MCTP_MSG_TYPE_PLDM,
            MCTP_MSG_TYPE_NCSI, MCTP_MSG_TYPE_ETHERNET,
            MCTP_MSG_TYPE_NVME_MI, MCTP_MSG_TYPE_SPDM,
            MCTP_MSG_TYPE_SECURED, MCTP_MSG_TYPE_CXL_FM,
            MCTP_MSG_TYPE_CXL_CCI,
            MCTP_MSG_TYPE_VENDOR_PCI, MCTP_MSG_TYPE_VENDOR_IANA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_tag_mask() {
        assert_eq!(MCTP_TAG_MASK, 0x07);
        assert_eq!(MCTP_TAG_OWNER, 0x08);
    }

    #[test]
    fn test_tag_no_overlap() {
        assert_eq!(MCTP_TAG_MASK & MCTP_TAG_OWNER, 0);
    }

    #[test]
    fn test_eid_range() {
        assert!(MCTP_EID_MIN < MCTP_EID_MAX);
    }

    #[test]
    fn test_net_distinct() {
        assert_ne!(MCTP_NET_ANY, MCTP_NET_DEFAULT);
    }
}
