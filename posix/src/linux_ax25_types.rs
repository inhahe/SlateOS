//! `<linux/ax25.h>` — AX.25 amateur radio constants.
//!
//! AX.25 is a data link layer protocol for amateur (ham)
//! radio networks.  These constants define socket options,
//! address parameters, frame types, and IOCTL commands.

// ---------------------------------------------------------------------------
// AX.25 protocol family
// ---------------------------------------------------------------------------

/// AX.25 address family.
pub const AF_AX25: u32 = 3;
/// AX.25 protocol family.
pub const PF_AX25: u32 = AF_AX25;

// ---------------------------------------------------------------------------
// AX.25 socket options (SOL_AX25)
// ---------------------------------------------------------------------------

/// Window size.
pub const AX25_WINDOW: u32 = 1;
/// T1 timer (frame acknowledgment).
pub const AX25_T1: u32 = 2;
/// N2 retry count.
pub const AX25_N2: u32 = 3;
/// T3 timer (link activity).
pub const AX25_T3: u32 = 4;
/// T2 timer (response delay).
pub const AX25_T2: u32 = 5;
/// Backoff type.
pub const AX25_BACKOFF: u32 = 6;
/// Extended window (modulo 128).
pub const AX25_EXTSEQ: u32 = 7;
/// PID filter.
pub const AX25_PIDINCL: u32 = 8;
/// Idle timer.
pub const AX25_IDLE: u32 = 9;
/// Maximum packet length.
pub const AX25_PACLEN: u32 = 10;
/// Idle mode.
pub const AX25_IAMDIGI: u32 = 12;

// ---------------------------------------------------------------------------
// AX.25 frame types / protocol IDs (PID)
// ---------------------------------------------------------------------------

/// IP over AX.25.
pub const AX25_PID_IP: u8 = 0xCC;
/// ARP over AX.25.
pub const AX25_PID_ARP: u8 = 0xCD;
/// NET/ROM.
pub const AX25_PID_NETROM: u8 = 0xCF;
/// No layer 3 protocol.
pub const AX25_PID_NO_L3: u8 = 0xF0;
/// Segment fragment.
pub const AX25_PID_SEGMENT: u8 = 0x08;
/// ROSE (X.25 over AX.25).
pub const AX25_PID_ROSE: u8 = 0x01;
/// FlexNet.
pub const AX25_PID_FLEXNET: u8 = 0x06;
/// Text (human-readable).
pub const AX25_PID_TEXT: u8 = 0xF0;

// ---------------------------------------------------------------------------
// AX.25 address length
// ---------------------------------------------------------------------------

/// AX.25 address length (callsign + SSID, 7 bytes).
pub const AX25_ADDR_LEN: u32 = 7;
/// Maximum digipeater count.
pub const AX25_MAX_DIGIS: u32 = 8;

// ---------------------------------------------------------------------------
// AX.25 IOCTL commands
// ---------------------------------------------------------------------------

/// Set AX.25 call.
pub const SIOCAX25ADDUID: u32 = 0x8951;
/// Delete AX.25 call.
pub const SIOCAX25DELUID: u32 = 0x8952;
/// Get AX.25 info.
pub const SIOCAX25GETUID: u32 = 0x8953;
/// Get AX.25 parameters.
pub const SIOCAX25GETPARMS: u32 = 0x8954;
/// Set AX.25 parameters.
pub const SIOCAX25SETPARMS: u32 = 0x8955;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_ax25() {
        assert_eq!(AF_AX25, 3);
        assert_eq!(PF_AX25, AF_AX25);
    }

    #[test]
    fn test_sockopts_distinct() {
        let opts = [
            AX25_WINDOW, AX25_T1, AX25_N2, AX25_T3, AX25_T2,
            AX25_BACKOFF, AX25_EXTSEQ, AX25_PIDINCL, AX25_IDLE,
            AX25_PACLEN, AX25_IAMDIGI,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_pids_values() {
        assert_eq!(AX25_PID_IP, 0xCC);
        assert_eq!(AX25_PID_ARP, 0xCD);
        assert_eq!(AX25_PID_NETROM, 0xCF);
    }

    #[test]
    fn test_addr_len() {
        assert_eq!(AX25_ADDR_LEN, 7);
    }

    #[test]
    fn test_max_digis() {
        assert_eq!(AX25_MAX_DIGIS, 8);
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [
            SIOCAX25ADDUID, SIOCAX25DELUID, SIOCAX25GETUID,
            SIOCAX25GETPARMS, SIOCAX25SETPARMS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_window_is_one() {
        assert_eq!(AX25_WINDOW, 1);
    }
}
