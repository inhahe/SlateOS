//! `<linux/pci-doe.h>` — PCIe Data Object Exchange (DOE) constants.
//!
//! PCIe DOE is a PCIe extended capability for exchanging data objects
//! between host software and PCIe devices/switches. It provides a
//! mailbox-like interface for structured data exchange beyond standard
//! config space. DOE is used by CXL (for component/port discovery),
//! SPDM (for device authentication and attestation), and IDE (for
//! encryption key management). Added in PCIe 6.0.

// ---------------------------------------------------------------------------
// DOE protocol IDs (vendor ID + data object type)
// ---------------------------------------------------------------------------

/// CXL DOE protocol (compliance mode).
pub const PCI_DOE_PROTOCOL_CXL: u32 = 0;
/// SPDM DOE protocol (device attestation).
pub const PCI_DOE_PROTOCOL_SPDM: u32 = 1;
/// Secured SPDM (encrypted SPDM session).
pub const PCI_DOE_PROTOCOL_SECURED_SPDM: u32 = 2;

// ---------------------------------------------------------------------------
// DOE status register bits
// ---------------------------------------------------------------------------

/// DOE is busy (processing a request).
pub const PCI_DOE_STATUS_BUSY: u32 = 1 << 0;
/// DOE has data object ready (response available).
pub const PCI_DOE_STATUS_DATA_READY: u32 = 1 << 1;
/// DOE error (protocol/format error).
pub const PCI_DOE_STATUS_ERROR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// DOE control register bits
// ---------------------------------------------------------------------------

/// Abort current DOE exchange.
pub const PCI_DOE_CTRL_ABORT: u32 = 1 << 0;
/// DOE interrupt enable.
pub const PCI_DOE_CTRL_INT_ENABLE: u32 = 1 << 1;
/// GO — start processing the data object.
pub const PCI_DOE_CTRL_GO: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// DOE header fields
// ---------------------------------------------------------------------------

/// Vendor ID for PCI-SIG defined protocols.
pub const PCI_DOE_VENDOR_PCI_SIG: u32 = 0x0001;
/// Discovery protocol data object type.
pub const PCI_DOE_TYPE_DISCOVERY: u32 = 0x00;
/// CXL compliance protocol data object type.
pub const PCI_DOE_TYPE_CXL_COMPLIANCE: u32 = 0x01;

// ---------------------------------------------------------------------------
// DOE discovery response fields
// ---------------------------------------------------------------------------

/// More protocols available (not last entry).
pub const PCI_DOE_DISC_MORE: u32 = 1 << 0;
/// Last protocol entry.
pub const PCI_DOE_DISC_LAST: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            PCI_DOE_PROTOCOL_CXL,
            PCI_DOE_PROTOCOL_SPDM,
            PCI_DOE_PROTOCOL_SECURED_SPDM,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_status_bits_no_overlap() {
        let bits = [
            PCI_DOE_STATUS_BUSY,
            PCI_DOE_STATUS_DATA_READY,
            PCI_DOE_STATUS_ERROR,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_control_bits_no_overlap() {
        let bits = [PCI_DOE_CTRL_ABORT, PCI_DOE_CTRL_INT_ENABLE, PCI_DOE_CTRL_GO];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_types_distinct() {
        assert_ne!(PCI_DOE_TYPE_DISCOVERY, PCI_DOE_TYPE_CXL_COMPLIANCE);
    }
}
