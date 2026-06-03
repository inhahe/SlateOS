//! `<linux/mctp.h>` — Management Component Transport Protocol ABI.
//!
//! MCTP (DMTF DSP0236) is the management-bus transport used by every
//! modern BMC. Linux 5.15 added native AF_MCTP support — `mctp(8)`
//! configures endpoints, OpenBMC and `pldm` daemons exchange messages
//! over `socket(AF_MCTP, SOCK_DGRAM, 0)` using the constants below.

// ---------------------------------------------------------------------------
// Address family / protocol family
// ---------------------------------------------------------------------------

/// `AF_MCTP` / `PF_MCTP` — Linux 5.15+.
pub const AF_MCTP: u32 = 45;

// ---------------------------------------------------------------------------
// Special address values
// ---------------------------------------------------------------------------

/// Null endpoint id — the unconfigured value.
pub const MCTP_ADDR_NULL: u8 = 0x00;
/// Broadcast endpoint id — sent to all on the bus.
pub const MCTP_ADDR_ANY: u8 = 0xFF;

// ---------------------------------------------------------------------------
// `struct sockaddr_mctp.smctp_tag` flags
// ---------------------------------------------------------------------------

/// Mask covering the 3-bit tag value.
pub const MCTP_TAG_MASK: u8 = 0x07;
/// "Tag owner" bit — set by the side that allocated the tag.
pub const MCTP_TAG_OWNER: u8 = 0x08;
/// Preallocated tag — owner asks the kernel for the well-known tag.
pub const MCTP_TAG_PREALLOC: u8 = 0x10;

// ---------------------------------------------------------------------------
// Message types (DMTF DSP0239 IANA-style registry)
// ---------------------------------------------------------------------------

pub const MCTP_TYPE_MCTP_CONTROL: u8 = 0x00;
pub const MCTP_TYPE_PLDM: u8 = 0x01;
pub const MCTP_TYPE_NCSI: u8 = 0x02;
pub const MCTP_TYPE_ETHERNET: u8 = 0x03;
pub const MCTP_TYPE_NVME: u8 = 0x04;
pub const MCTP_TYPE_SPDM: u8 = 0x05;
pub const MCTP_TYPE_SECURE_MESSAGE: u8 = 0x06;
pub const MCTP_TYPE_CXL_FM_API: u8 = 0x07;
pub const MCTP_TYPE_CXL_CCI: u8 = 0x08;
pub const MCTP_TYPE_VENDOR_PCI: u8 = 0x7E;
pub const MCTP_TYPE_VENDOR_IANA: u8 = 0x7F;

// ---------------------------------------------------------------------------
// Net flags (smctp_network)
// ---------------------------------------------------------------------------

/// "Any" network — match every configured net.
pub const MCTP_NET_ANY: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_family() {
        // AF_MCTP was allocated as 45 in Linux 5.15.
        assert_eq!(AF_MCTP, 45);
    }

    #[test]
    fn test_special_addresses() {
        assert_eq!(MCTP_ADDR_NULL, 0);
        assert_eq!(MCTP_ADDR_ANY, 0xFF);
        assert_ne!(MCTP_ADDR_NULL, MCTP_ADDR_ANY);
    }

    #[test]
    fn test_tag_layout() {
        // 3-bit tag mask.
        assert_eq!(MCTP_TAG_MASK, 0b0000_0111);
        // Owner bit is bit 3.
        assert_eq!(MCTP_TAG_OWNER, 0b0000_1000);
        assert!(MCTP_TAG_OWNER.is_power_of_two());
        // Prealloc bit is bit 4 — does not overlap mask or owner.
        assert_eq!(MCTP_TAG_PREALLOC, 0b0001_0000);
        assert_eq!(MCTP_TAG_PREALLOC & (MCTP_TAG_MASK | MCTP_TAG_OWNER), 0);
    }

    #[test]
    fn test_message_types_distinct_and_in_byte() {
        let m = [
            MCTP_TYPE_MCTP_CONTROL,
            MCTP_TYPE_PLDM,
            MCTP_TYPE_NCSI,
            MCTP_TYPE_ETHERNET,
            MCTP_TYPE_NVME,
            MCTP_TYPE_SPDM,
            MCTP_TYPE_SECURE_MESSAGE,
            MCTP_TYPE_CXL_FM_API,
            MCTP_TYPE_CXL_CCI,
            MCTP_TYPE_VENDOR_PCI,
            MCTP_TYPE_VENDOR_IANA,
        ];
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
            // All fit in a 7-bit field per DSP0236.
            assert!(m[i] & 0x80 == 0);
        }
    }

    #[test]
    fn test_message_types_dense_low_block_and_vendor_high() {
        // Types 0..8 are a dense block of standardized protocols.
        let std_block = [
            MCTP_TYPE_MCTP_CONTROL,
            MCTP_TYPE_PLDM,
            MCTP_TYPE_NCSI,
            MCTP_TYPE_ETHERNET,
            MCTP_TYPE_NVME,
            MCTP_TYPE_SPDM,
            MCTP_TYPE_SECURE_MESSAGE,
            MCTP_TYPE_CXL_FM_API,
            MCTP_TYPE_CXL_CCI,
        ];
        for (i, &v) in std_block.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Vendor types are at the top of the 7-bit space.
        assert_eq!(MCTP_TYPE_VENDOR_PCI, 0x7E);
        assert_eq!(MCTP_TYPE_VENDOR_IANA, 0x7F);
    }

    #[test]
    fn test_net_any_is_zero() {
        assert_eq!(MCTP_NET_ANY, 0);
    }
}
