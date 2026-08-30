//! `<linux/vm_sockets_diag.h>` — Additional vsock diagnostics constants.
//!
//! Supplementary vsock diagnostics constants covering socket state,
//! diagnostic info, and shutdown flags.

// ---------------------------------------------------------------------------
// vsock socket states (for diagnostics)
// ---------------------------------------------------------------------------

/// Free.
pub const SS_FREE: u32 = 0;
/// Unconnected.
pub const SS_UNCONNECTED: u32 = 1;
/// Connecting.
pub const SS_CONNECTING: u32 = 2;
/// Connected.
pub const SS_CONNECTED: u32 = 3;
/// Disconnecting.
pub const SS_DISCONNECTING: u32 = 4;

// ---------------------------------------------------------------------------
// vsock shutdown flags
// ---------------------------------------------------------------------------

/// Shutdown receive.
pub const SHUTDOWN_MASK_RCV: u32 = 1;
/// Shutdown send.
pub const SHUTDOWN_MASK_SND: u32 = 2;
/// Shutdown both.
pub const SHUTDOWN_MASK_BOTH: u32 = 3;

// ---------------------------------------------------------------------------
// vsock diagnostic attributes
// ---------------------------------------------------------------------------

/// Unspec.
pub const SK_DIAG_VSOCK_ATTR_UNSPEC: u32 = 0;
/// Source CID.
pub const SK_DIAG_VSOCK_ATTR_SRC_CID: u32 = 1;
/// Source port.
pub const SK_DIAG_VSOCK_ATTR_SRC_PORT: u32 = 2;
/// Destination CID.
pub const SK_DIAG_VSOCK_ATTR_DST_CID: u32 = 3;
/// Destination port.
pub const SK_DIAG_VSOCK_ATTR_DST_PORT: u32 = 4;

// ---------------------------------------------------------------------------
// vsock diagnostic info flags
// ---------------------------------------------------------------------------

/// Socket shutdown.
pub const SK_DIAG_VSOCK_F_SHUTDOWN: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            SS_FREE,
            SS_UNCONNECTED,
            SS_CONNECTING,
            SS_CONNECTED,
            SS_DISCONNECTING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_shutdown_masks() {
        assert_eq!(SHUTDOWN_MASK_BOTH, SHUTDOWN_MASK_RCV | SHUTDOWN_MASK_SND);
    }

    #[test]
    fn test_diag_attrs_distinct() {
        let attrs = [
            SK_DIAG_VSOCK_ATTR_UNSPEC,
            SK_DIAG_VSOCK_ATTR_SRC_CID,
            SK_DIAG_VSOCK_ATTR_SRC_PORT,
            SK_DIAG_VSOCK_ATTR_DST_CID,
            SK_DIAG_VSOCK_ATTR_DST_PORT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_diag_flag() {
        assert!(SK_DIAG_VSOCK_F_SHUTDOWN.is_power_of_two());
    }
}
