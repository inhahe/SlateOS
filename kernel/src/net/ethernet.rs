//! Ethernet frame parsing and construction.
//!
//! Handles IEEE 802.3 Ethernet II frames with 14-byte header:
//! - 6 bytes destination MAC
//! - 6 bytes source MAC
//! - 2 bytes EtherType
//! - Payload (46–1500 bytes)

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::virtio::net::MacAddress;

// ---------------------------------------------------------------------------
// EtherType constants
// ---------------------------------------------------------------------------

/// EtherType: IPv4.
pub const ETHERTYPE_IPV4: u16 = 0x0800;
/// EtherType: ARP.
pub const ETHERTYPE_ARP: u16 = 0x0806;

/// Ethernet header size (dest MAC + src MAC + EtherType).
pub const ETH_HEADER_SIZE: usize = 14;

/// Broadcast MAC address (ff:ff:ff:ff:ff:ff).
pub const BROADCAST_MAC: MacAddress = MacAddress([0xFF; 6]);

// ---------------------------------------------------------------------------
// Ethernet frame
// ---------------------------------------------------------------------------

/// A parsed Ethernet frame.
pub struct EthernetFrame<'a> {
    /// Destination MAC address.
    pub dst: MacAddress,
    /// Source MAC address.
    pub src: MacAddress,
    /// EtherType (protocol identifier).
    pub ethertype: u16,
    /// Frame payload (after the 14-byte header).
    pub payload: &'a [u8],
}

impl<'a> EthernetFrame<'a> {
    /// Parse an Ethernet frame from raw bytes.
    pub fn parse(data: &'a [u8]) -> KernelResult<Self> {
        if data.len() < ETH_HEADER_SIZE {
            return Err(KernelError::InvalidArgument);
        }

        let mut dst = [0u8; 6];
        let mut src = [0u8; 6];
        dst.copy_from_slice(&data[..6]);
        src.copy_from_slice(&data[6..12]);

        // EtherType is big-endian.
        let ethertype = u16::from_be_bytes([data[12], data[13]]);

        Ok(Self {
            dst: MacAddress(dst),
            src: MacAddress(src),
            ethertype,
            payload: &data[ETH_HEADER_SIZE..],
        })
    }
}

/// Build an Ethernet frame.
///
/// Returns the complete frame bytes (header + payload).
#[allow(clippy::arithmetic_side_effects)]
pub fn build_frame(dst: &MacAddress, src: &MacAddress, ethertype: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(ETH_HEADER_SIZE + payload.len());
    frame.extend_from_slice(&dst.0);
    frame.extend_from_slice(&src.0);
    frame.extend_from_slice(&ethertype.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Process an incoming Ethernet frame.
///
/// Dispatches to ARP or IPv4 based on EtherType.
pub fn process_frame(data: &[u8]) -> KernelResult<()> {
    let frame = EthernetFrame::parse(data)?;

    // Check if the frame is addressed to us, broadcast, or multicast.
    //
    // IPv4 multicast (224.0.0.0/4) maps to Ethernet MAC addresses with
    // the prefix 01:00:5e (IEEE 802.3 §7.8).  The low bit of the first
    // octet being set indicates a multicast MAC (group address).
    let our_mac = super::interface::mac();
    let is_for_us = frame.dst.0 == our_mac.0
        || frame.dst.0 == BROADCAST_MAC.0
        || (frame.dst.0[0] & 0x01) != 0; // Multicast bit set.

    if !is_for_us {
        return Ok(()); // Not for us, silently drop.
    }

    match frame.ethertype {
        ETHERTYPE_ARP => super::arp::process_arp(frame.payload),
        ETHERTYPE_IPV4 => super::ipv4::process_ipv4(frame.payload),
        super::lldp::ETHERTYPE_LLDP => super::lldp::process_frame(&frame.src, frame.payload),
        _ => {
            // Unknown protocol — silently drop.
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Ethernet frame parsing and construction tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ethernet] Running self-test...");

    test_parse_valid_frame()?;
    test_parse_too_short()?;
    test_build_frame_roundtrip()?;
    test_broadcast_mac()?;
    test_ethertype_constants()?;

    crate::serial_println!("[ethernet] Self-test PASSED (5 tests)");
    Ok(())
}

/// Parse a valid Ethernet frame with known bytes.
fn test_parse_valid_frame() -> KernelResult<()> {
    // Build a frame: dst=ff:ff:ff:ff:ff:ff, src=AA:BB:CC:DD:EE:FF,
    // ethertype=0x0800 (IPv4), payload=[1,2,3,4].
    let mut raw = [0u8; 18]; // 14 header + 4 payload
    // Dst MAC: broadcast.
    raw[0..6].copy_from_slice(&[0xFF; 6]);
    // Src MAC.
    raw[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    // EtherType: IPv4 big-endian → [0x08, 0x00].
    raw[12] = 0x08;
    raw[13] = 0x00;
    // Payload.
    raw[14..18].copy_from_slice(&[1, 2, 3, 4]);

    let frame = EthernetFrame::parse(&raw)?;

    if frame.dst.0 != [0xFF; 6] {
        crate::serial_println!("[ethernet]   FAIL: dst MAC");
        return Err(KernelError::InternalError);
    }
    if frame.src.0 != [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF] {
        crate::serial_println!("[ethernet]   FAIL: src MAC");
        return Err(KernelError::InternalError);
    }
    if frame.ethertype != ETHERTYPE_IPV4 {
        crate::serial_println!(
            "[ethernet]   FAIL: ethertype = {:#06x}", frame.ethertype
        );
        return Err(KernelError::InternalError);
    }
    if frame.payload != &[1, 2, 3, 4] {
        crate::serial_println!("[ethernet]   FAIL: payload");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ethernet]   parse valid frame: OK");
    Ok(())
}

/// Parse rejects frames shorter than 14 bytes.
fn test_parse_too_short() -> KernelResult<()> {
    let short = [0u8; 13];
    match EthernetFrame::parse(&short) {
        Err(KernelError::InvalidArgument) => {}
        other => {
            crate::serial_println!(
                "[ethernet]   FAIL: parse(13 bytes) = {:?}",
                other.map(|_| ())
            );
            return Err(KernelError::InternalError);
        }
    }

    // Exactly 14 bytes should succeed (empty payload).
    let exact = [0u8; 14];
    let frame = EthernetFrame::parse(&exact)?;
    if !frame.payload.is_empty() {
        crate::serial_println!("[ethernet]   FAIL: 14-byte payload not empty");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ethernet]   parse too short: OK");
    Ok(())
}

/// Build a frame and parse it back — roundtrip test.
fn test_build_frame_roundtrip() -> KernelResult<()> {
    let dst = MacAddress([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    let src = MacAddress([0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F]);
    let payload = [0xDE, 0xAD, 0xBE, 0xEF];

    let frame_bytes = build_frame(&dst, &src, ETHERTYPE_ARP, &payload);

    // Should be 14 + 4 = 18 bytes.
    if frame_bytes.len() != 18 {
        crate::serial_println!(
            "[ethernet]   FAIL: frame length = {}", frame_bytes.len()
        );
        return Err(KernelError::InternalError);
    }

    // Parse it back.
    let parsed = EthernetFrame::parse(&frame_bytes)?;
    if parsed.dst.0 != dst.0 {
        crate::serial_println!("[ethernet]   FAIL: roundtrip dst");
        return Err(KernelError::InternalError);
    }
    if parsed.src.0 != src.0 {
        crate::serial_println!("[ethernet]   FAIL: roundtrip src");
        return Err(KernelError::InternalError);
    }
    if parsed.ethertype != ETHERTYPE_ARP {
        crate::serial_println!("[ethernet]   FAIL: roundtrip ethertype");
        return Err(KernelError::InternalError);
    }
    if parsed.payload != &payload {
        crate::serial_println!("[ethernet]   FAIL: roundtrip payload");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ethernet]   build/parse roundtrip: OK");
    Ok(())
}

/// Verify BROADCAST_MAC constant.
fn test_broadcast_mac() -> KernelResult<()> {
    if BROADCAST_MAC.0 != [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] {
        crate::serial_println!("[ethernet]   FAIL: BROADCAST_MAC");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ethernet]   broadcast MAC: OK");
    Ok(())
}

/// Verify EtherType constants.
fn test_ethertype_constants() -> KernelResult<()> {
    if ETHERTYPE_IPV4 != 0x0800 {
        crate::serial_println!("[ethernet]   FAIL: ETHERTYPE_IPV4");
        return Err(KernelError::InternalError);
    }
    if ETHERTYPE_ARP != 0x0806 {
        crate::serial_println!("[ethernet]   FAIL: ETHERTYPE_ARP");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ethernet]   ethertype constants: OK");
    Ok(())
}
