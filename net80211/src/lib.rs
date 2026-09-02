//! `net80211` — shared, `no_std`, allocation-free IEEE 802.11 frame parsing
//! and construction.
//!
//! This is the WiFi counterpart of [`netproto`]: the *privilege-free* wire
//! format, with no policy, no state machine and no hardware in it. It holds
//! the byte-level layout of 802.11 MAC headers, management-frame bodies,
//! information elements, the RSN element, EAPOL-Key frames, and the LLC/SNAP
//! shim that converts an 802.11 data frame to and from an Ethernet frame —
//! plus, in [`kdf`], the clause-12 key derivation that turns a PMK and two
//! nonces into the keys those frames are protected with.
//!
//! One module is not wire format: [`supplicant`] is the station-side state
//! machine that drives the 4-way handshake over those frames. It lives here
//! rather than in the supplicant binary because it is the only consumer that
//! ties the other modules together, and because a state machine with no I/O
//! in it is testable in a way that one wrapped around a socket is not.
//!
//! Three consumers need exactly this code and must not each grow their own
//! copy of it:
//!
//! - the kernel-side wireless driver, which builds probe requests and parses
//!   beacons before any of the rest of the stack exists;
//! - the supplicant (`userspace/wpa`), which today derives a PMK and runs a
//!   state machine but has no frames to run it over;
//! - the netstack, which sees 802.11 data frames as Ethernet once the
//!   LLC/SNAP header is off.
//!
//! ## The one thing to remember: 802.11 is little-endian
//!
//! Every fixed-length field in an 802.11 MAC header and management body —
//! Frame Control, Duration/ID, Sequence Control, QoS Control, capability
//! info, status and reason codes, the RSN version — is **little-endian**.
//! This is the opposite of Ethernet/IP/TCP, where `netproto` uses
//! `from_be_bytes` throughout. Addresses and other octet strings are, as
//! always, in transmission order and are not byte-swapped.
//!
//! Two fields deliberately break the rule and are big-endian, because they
//! are not 802.11 fields at all: the EtherType inside a SNAP header
//! ([`llc`]), and everything in an EAPOL frame ([`eapol`]), which is IEEE
//! 802.1X. Both are called out at their definitions.
//!
//! ## Design notes
//!
//! - **Bytes, not UTF-8.** An SSID is `[u8]`, not a string: the standard
//!   permits any octet sequence, including embedded NULs and invalid UTF-8,
//!   and a hidden network's SSID element is zero-length or all-zero.
//! - **No panics on bad input.** Parsers return `None` on short or malformed
//!   buffers. Every index is behind a validated length check. Frames arrive
//!   from the air, from anyone, so a malformed one is the expected case and
//!   not an exceptional one.
//! - **Borrowing parsers, caller-buffer builders.** Nothing allocates.
//!
//! ## References
//!
//! - IEEE Std 802.11-2020, clause 9 (frame formats), clause 12 (security).
//! - IEEE Std 802.1X-2020, clause 11 (EAPOL).
//! - RFC 1042 (IP over IEEE 802 networks — the SNAP encapsulation).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

pub mod eapol;
pub mod fcs;
pub mod frame;
pub mod ie;
pub mod kdf;
pub mod llc;
pub mod mgmt;
pub mod rsn;
pub mod supplicant;

/// A 6-byte 802.11 MAC address. Identical in layout to [`netproto::MacAddr`],
/// but this crate must not depend on `netproto` — a wireless driver needs the
/// frame layer before it has an IP stack to hand it to.
pub type MacAddr = [u8; 6];

/// The broadcast MAC address (`ff:ff:ff:ff:ff:ff`), used as the destination
/// of a wildcard probe request and of a beacon.
pub const BROADCAST_MAC: MacAddr = [0xFF; 6];

/// The maximum length of an SSID, in octets (IEEE 802.11-2020 §9.4.2.2).
///
/// This is a length in *bytes*, not characters: a 32-byte SSID may be far
/// fewer than 32 codepoints once decoded as UTF-8, which is why nothing in
/// this crate decodes it.
pub const MAX_SSID_LEN: usize = 32;

/// True if `addr` is a group address — the low bit of the first octet, exactly
/// as in IEEE 802.3 (§9.2.2). Broadcast is a group address.
#[must_use]
pub fn is_group_addr(addr: &MacAddr) -> bool {
    (addr[0] & 0x01) != 0
}

/// Read a little-endian `u16` from `buf[at..at + 2]`, or `None` if the buffer
/// is too short.
///
/// Every fixed 802.11 field goes through this rather than through open-coded
/// indexing, so that "did I remember it is little-endian here?" is asked once
/// instead of at four hundred call sites.
#[must_use]
pub(crate) fn le_u16(buf: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let s: [u8; 2] = buf.get(at..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(s))
}

/// Read a little-endian `u32` from `buf[at..at + 4]`, or `None` if short.
#[must_use]
pub(crate) fn le_u32(buf: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let s: [u8; 4] = buf.get(at..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(s))
}

/// Read a little-endian `u64` from `buf[at..at + 8]`, or `None` if short.
#[must_use]
pub(crate) fn le_u64(buf: &[u8], at: usize) -> Option<u64> {
    let end = at.checked_add(8)?;
    let s: [u8; 8] = buf.get(at..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(s))
}

/// Read a 6-byte address from `buf[at..at + 6]`, or `None` if short.
#[must_use]
pub(crate) fn addr_at(buf: &[u8], at: usize) -> Option<MacAddr> {
    let end = at.checked_add(6)?;
    let s = buf.get(at..end)?;
    let mut a = [0u8; 6];
    a.copy_from_slice(s);
    Some(a)
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

    #[test]
    fn group_addresses() {
        assert!(is_group_addr(&BROADCAST_MAC));
        // The IPv4 multicast OUI 01:00:5e.
        assert!(is_group_addr(&[0x01, 0x00, 0x5E, 0, 0, 1]));
        // A QEMU unicast address.
        assert!(!is_group_addr(&[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]));
    }

    #[test]
    fn little_endian_readers_are_little_endian() {
        let buf = [0x34, 0x12, 0x78, 0x56, 0x34, 0x12];
        assert_eq!(le_u16(&buf, 0), Some(0x1234));
        assert_eq!(le_u32(&buf, 0), Some(0x5678_1234));
    }

    #[test]
    fn readers_refuse_to_run_off_the_end() {
        let buf = [0u8; 3];
        assert_eq!(le_u16(&buf, 2), None);
        assert_eq!(le_u32(&buf, 0), None);
        assert_eq!(le_u64(&buf, 0), None);
        assert_eq!(addr_at(&buf, 0), None);
        // An `at` past the end must not wrap into a valid range.
        assert_eq!(le_u16(&buf, usize::MAX), None);
        assert_eq!(le_u32(&buf, usize::MAX), None);
        assert_eq!(le_u64(&buf, usize::MAX), None);
        assert_eq!(addr_at(&buf, usize::MAX), None);
    }
}
