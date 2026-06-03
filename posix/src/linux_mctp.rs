//! `<linux/mctp.h>` — Management Component Transport Protocol constants.
//!
//! MCTP is a protocol for communication between management controllers
//! (BMCs, CPLDs, etc.) over SMBus, PCIe VDM, or serial. Used by PLDM,
//! SPDM, and other platform management protocols.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// AF_MCTP address family.
pub const AF_MCTP: i32 = 45;

// ---------------------------------------------------------------------------
// MCTP endpoint IDs
// ---------------------------------------------------------------------------

/// Null EID (not yet assigned).
pub const MCTP_ADDR_NULL: u8 = 0;
/// Any EID (wildcard for bind).
pub const MCTP_ADDR_ANY: u8 = 0xFF;
/// Broadcast EID.
pub const MCTP_ADDR_BROADCAST: u8 = 0xFF;

/// Minimum valid EID.
pub const MCTP_EID_MIN: u8 = 8;
/// Maximum valid EID.
pub const MCTP_EID_MAX: u8 = 254;

// ---------------------------------------------------------------------------
// MCTP message types
// ---------------------------------------------------------------------------

/// MCTP control.
pub const MCTP_TYPE_CONTROL: u8 = 0x00;
/// PLDM (Platform Level Data Model).
pub const MCTP_TYPE_PLDM: u8 = 0x01;
/// NCSI over MCTP.
pub const MCTP_TYPE_NCSI: u8 = 0x02;
/// Ethernet over MCTP.
pub const MCTP_TYPE_ETHERNET: u8 = 0x03;
/// NVMe-MI over MCTP.
pub const MCTP_TYPE_NVME_MI: u8 = 0x04;
/// SPDM (Security Protocol and Data Model).
pub const MCTP_TYPE_SPDM: u8 = 0x05;
/// Secured MCTP.
pub const MCTP_TYPE_SECURED_MCTP: u8 = 0x06;
/// CXL FM-API.
pub const MCTP_TYPE_CXL_FM_API: u8 = 0x07;
/// CXL CCI.
pub const MCTP_TYPE_CXL_CCI: u8 = 0x08;
/// Vendor defined (range start).
pub const MCTP_TYPE_VENDOR_DEFINED_PCI: u8 = 0x7E;
/// Vendor defined (IANA).
pub const MCTP_TYPE_VENDOR_DEFINED_IANA: u8 = 0x7F;

// ---------------------------------------------------------------------------
// MCTP socket options
// ---------------------------------------------------------------------------

/// Set/get MCTP network.
pub const MCTP_OPT_NET: i32 = 1;

// ---------------------------------------------------------------------------
// MCTP header flags
// ---------------------------------------------------------------------------

/// Start of message flag.
pub const MCTP_HDR_FLAG_SOM: u8 = 0x80;
/// End of message flag.
pub const MCTP_HDR_FLAG_EOM: u8 = 0x40;
/// Tag owner flag.
pub const MCTP_HDR_FLAG_TO: u8 = 0x08;
/// Tag mask.
pub const MCTP_HDR_TAG_MASK: u8 = 0x07;

// ---------------------------------------------------------------------------
// Socket address
// ---------------------------------------------------------------------------

/// MCTP socket address.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrMctp {
    /// Address family (AF_MCTP).
    pub smctp_family: u16,
    /// Padding.
    pub __smctp_pad0: u16,
    /// Network ID.
    pub smctp_network: u32,
    /// Endpoint address.
    pub smctp_addr: u8,
    /// Message type.
    pub smctp_type: u8,
    /// Tag.
    pub smctp_tag: u8,
    /// Padding.
    pub __smctp_pad1: u8,
}

impl SockaddrMctp {
    /// Create a zeroed MCTP address.
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
    fn test_af_mctp() {
        assert_eq!(AF_MCTP, 45);
    }

    #[test]
    fn test_eid_range() {
        assert!(MCTP_EID_MIN < MCTP_EID_MAX);
        assert_eq!(MCTP_EID_MIN, 8);
        assert_eq!(MCTP_EID_MAX, 254);
    }

    #[test]
    fn test_msg_types_distinct() {
        let types = [
            MCTP_TYPE_CONTROL,
            MCTP_TYPE_PLDM,
            MCTP_TYPE_NCSI,
            MCTP_TYPE_ETHERNET,
            MCTP_TYPE_NVME_MI,
            MCTP_TYPE_SPDM,
            MCTP_TYPE_SECURED_MCTP,
            MCTP_TYPE_CXL_FM_API,
            MCTP_TYPE_CXL_CCI,
            MCTP_TYPE_VENDOR_DEFINED_PCI,
            MCTP_TYPE_VENDOR_DEFINED_IANA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_header_flags() {
        assert_eq!(MCTP_HDR_FLAG_SOM, 0x80);
        assert_eq!(MCTP_HDR_FLAG_EOM, 0x40);
        assert_eq!(MCTP_HDR_FLAG_TO, 0x08);
        assert_eq!(MCTP_HDR_TAG_MASK, 0x07);
    }

    #[test]
    fn test_sockaddr_size() {
        assert_eq!(core::mem::size_of::<SockaddrMctp>(), 12);
    }

    #[test]
    fn test_sockaddr_zeroed() {
        let addr = SockaddrMctp::zeroed();
        assert_eq!(addr.smctp_family, 0);
        assert_eq!(addr.smctp_addr, 0);
    }
}
