//! ICMPv6 (RFC 4443) and Neighbor Discovery (RFC 4861) wire-format primitives.
//!
//! This models the byte layout of the Neighbor Discovery messages a host needs
//! to bring an IPv6 link up and resolve on-link neighbors:
//!
//! - **Router Solicitation** (type 133) — ask routers to advertise a prefix.
//! - **Neighbor Solicitation** (type 135) — resolve an on-link IPv6 address to
//!   a MAC, and Duplicate Address Detection.
//! - **Neighbor Advertisement** (type 136) — the reply carrying the target's
//!   link-layer address.
//!
//! Like the rest of `netproto`, this is pure, `no_std`, allocation-free byte
//! plumbing: builders write into a caller-provided buffer and compute the
//! IPv6-pseudo-header ICMPv6 checksum; parsers borrow a `&[u8]`, verify the
//! checksum against the caller-supplied source/destination, and never index
//! past a validated bound. The Router Advertisement / neighbor-cache *policy*
//! (who to trust, when to retransmit, cache eviction) belongs to the daemon
//! that owns per-link state, not here.

use crate::checksum;
use crate::ipv6::{self, Ipv6Addr};
use crate::MacAddr;

/// IPv6 next-header value for ICMPv6.
pub const NH_ICMPV6: u8 = 58;

/// ICMPv6 type: Router Solicitation (RFC 4861 §4.1).
pub const TYPE_ROUTER_SOLICITATION: u8 = 133;
/// ICMPv6 type: Router Advertisement (RFC 4861 §4.2).
pub const TYPE_ROUTER_ADVERTISEMENT: u8 = 134;
/// ICMPv6 type: Neighbor Solicitation (RFC 4861 §4.3).
pub const TYPE_NEIGHBOR_SOLICITATION: u8 = 135;
/// ICMPv6 type: Neighbor Advertisement (RFC 4861 §4.4).
pub const TYPE_NEIGHBOR_ADVERTISEMENT: u8 = 136;

/// NDP option: Source Link-Layer Address (RFC 4861 §4.6.1).
pub const OPT_SOURCE_LINK_ADDR: u8 = 1;
/// NDP option: Target Link-Layer Address (RFC 4861 §4.6.1).
pub const OPT_TARGET_LINK_ADDR: u8 = 2;
/// NDP option: Prefix Information (RFC 4861 §4.6.2).
pub const OPT_PREFIX_INFORMATION: u8 = 3;

/// Neighbor Advertisement flag: sender is a router (R).
pub const NA_FLAG_ROUTER: u8 = 0x80;
/// Neighbor Advertisement flag: sent in response to a solicitation (S).
pub const NA_FLAG_SOLICITED: u8 = 0x40;
/// Neighbor Advertisement flag: override an existing cache entry (O).
pub const NA_FLAG_OVERRIDE: u8 = 0x20;

/// The all-nodes link-local multicast address (`ff02::1`).
pub const ALL_NODES_LINK_LOCAL: Ipv6Addr =
    [0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01];
/// The all-routers link-local multicast address (`ff02::2`).
pub const ALL_ROUTERS_LINK_LOCAL: Ipv6Addr =
    [0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02];
/// The unspecified address (`::`), used as the source during DAD.
pub const UNSPECIFIED: Ipv6Addr = [0u8; 16];

/// The solicited-node multicast address for `target`
/// (`ff02::1:ffXX:XXXX`, low 24 bits copied from the target), RFC 4291 §2.7.1.
#[must_use]
pub fn solicited_node_multicast(target: &Ipv6Addr) -> Ipv6Addr {
    [
        0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01, 0xFF, target[13], target[14], target[15],
    ]
}

/// Build the EUI-64 modified interface identifier (8 bytes) from a MAC.
#[must_use]
fn eui64_iid(mac: &MacAddr) -> [u8; 8] {
    [
        mac[0] ^ 0x02,
        mac[1],
        mac[2],
        0xFF,
        0xFE,
        mac[3],
        mac[4],
        mac[5],
    ]
}

/// Derive the link-local address (`fe80::/64` + EUI-64 interface id) from a MAC,
/// RFC 4291 Appendix A.
#[must_use]
pub fn link_local_from_mac(mac: &MacAddr) -> Ipv6Addr {
    let mut addr = [0u8; 16];
    addr[0] = 0xFE;
    addr[1] = 0x80;
    addr[8..16].copy_from_slice(&eui64_iid(mac));
    addr
}

/// Form a SLAAC global address from an advertised /64 `prefix` and a MAC (the
/// EUI-64 interface identifier fills the low 64 bits), RFC 4862 §5.5.3.
#[must_use]
pub fn slaac_from_prefix(prefix64: &[u8; 8], mac: &MacAddr) -> Ipv6Addr {
    let mut addr = [0u8; 16];
    addr[0..8].copy_from_slice(prefix64);
    addr[8..16].copy_from_slice(&eui64_iid(mac));
    addr
}

/// Compute the ICMPv6 checksum over the IPv6 pseudo-header + `msg`. `msg` is the
/// full ICMPv6 message (type byte onward) with its checksum field zeroed.
#[must_use]
pub fn checksum(src: &Ipv6Addr, dst: &Ipv6Addr, msg: &[u8]) -> u16 {
    let sum = ipv6::pseudo_header_sum(src, dst, msg.len() as u32, NH_ICMPV6);
    checksum::internet_continue(sum, msg)
}

/// Verify a received ICMPv6 message's checksum against the packet's addresses.
/// A valid message (checksum field intact) sums to `0`.
#[must_use]
pub fn verify_checksum(src: &Ipv6Addr, dst: &Ipv6Addr, msg: &[u8]) -> bool {
    let sum = ipv6::pseudo_header_sum(src, dst, msg.len() as u32, NH_ICMPV6);
    checksum::internet_continue(sum, msg) == 0
}

/// Write a single-byte link-layer-address option (`type`, len=1, 6-byte MAC)
/// into `out[at..at+8]`. Returns the offset past the option, or `None` if
/// `out` is too small.
fn write_llad_option(out: &mut [u8], at: usize, opt_type: u8, mac: &MacAddr) -> Option<usize> {
    let end = at.checked_add(8)?;
    if out.len() < end {
        return None;
    }
    out[at] = opt_type;
    out[at + 1] = 1; // length in units of 8 bytes
    out[at + 2..at + 8].copy_from_slice(mac);
    Some(end)
}

/// Serialize a Neighbor Solicitation for `target` into `out`, including a
/// Source Link-Layer Address option carrying `src_mac`, and compute the
/// checksum against `src`/`dst`. `dst` is normally
/// [`solicited_node_multicast`] of `target` (address resolution / DAD) or the
/// target's unicast address (unreachability detection). Returns the byte count.
#[must_use]
pub fn write_neighbor_solicitation(
    out: &mut [u8],
    src: &Ipv6Addr,
    dst: &Ipv6Addr,
    target: &Ipv6Addr,
    src_mac: &MacAddr,
) -> Option<usize> {
    // 4 (header) + 4 (reserved) + 16 (target) + 8 (SLLA option) = 32.
    let total = 32;
    if out.len() < total {
        return None;
    }
    out[..total].fill(0);
    out[0] = TYPE_NEIGHBOR_SOLICITATION;
    out[1] = 0; // code
                // out[2..4] checksum placeholder, out[4..8] reserved
    out[8..24].copy_from_slice(target);
    write_llad_option(out, 24, OPT_SOURCE_LINK_ADDR, src_mac)?;
    let csum = checksum(src, dst, &out[..total]);
    out[2..4].copy_from_slice(&csum.to_be_bytes());
    Some(total)
}

/// Serialize a Neighbor Advertisement for `target` into `out`, with `flags`
/// (`NA_FLAG_*`) and a Target Link-Layer Address option carrying `target_mac`,
/// and compute the checksum against `src`/`dst`. Returns the byte count.
#[must_use]
pub fn write_neighbor_advertisement(
    out: &mut [u8],
    src: &Ipv6Addr,
    dst: &Ipv6Addr,
    target: &Ipv6Addr,
    flags: u8,
    target_mac: &MacAddr,
) -> Option<usize> {
    let total = 32;
    if out.len() < total {
        return None;
    }
    out[..total].fill(0);
    out[0] = TYPE_NEIGHBOR_ADVERTISEMENT;
    out[1] = 0; // code
                // out[2..4] checksum placeholder
    out[4] = flags; // R/S/O bits; remaining 3 reserved bytes stay zero
    out[8..24].copy_from_slice(target);
    write_llad_option(out, 24, OPT_TARGET_LINK_ADDR, target_mac)?;
    let csum = checksum(src, dst, &out[..total]);
    out[2..4].copy_from_slice(&csum.to_be_bytes());
    Some(total)
}

/// Serialize a Router Solicitation into `out`, with a Source Link-Layer Address
/// option carrying `src_mac`, and compute the checksum against `src`/`dst`
/// (`dst` is normally [`ALL_ROUTERS_LINK_LOCAL`]). Returns the byte count.
#[must_use]
pub fn write_router_solicitation(
    out: &mut [u8],
    src: &Ipv6Addr,
    dst: &Ipv6Addr,
    src_mac: &MacAddr,
) -> Option<usize> {
    // 4 (header) + 4 (reserved) + 8 (SLLA option) = 16.
    let total = 16;
    if out.len() < total {
        return None;
    }
    out[..total].fill(0);
    out[0] = TYPE_ROUTER_SOLICITATION;
    out[1] = 0; // code
                // out[2..4] checksum placeholder, out[4..8] reserved
    write_llad_option(out, 8, OPT_SOURCE_LINK_ADDR, src_mac)?;
    let csum = checksum(src, dst, &out[..total]);
    out[2..4].copy_from_slice(&csum.to_be_bytes());
    Some(total)
}

/// A parsed Neighbor Solicitation or Advertisement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeighborMessage {
    /// The target IPv6 address the message concerns.
    pub target: Ipv6Addr,
    /// The R/S/O flags byte (meaningful for advertisements; `0` for
    /// solicitations).
    pub flags: u8,
    /// The link-layer address carried in the SLLA/TLLA option, if present.
    pub link_addr: Option<MacAddr>,
}

/// Walk the option area starting at `opts`, returning the first MAC found in an
/// option of `want_type` (SLLA or TLLA). Malformed options (zero length or a
/// length running past the buffer) stop the walk.
fn find_llad(opts: &[u8], want_type: u8) -> Option<MacAddr> {
    let mut i = 0;
    while i + 2 <= opts.len() {
        let opt_type = opts[i];
        let len_units = opts[i + 1] as usize;
        if len_units == 0 {
            return None; // invalid: options must be > 0 length units
        }
        let opt_len = len_units.checked_mul(8)?;
        if i + opt_len > opts.len() {
            return None; // runs past the buffer
        }
        if opt_type == want_type && opt_len >= 8 {
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&opts[i + 2..i + 8]);
            return Some(mac);
        }
        i += opt_len;
    }
    None
}

/// Parse a Neighbor Solicitation `msg` (the full ICMPv6 message), verifying its
/// checksum against `src`/`dst`. Returns the target and the sender's MAC (from
/// the SLLA option, if any). Returns `None` on a short buffer, wrong type, or
/// checksum failure.
#[must_use]
pub fn parse_neighbor_solicitation(
    msg: &[u8],
    src: &Ipv6Addr,
    dst: &Ipv6Addr,
) -> Option<NeighborMessage> {
    if msg.len() < 24 || msg[0] != TYPE_NEIGHBOR_SOLICITATION {
        return None;
    }
    if !verify_checksum(src, dst, msg) {
        return None;
    }
    let mut target = [0u8; 16];
    target.copy_from_slice(&msg[8..24]);
    Some(NeighborMessage {
        target,
        flags: 0,
        link_addr: find_llad(&msg[24..], OPT_SOURCE_LINK_ADDR),
    })
}

/// Parse a Neighbor Advertisement `msg`, verifying its checksum against
/// `src`/`dst`. Returns the target, the R/S/O flags, and the target's MAC (from
/// the TLLA option, if any). Returns `None` on a short buffer, wrong type, or
/// checksum failure.
#[must_use]
pub fn parse_neighbor_advertisement(
    msg: &[u8],
    src: &Ipv6Addr,
    dst: &Ipv6Addr,
) -> Option<NeighborMessage> {
    if msg.len() < 24 || msg[0] != TYPE_NEIGHBOR_ADVERTISEMENT {
        return None;
    }
    if !verify_checksum(src, dst, msg) {
        return None;
    }
    let mut target = [0u8; 16];
    target.copy_from_slice(&msg[8..24]);
    Some(NeighborMessage {
        target,
        flags: msg[4],
        link_addr: find_llad(&msg[24..], OPT_TARGET_LINK_ADDR),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC_A: MacAddr = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    const MAC_B: MacAddr = [0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF];

    #[test]
    fn solicited_node_copies_low_24_bits() {
        let target = [
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0xDE, 0xAD, 0xBE, 0xEF,
        ];
        let sn = solicited_node_multicast(&target);
        assert_eq!(
            sn,
            [0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01, 0xFF, 0xAD, 0xBE, 0xEF]
        );
    }

    #[test]
    fn link_local_eui64_flips_ul_bit() {
        let ll = link_local_from_mac(&MAC_A);
        assert_eq!(ll[0], 0xFE);
        assert_eq!(ll[1], 0x80);
        // 0x52 with the universal/local bit flipped is 0x50.
        assert_eq!(ll[8], 0x50);
        assert_eq!(&ll[11..13], &[0xFF, 0xFE]);
        assert_eq!(&ll[13..16], &[0x12, 0x34, 0x56]);
    }

    #[test]
    fn slaac_places_prefix_and_iid() {
        let prefix = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0];
        let a = slaac_from_prefix(&prefix, &MAC_A);
        assert_eq!(&a[0..8], &prefix);
        assert_eq!(a[8], 0x50);
        assert_eq!(&a[13..16], &[0x12, 0x34, 0x56]);
    }

    #[test]
    fn ns_roundtrips_and_verifies() {
        let src = link_local_from_mac(&MAC_A);
        let target = link_local_from_mac(&MAC_B);
        let dst = solicited_node_multicast(&target);
        let mut buf = [0u8; 64];
        let n = write_neighbor_solicitation(&mut buf, &src, &dst, &target, &MAC_A).unwrap();
        assert_eq!(n, 32);
        // A correct message re-checksums to zero.
        assert!(verify_checksum(&src, &dst, &buf[..n]));
        let parsed = parse_neighbor_solicitation(&buf[..n], &src, &dst).unwrap();
        assert_eq!(parsed.target, target);
        assert_eq!(parsed.link_addr, Some(MAC_A));
    }

    #[test]
    fn na_roundtrips_with_flags_and_tlla() {
        let src = link_local_from_mac(&MAC_B);
        let dst = link_local_from_mac(&MAC_A);
        let target = src;
        let mut buf = [0u8; 64];
        let flags = NA_FLAG_SOLICITED | NA_FLAG_OVERRIDE;
        let n =
            write_neighbor_advertisement(&mut buf, &src, &dst, &target, flags, &MAC_B).unwrap();
        assert_eq!(n, 32);
        let parsed = parse_neighbor_advertisement(&buf[..n], &src, &dst).unwrap();
        assert_eq!(parsed.target, target);
        assert_eq!(parsed.flags, flags);
        assert_eq!(parsed.link_addr, Some(MAC_B));
    }

    #[test]
    fn rs_roundtrips() {
        let src = link_local_from_mac(&MAC_A);
        let dst = ALL_ROUTERS_LINK_LOCAL;
        let mut buf = [0u8; 32];
        let n = write_router_solicitation(&mut buf, &src, &dst, &MAC_A).unwrap();
        assert_eq!(n, 16);
        assert!(verify_checksum(&src, &dst, &buf[..n]));
        assert_eq!(buf[0], TYPE_ROUTER_SOLICITATION);
    }

    #[test]
    fn checksum_binds_to_addresses() {
        let src = link_local_from_mac(&MAC_A);
        let target = link_local_from_mac(&MAC_B);
        let dst = solicited_node_multicast(&target);
        let mut buf = [0u8; 64];
        let n = write_neighbor_solicitation(&mut buf, &src, &dst, &target, &MAC_A).unwrap();
        // Wrong source → verification fails and the typed parse rejects it.
        let mut wrong = src;
        wrong[0] ^= 0xFF;
        assert!(!verify_checksum(&wrong, &dst, &buf[..n]));
        assert!(parse_neighbor_solicitation(&buf[..n], &wrong, &dst).is_none());
    }

    #[test]
    fn wrong_type_and_short_rejected() {
        let src = link_local_from_mac(&MAC_A);
        let dst = ALL_NODES_LINK_LOCAL;
        let mut buf = [0u8; 64];
        let n = write_neighbor_advertisement(
            &mut buf,
            &src,
            &dst,
            &src,
            NA_FLAG_SOLICITED,
            &MAC_A,
        )
        .unwrap();
        // Parsing an advertisement as a solicitation fails on the type check.
        assert!(parse_neighbor_solicitation(&buf[..n], &src, &dst).is_none());
        assert!(parse_neighbor_advertisement(&buf[..8], &src, &dst).is_none());
    }

    #[test]
    fn malformed_option_length_stops_walk() {
        // A TLLA option claiming length 0 is invalid → no MAC extracted.
        let src = link_local_from_mac(&MAC_A);
        let dst = ALL_NODES_LINK_LOCAL;
        let mut buf = [0u8; 64];
        let n =
            write_neighbor_advertisement(&mut buf, &src, &dst, &src, 0, &MAC_A).unwrap();
        buf[25] = 0; // corrupt the option length; re-checksum so parse reaches the walk
        buf[2..4].copy_from_slice(&[0, 0]); // zero the field before recomputing
        let csum = checksum(&src, &dst, &buf[..n]);
        buf[2..4].copy_from_slice(&csum.to_be_bytes());
        let parsed = parse_neighbor_advertisement(&buf[..n], &src, &dst).unwrap();
        assert_eq!(parsed.link_addr, None);
    }
}
