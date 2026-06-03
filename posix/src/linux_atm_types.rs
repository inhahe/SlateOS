//! `<linux/atm.h>` — ATM (Asynchronous Transfer Mode) constants.
//!
//! ATM is a cell-based switching/multiplexing technology.
//! These constants define ATM address families, QoS traffic
//! classes, AAL types, and cell header fields.

// ---------------------------------------------------------------------------
// ATM address family / socket
// ---------------------------------------------------------------------------

/// ATM PVC (Permanent Virtual Circuit).
pub const ATM_PVC: u32 = 0;
/// ATM SVC (Switched Virtual Circuit).
pub const ATM_SVC: u32 = 1;

// ---------------------------------------------------------------------------
// ATM cell header constants
// ---------------------------------------------------------------------------

/// ATM cell payload size (bytes).
pub const ATM_CELL_PAYLOAD: u32 = 48;
/// ATM cell header size (bytes).
pub const ATM_CELL_SIZE: u32 = 53;
/// Maximum AAL5 segment size.
pub const ATM_MAX_AAL5_PDU: u32 = 65535;
/// ATM header size (bytes).
pub const ATM_AAL0_SDU: u32 = 52;

// ---------------------------------------------------------------------------
// ATM AAL (ATM Adaptation Layer) types
// ---------------------------------------------------------------------------

/// No AAL (raw cells).
pub const ATM_NO_AAL: u32 = 0;
/// AAL0 (cell-level).
pub const ATM_AAL0: u32 = 13;
/// AAL1 (constant bit rate).
pub const ATM_AAL1: u32 = 1;
/// AAL2 (variable bit rate, short packets).
pub const ATM_AAL2: u32 = 2;
/// AAL3/4 (connection-oriented).
pub const ATM_AAL34: u32 = 3;
/// AAL5 (connection-oriented data, most common).
pub const ATM_AAL5: u32 = 5;

// ---------------------------------------------------------------------------
// ATM traffic classes (QoS)
// ---------------------------------------------------------------------------

/// No QoS / unspecified.
pub const ATM_NONE: u32 = 0;
/// Unspecified Bit Rate.
pub const ATM_UBR: u32 = 1;
/// Constant Bit Rate.
pub const ATM_CBR: u32 = 2;
/// Variable Bit Rate (non real-time).
pub const ATM_VBR: u32 = 3;
/// Available Bit Rate.
pub const ATM_ABR: u32 = 4;
/// Any class (wildcard).
pub const ATM_ANYCLASS: u32 = 5;

// ---------------------------------------------------------------------------
// ATM IOCTL commands
// ---------------------------------------------------------------------------

/// Get number of ATM devices.
pub const ATM_GETLINKRATE: u32 = 0x6180;
/// Get device names.
pub const ATM_GETNAMES: u32 = 0x6181;
/// Get type.
pub const ATM_GETTYPE: u32 = 0x6182;
/// Get ESI (End System Identifier).
pub const ATM_GETESI: u32 = 0x6183;
/// Get address.
pub const ATM_GETADDR: u32 = 0x6184;
/// Get loop-mode.
pub const ATM_RSTADDR: u32 = 0x6185;
/// Add address.
pub const ATM_ADDADDR: u32 = 0x6186;
/// Delete address.
pub const ATM_DELADDR: u32 = 0x6187;
/// Get stats.
pub const ATM_GETSTAT: u32 = 0x6188;
/// Get loop mode.
pub const ATM_GETSTATZ: u32 = 0x6189;
/// Get CI range.
pub const ATM_GETCIRANGE: u32 = 0x618A;
/// Set CI range.
pub const ATM_SETCIRANGE: u32 = 0x618B;
/// Set ESI.
pub const ATM_SETESI: u32 = 0x618C;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aal_types_distinct() {
        let types = [
            ATM_NO_AAL, ATM_AAL0, ATM_AAL1, ATM_AAL2, ATM_AAL34, ATM_AAL5,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_traffic_classes_distinct() {
        let classes = [ATM_NONE, ATM_UBR, ATM_CBR, ATM_VBR, ATM_ABR, ATM_ANYCLASS];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_cell_payload_size() {
        assert_eq!(ATM_CELL_PAYLOAD, 48);
    }

    #[test]
    fn test_cell_total_size() {
        assert_eq!(ATM_CELL_SIZE, 53);
    }

    #[test]
    fn test_pvc_svc_distinct() {
        assert_ne!(ATM_PVC, ATM_SVC);
    }

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            ATM_GETLINKRATE,
            ATM_GETNAMES,
            ATM_GETTYPE,
            ATM_GETESI,
            ATM_GETADDR,
            ATM_RSTADDR,
            ATM_ADDADDR,
            ATM_DELADDR,
            ATM_GETSTAT,
            ATM_GETSTATZ,
            ATM_GETCIRANGE,
            ATM_SETCIRANGE,
            ATM_SETESI,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_none_is_zero() {
        assert_eq!(ATM_NONE, 0);
    }

    #[test]
    fn test_no_aal_is_zero() {
        assert_eq!(ATM_NO_AAL, 0);
    }
}
