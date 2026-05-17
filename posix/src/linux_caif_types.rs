//! `<linux/caif/caif_socket.h>` — CAIF (Communication CPU to Application CPU Interface) constants.
//!
//! CAIF is the protocol used between the application CPU and the
//! cellular modem CPU in ST-Ericsson (now part of Intel/MediaTek)
//! mobile platforms. It multiplexes data, control, and debug channels
//! over a shared physical link (SPI, shared memory, HSI). The Linux
//! CAIF stack provides AF_CAIF sockets for userspace access to modem
//! services. Used on some Android devices with ST-Ericsson modems.

// ---------------------------------------------------------------------------
// CAIF protocol types (used in socket creation)
// ---------------------------------------------------------------------------

/// AT command channel (modem control).
pub const CAIFPROTO_AT: u32 = 0;
/// Datagram channel (IP packets).
pub const CAIFPROTO_DATAGRAM: u32 = 1;
/// Datagram loop (loopback for testing).
pub const CAIFPROTO_DATAGRAM_LOOP: u32 = 2;
/// Utility channel.
pub const CAIFPROTO_UTIL: u32 = 3;
/// RFM (Remote File Manager) channel.
pub const CAIFPROTO_RFM: u32 = 4;
/// Debug/trace channel.
pub const CAIFPROTO_DEBUG: u32 = 5;

// ---------------------------------------------------------------------------
// CAIF address family
// ---------------------------------------------------------------------------

/// CAIF address family number.
pub const AF_CAIF: u32 = 37;

// ---------------------------------------------------------------------------
// CAIF link layer types
// ---------------------------------------------------------------------------

/// Serial (UART) link layer.
pub const CAIF_LINK_SERIAL: u32 = 1;
/// SPI link layer.
pub const CAIF_LINK_SPI: u32 = 2;
/// Shared memory link layer.
pub const CAIF_LINK_SHM: u32 = 3;
/// HSI (High Speed Synchronous Interface) link layer.
pub const CAIF_LINK_HSI: u32 = 4;
/// USB link layer.
pub const CAIF_LINK_USB: u32 = 5;
/// Loopback link layer.
pub const CAIF_LINK_LOOP: u32 = 6;

// ---------------------------------------------------------------------------
// CAIF connection types (for datagram)
// ---------------------------------------------------------------------------

/// IPv4 connection.
pub const CAIF_CT_IPV4: u32 = 1;
/// IPv6 connection.
pub const CAIF_CT_IPV6: u32 = 2;

// ---------------------------------------------------------------------------
// CAIF socket options
// ---------------------------------------------------------------------------

/// Link select (choose which physical link).
pub const CAIF_SO_LINK_SELECT: u32 = 127;
/// Request connection ID.
pub const CAIF_SO_REQ_PARAM: u32 = 128;

// ---------------------------------------------------------------------------
// CAIF channel priorities
// ---------------------------------------------------------------------------

/// Low priority channel.
pub const CAIF_PRIO_LOW: u32 = 0;
/// Normal priority channel.
pub const CAIF_PRIO_NORMAL: u32 = 1;
/// High priority channel.
pub const CAIF_PRIO_HIGH: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_types_distinct() {
        let protos = [
            CAIFPROTO_AT, CAIFPROTO_DATAGRAM,
            CAIFPROTO_DATAGRAM_LOOP, CAIFPROTO_UTIL,
            CAIFPROTO_RFM, CAIFPROTO_DEBUG,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_link_types_distinct() {
        let links = [
            CAIF_LINK_SERIAL, CAIF_LINK_SPI, CAIF_LINK_SHM,
            CAIF_LINK_HSI, CAIF_LINK_USB, CAIF_LINK_LOOP,
        ];
        for i in 0..links.len() {
            for j in (i + 1)..links.len() {
                assert_ne!(links[i], links[j]);
            }
        }
    }

    #[test]
    fn test_af_caif() {
        assert_eq!(AF_CAIF, 37);
    }

    #[test]
    fn test_connection_types_distinct() {
        assert_ne!(CAIF_CT_IPV4, CAIF_CT_IPV6);
    }

    #[test]
    fn test_priorities_ordered() {
        assert!(CAIF_PRIO_LOW < CAIF_PRIO_NORMAL);
        assert!(CAIF_PRIO_NORMAL < CAIF_PRIO_HIGH);
    }

    #[test]
    fn test_socket_options_distinct() {
        assert_ne!(CAIF_SO_LINK_SELECT, CAIF_SO_REQ_PARAM);
    }
}
