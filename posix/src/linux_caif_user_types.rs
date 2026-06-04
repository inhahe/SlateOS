//! `<linux/caif/caif_socket.h>` — CAIF (Communication CPU-to-Application
//! CPU InterFace) socket user surface.
//!
//! CAIF is the ST-Ericsson modem-control protocol that survived into
//! mainline Linux as a socket family. Userspace creates connections
//! over physical links (UART, USB, loopback) and exchanges typed
//! channels (RFM, control, video, debug).

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// Linux address-family number for CAIF.
pub const AF_CAIF: u32 = 37;

/// Protocol family alias — identical to AF_CAIF.
pub const PF_CAIF: u32 = AF_CAIF;

// ---------------------------------------------------------------------------
// Socket type / protocol selectors (`enum caif_protocol_type`)
// ---------------------------------------------------------------------------

pub const CAIFPROTO_AT: u32 = 0;
pub const CAIFPROTO_DATAGRAM: u32 = 1;
pub const CAIFPROTO_DATAGRAM_LOOP: u32 = 2;
pub const CAIFPROTO_UTIL: u32 = 3;
pub const CAIFPROTO_RFM: u32 = 4;
pub const CAIFPROTO_DEBUG: u32 = 5;

// ---------------------------------------------------------------------------
// AT connection types (`enum caif_at_type`)
// ---------------------------------------------------------------------------

pub const CAIF_ATTYPE_PLAIN: u32 = 2;

// ---------------------------------------------------------------------------
// Debug-channel sub-types (`enum caif_debug_type`)
// ---------------------------------------------------------------------------

pub const CAIF_DEBUG_TRACE_INTERACTIVE: u32 = 0;
pub const CAIF_DEBUG_TRACE: u32 = 1;
pub const CAIF_DEBUG_INTERACTIVE: u32 = 2;

// ---------------------------------------------------------------------------
// Debug-service types (`enum caif_debug_service`)
// ---------------------------------------------------------------------------

pub const CAIF_RADIO_DEBUG_SERVICE: u32 = 1;
pub const CAIF_APP_DEBUG_SERVICE: u32 = 2;

// ---------------------------------------------------------------------------
// SOL_CAIF socket options
// ---------------------------------------------------------------------------

/// Socket-options level.
pub const SOL_CAIF: u32 = 278;

/// Send a link-select hint to the connection manager.
pub const CAIFSO_LINK_SELECT: u32 = 127;

/// Request priority for the channel (priority class 1..7).
pub const CAIFSO_REQ_PARAM: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_family_constants() {
        assert_eq!(AF_CAIF, 37);
        assert_eq!(PF_CAIF, AF_CAIF);
    }

    #[test]
    fn test_protocol_types_dense_0_to_5() {
        let p = [
            CAIFPROTO_AT,
            CAIFPROTO_DATAGRAM,
            CAIFPROTO_DATAGRAM_LOOP,
            CAIFPROTO_UTIL,
            CAIFPROTO_RFM,
            CAIFPROTO_DEBUG,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_debug_subtypes_dense_0_to_2() {
        let d = [
            CAIF_DEBUG_TRACE_INTERACTIVE,
            CAIF_DEBUG_TRACE,
            CAIF_DEBUG_INTERACTIVE,
        ];
        for (i, &v) in d.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_debug_services_distinct() {
        assert_ne!(CAIF_RADIO_DEBUG_SERVICE, CAIF_APP_DEBUG_SERVICE);
        assert_eq!(CAIF_RADIO_DEBUG_SERVICE, 1);
        assert_eq!(CAIF_APP_DEBUG_SERVICE, 2);
    }

    #[test]
    fn test_at_plain_value() {
        assert_eq!(CAIF_ATTYPE_PLAIN, 2);
    }

    #[test]
    fn test_socket_option_namespace() {
        assert_eq!(SOL_CAIF, 278);
        // The two SO options are adjacent.
        assert_eq!(CAIFSO_REQ_PARAM - CAIFSO_LINK_SELECT, 1);
        assert_eq!(CAIFSO_LINK_SELECT, 127);
    }
}
