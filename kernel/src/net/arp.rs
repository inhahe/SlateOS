//! ARP (Address Resolution Protocol) implementation.
//!
//! Handles ARP request/reply for IPv4-over-Ethernet (RFC 826).
//! Maintains a simple ARP cache mapping IP addresses to MAC addresses.
//! Cache entries expire after 5 minutes (matching Linux defaults) to
//! handle MAC address changes from device replacement or DHCP renewal.
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
//!
//! Note: per-namespace firewall rules are already implemented in
//! `net::firewall` (see `ns_*` functions).  Per-namespace ARP is the
//! remaining piece for full network namespace isolation.

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

/// ARP cache entry lifetime in nanoseconds (5 minutes).
///
/// After this duration, entries are treated as stale and evicted on
/// the next lookup or insert.  A fresh ARP request is sent to
/// re-resolve the address.  5 minutes matches the typical default
/// on Linux and most other OS implementations.
const ARP_ENTRY_LIFETIME_NS: u64 = 300_000_000_000; // 5 min

// ---------------------------------------------------------------------------
// ARP cache
// ---------------------------------------------------------------------------

/// One entry in the ARP cache.
#[derive(Debug, Clone, Copy)]
struct ArpEntry {
    ip: Ipv4Addr,
    mac: MacAddress,
    valid: bool,
    /// Timestamp (monotonic ns) when this entry was last updated.
    /// Entries older than [`ARP_ENTRY_LIFETIME_NS`] are considered stale.
    updated_ns: u64,
}

impl ArpEntry {
    /// Check if this entry is still fresh (not expired).
    fn is_fresh(&self, now_ns: u64) -> bool {
        self.valid && now_ns.wrapping_sub(self.updated_ns) < ARP_ENTRY_LIFETIME_NS
    }
}

/// The ARP cache.
static ARP_CACHE: Mutex<[ArpEntry; ARP_CACHE_SIZE]> = Mutex::new(
    [ArpEntry {
        ip: Ipv4Addr::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        valid: false,
        updated_ns: 0,
    }; ARP_CACHE_SIZE]
);

/// Look up a MAC address in the ARP cache.
///
/// Returns `None` if the entry doesn't exist or has expired.
pub fn lookup(ip: Ipv4Addr) -> Option<MacAddress> {
    let now = crate::hrtimer::now_ns();
    let cache = ARP_CACHE.lock();
    for entry in cache.iter() {
        if entry.ip == ip && entry.is_fresh(now) {
            return Some(entry.mac);
        }
    }
    None
}

/// Insert or update an ARP cache entry.
fn cache_insert(ip: Ipv4Addr, mac: MacAddress) {
    let now = crate::hrtimer::now_ns();
    let mut cache = ARP_CACHE.lock();

    // Check if already present — update.
    for entry in cache.iter_mut() {
        if entry.valid && entry.ip == ip {
            entry.mac = mac;
            entry.updated_ns = now;
            return;
        }
    }

    // Find an empty or expired slot.
    for entry in cache.iter_mut() {
        if !entry.is_fresh(now) {
            *entry = ArpEntry { ip, mac, valid: true, updated_ns: now };
            return;
        }
    }

    // Cache full with all fresh entries — evict the oldest.
    let mut oldest_idx = 0;
    let mut oldest_time = u64::MAX;
    for (i, entry) in cache.iter().enumerate() {
        if entry.updated_ns < oldest_time {
            oldest_time = entry.updated_ns;
            oldest_idx = i;
        }
    }
    if let Some(slot) = cache.get_mut(oldest_idx) {
        *slot = ArpEntry { ip, mac, valid: true, updated_ns: now };
    }
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

/// Send a gratuitous ARP announcement.
///
/// A gratuitous ARP is an ARP request where both sender and target
/// protocol addresses are our own IP.  This serves two purposes:
///
/// 1. **Cache update**: Neighbors that have a stale ARP entry for our IP
///    (e.g., after DHCP renewal with a new MAC) will update their cache
///    from the sender fields.
/// 2. **Duplicate detection**: If another host on the LAN has the same IP,
///    it will respond with its own ARP reply, alerting us to the conflict.
///
/// Sent automatically when the interface is configured via DHCP or
/// manual configuration.
pub fn send_gratuitous() -> KernelResult<()> {
    let our_mac = interface::mac();
    let our_ip = interface::ip();

    if our_ip.is_unspecified() {
        return Ok(()); // No IP configured — nothing to announce.
    }

    // Gratuitous ARP: request for our own IP, broadcast target MAC.
    let arp_data = build_arp(
        ARP_REQUEST,
        &our_mac,
        our_ip,
        &MacAddress([0; 6]),
        our_ip, // Target IP = our IP (gratuitous).
    );
    let frame = ethernet::build_frame(&BROADCAST_MAC, &our_mac, ETHERTYPE_ARP, &arp_data);

    super::send_frame(&frame)?;
    crate::serial_println!("[arp] Gratuitous ARP sent for {}", our_ip);

    Ok(())
}
