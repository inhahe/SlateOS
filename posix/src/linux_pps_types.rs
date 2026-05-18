//! `<linux/pps.h>` — PPS (Pulse Per Second) constants.
//!
//! PPS provides high-precision time synchronization using
//! external pulse signals. These constants define capture
//! modes, IOCTL commands, and version information.

// ---------------------------------------------------------------------------
// PPS API version
// ---------------------------------------------------------------------------

/// PPS API version.
pub const PPS_API_VERS: u32 = 1;

// ---------------------------------------------------------------------------
// Capture modes (PPS_CAPTURE*)
// ---------------------------------------------------------------------------

/// Capture assert edge.
pub const PPS_CAPTUREASSERT: u32 = 0x01;
/// Capture clear edge.
pub const PPS_CAPTURECLEAR: u32 = 0x02;
/// Capture both edges.
pub const PPS_CAPTUREBOTH: u32 = PPS_CAPTUREASSERT | PPS_CAPTURECLEAR;
/// Offset assert.
pub const PPS_OFFSETASSERT: u32 = 0x10;
/// Offset clear.
pub const PPS_OFFSETCLEAR: u32 = 0x20;
/// Echo assert.
pub const PPS_ECHOASSERT: u32 = 0x40;
/// Echo clear.
pub const PPS_ECHOCLEAR: u32 = 0x80;
/// Kernel consumer.
pub const PPS_CANWAIT: u32 = 0x100;
/// Can wait.
pub const PPS_CANPOLL: u32 = 0x200;

// ---------------------------------------------------------------------------
// Kernel consumer
// ---------------------------------------------------------------------------

/// Kernel consumer (time discipline).
pub const PPS_KC_HARDPPS: u32 = 0;
/// Edge assert for hardpps.
pub const PPS_KC_HARDPPS_ASSERT: u32 = 0x01;
/// Edge clear for hardpps.
pub const PPS_KC_HARDPPS_CLEAR: u32 = 0x02;

// ---------------------------------------------------------------------------
// IOCTL commands
// ---------------------------------------------------------------------------

/// Get parameters.
pub const PPS_GETPARAMS: u32 = 0x800870A1;
/// Set parameters.
pub const PPS_SETPARAMS: u32 = 0x400870A2;
/// Get capabilities.
pub const PPS_GETCAP: u32 = 0x800470A3;
/// Fetch event.
pub const PPS_FETCH: u32 = 0xC01070A4;
/// Bind kernel consumer.
pub const PPS_KC_BIND: u32 = 0x400470A5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_version() {
        assert_eq!(PPS_API_VERS, 1);
    }

    #[test]
    fn test_capture_modes() {
        assert_eq!(PPS_CAPTUREASSERT, 0x01);
        assert_eq!(PPS_CAPTURECLEAR, 0x02);
        assert_eq!(PPS_CAPTUREBOTH, 0x03);
    }

    #[test]
    fn test_capture_both() {
        assert_eq!(PPS_CAPTUREBOTH, PPS_CAPTUREASSERT | PPS_CAPTURECLEAR);
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            PPS_CAPTUREASSERT, PPS_CAPTURECLEAR,
            PPS_OFFSETASSERT, PPS_OFFSETCLEAR,
            PPS_ECHOASSERT, PPS_ECHOCLEAR,
            PPS_CANWAIT, PPS_CANPOLL,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            PPS_GETPARAMS, PPS_SETPARAMS, PPS_GETCAP,
            PPS_FETCH, PPS_KC_BIND,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_kc_hardpps() {
        assert_eq!(PPS_KC_HARDPPS, 0);
        assert_ne!(PPS_KC_HARDPPS_ASSERT, PPS_KC_HARDPPS_CLEAR);
    }
}
