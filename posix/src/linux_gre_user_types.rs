//! `<linux/if_tunnel.h>` / `<net/gre.h>` — Generic Routing Encapsulation.
//!
//! GRE (RFC 2784/2890) carries arbitrary L3 payloads over IP. Linux's
//! `ip_gre`, `ip6_gre`, and `gretap` drivers expose GRE tunnels to
//! userspace (`iproute2 ip link add type gre`); WireGuard's predecessors,
//! Cisco-interop tunnels, and PPTP (RFC 2637, GRE version 1) all share
//! the on-the-wire bit layout below.

// ---------------------------------------------------------------------------
// IP protocol number
// ---------------------------------------------------------------------------

/// `IPPROTO_GRE` — assigned IANA protocol for GRE-in-IP.
pub const IPPROTO_GRE: u32 = 47;

// ---------------------------------------------------------------------------
// GRE header flag bits (network byte order: high byte of 16-bit flags word)
// ---------------------------------------------------------------------------

/// Checksum present (RFC 2784).
pub const GRE_CSUM: u16 = 0x8000;
/// Routing present (deprecated by RFC 2890).
pub const GRE_ROUTING: u16 = 0x4000;
/// Key present (RFC 2890).
pub const GRE_KEY: u16 = 0x2000;
/// Sequence number present (RFC 2890).
pub const GRE_SEQ: u16 = 0x1000;
/// Strict source route (deprecated).
pub const GRE_STRICT: u16 = 0x0800;
/// Recursion control mask (3 bits).
pub const GRE_REC: u16 = 0x0700;
/// Acknowledgment present (PPTP/GRE v1, RFC 2637).
pub const GRE_ACK: u16 = 0x0080;
/// Flags reserved bit mask.
pub const GRE_FLAGS: u16 = 0x00F8;
/// Version field mask (low 3 bits).
pub const GRE_VERSION: u16 = 0x0007;

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

/// GRE version 0 — standard RFC 2784 encapsulation.
pub const GRE_VERSION_0: u16 = 0;
/// GRE version 1 — PPTP, RFC 2637.
pub const GRE_VERSION_1: u16 = 1;

// ---------------------------------------------------------------------------
// Header sizes
// ---------------------------------------------------------------------------

/// Minimum GRE header (flags + protocol).
pub const GRE_HEADER_MIN_SIZE: u32 = 4;
/// Header with checksum/reserved fields.
pub const GRE_HEADER_CSUM_SIZE: u32 = 8;
/// Header with checksum + key + sequence.
pub const GRE_HEADER_FULL_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_number() {
        // IANA-assigned, immutable.
        assert_eq!(IPPROTO_GRE, 47);
    }

    #[test]
    fn test_flag_bits_distinct_and_pow2() {
        let f = [GRE_CSUM, GRE_ROUTING, GRE_KEY, GRE_SEQ, GRE_STRICT, GRE_ACK];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_version_mask_and_values() {
        assert_eq!(GRE_VERSION, 0x0007);
        // Versions 0 and 1 both fit in the version mask.
        assert_eq!(GRE_VERSION_0 & GRE_VERSION, GRE_VERSION_0);
        assert_eq!(GRE_VERSION_1 & GRE_VERSION, GRE_VERSION_1);
        assert_ne!(GRE_VERSION_0, GRE_VERSION_1);
    }

    #[test]
    fn test_field_masks_dont_overlap() {
        // version, flags, recursion, ack must be disjoint with each other.
        assert_eq!(GRE_VERSION & GRE_FLAGS, 0);
        assert_eq!(GRE_VERSION & GRE_REC, 0);
        assert_eq!(GRE_REC & GRE_FLAGS, 0);
        assert_eq!(GRE_ACK & GRE_VERSION, 0);
    }

    #[test]
    fn test_header_sizes_ordered() {
        assert!(GRE_HEADER_MIN_SIZE < GRE_HEADER_CSUM_SIZE);
        assert!(GRE_HEADER_CSUM_SIZE < GRE_HEADER_FULL_SIZE);
        // Each optional 4-byte field doubles the minimum.
        assert_eq!(GRE_HEADER_MIN_SIZE * 2, GRE_HEADER_CSUM_SIZE);
        assert_eq!(GRE_HEADER_MIN_SIZE * 4, GRE_HEADER_FULL_SIZE);
    }
}
