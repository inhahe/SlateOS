//! `<linux/atmdev.h>` — ATM userspace device interface.
//!
//! ATM (Asynchronous Transfer Mode) is mostly historical, but the
//! Linux ABI still ships for DSL stacks (PPPoA, br2684). atmd,
//! atmsigd, and `atmtcp` exchange cells via the AAL{0,5} stream
//! interface and ATM-specific ioctls.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// `AF_ATMPVC` — permanent virtual circuit family.
pub const AF_ATMPVC: u32 = 8;
/// `AF_ATMSVC` — switched virtual circuit family.
pub const AF_ATMSVC: u32 = 20;

// ---------------------------------------------------------------------------
// Cell sizes
// ---------------------------------------------------------------------------

/// ATM cell header is 5 bytes (4 header + HEC).
pub const ATM_CELL_HEADER_SIZE: u32 = 5;
/// ATM cell payload is 48 bytes.
pub const ATM_CELL_PAYLOAD: u32 = 48;
/// Total cell size (53 bytes).
pub const ATM_CELL_SIZE: u32 = ATM_CELL_HEADER_SIZE + ATM_CELL_PAYLOAD;
/// AAL5 max SDU size (RFC 2684 §3).
pub const ATM_MAX_AAL5_PDU: u32 = 65_535;

// ---------------------------------------------------------------------------
// AAL types
// ---------------------------------------------------------------------------

/// AAL0 — raw cells.
pub const ATM_AAL0: u32 = 13;
/// AAL1 — constant-bit-rate audio.
pub const ATM_AAL1: u32 = 1;
/// AAL2 — variable-bit-rate audio.
pub const ATM_AAL2: u32 = 2;
/// AAL3/4 — connection-oriented data.
pub const ATM_AAL34: u32 = 3;
/// AAL5 — IP-over-ATM (the only one anyone uses).
pub const ATM_AAL5: u32 = 5;

// ---------------------------------------------------------------------------
// VPI/VCI ranges
// ---------------------------------------------------------------------------

/// Maximum VPI value (8-bit at UNI).
pub const ATM_MAX_VPI: u32 = 0xff;
/// Maximum VPI at NNI (12 bits).
pub const ATM_MAX_VPI_NNI: u32 = 0x0fff;
/// Maximum VCI (16 bits).
pub const ATM_MAX_VCI: u32 = 0xffff;
/// Reserved VCI for signalling (5).
pub const ATM_VCI_SIG: u32 = 5;
/// Reserved VCI for OAM F4 segment (3).
pub const ATM_VCI_F4_SEG: u32 = 3;
/// Reserved VCI for OAM F4 end-to-end (4).
pub const ATM_VCI_F4_E2E: u32 = 4;

// ---------------------------------------------------------------------------
// ioctl numbers (group letter 'a')
// ---------------------------------------------------------------------------

/// `ATM_GETLINKRATE` — query link rate in bps.
pub const ATM_GETLINKRATE: u32 = 0x4008_6112;
/// `ATM_GETNAMES` — list ATM device names.
pub const ATM_GETNAMES: u32 = 0x4008_6113;
/// `ATM_GETTYPE` — query device type string.
pub const ATM_GETTYPE: u32 = 0x4008_6114;
/// `ATM_GETESI` — query end-system identifier (6 bytes).
pub const ATM_GETESI: u32 = 0x4008_6115;
/// `ATM_GETSTAT` — get statistics counters.
pub const ATM_GETSTAT: u32 = 0x4008_6116;
/// `ATM_GETSTATZ` — get-and-zero stats.
pub const ATM_GETSTATZ: u32 = 0x4008_6117;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_families() {
        assert_eq!(AF_ATMPVC, 8);
        assert_eq!(AF_ATMSVC, 20);
        assert_ne!(AF_ATMPVC, AF_ATMSVC);
    }

    #[test]
    fn test_cell_sizes() {
        // The 5+48=53 cell is the defining feature of ATM.
        assert_eq!(ATM_CELL_SIZE, 53);
        assert_eq!(ATM_CELL_HEADER_SIZE, 5);
        assert_eq!(ATM_CELL_PAYLOAD, 48);
        // AAL5 MAX SDU is exactly 2^16 - 1 to fit the trailer length
        // field.
        assert_eq!(ATM_MAX_AAL5_PDU, 65_535);
    }

    #[test]
    fn test_aal_types_distinct() {
        let a = [ATM_AAL0, ATM_AAL1, ATM_AAL2, ATM_AAL34, ATM_AAL5];
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
    }

    #[test]
    fn test_vpi_vci_ranges() {
        // VPI is 8-bit at UNI, 12-bit at NNI; VCI is always 16-bit.
        assert_eq!(ATM_MAX_VPI, (1u32 << 8) - 1);
        assert_eq!(ATM_MAX_VPI_NNI, (1u32 << 12) - 1);
        assert_eq!(ATM_MAX_VCI, (1u32 << 16) - 1);
        // Reserved VCIs are ITU-T I.361 fixed assignments.
        assert_eq!(ATM_VCI_SIG, 5);
        assert_eq!(ATM_VCI_F4_SEG, 3);
        assert_eq!(ATM_VCI_F4_E2E, 4);
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_a() {
        let ops = [
            ATM_GETLINKRATE,
            ATM_GETNAMES,
            ATM_GETTYPE,
            ATM_GETESI,
            ATM_GETSTAT,
            ATM_GETSTATZ,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 'a' (0x61) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'a' as u32);
        }
    }
}
