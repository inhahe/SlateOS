//! `<linux/can/j1939.h>` — Additional J1939 (CAN SAE J1939) constants.
//!
//! Supplementary J1939 constants covering socket options,
//! address types, and PGN (Parameter Group Number) values.

// ---------------------------------------------------------------------------
// J1939 socket options
// ---------------------------------------------------------------------------

/// Set/get J1939 source address filter.
pub const SO_J1939_FILTER: u32 = 1;
/// Set/get J1939 promiscuous mode.
pub const SO_J1939_PROMISC: u32 = 2;
/// Set/get send priority.
pub const SO_J1939_SEND_PRIO: u32 = 3;
/// Errqueue.
pub const SO_J1939_ERRQUEUE: u32 = 4;

// ---------------------------------------------------------------------------
// J1939 address constants
// ---------------------------------------------------------------------------

/// No address assigned.
pub const J1939_NO_ADDR: u8 = 0xFE;
/// Idle address.
pub const J1939_IDLE_ADDR: u8 = 0xFE;
/// Global/broadcast address.
pub const J1939_MAX_UNICAST_ADDR: u8 = 0xFD;

// ---------------------------------------------------------------------------
// J1939 PGN special values
// ---------------------------------------------------------------------------

/// No PGN.
pub const J1939_NO_PGN: u32 = 0x40000;
/// PGN request.
pub const J1939_PGN_REQUEST: u32 = 0x0EA00;
/// PGN address claimed.
pub const J1939_PGN_ADDRESS_CLAIMED: u32 = 0x0EE00;
/// PGN commanded address.
pub const J1939_PGN_ADDRESS_COMMANDED: u32 = 0x0FED8;
/// Maximum PGN value.
pub const J1939_PGN_MAX: u32 = 0x3FFFF;
/// PDU format threshold (PDU1 vs PDU2).
pub const J1939_PGN_PDU1_MAX: u32 = 0x3FF00;

// ---------------------------------------------------------------------------
// J1939 name fields (64-bit NAME in network byte order)
// ---------------------------------------------------------------------------

/// Arbitrary address capable bit.
pub const J1939_NAME_AAC: u64 = 1 << 63;

// ---------------------------------------------------------------------------
// J1939 message flags (in msg_flags)
// ---------------------------------------------------------------------------

/// Echoed message.
pub const J1939_MSG_ECHO: u32 = 0x01;
/// Connection management flag.
pub const J1939_MSG_CONN_MGMT: u32 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sock_opts_distinct() {
        let opts = [
            SO_J1939_FILTER,
            SO_J1939_PROMISC,
            SO_J1939_SEND_PRIO,
            SO_J1939_ERRQUEUE,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_addresses() {
        assert_eq!(J1939_NO_ADDR, 0xFE);
        assert!(J1939_MAX_UNICAST_ADDR < J1939_NO_ADDR);
    }

    #[test]
    fn test_pgn_values_distinct() {
        let pgns = [
            J1939_NO_PGN,
            J1939_PGN_REQUEST,
            J1939_PGN_ADDRESS_CLAIMED,
            J1939_PGN_ADDRESS_COMMANDED,
        ];
        for i in 0..pgns.len() {
            for j in (i + 1)..pgns.len() {
                assert_ne!(pgns[i], pgns[j]);
            }
        }
    }

    #[test]
    fn test_pgn_max() {
        assert!(J1939_PGN_REQUEST <= J1939_PGN_MAX);
        assert!(J1939_PGN_ADDRESS_CLAIMED <= J1939_PGN_MAX);
        assert!(J1939_PGN_ADDRESS_COMMANDED <= J1939_PGN_MAX);
    }

    #[test]
    fn test_name_aac_bit() {
        assert!(J1939_NAME_AAC.is_power_of_two());
        assert_eq!(J1939_NAME_AAC, 1u64 << 63);
    }

    #[test]
    fn test_msg_flags_no_overlap() {
        assert_eq!(J1939_MSG_ECHO & J1939_MSG_CONN_MGMT, 0);
    }
}
