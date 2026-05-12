//! ARP (Address Resolution Protocol) implementation.
//!
//! Handles ARP request/reply for IPv4-over-Ethernet (RFC 826).
//! Maintains a simple ARP cache mapping IP addresses to MAC addresses.
//!
//! ## Protocol overview
//!
//! ARP resolves IPv4 addresses to Ethernet MAC addresses on a LAN.
//! When we need to send to an IP we don't have a MAC for, we broadcast
//! an ARP request.  The target replies with its MAC.  We also respond
//! to ARP requests for our own IP.
//!
//! ## Namespace note
//!
//! The ARP cache is global — shared across all network namespaces.
//! This is correct for the current design where all namespaces share
//! a single physical NIC (so the same MAC↔IP mappings apply to all).
//! When virtual ethernet (veth) pairs are implemented, each namespace
//! will need its own ARP cache for its virtual interface's LAN segment.

use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::virtio::net::MacAddress;

use super::ethernet::{self, BROADCAST_MAC, ETHERTYPE_ARP};
use super::interface::{self, Ipv4Addr};

// ---------------------------------------------------------------------------
// ARP packet structure
// ---------------------------------------------------------------------------

/// ARP hardware type: Ethernet.
const HW_TYPE_ETHERNET: u16 = 1;
/// ARP protocol type: IPv4.
const PROTO_TYPE_IPV4: u16 = 0x0800;
/// ARP operation: request.
const ARP_REQUEST: u16 = 1;
/// ARP operation: reply.
const ARP_REPLY: u16 = 2;

/// ARP packet size for IPv4/Ethernet (fixed: 28 bytes).
const ARP_PACKET_SIZE: usize = 28;

/// Maximum ARP cache entries.
const ARP_CACHE_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// ARP cache
// ---------------------------------------------------------------------------

/// One entry in the ARP cache.
#[derive(Debug, Clone, Copy)]
struct ArpEntry {
    ip: Ipv4Addr,
    mac: MacAddress,
    valid: bool,
}

/// The ARP cache.
static ARP_CACHE: Mutex<[ArpEntry; ARP_CACHE_SIZE]> = Mutex::new(
    [ArpEntry {
        ip: Ipv4Addr::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        valid: false,
    }; ARP_CACHE_SIZE]
);

/// Look up a MAC address in the ARP cache.
pub fn lookup(ip: Ipv4Addr) -> Option<MacAddress> {
    let cache = ARP_CACHE.lock();
    for entry in cache.iter() {
        if entry.valid && entry.ip == ip {
            return Some(entry.mac);
        }
    }
    None
}

/// Insert or update an ARP cache entry.
fn cache_insert(ip: Ipv4Addr, mac: MacAddress) {
    let mut cache = ARP_CACHE.lock();

    // Check if already present — update.
    for entry in cache.iter_mut() {
        if entry.valid && entry.ip == ip {
            entry.mac = mac;
            return;
        }
    }

    // Find an empty slot.
    for entry in cache.iter_mut() {
        if !entry.valid {
            *entry = ArpEntry { ip, mac, valid: true };
            return;
        }
    }

    // Cache full — evict the first entry (simple strategy).
    cache[0] = ArpEntry { ip, mac, valid: true };
}

// ---------------------------------------------------------------------------
// ARP packet parsing/building
// ---------------------------------------------------------------------------

/// Parse an ARP packet from raw bytes.
///
/// Returns (operation, sender_mac, sender_ip, target_mac, target_ip).
#[allow(clippy::type_complexity)]
fn parse_arp(data: &[u8]) -> KernelResult<(u16, MacAddress, Ipv4Addr, MacAddress, Ipv4Addr)> {
    if data.len() < ARP_PACKET_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    let hw_type = u16::from_be_bytes([data[0], data[1]]);
    let proto_type = u16::from_be_bytes([data[2], data[3]]);
    let hw_len = data[4];
    let proto_len = data[5];
    let operation = u16::from_be_bytes([data[6], data[7]]);

    // Validate: must be Ethernet/IPv4.
    if hw_type != HW_TYPE_ETHERNET || proto_type != PROTO_TYPE_IPV4
        || hw_len != 6 || proto_len != 4
    {
        return Err(KernelError::InvalidArgument);
    }

    let mut sender_mac = [0u8; 6];
    sender_mac.copy_from_slice(&data[8..14]);
    let mut sender_ip = [0u8; 4];
    sender_ip.copy_from_slice(&data[14..18]);
    let mut target_mac = [0u8; 6];
    target_mac.copy_from_slice(&data[18..24]);
    let mut target_ip = [0u8; 4];
    target_ip.copy_from_slice(&data[24..28]);

    Ok((
        operation,
        MacAddress(sender_mac),
        Ipv4Addr(sender_ip),
        MacAddress(target_mac),
        Ipv4Addr(target_ip),
    ))
}

/// Build an ARP packet.
fn build_arp(
    operation: u16,
    sender_mac: &MacAddress,
    sender_ip: Ipv4Addr,
    target_mac: &MacAddress,
    target_ip: Ipv4Addr,
) -> [u8; ARP_PACKET_SIZE] {
    let mut pkt = [0u8; ARP_PACKET_SIZE];
    // Hardware type: Ethernet (1).
    pkt[0..2].copy_from_slice(&HW_TYPE_ETHERNET.to_be_bytes());
    // Protocol type: IPv4.
    pkt[2..4].copy_from_slice(&PROTO_TYPE_IPV4.to_be_bytes());
    // Hardware address length.
    pkt[4] = 6;
    // Protocol address length.
    pkt[5] = 4;
    // Operation.
    pkt[6..8].copy_from_slice(&operation.to_be_bytes());
    // Sender hardware address.
    pkt[8..14].copy_from_slice(&sender_mac.0);
    // Sender protocol address.
    pkt[14..18].copy_from_slice(&sender_ip.0);
    // Target hardware address.
    pkt[18..24].copy_from_slice(&target_mac.0);
    // Target protocol address.
    pkt[24..28].copy_from_slice(&target_ip.0);
    pkt
}

// ---------------------------------------------------------------------------
// ARP processing
// ---------------------------------------------------------------------------

/// Process an incoming ARP packet.
pub fn process_arp(data: &[u8]) -> KernelResult<()> {
    let (operation, sender_mac, sender_ip, _target_mac, target_ip) = parse_arp(data)?;

    // Always learn the sender's MAC from any ARP we see.
    if !sender_ip.is_unspecified() {
        cache_insert(sender_ip, sender_mac);
    }

    match operation {
        ARP_REQUEST => {
            // Is this a request for our IP?
            let our_ip = interface::ip();
            if !our_ip.is_unspecified() && target_ip == our_ip {
                // Send ARP reply.
                send_reply(&sender_mac, sender_ip)?;
            }
        }
        ARP_REPLY => {
            // Already cached the sender above — nothing else to do.
            crate::serial_println!(
                "[arp] Reply: {} is at {}",
                sender_ip, sender_mac
            );
        }
        _ => {
            // Unknown ARP operation — ignore.
        }
    }

    Ok(())
}

/// Send an ARP reply to the given target.
fn send_reply(target_mac: &MacAddress, target_ip: Ipv4Addr) -> KernelResult<()> {
    let our_mac = interface::mac();
    let our_ip = interface::ip();

    let arp_data = build_arp(ARP_REPLY, &our_mac, our_ip, target_mac, target_ip);
    let frame = ethernet::build_frame(target_mac, &our_mac, ETHERTYPE_ARP, &arp_data);

    super::send_frame(&frame)?;

    Ok(())
}

/// Send an ARP request for the given IP address.
pub fn send_request(target_ip: Ipv4Addr) -> KernelResult<()> {
    let our_mac = interface::mac();
    let our_ip = interface::ip();

    let arp_data = build_arp(
        ARP_REQUEST,
        &our_mac,
        our_ip,
        &MacAddress([0; 6]),  // Target MAC unknown — that's what we're asking for.
        target_ip,
    );
    let frame = ethernet::build_frame(&BROADCAST_MAC, &our_mac, ETHERTYPE_ARP, &arp_data);

    super::send_frame(&frame)?;

    Ok(())
}

/// Resolve an IP address to a MAC address.
///
/// First checks the ARP cache.  If not found, sends an ARP request
/// and polls for a reply (blocking, with timeout).
#[allow(clippy::arithmetic_side_effects)]
pub fn resolve(ip: Ipv4Addr) -> KernelResult<MacAddress> {
    // Check cache first.
    if let Some(mac) = lookup(ip) {
        return Ok(mac);
    }

    // Broadcast is always ff:ff:ff:ff:ff:ff.
    if ip.is_broadcast() {
        return Ok(BROADCAST_MAC);
    }

    // Send ARP request and wait for reply.
    send_request(ip)?;

    // Poll for up to ~1 second (1000 iterations with spin + NIC poll).
    for _ in 0..1000 {
        // Poll the NIC for any incoming frames (including the ARP reply).
        super::poll();

        // Check if the reply arrived.
        if let Some(mac) = lookup(ip) {
            return Ok(mac);
        }

        // Brief spin.
        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }

    Err(KernelError::TimedOut)
}
