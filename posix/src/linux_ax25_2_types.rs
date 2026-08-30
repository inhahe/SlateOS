//! `<linux/ax25.h>` — Additional AX.25 constants.
//!
//! Supplementary AX.25 amateur radio networking constants covering
//! socket options, protocol IDs, and parameter types.

// ---------------------------------------------------------------------------
// AX.25 socket options
// ---------------------------------------------------------------------------

/// Window size.
pub const AX25_WINDOW: u32 = 1;
/// Timer T1.
pub const AX25_T1: u32 = 2;
/// Maximum retries N2.
pub const AX25_N2: u32 = 3;
/// Timer T3.
pub const AX25_T3: u32 = 4;
/// Timer T2.
pub const AX25_T2: u32 = 5;
/// Backoff.
pub const AX25_BACKOFF: u32 = 6;
/// Extended sequence numbers.
pub const AX25_EXTSEQ: u32 = 7;
/// PID included.
pub const AX25_PIDINCL: u32 = 8;
/// Idle timer.
pub const AX25_IDLE: u32 = 9;
/// Paclen.
pub const AX25_PACLEN: u32 = 10;
/// IAMDIGI.
pub const AX25_IAMDIGI: u32 = 12;

// ---------------------------------------------------------------------------
// AX.25 protocol IDs
// ---------------------------------------------------------------------------

/// AX.25 layer 3.
pub const AX25_P_ROSE: u32 = 0x01;
/// Compressed TCP/IP.
pub const AX25_P_VJCOMP: u32 = 0x06;
/// Uncompressed TCP/IP.
pub const AX25_P_VJUNCOMP: u32 = 0x07;
/// Segmentation fragment.
pub const AX25_P_SEGMENT: u32 = 0x08;
/// TEXNET datagram.
pub const AX25_P_TEXNET: u32 = 0xC3;
/// Link quality protocol.
pub const AX25_P_LQ: u32 = 0xC4;
/// Appletalk.
pub const AX25_P_ATALK: u32 = 0xCA;
/// Appletalk ARP.
pub const AX25_P_ATALK_ARP: u32 = 0xCB;
/// IP.
pub const AX25_P_IP: u32 = 0xCC;
/// ARP.
pub const AX25_P_ARP: u32 = 0xCD;
/// FlexNet.
pub const AX25_P_FLEXNET: u32 = 0xCE;
/// NET/ROM.
pub const AX25_P_NETROM: u32 = 0xCF;
/// No layer 3.
pub const AX25_P_TEXT: u32 = 0xF0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sockopts_distinct() {
        let opts = [
            AX25_WINDOW,
            AX25_T1,
            AX25_N2,
            AX25_T3,
            AX25_T2,
            AX25_BACKOFF,
            AX25_EXTSEQ,
            AX25_PIDINCL,
            AX25_IDLE,
            AX25_PACLEN,
            AX25_IAMDIGI,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_protocol_ids_distinct() {
        let pids = [
            AX25_P_ROSE,
            AX25_P_VJCOMP,
            AX25_P_VJUNCOMP,
            AX25_P_SEGMENT,
            AX25_P_TEXNET,
            AX25_P_LQ,
            AX25_P_ATALK,
            AX25_P_ATALK_ARP,
            AX25_P_IP,
            AX25_P_ARP,
            AX25_P_FLEXNET,
            AX25_P_NETROM,
            AX25_P_TEXT,
        ];
        for i in 0..pids.len() {
            for j in (i + 1)..pids.len() {
                assert_ne!(pids[i], pids[j]);
            }
        }
    }
}
