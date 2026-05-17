//! `<linux/mctp.h>` — MCTP (Management Component Transport Protocol) constants.
//!
//! MCTP (DMTF DSP0236) is a transport protocol for platform
//! management communication between BMCs, CPUs, GPUs, NICs, and
//! other components within a server. It runs over I2C/SMBus, PCIe
//! VDM, serial, and USB. Linux provides a socket-based MCTP stack
//! (AF_MCTP) for userspace daemons like pldmd and mctp-demux-daemon
//! to send/receive MCTP messages. Used in OpenBMC and server
//! management firmware.

// ---------------------------------------------------------------------------
// MCTP address family
// ---------------------------------------------------------------------------

/// MCTP address family.
pub const AF_MCTP: u32 = 45;

// ---------------------------------------------------------------------------
// MCTP endpoint IDs (EIDs)
// ---------------------------------------------------------------------------

/// Null EID (not assigned).
pub const MCTP_EID_NULL: u8 = 0;
/// Broadcast EID.
pub const MCTP_EID_BROADCAST: u8 = 0xFF;
/// Minimum valid EID.
pub const MCTP_EID_MIN: u8 = 8;
/// Maximum valid EID.
pub const MCTP_EID_MAX: u8 = 254;

// ---------------------------------------------------------------------------
// MCTP message types (MCTP type field in message header)
// ---------------------------------------------------------------------------

/// MCTP Control Protocol messages.
pub const MCTP_TYPE_CONTROL: u8 = 0x00;
/// PLDM (Platform Level Data Model) messages.
pub const MCTP_TYPE_PLDM: u8 = 0x01;
/// NCSI (Network Controller Sideband Interface) over MCTP.
pub const MCTP_TYPE_NCSI: u8 = 0x02;
/// Ethernet over MCTP.
pub const MCTP_TYPE_ETHERNET: u8 = 0x03;
/// NVMe Management Interface over MCTP.
pub const MCTP_TYPE_NVME_MGMT: u8 = 0x04;
/// SPDM (Security Protocol and Data Model).
pub const MCTP_TYPE_SPDM: u8 = 0x05;
/// Secured MCTP messages.
pub const MCTP_TYPE_SECURED: u8 = 0x06;
/// CXL FM-API over MCTP.
pub const MCTP_TYPE_CXL_FM: u8 = 0x07;
/// CXL CCI over MCTP.
pub const MCTP_TYPE_CXL_CCI: u8 = 0x08;
/// Vendor defined (PCI).
pub const MCTP_TYPE_VENDOR_PCI: u8 = 0x7E;
/// Vendor defined (IANA).
pub const MCTP_TYPE_VENDOR_IANA: u8 = 0x7F;

// ---------------------------------------------------------------------------
// MCTP network ID
// ---------------------------------------------------------------------------

/// Default network (local).
pub const MCTP_NET_DEFAULT: u32 = 1;

// ---------------------------------------------------------------------------
// MCTP netlink attributes (for route/neighbor management)
// ---------------------------------------------------------------------------

/// EID attribute.
pub const MCTP_ATTR_EID: u32 = 1;
/// Network ID attribute.
pub const MCTP_ATTR_NET: u32 = 2;
/// Interface index attribute.
pub const MCTP_ATTR_IFINDEX: u32 = 3;
/// Physical address (e.g., I2C slave address).
pub const MCTP_ATTR_PHYS_ADDR: u32 = 4;

// ---------------------------------------------------------------------------
// MCTP header flags
// ---------------------------------------------------------------------------

/// Start of message (SOM) flag.
pub const MCTP_HDR_FLAG_SOM: u8 = 1 << 7;
/// End of message (EOM) flag.
pub const MCTP_HDR_FLAG_EOM: u8 = 1 << 6;
/// Tag owner flag.
pub const MCTP_HDR_FLAG_TO: u8 = 1 << 3;
/// Tag value mask (3 bits).
pub const MCTP_HDR_TAG_MASK: u8 = 0x07;

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
        assert!(MCTP_EID_NULL < MCTP_EID_MIN);
    }

    #[test]
    fn test_message_types_distinct() {
        let types = [
            MCTP_TYPE_CONTROL, MCTP_TYPE_PLDM, MCTP_TYPE_NCSI,
            MCTP_TYPE_ETHERNET, MCTP_TYPE_NVME_MGMT, MCTP_TYPE_SPDM,
            MCTP_TYPE_SECURED, MCTP_TYPE_CXL_FM, MCTP_TYPE_CXL_CCI,
            MCTP_TYPE_VENDOR_PCI, MCTP_TYPE_VENDOR_IANA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            MCTP_ATTR_EID, MCTP_ATTR_NET,
            MCTP_ATTR_IFINDEX, MCTP_ATTR_PHYS_ADDR,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_header_flags_no_overlap() {
        // SOM and EOM are in different bit positions
        assert_eq!(MCTP_HDR_FLAG_SOM & MCTP_HDR_FLAG_EOM, 0);
        assert_eq!(MCTP_HDR_FLAG_SOM & MCTP_HDR_FLAG_TO, 0);
        assert_eq!(MCTP_HDR_FLAG_EOM & MCTP_HDR_FLAG_TO, 0);
    }

    #[test]
    fn test_tag_mask() {
        assert_eq!(MCTP_HDR_TAG_MASK, 0x07);
        // Tag mask should not overlap with TO flag
        assert_eq!(MCTP_HDR_TAG_MASK & MCTP_HDR_FLAG_TO, 0);
    }
}
