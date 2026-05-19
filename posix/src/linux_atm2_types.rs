//! `<linux/atm.h>` — Additional ATM constants.
//!
//! Supplementary ATM networking constants covering service categories,
//! AAL types, and traffic class definitions.

// ---------------------------------------------------------------------------
// ATM AAL types
// ---------------------------------------------------------------------------

/// AAL0.
pub const ATM_AAL0: u32 = 0;
/// AAL1.
pub const ATM_AAL1: u32 = 1;
/// AAL2.
pub const ATM_AAL2: u32 = 2;
/// AAL3/4.
pub const ATM_AAL34: u32 = 3;
/// AAL5.
pub const ATM_AAL5: u32 = 5;

// ---------------------------------------------------------------------------
// ATM service categories
// ---------------------------------------------------------------------------

/// No traffic.
pub const ATM_NONE: u32 = 0;
/// Constant bit rate.
pub const ATM_CBR: u32 = 1;
/// Variable bit rate real-time.
pub const ATM_VBR: u32 = 2;
/// Available bit rate.
pub const ATM_ABR: u32 = 3;
/// Any class.
pub const ATM_ANYCLASS: u32 = 4;
/// Unspecified bit rate.
pub const ATM_UBR: u32 = 5;
/// Maximum QoS.
pub const ATM_MAX_PCR: i32 = -1;

// ---------------------------------------------------------------------------
// ATM cell header sizes
// ---------------------------------------------------------------------------

/// Cell header size (UNI).
pub const ATM_CELL_HEADER: u32 = 5;
/// Cell payload size.
pub const ATM_CELL_PAYLOAD: u32 = 48;
/// Full cell size.
pub const ATM_CELL_SIZE: u32 = ATM_CELL_HEADER + ATM_CELL_PAYLOAD;
/// AAL5 trailer size.
pub const ATM_AAL5_TRAILER: u32 = 8;

// ---------------------------------------------------------------------------
// ATM ioctl numbers
// ---------------------------------------------------------------------------

/// ATM ioctl magic.
pub const ATM_IOC_MAGIC: u8 = b'a';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aal_types_distinct() {
        let aals = [ATM_AAL0, ATM_AAL1, ATM_AAL2, ATM_AAL34, ATM_AAL5];
        for i in 0..aals.len() {
            for j in (i + 1)..aals.len() {
                assert_ne!(aals[i], aals[j]);
            }
        }
    }

    #[test]
    fn test_service_categories_distinct() {
        let cats = [ATM_NONE, ATM_CBR, ATM_VBR, ATM_ABR, ATM_ANYCLASS, ATM_UBR];
        for i in 0..cats.len() {
            for j in (i + 1)..cats.len() {
                assert_ne!(cats[i], cats[j]);
            }
        }
    }

    #[test]
    fn test_cell_size() {
        assert_eq!(ATM_CELL_SIZE, ATM_CELL_HEADER + ATM_CELL_PAYLOAD);
        assert_eq!(ATM_CELL_SIZE, 53);
    }

    #[test]
    fn test_max_pcr_negative() {
        assert!(ATM_MAX_PCR < 0);
    }
}
