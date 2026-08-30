//! `<linux/lwtunnel.h>` — Lightweight tunnel (LWT) encapsulation constants.
//!
//! Lightweight tunnels attach encapsulation metadata to routes rather
//! than requiring separate tunnel interfaces. When a packet matches
//! a route with LWT metadata, the kernel applies the encapsulation
//! inline (MPLS push, VXLAN encap, SEG6, BPF, etc.) without context
//! switching to a tunnel device. This is more efficient and scalable
//! for large-scale overlays. Configured via `ip route add ... encap
//! <type>`. Used by MPLS, SRv6, VXLAN, and BPF-based overlays.

// ---------------------------------------------------------------------------
// Lightweight tunnel encap types (LWTUNNEL_ENCAP_*)
// ---------------------------------------------------------------------------

/// No encapsulation.
pub const LWTUNNEL_ENCAP_NONE: u32 = 0;
/// MPLS label push encapsulation.
pub const LWTUNNEL_ENCAP_MPLS: u32 = 1;
/// IP encapsulation (ip-in-ip, GRE via metadata).
pub const LWTUNNEL_ENCAP_IP: u32 = 2;
/// ILA (Identifier-Locator Addressing) encapsulation.
pub const LWTUNNEL_ENCAP_ILA: u32 = 3;
/// IPv6 encapsulation (ip6-in-ip6, etc.).
pub const LWTUNNEL_ENCAP_IP6: u32 = 4;
/// SRv6 (Segment Routing v6) encapsulation.
pub const LWTUNNEL_ENCAP_SEG6: u32 = 5;
/// BPF program encapsulation.
pub const LWTUNNEL_ENCAP_BPF: u32 = 6;
/// SRv6 local action (End, End.X, etc.).
pub const LWTUNNEL_ENCAP_SEG6_LOCAL: u32 = 7;
/// RPL (Routing Protocol for Low-Power networks) encapsulation.
pub const LWTUNNEL_ENCAP_RPL: u32 = 8;
/// IOAM6 (In-situ OAM for IPv6) encapsulation.
pub const LWTUNNEL_ENCAP_IOAM6: u32 = 9;
/// XFRM (IPsec) encapsulation.
pub const LWTUNNEL_ENCAP_XFRM: u32 = 10;

// ---------------------------------------------------------------------------
// IP encap attributes (LWTUNNEL_IP_*)
// ---------------------------------------------------------------------------

/// Tunnel ID.
pub const LWTUNNEL_IP_ID: u32 = 1;
/// Destination IP address.
pub const LWTUNNEL_IP_DST: u32 = 2;
/// Source IP address.
pub const LWTUNNEL_IP_SRC: u32 = 3;
/// TTL.
pub const LWTUNNEL_IP_TTL: u32 = 4;
/// TOS (Type of Service).
pub const LWTUNNEL_IP_TOS: u32 = 5;
/// Tunnel flags.
pub const LWTUNNEL_IP_FLAGS: u32 = 6;
/// Options (VXLAN/Geneve/GTP options).
pub const LWTUNNEL_IP_OPTS: u32 = 7;

// ---------------------------------------------------------------------------
// IP6 encap attributes (LWTUNNEL_IP6_*)
// ---------------------------------------------------------------------------

/// Tunnel ID (IPv6).
pub const LWTUNNEL_IP6_ID: u32 = 1;
/// Destination IPv6 address.
pub const LWTUNNEL_IP6_DST: u32 = 2;
/// Source IPv6 address.
pub const LWTUNNEL_IP6_SRC: u32 = 3;
/// Hop limit.
pub const LWTUNNEL_IP6_HOPLIMIT: u32 = 4;
/// Traffic class.
pub const LWTUNNEL_IP6_TC: u32 = 5;
/// Tunnel flags (IPv6).
pub const LWTUNNEL_IP6_FLAGS: u32 = 6;
/// Options (IPv6).
pub const LWTUNNEL_IP6_OPTS: u32 = 7;

// ---------------------------------------------------------------------------
// BPF encap attributes
// ---------------------------------------------------------------------------

/// BPF program FD (input direction).
pub const LWT_BPF_IN: u32 = 1;
/// BPF program FD (output direction).
pub const LWT_BPF_OUT: u32 = 2;
/// BPF program FD (xmit direction).
pub const LWT_BPF_XMIT: u32 = 3;
/// BPF headroom requirement.
pub const LWT_BPF_XMIT_HEADROOM: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encap_types_distinct() {
        let types = [
            LWTUNNEL_ENCAP_NONE,
            LWTUNNEL_ENCAP_MPLS,
            LWTUNNEL_ENCAP_IP,
            LWTUNNEL_ENCAP_ILA,
            LWTUNNEL_ENCAP_IP6,
            LWTUNNEL_ENCAP_SEG6,
            LWTUNNEL_ENCAP_BPF,
            LWTUNNEL_ENCAP_SEG6_LOCAL,
            LWTUNNEL_ENCAP_RPL,
            LWTUNNEL_ENCAP_IOAM6,
            LWTUNNEL_ENCAP_XFRM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ip_attrs_distinct() {
        let attrs = [
            LWTUNNEL_IP_ID,
            LWTUNNEL_IP_DST,
            LWTUNNEL_IP_SRC,
            LWTUNNEL_IP_TTL,
            LWTUNNEL_IP_TOS,
            LWTUNNEL_IP_FLAGS,
            LWTUNNEL_IP_OPTS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ip6_attrs_distinct() {
        let attrs = [
            LWTUNNEL_IP6_ID,
            LWTUNNEL_IP6_DST,
            LWTUNNEL_IP6_SRC,
            LWTUNNEL_IP6_HOPLIMIT,
            LWTUNNEL_IP6_TC,
            LWTUNNEL_IP6_FLAGS,
            LWTUNNEL_IP6_OPTS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_bpf_attrs_distinct() {
        let attrs = [LWT_BPF_IN, LWT_BPF_OUT, LWT_BPF_XMIT, LWT_BPF_XMIT_HEADROOM];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ip_ip6_parallel() {
        // IPv4 and IPv6 attributes use the same numbering scheme
        assert_eq!(LWTUNNEL_IP_ID, LWTUNNEL_IP6_ID);
        assert_eq!(LWTUNNEL_IP_DST, LWTUNNEL_IP6_DST);
        assert_eq!(LWTUNNEL_IP_SRC, LWTUNNEL_IP6_SRC);
    }
}
