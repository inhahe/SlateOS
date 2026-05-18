//! `<scsi/scsi_proto.h>` (sense key subset) — SCSI sense data constants.
//!
//! When a SCSI command fails with CHECK CONDITION status, the device
//! provides sense data describing the error. The sense key is a 4-bit
//! code categorising the error (hardware, medium, illegal request,
//! etc.). Additional sense code (ASC) and qualifier (ASCQ) give more
//! specific information.

// ---------------------------------------------------------------------------
// Sense keys
// ---------------------------------------------------------------------------

/// No error (informational).
pub const NO_SENSE: u8 = 0x0;
/// Command completed but with recovery action.
pub const RECOVERED_ERROR: u8 = 0x1;
/// Device is not ready (spin-up needed, media not present).
pub const NOT_READY: u8 = 0x2;
/// Unrecoverable read/write error.
pub const MEDIUM_ERROR: u8 = 0x3;
/// Hardware failure in device or controller.
pub const HARDWARE_ERROR: u8 = 0x4;
/// Invalid CDB field, parameter, or unsupported operation.
pub const ILLEGAL_REQUEST: u8 = 0x5;
/// Unit attention: device was reset or media changed.
pub const UNIT_ATTENTION: u8 = 0x6;
/// Write or erase on write-protected media.
pub const DATA_PROTECT: u8 = 0x7;
/// Blank check: blank or end-of-data on sequential media.
pub const BLANK_CHECK: u8 = 0x8;
/// Vendor-specific sense key.
pub const VENDOR_SPECIFIC: u8 = 0x9;
/// Copy or compare command aborted.
pub const COPY_ABORTED: u8 = 0xA;
/// Command aborted by target.
pub const ABORTED_COMMAND: u8 = 0xB;
/// (Obsolete, was "equal").
pub const VOLUME_OVERFLOW: u8 = 0xD;
/// Data miscompare on VERIFY command.
pub const MISCOMPARE: u8 = 0xE;
/// Command completed (reserved in current spec).
pub const COMPLETED: u8 = 0xF;

// ---------------------------------------------------------------------------
// Sense data response codes
// ---------------------------------------------------------------------------

/// Fixed-format sense data (current errors).
pub const SENSE_RESPONSE_CURRENT_FIXED: u8 = 0x70;
/// Fixed-format sense data (deferred errors).
pub const SENSE_RESPONSE_DEFERRED_FIXED: u8 = 0x71;
/// Descriptor-format sense data (current errors).
pub const SENSE_RESPONSE_CURRENT_DESC: u8 = 0x72;
/// Descriptor-format sense data (deferred errors).
pub const SENSE_RESPONSE_DEFERRED_DESC: u8 = 0x73;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sense_keys_distinct() {
        let keys = [
            NO_SENSE, RECOVERED_ERROR, NOT_READY,
            MEDIUM_ERROR, HARDWARE_ERROR, ILLEGAL_REQUEST,
            UNIT_ATTENTION, DATA_PROTECT, BLANK_CHECK,
            VENDOR_SPECIFIC, COPY_ABORTED, ABORTED_COMMAND,
            VOLUME_OVERFLOW, MISCOMPARE, COMPLETED,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_sense_keys_fit_4_bits() {
        let keys = [
            NO_SENSE, RECOVERED_ERROR, NOT_READY,
            MEDIUM_ERROR, HARDWARE_ERROR, ILLEGAL_REQUEST,
            UNIT_ATTENTION, DATA_PROTECT, BLANK_CHECK,
            VENDOR_SPECIFIC, COPY_ABORTED, ABORTED_COMMAND,
            VOLUME_OVERFLOW, MISCOMPARE, COMPLETED,
        ];
        for &k in &keys {
            assert!(k <= 0x0F, "sense key 0x{:X} doesn't fit in 4 bits", k);
        }
    }

    #[test]
    fn test_no_sense_is_zero() {
        assert_eq!(NO_SENSE, 0);
    }

    #[test]
    fn test_response_codes_distinct() {
        let codes = [
            SENSE_RESPONSE_CURRENT_FIXED,
            SENSE_RESPONSE_DEFERRED_FIXED,
            SENSE_RESPONSE_CURRENT_DESC,
            SENSE_RESPONSE_DEFERRED_DESC,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_response_codes_range() {
        assert_eq!(SENSE_RESPONSE_CURRENT_FIXED, 0x70);
        assert_eq!(SENSE_RESPONSE_DEFERRED_DESC, 0x73);
    }
}
