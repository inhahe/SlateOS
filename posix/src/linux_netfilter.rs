//! `<linux/netfilter.h>` — netfilter framework constants.
//!
//! Provides hook points, verdict values, and protocol family
//! constants for the Linux netfilter subsystem.

// ---------------------------------------------------------------------------
// Netfilter protocol families
// ---------------------------------------------------------------------------

/// Unspecified.
pub const NFPROTO_UNSPEC: u8 = 0;
/// IPv4.
pub const NFPROTO_IPV4: u8 = 2;
/// ARP.
pub const NFPROTO_ARP: u8 = 3;
/// Netdev.
pub const NFPROTO_NETDEV: u8 = 5;
/// Bridge.
pub const NFPROTO_BRIDGE: u8 = 7;
/// IPv6.
pub const NFPROTO_IPV6: u8 = 10;
/// DECnet (legacy).
pub const NFPROTO_DECNET: u8 = 12;
/// Number of protocol families.
pub const NFPROTO_NUMPROTO: u8 = 13;

// ---------------------------------------------------------------------------
// Netfilter hook points (IPv4/IPv6)
// ---------------------------------------------------------------------------

/// Incoming packets, before routing.
pub const NF_INET_PRE_ROUTING: u32 = 0;
/// Packets for local delivery.
pub const NF_INET_LOCAL_IN: u32 = 1;
/// Forwarded packets.
pub const NF_INET_FORWARD: u32 = 2;
/// Locally generated packets, before routing.
pub const NF_INET_LOCAL_OUT: u32 = 3;
/// Outgoing packets, after routing.
pub const NF_INET_POST_ROUTING: u32 = 4;
/// Number of hook points.
pub const NF_INET_NUMHOOKS: u32 = 5;

// ---------------------------------------------------------------------------
// Netfilter verdicts
// ---------------------------------------------------------------------------

/// Drop the packet.
pub const NF_DROP: i32 = 0;
/// Accept the packet.
pub const NF_ACCEPT: i32 = 1;
/// Stolen — module took ownership.
pub const NF_STOLEN: i32 = 2;
/// Queue to userspace (nfqueue).
pub const NF_QUEUE: i32 = 3;
/// Repeat the hook.
pub const NF_REPEAT: i32 = 4;
/// Stop processing.
pub const NF_STOP: i32 = 5;
/// Maximum verdict value.
pub const NF_MAX_VERDICT: i32 = NF_STOP;

/// Verdict mask (lower bits).
pub const NF_VERDICT_MASK: u32 = 0x0000_FFFF;
/// Queue number shift.
pub const NF_VERDICT_QMASK: u32 = 0xFFFF_0000;
/// Queue number bit shift.
pub const NF_VERDICT_QBITS: u32 = 16;

// ---------------------------------------------------------------------------
// Netfilter sockopt base values
// ---------------------------------------------------------------------------

/// Base for nf_sockopt_ops (GET).
pub const NF_SO_GET_INFO: i32 = 0;
/// Base for nf_sockopt_ops (SET).
pub const NF_SO_SET_INFO: i32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_families() {
        assert_eq!(NFPROTO_UNSPEC, 0);
        assert_eq!(NFPROTO_IPV4, 2);
        assert_eq!(NFPROTO_IPV6, 10);
    }

    #[test]
    fn test_hook_points_sequential() {
        assert_eq!(NF_INET_PRE_ROUTING, 0);
        assert_eq!(NF_INET_LOCAL_IN, 1);
        assert_eq!(NF_INET_FORWARD, 2);
        assert_eq!(NF_INET_LOCAL_OUT, 3);
        assert_eq!(NF_INET_POST_ROUTING, 4);
        assert_eq!(NF_INET_NUMHOOKS, 5);
    }

    #[test]
    fn test_verdicts_sequential() {
        assert_eq!(NF_DROP, 0);
        assert_eq!(NF_ACCEPT, 1);
        assert_eq!(NF_STOLEN, 2);
        assert_eq!(NF_QUEUE, 3);
        assert_eq!(NF_REPEAT, 4);
        assert_eq!(NF_STOP, 5);
    }

    #[test]
    fn test_verdicts_distinct() {
        let verdicts = [NF_DROP, NF_ACCEPT, NF_STOLEN, NF_QUEUE, NF_REPEAT, NF_STOP];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
    }

    #[test]
    fn test_verdict_mask() {
        // Verdict mask should extract only the lower 16 bits.
        let verdict_with_queue = (42u32 << NF_VERDICT_QBITS) | (NF_QUEUE as u32);
        assert_eq!(verdict_with_queue & NF_VERDICT_MASK, NF_QUEUE as u32);
        assert_eq!(
            (verdict_with_queue & NF_VERDICT_QMASK) >> NF_VERDICT_QBITS,
            42
        );
    }
}
