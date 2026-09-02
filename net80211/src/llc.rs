//! LLC/SNAP encapsulation — the eight octets that separate an 802.11 data
//! frame's payload from an Ethernet frame's (RFC 1042).
//!
//! ```text
//! +------+------+---------+-----------+---------------+
//! | DSAP | SSAP | Control |  OUI (3)  | EtherType (2) |
//! | 0xAA | 0xAA |  0x03   | 00:00:00  |   big-endian  |
//! +------+------+---------+-----------+---------------+
//! ```
//!
//! An 802.11 data frame has no EtherType field of its own. The upper-layer
//! protocol is identified by this SNAP header, which sits at the front of the
//! frame body; the addresses come from the MAC header, where — depending on
//! the DS bits — the source and destination may be in any of four positions
//! (see [`crate::frame::MacHeader::data_addr_roles`]).
//!
//! # The EtherType here is big-endian
//!
//! Everything else in an 802.11 header is little-endian. The SNAP EtherType
//! is not: it is an Ethernet field that happens to be carried inside an 802.11
//! frame, and it keeps Ethernet's byte order. Getting this backwards produces
//! frames that look almost right — `0x0008` instead of `0x0800` — and are
//! discarded silently by every peer.
//!
//! # Two OUIs, not one
//!
//! RFC 1042 uses OUI `00:00:00`. IEEE 802.1H defines a second, *bridge-tunnel*
//! encapsulation under OUI `00:00:F8`, used for exactly two protocols —
//! AppleTalk AARP (`0x80F3`) and IPX (`0x8137`) — whose EtherType values
//! collide with 802.3 length fields. A receiver must accept both; a
//! transmitter picks by protocol. Accepting only `00:00:00` is the common
//! shortcut and it drops IPX on the floor.

use crate::MacAddr;

/// Length of the LLC/SNAP header.
pub const SNAP_LEN: usize = 8;

/// LLC DSAP/SSAP value that selects SNAP.
pub const LLC_SNAP_SAP: u8 = 0xAA;
/// LLC Control value: Unnumbered Information.
pub const LLC_CONTROL_UI: u8 = 0x03;

/// The RFC 1042 encapsulation OUI, used for everything except the two
/// protocols listed for [`BRIDGE_TUNNEL_OUI`].
pub const RFC1042_OUI: [u8; 3] = [0x00, 0x00, 0x00];
/// The IEEE 802.1H bridge-tunnel OUI.
pub const BRIDGE_TUNNEL_OUI: [u8; 3] = [0x00, 0x00, 0xF8];

/// EtherType for AppleTalk AARP — carried under [`BRIDGE_TUNNEL_OUI`].
pub const ETHERTYPE_AARP: u16 = 0x80F3;
/// EtherType for IPX — carried under [`BRIDGE_TUNNEL_OUI`].
pub const ETHERTYPE_IPX: u16 = 0x8137;
/// EtherType for IEEE 802.1X / EAPOL — how the WPA handshake is carried.
pub const ETHERTYPE_EAPOL: u16 = 0x888E;

/// The OUI a transmitter must use for `ethertype` (IEEE 802.1H).
#[must_use]
pub fn oui_for(ethertype: u16) -> [u8; 3] {
    if ethertype == ETHERTYPE_AARP || ethertype == ETHERTYPE_IPX {
        BRIDGE_TUNNEL_OUI
    } else {
        RFC1042_OUI
    }
}

/// A parsed SNAP header and the payload after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snap<'a> {
    /// The encapsulation OUI — [`RFC1042_OUI`] or [`BRIDGE_TUNNEL_OUI`].
    pub oui: [u8; 3],
    /// The EtherType of the encapsulated protocol (host order).
    pub ethertype: u16,
    /// Everything after the eight-octet header.
    pub payload: &'a [u8],
}

impl<'a> Snap<'a> {
    /// Parse an LLC/SNAP header from the front of an 802.11 data frame body.
    ///
    /// Returns `None` if the body is shorter than eight octets or the LLC
    /// header is not `AA AA 03` — i.e. it is not SNAP at all. Unrecognised
    /// OUIs are accepted and reported, not rejected: an OUI this crate does
    /// not know is a protocol it does not route, which is the caller's
    /// decision, whereas a non-SNAP LLC header is a different framing
    /// entirely.
    #[must_use]
    pub fn parse(body: &'a [u8]) -> Option<Self> {
        let hdr = body.get(..SNAP_LEN)?;
        if *hdr.first()? != LLC_SNAP_SAP
            || *hdr.get(1)? != LLC_SNAP_SAP
            || *hdr.get(2)? != LLC_CONTROL_UI
        {
            return None;
        }
        let mut oui = [0u8; 3];
        oui.copy_from_slice(hdr.get(3..6)?);
        // Big-endian: this is an Ethernet field, not an 802.11 one.
        let ethertype = u16::from_be_bytes([*hdr.get(6)?, *hdr.get(7)?]);
        Some(Snap {
            oui,
            ethertype,
            payload: body.get(SNAP_LEN..)?,
        })
    }

    /// True if the OUI is one of the two defined encapsulations.
    #[must_use]
    pub fn is_known_encapsulation(&self) -> bool {
        self.oui == RFC1042_OUI || self.oui == BRIDGE_TUNNEL_OUI
    }
}

/// Write an LLC/SNAP header into `out[..8]`, choosing the OUI per
/// [`oui_for`].
///
/// `None` if `out` is shorter than [`SNAP_LEN`].
#[must_use]
pub fn write_header(out: &mut [u8], ethertype: u16) -> Option<usize> {
    let dst = out.get_mut(..SNAP_LEN)?;
    *dst.get_mut(0)? = LLC_SNAP_SAP;
    *dst.get_mut(1)? = LLC_SNAP_SAP;
    *dst.get_mut(2)? = LLC_CONTROL_UI;
    dst.get_mut(3..6)?.copy_from_slice(&oui_for(ethertype));
    dst.get_mut(6..8)?.copy_from_slice(&ethertype.to_be_bytes());
    Some(SNAP_LEN)
}

/// Length of an Ethernet II header — duplicated from `netproto` rather than
/// depended on, for the reason given on [`crate::MacAddr`].
pub const ETHERNET_HEADER_LEN: usize = 14;

/// Convert an 802.11 data frame into an Ethernet II frame in `out`.
///
/// `src` and `dst` come from [`crate::frame::MacHeader::data_addr_roles`] —
/// **not** from Address 1 and Address 2, which are the receiver and
/// transmitter of this hop and are the wrong addresses in three of the four DS
/// configurations.
///
/// Returns the number of octets written, or `None` if `body` is not a SNAP
/// frame or `out` is too small.
#[must_use]
pub fn to_ethernet(out: &mut [u8], dst: &MacAddr, src: &MacAddr, body: &[u8]) -> Option<usize> {
    let snap = Snap::parse(body)?;
    let total = ETHERNET_HEADER_LEN.checked_add(snap.payload.len())?;
    let o = out.get_mut(..total)?;
    o.get_mut(0..6)?.copy_from_slice(dst);
    o.get_mut(6..12)?.copy_from_slice(src);
    o.get_mut(12..14)?
        .copy_from_slice(&snap.ethertype.to_be_bytes());
    o.get_mut(ETHERNET_HEADER_LEN..)?
        .copy_from_slice(snap.payload);
    Some(total)
}

/// The reverse of [`to_ethernet`]: turn an Ethernet II frame into the *body*
/// of an 802.11 data frame — the SNAP header plus the payload.
///
/// The MAC header is not built here, because the addresses it needs depend on
/// which DS configuration the link is in and this function does not know.
/// Returns the number of octets written to `out`, and the Ethernet
/// destination and source, which the caller must place into the MAC header
/// according to that configuration.
#[must_use]
pub fn from_ethernet(out: &mut [u8], eth: &[u8]) -> Option<(usize, MacAddr, MacAddr)> {
    let hdr = eth.get(..ETHERNET_HEADER_LEN)?;
    let mut dst = [0u8; 6];
    let mut src = [0u8; 6];
    dst.copy_from_slice(hdr.get(0..6)?);
    src.copy_from_slice(hdr.get(6..12)?);
    let ethertype = u16::from_be_bytes([*hdr.get(12)?, *hdr.get(13)?]);
    let payload = eth.get(ETHERNET_HEADER_LEN..)?;

    let total = SNAP_LEN.checked_add(payload.len())?;
    let o = out.get_mut(..total)?;
    write_header(o, ethertype)?;
    o.get_mut(SNAP_LEN..)?.copy_from_slice(payload);
    Some((total, dst, src))
}

// The five defensive lints the workspace turns on are for production code: a
// test that indexes a fixed-size fixture, or unwraps a value it just
// constructed, is *asserting*, and an assertion that fails by panicking is a
// test doing its job rather than a robustness hole.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
#[cfg(test)]
mod tests {
    use super::*;

    const A: MacAddr = [0x02, 0, 0, 0, 0, 1];
    const B: MacAddr = [0x02, 0, 0, 0, 0, 2];

    #[test]
    fn ethertype_is_big_endian_in_the_snap_header() {
        let mut buf = [0u8; SNAP_LEN];
        assert_eq!(write_header(&mut buf, 0x0800), Some(SNAP_LEN));
        assert_eq!(buf, [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00, 0x08, 0x00]);
        assert_eq!(Snap::parse(&buf).map(|s| s.ethertype), Some(0x0800));
    }

    #[test]
    fn ipx_and_aarp_use_the_bridge_tunnel_oui_and_nothing_else_does() {
        assert_eq!(oui_for(ETHERTYPE_IPX), BRIDGE_TUNNEL_OUI);
        assert_eq!(oui_for(ETHERTYPE_AARP), BRIDGE_TUNNEL_OUI);
        assert_eq!(oui_for(0x0800), RFC1042_OUI);
        assert_eq!(oui_for(0x86DD), RFC1042_OUI);
        assert_eq!(oui_for(ETHERTYPE_EAPOL), RFC1042_OUI);

        let mut buf = [0u8; SNAP_LEN];
        write_header(&mut buf, ETHERTYPE_IPX).expect("fits");
        let s = Snap::parse(&buf).expect("parses");
        assert_eq!(s.oui, BRIDGE_TUNNEL_OUI);
        assert!(s.is_known_encapsulation());
    }

    #[test]
    fn a_non_snap_llc_header_is_refused() {
        // 802.2 LLC with real SAPs (Spanning Tree: 42 42 03) is not SNAP.
        let stp = [0x42, 0x42, 0x03, 0, 0, 0, 0, 0];
        assert!(Snap::parse(&stp).is_none());
        // The control octet matters too.
        let bad_ctl = [0xAA, 0xAA, 0x00, 0, 0, 0, 0x08, 0x00];
        assert!(Snap::parse(&bad_ctl).is_none());
    }

    #[test]
    fn an_unknown_oui_parses_but_is_reported_as_unknown() {
        let buf = [0xAA, 0xAA, 0x03, 0x00, 0x0F, 0xAC, 0x08, 0x00];
        let s = Snap::parse(&buf).expect("SNAP framing is well-formed");
        assert_eq!(s.oui, [0x00, 0x0F, 0xAC]);
        assert!(!s.is_known_encapsulation());
    }

    #[test]
    fn a_body_shorter_than_the_header_is_refused() {
        for short in 0..SNAP_LEN {
            let buf = [0xAAu8, 0xAA, 0x03, 0, 0, 0, 0x08, 0x00];
            assert!(
                Snap::parse(&buf[..short]).is_none(),
                "{short} octets must not parse"
            );
        }
    }

    #[test]
    fn round_trip_between_ethernet_and_an_eight_oh_two_eleven_body() {
        let mut eth = [0u8; ETHERNET_HEADER_LEN + 4];
        eth[0..6].copy_from_slice(&A);
        eth[6..12].copy_from_slice(&B);
        eth[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        eth[14..].copy_from_slice(&[1, 2, 3, 4]);

        let mut body = [0u8; 32];
        let (n, dst, src) = from_ethernet(&mut body, &eth).expect("converts");
        assert_eq!((dst, src), (A, B));
        assert_eq!(n, SNAP_LEN + 4);
        assert_eq!(
            Snap::parse(&body[..n]).map(|s| s.payload),
            Some(&[1, 2, 3, 4][..])
        );

        let mut back = [0u8; 32];
        let m = to_ethernet(&mut back, &dst, &src, &body[..n]).expect("converts");
        assert_eq!(&back[..m], &eth[..]);
    }

    #[test]
    fn a_payloadless_frame_survives_the_round_trip() {
        let mut eth = [0u8; ETHERNET_HEADER_LEN];
        eth[0..6].copy_from_slice(&A);
        eth[6..12].copy_from_slice(&B);
        eth[12..14].copy_from_slice(&ETHERTYPE_EAPOL.to_be_bytes());

        let mut body = [0u8; 16];
        let (n, dst, src) = from_ethernet(&mut body, &eth).expect("converts");
        assert_eq!(n, SNAP_LEN);
        let mut back = [0u8; 16];
        let m = to_ethernet(&mut back, &dst, &src, &body[..n]).expect("converts");
        assert_eq!(&back[..m], &eth[..]);
    }

    #[test]
    fn conversions_refuse_short_buffers_rather_than_truncating() {
        let mut eth = [0u8; ETHERNET_HEADER_LEN + 4];
        eth[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        for short in 0..SNAP_LEN + 4 {
            let mut body = [0u8; SNAP_LEN + 4];
            assert!(
                from_ethernet(&mut body[..short], &eth).is_none(),
                "{short} octets"
            );
        }
        // A short *input* is refused too: 13 octets is not an Ethernet header.
        let mut body = [0u8; 32];
        for short in 0..ETHERNET_HEADER_LEN {
            assert!(
                from_ethernet(&mut body, &eth[..short]).is_none(),
                "{short} input octets"
            );
        }

        let mut snap_body = [0u8; SNAP_LEN + 4];
        write_header(&mut snap_body, 0x0800).expect("fits");
        for short in 0..ETHERNET_HEADER_LEN + 4 {
            let mut out = [0u8; ETHERNET_HEADER_LEN + 4];
            assert!(
                to_ethernet(&mut out[..short], &A, &B, &snap_body).is_none(),
                "{short}"
            );
        }
    }

    #[test]
    fn writing_a_snap_header_into_a_short_buffer_fails() {
        for short in 0..SNAP_LEN {
            let mut buf = [0u8; SNAP_LEN];
            assert!(write_header(&mut buf[..short], 0x0800).is_none());
        }
    }
}
