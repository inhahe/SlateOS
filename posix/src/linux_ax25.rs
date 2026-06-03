//! `<linux/ax25.h>` — AX.25 amateur radio protocol constants.
//!
//! AX.25 is the data link layer protocol used in amateur (ham)
//! radio packet networking. It provides connectionless (UI frame)
//! and connection-oriented (I frame) communication between amateur
//! radio stations identified by callsigns.

// ---------------------------------------------------------------------------
// Protocol family
// ---------------------------------------------------------------------------

/// AX.25 protocol family.
pub const PF_AX25: u16 = 3;
/// AX.25 address family.
pub const AF_AX25: u16 = 3;

// ---------------------------------------------------------------------------
// Frame types
// ---------------------------------------------------------------------------

/// Information frame (data transfer).
pub const AX25_I_FRAME: u8 = 0x00;
/// Supervisory: Receive Ready.
pub const AX25_RR: u8 = 0x01;
/// Supervisory: Receive Not Ready.
pub const AX25_RNR: u8 = 0x05;
/// Supervisory: Reject.
pub const AX25_REJ: u8 = 0x09;
/// Unnumbered: Set Async Balanced Mode.
pub const AX25_SABM: u8 = 0x2F;
/// Unnumbered: Set Async Balanced Mode Extended.
pub const AX25_SABME: u8 = 0x6F;
/// Unnumbered: Disconnect.
pub const AX25_DISC: u8 = 0x43;
/// Unnumbered: Disconnect Mode.
pub const AX25_DM: u8 = 0x0F;
/// Unnumbered: Unnumbered Acknowledge.
pub const AX25_UA: u8 = 0x63;
/// Unnumbered: Frame Reject.
pub const AX25_FRMR: u8 = 0x87;
/// Unnumbered: Unnumbered Information.
pub const AX25_UI: u8 = 0x03;

// ---------------------------------------------------------------------------
// PID (Protocol Identifier) field values
// ---------------------------------------------------------------------------

/// AX.25 Layer 3 (ISO 8208).
pub const AX25_PID_X25: u8 = 0x01;
/// Compressed TCP/IP.
pub const AX25_PID_CTCP: u8 = 0x06;
/// Uncompressed TCP/IP.
pub const AX25_PID_UTCP: u8 = 0x07;
/// Segmentation fragment.
pub const AX25_PID_SEG: u8 = 0x08;
/// TEXNET datagram.
pub const AX25_PID_TEXNET: u8 = 0xC3;
/// Link Quality Protocol.
pub const AX25_PID_LQP: u8 = 0xC4;
/// Appletalk.
pub const AX25_PID_ATALK: u8 = 0xCA;
/// IP.
pub const AX25_PID_IP: u8 = 0xCC;
/// ARP.
pub const AX25_PID_ARP: u8 = 0xCD;
/// No layer 3 protocol.
pub const AX25_PID_TEXT: u8 = 0xF0;

// ---------------------------------------------------------------------------
// Address lengths
// ---------------------------------------------------------------------------

/// Callsign length (6 characters + SSID byte).
pub const AX25_ADDR_LEN: u8 = 7;
/// Maximum number of digipeaters.
pub const AX25_MAX_DIGIS: u8 = 8;

// ---------------------------------------------------------------------------
// Socket options (SOL_AX25)
// ---------------------------------------------------------------------------

/// AX.25 socket option level.
pub const SOL_AX25: u32 = 257;
/// Window size.
pub const AX25_WINDOW: u32 = 1;
/// T1 timer (retransmit).
pub const AX25_T1: u32 = 2;
/// T2 timer (ack delay).
pub const AX25_T2: u32 = 5;
/// T3 timer (idle poll).
pub const AX25_T3: u32 = 3;
/// N2 retry count.
pub const AX25_N2: u32 = 4;
/// Backoff strategy.
pub const AX25_BACKOFF: u32 = 6;
/// Extended (modulo 128) mode.
pub const AX25_EXTSEQ: u32 = 7;
/// Idle timer.
pub const AX25_IDLE: u32 = 8;
/// Packet length.
pub const AX25_PACLEN: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_family() {
        assert_eq!(PF_AX25, AF_AX25);
    }

    #[test]
    fn test_frame_types_distinct() {
        let frames = [
            AX25_I_FRAME,
            AX25_RR,
            AX25_RNR,
            AX25_REJ,
            AX25_SABM,
            AX25_SABME,
            AX25_DISC,
            AX25_DM,
            AX25_UA,
            AX25_FRMR,
            AX25_UI,
        ];
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert_ne!(frames[i], frames[j]);
            }
        }
    }

    #[test]
    fn test_pid_values_distinct() {
        let pids = [
            AX25_PID_X25,
            AX25_PID_CTCP,
            AX25_PID_UTCP,
            AX25_PID_SEG,
            AX25_PID_TEXNET,
            AX25_PID_LQP,
            AX25_PID_ATALK,
            AX25_PID_IP,
            AX25_PID_ARP,
            AX25_PID_TEXT,
        ];
        for i in 0..pids.len() {
            for j in (i + 1)..pids.len() {
                assert_ne!(pids[i], pids[j]);
            }
        }
    }

    #[test]
    fn test_addr_len() {
        assert_eq!(AX25_ADDR_LEN, 7);
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            AX25_WINDOW,
            AX25_T1,
            AX25_T2,
            AX25_T3,
            AX25_N2,
            AX25_BACKOFF,
            AX25_EXTSEQ,
            AX25_IDLE,
            AX25_PACLEN,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
