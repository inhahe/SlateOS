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

    // Check if the frame is addressed to us or is broadcast.
    let our_mac = super::interface::mac();
    let is_for_us = frame.dst.0 == our_mac.0
        || frame.dst.0 == BROADCAST_MAC.0;

    if !is_for_us {
        return Ok(()); // Not for us, silently drop.
    }

    match frame.ethertype {
        ETHERTYPE_ARP => super::arp::process_arp(frame.payload),
        ETHERTYPE_IPV4 => super::ipv4::process_ipv4(frame.payload),
        _ => {
            // Unknown protocol — silently drop.
            Ok(())
        }
    }
}
