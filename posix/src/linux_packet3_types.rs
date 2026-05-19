//! `<linux/if_packet.h>` — Additional packet socket constants.
//!
//! Supplementary AF_PACKET constants covering TPACKET versions,
//! ring status flags, and fanout modes.

// ---------------------------------------------------------------------------
// TPACKET versions
// ---------------------------------------------------------------------------

/// TPACKET v1.
pub const TPACKET_V1: u32 = 0;
/// TPACKET v2.
pub const TPACKET_V2: u32 = 1;
/// TPACKET v3.
pub const TPACKET_V3: u32 = 2;

// ---------------------------------------------------------------------------
// TPACKET status flags (for tp_status)
// ---------------------------------------------------------------------------

/// Kernel owns the frame.
pub const TP_STATUS_KERNEL: u32 = 0;
/// User space owns the frame.
pub const TP_STATUS_USER: u32 = 1 << 0;
/// Frame copy (not zero-copy).
pub const TP_STATUS_COPY: u32 = 1 << 1;
/// Frame was lost (overflow).
pub const TP_STATUS_LOSING: u32 = 1 << 2;
/// CSUMNOTREADY — checksum not computed yet.
pub const TP_STATUS_CSUMNOTREADY: u32 = 1 << 3;
/// VLAN valid.
pub const TP_STATUS_VLAN_VALID: u32 = 1 << 4;
/// Block kernel.
pub const TP_STATUS_BLK_TMO: u32 = 1 << 5;
/// VLAN tag present in tpid.
pub const TP_STATUS_VLAN_TPID_VALID: u32 = 1 << 6;
/// Checksum valid.
pub const TP_STATUS_CSUM_VALID: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// TX status flags
// ---------------------------------------------------------------------------

/// Available for TX.
pub const TP_STATUS_AVAILABLE: u32 = 0;
/// Send request.
pub const TP_STATUS_SEND_REQUEST: u32 = 1 << 0;
/// Sending in progress.
pub const TP_STATUS_SENDING: u32 = 1 << 1;
/// Wrong format.
pub const TP_STATUS_WRONG_FORMAT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Packet fanout modes
// ---------------------------------------------------------------------------

/// Hash-based fanout.
pub const PACKET_FANOUT_HASH: u32 = 0;
/// Load-balanced fanout.
pub const PACKET_FANOUT_LB: u32 = 1;
/// CPU-based fanout.
pub const PACKET_FANOUT_CPU: u32 = 2;
/// Rollover fanout.
pub const PACKET_FANOUT_ROLLOVER: u32 = 3;
/// Random fanout.
pub const PACKET_FANOUT_RND: u32 = 4;
/// Queue mapping fanout.
pub const PACKET_FANOUT_QM: u32 = 5;
/// CBPF-based fanout.
pub const PACKET_FANOUT_CBPF: u32 = 6;
/// EBPF-based fanout.
pub const PACKET_FANOUT_EBPF: u32 = 7;

// ---------------------------------------------------------------------------
// Fanout flags
// ---------------------------------------------------------------------------

/// Fanout rollover flag.
pub const PACKET_FANOUT_FLAG_ROLLOVER: u32 = 0x1000;
/// Fanout uniqueid flag.
pub const PACKET_FANOUT_FLAG_UNIQUEID: u32 = 0x2000;
/// Fanout ignore outgoing flag.
pub const PACKET_FANOUT_FLAG_IGNORE_OUTGOING: u32 = 0x4000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tpacket_versions_distinct() {
        let vers = [TPACKET_V1, TPACKET_V2, TPACKET_V3];
        for i in 0..vers.len() {
            for j in (i + 1)..vers.len() {
                assert_ne!(vers[i], vers[j]);
            }
        }
    }

    #[test]
    fn test_rx_status_flags_no_overlap() {
        let flags = [
            TP_STATUS_USER, TP_STATUS_COPY, TP_STATUS_LOSING,
            TP_STATUS_CSUMNOTREADY, TP_STATUS_VLAN_VALID,
            TP_STATUS_BLK_TMO, TP_STATUS_VLAN_TPID_VALID,
            TP_STATUS_CSUM_VALID,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tx_status_flags_no_overlap() {
        let flags = [
            TP_STATUS_SEND_REQUEST, TP_STATUS_SENDING,
            TP_STATUS_WRONG_FORMAT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fanout_modes_distinct() {
        let modes = [
            PACKET_FANOUT_HASH, PACKET_FANOUT_LB,
            PACKET_FANOUT_CPU, PACKET_FANOUT_ROLLOVER,
            PACKET_FANOUT_RND, PACKET_FANOUT_QM,
            PACKET_FANOUT_CBPF, PACKET_FANOUT_EBPF,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_fanout_flags_no_overlap() {
        let flags = [
            PACKET_FANOUT_FLAG_ROLLOVER,
            PACKET_FANOUT_FLAG_UNIQUEID,
            PACKET_FANOUT_FLAG_IGNORE_OUTGOING,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
