//! `<linux/x25.h>` — Additional X.25 constants.
//!
//! Supplementary X.25 networking constants covering socket ioctls,
//! facility types, and call condition codes.

// ---------------------------------------------------------------------------
// X.25 ioctl commands
// ---------------------------------------------------------------------------

/// Subscribe to X.25.
pub const SIOCX25GSUBSCRIP: u32 = 0x8930;
/// Set subscription.
pub const SIOCX25SSUBSCRIP: u32 = 0x8931;
/// Get route.
pub const SIOCX25GFACILITIES: u32 = 0x8932;
/// Set route.
pub const SIOCX25SFACILITIES: u32 = 0x8933;
/// Get call user data.
pub const SIOCX25GCALLUSERDATA: u32 = 0x8934;
/// Set call user data.
pub const SIOCX25SCALLUSERDATA: u32 = 0x8935;
/// Get cause and diagnostic.
pub const SIOCX25GCAUSEDIAG: u32 = 0x8936;
/// Send call user data.
pub const SIOCX25SCUDMATCHLEN: u32 = 0x8937;
/// Get DTE facilities.
pub const SIOCX25GDTEFACILITIES: u32 = 0x8938;
/// Set DTE facilities.
pub const SIOCX25SDTEFACILITIES: u32 = 0x8939;

// ---------------------------------------------------------------------------
// X.25 packet types
// ---------------------------------------------------------------------------

/// Call request.
pub const X25_CALL_REQUEST: u32 = 0x0B;
/// Call accepted.
pub const X25_CALL_ACCEPTED: u32 = 0x0F;
/// Clear request.
pub const X25_CLEAR_REQUEST: u32 = 0x13;
/// Clear confirmation.
pub const X25_CLEAR_CONFIRMATION: u32 = 0x17;
/// Data.
pub const X25_DATA: u32 = 0x00;
/// Interrupt.
pub const X25_INTERRUPT: u32 = 0x23;
/// Interrupt confirmation.
pub const X25_INTERRUPT_CONFIRMATION: u32 = 0x27;
/// Reset request.
pub const X25_RESET_REQUEST: u32 = 0x1B;
/// Reset confirmation.
pub const X25_RESET_CONFIRMATION: u32 = 0x1F;
/// Diagnostic.
pub const X25_DIAGNOSTIC: u32 = 0xF1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            SIOCX25GSUBSCRIP, SIOCX25SSUBSCRIP,
            SIOCX25GFACILITIES, SIOCX25SFACILITIES,
            SIOCX25GCALLUSERDATA, SIOCX25SCALLUSERDATA,
            SIOCX25GCAUSEDIAG, SIOCX25SCUDMATCHLEN,
            SIOCX25GDTEFACILITIES, SIOCX25SDTEFACILITIES,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_packet_types_distinct() {
        let types = [
            X25_CALL_REQUEST, X25_CALL_ACCEPTED,
            X25_CLEAR_REQUEST, X25_CLEAR_CONFIRMATION,
            X25_DATA, X25_INTERRUPT, X25_INTERRUPT_CONFIRMATION,
            X25_RESET_REQUEST, X25_RESET_CONFIRMATION,
            X25_DIAGNOSTIC,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
