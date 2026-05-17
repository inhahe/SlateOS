//! `<linux/packet_diag.h>` — Packet (AF_PACKET) socket diagnostics constants.
//!
//! The packet socket diagnostics interface allows `ss` and monitoring
//! tools to query AF_PACKET raw socket state via netlink. AF_PACKET
//! sockets are used by tcpdump, Wireshark, DHCP clients, and network
//! testing tools for direct L2 access. The diag interface reports
//! protocol binding, interface, fanout group membership, ring buffer
//! configuration, and BPF filter info.

// ---------------------------------------------------------------------------
// Packet diag show flags (PACKET_SHOW_*)
// ---------------------------------------------------------------------------

/// Show packet socket info.
pub const PACKET_SHOW_INFO: u32 = 1 << 0;
/// Show multicast list.
pub const PACKET_SHOW_MCLIST: u32 = 1 << 1;
/// Show ring buffer configuration.
pub const PACKET_SHOW_RING_CFG: u32 = 1 << 2;
/// Show fanout info.
pub const PACKET_SHOW_FANOUT: u32 = 1 << 3;
/// Show memory info.
pub const PACKET_SHOW_MEMINFO: u32 = 1 << 4;
/// Show attached BPF filter.
pub const PACKET_SHOW_FILTER: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Packet diag response attributes (PACKET_DIAG_*)
// ---------------------------------------------------------------------------

/// Packet socket info.
pub const PACKET_DIAG_INFO: u32 = 0;
/// Multicast list.
pub const PACKET_DIAG_MCLIST: u32 = 1;
/// RX ring configuration.
pub const PACKET_DIAG_RX_RING: u32 = 2;
/// TX ring configuration.
pub const PACKET_DIAG_TX_RING: u32 = 3;
/// Fanout info.
pub const PACKET_DIAG_FANOUT: u32 = 4;
/// UID of socket owner.
pub const PACKET_DIAG_UID: u32 = 5;
/// Memory info.
pub const PACKET_DIAG_MEMINFO: u32 = 6;
/// BPF filter.
pub const PACKET_DIAG_FILTER: u32 = 7;

// ---------------------------------------------------------------------------
// Fanout types (PACKET_FANOUT_*)
// ---------------------------------------------------------------------------

/// Hash-based fanout (hash on packet header).
pub const PACKET_FANOUT_HASH: u32 = 0;
/// Load-balance fanout (round-robin).
pub const PACKET_FANOUT_LB: u32 = 1;
/// CPU-based fanout (CPU ID determines socket).
pub const PACKET_FANOUT_CPU: u32 = 2;
/// Rollover fanout (overflow to next socket).
pub const PACKET_FANOUT_ROLLOVER: u32 = 3;
/// Random fanout.
pub const PACKET_FANOUT_RND: u32 = 4;
/// Queue mapping fanout.
pub const PACKET_FANOUT_QM: u32 = 5;
/// eBPF-based fanout.
pub const PACKET_FANOUT_CBPF: u32 = 6;
/// eBPF program fanout.
pub const PACKET_FANOUT_EBPF: u32 = 7;

// ---------------------------------------------------------------------------
// Fanout flags
// ---------------------------------------------------------------------------

/// Enable rollover as fallback.
pub const PACKET_FANOUT_FLAG_ROLLOVER: u32 = 0x1000;
/// Use unique ID for fanout group.
pub const PACKET_FANOUT_FLAG_UNIQUEID: u32 = 0x2000;
/// Ignore outgoing packets.
pub const PACKET_FANOUT_FLAG_IGNORE_OUTGOING: u32 = 0x4000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_flags_no_overlap() {
        let flags = [
            PACKET_SHOW_INFO, PACKET_SHOW_MCLIST,
            PACKET_SHOW_RING_CFG, PACKET_SHOW_FANOUT,
            PACKET_SHOW_MEMINFO, PACKET_SHOW_FILTER,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_response_attrs_distinct() {
        let attrs = [
            PACKET_DIAG_INFO, PACKET_DIAG_MCLIST,
            PACKET_DIAG_RX_RING, PACKET_DIAG_TX_RING,
            PACKET_DIAG_FANOUT, PACKET_DIAG_UID,
            PACKET_DIAG_MEMINFO, PACKET_DIAG_FILTER,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_fanout_types_distinct() {
        let types = [
            PACKET_FANOUT_HASH, PACKET_FANOUT_LB,
            PACKET_FANOUT_CPU, PACKET_FANOUT_ROLLOVER,
            PACKET_FANOUT_RND, PACKET_FANOUT_QM,
            PACKET_FANOUT_CBPF, PACKET_FANOUT_EBPF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
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
