//! `<linux/caif/caif_socket.h>` — Additional CAIF constants.
//!
//! Supplementary CAIF (Communication CPU to Application CPU Interface)
//! constants covering channel types, connection types, and link operations.

// ---------------------------------------------------------------------------
// CAIF channel types
// ---------------------------------------------------------------------------

/// AT command channel.
pub const CAIF_CHTYPE_AT: u32 = 0;
/// Data/GPRS channel.
pub const CAIF_CHTYPE_DATA: u32 = 1;
/// Video channel.
pub const CAIF_CHTYPE_VIDEO: u32 = 2;
/// Debug/trace channel.
pub const CAIF_CHTYPE_DEBUG: u32 = 3;
/// Utility channel.
pub const CAIF_CHTYPE_UTIL: u32 = 4;
/// RFM (Remote File Manager) channel.
pub const CAIF_CHTYPE_RFM: u32 = 5;

// ---------------------------------------------------------------------------
// CAIF connection types (for socket)
// ---------------------------------------------------------------------------

/// AT type.
pub const CAIF_AT_TYPE: u32 = 2;
/// Data GPRS type.
pub const CAIF_DATAGRAM_TYPE: u32 = 3;
/// Debug link type.
pub const CAIF_DEBUG_TYPE: u32 = 6;
/// Utility type.
pub const CAIF_UTIL_TYPE: u32 = 7;
/// RFM type.
pub const CAIF_RFM_TYPE: u32 = 8;

// ---------------------------------------------------------------------------
// CAIF link selection
// ---------------------------------------------------------------------------

/// High bandwidth link.
pub const CAIF_LINK_HIGH_BANDW: u32 = 0;
/// Low latency link.
pub const CAIF_LINK_LOW_LATENCY: u32 = 1;

// ---------------------------------------------------------------------------
// CAIF protocol states
// ---------------------------------------------------------------------------

/// Disconnected.
pub const CAIF_DISCONNECTED: u32 = 0;
/// Connecting.
pub const CAIF_CONNECTING: u32 = 1;
/// Connected.
pub const CAIF_CONNECTED: u32 = 2;
/// Disconnecting.
pub const CAIF_DISCONNECTING: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ch_types_distinct() {
        let types = [
            CAIF_CHTYPE_AT, CAIF_CHTYPE_DATA, CAIF_CHTYPE_VIDEO,
            CAIF_CHTYPE_DEBUG, CAIF_CHTYPE_UTIL, CAIF_CHTYPE_RFM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_conn_types_distinct() {
        let types = [
            CAIF_AT_TYPE, CAIF_DATAGRAM_TYPE, CAIF_DEBUG_TYPE,
            CAIF_UTIL_TYPE, CAIF_RFM_TYPE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_link_selection_distinct() {
        assert_ne!(CAIF_LINK_HIGH_BANDW, CAIF_LINK_LOW_LATENCY);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            CAIF_DISCONNECTED, CAIF_CONNECTING,
            CAIF_CONNECTED, CAIF_DISCONNECTING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
