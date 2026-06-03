//! `<linux/atm.h>` — Asynchronous Transfer Mode constants.
//!
//! ATM is a cell-based switching technology that transports data
//! in fixed 53-byte cells. While largely replaced by Ethernet in
//! LANs, ATM remains in use in DSL access networks (ATM-over-DSL)
//! and legacy telecoms infrastructure.

// ---------------------------------------------------------------------------
// ATM cell constants
// ---------------------------------------------------------------------------

/// ATM cell payload size in bytes.
pub const ATM_CELL_PAYLOAD: u8 = 48;
/// ATM cell header size in bytes.
pub const ATM_CELL_HEADER: u8 = 5;
/// Total ATM cell size.
pub const ATM_CELL_SIZE: u8 = 53;

// ---------------------------------------------------------------------------
// AAL (ATM Adaptation Layer) types
// ---------------------------------------------------------------------------

/// AAL0 (raw cells).
pub const ATM_AAL0: u8 = 0;
/// AAL1 (constant bit rate).
pub const ATM_AAL1: u8 = 1;
/// AAL2 (variable bit rate, short packets).
pub const ATM_AAL2: u8 = 2;
/// AAL3/4 (connection-oriented data).
pub const ATM_AAL34: u8 = 3;
/// AAL5 (simple and efficient, most common).
pub const ATM_AAL5: u8 = 5;

// ---------------------------------------------------------------------------
// Traffic classes
// ---------------------------------------------------------------------------

/// No traffic class specified.
pub const ATM_NONE: u8 = 0;
/// Constant Bit Rate (CBR).
pub const ATM_CBR: u8 = 1;
/// Variable Bit Rate, non-real-time.
pub const ATM_VBR_NRT: u8 = 2;
/// Variable Bit Rate, real-time.
pub const ATM_VBR_RT: u8 = 3;
/// Available Bit Rate (ABR).
pub const ATM_ABR: u8 = 4;
/// Unspecified Bit Rate (UBR).
pub const ATM_UBR: u8 = 5;
/// UBR with minimum rate guarantee.
pub const ATM_UBR_PLUS: u8 = 6;

// ---------------------------------------------------------------------------
// ATM socket address constants
// ---------------------------------------------------------------------------

/// Maximum ATM address length.
pub const ATM_ESA_LEN: u8 = 20;
/// E.164 address length.
pub const ATM_E164_LEN: u8 = 12;

// ---------------------------------------------------------------------------
// VPI/VCI limits
// ---------------------------------------------------------------------------

/// Maximum VPI value (8-bit UNI).
pub const ATM_MAX_VPI_UNI: u16 = 255;
/// Maximum VPI value (12-bit NNI).
pub const ATM_MAX_VPI_NNI: u16 = 4095;
/// Maximum VCI value (16-bit).
pub const ATM_MAX_VCI: u32 = 65535;

// ---------------------------------------------------------------------------
// Well-known VCI values
// ---------------------------------------------------------------------------

/// Signaling VCI.
pub const ATM_VCI_SIGNAL: u16 = 5;
/// ILMI (management) VCI.
pub const ATM_VCI_ILMI: u16 = 16;
/// Meta-signaling VCI.
pub const ATM_VCI_META: u16 = 1;

// ---------------------------------------------------------------------------
// Socket options (SOL_ATM)
// ---------------------------------------------------------------------------

/// ATM socket level for setsockopt.
pub const SOL_ATM: u32 = 264;
/// Set QoS parameters.
pub const SO_ATMQOS: u32 = 1;
/// Set SAP (Service Access Point).
pub const SO_ATMSAP: u32 = 2;
/// Set/get multipoint flags.
pub const SO_ATMPVC: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_size() {
        assert_eq!(ATM_CELL_SIZE, ATM_CELL_HEADER + ATM_CELL_PAYLOAD);
    }

    #[test]
    fn test_aal_types_distinct() {
        let types = [ATM_AAL0, ATM_AAL1, ATM_AAL2, ATM_AAL34, ATM_AAL5];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_traffic_classes_distinct() {
        let classes = [
            ATM_NONE,
            ATM_CBR,
            ATM_VBR_NRT,
            ATM_VBR_RT,
            ATM_ABR,
            ATM_UBR,
            ATM_UBR_PLUS,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_vci_values_distinct() {
        let vcis = [ATM_VCI_SIGNAL, ATM_VCI_ILMI, ATM_VCI_META];
        for i in 0..vcis.len() {
            for j in (i + 1)..vcis.len() {
                assert_ne!(vcis[i], vcis[j]);
            }
        }
    }

    #[test]
    fn test_vpi_limits() {
        assert!(ATM_MAX_VPI_UNI < ATM_MAX_VPI_NNI);
    }
}
