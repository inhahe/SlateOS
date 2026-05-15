//! ICMPv6 (Internet Control Message Protocol for IPv6) implementation.
//!
//! Implements RFC 4443 (ICMPv6) and RFC 4861 (NDP) support:
//!
//! - **Type 1**: Destination Unreachable
//! - **Type 2**: Packet Too Big
//! - **Type 3**: Time Exceeded
//! - **Type 128**: Echo Request — generates Echo Reply
//! - **Type 129**: Echo Reply — matched to outstanding pings
//! - **Type 133**: Router Solicitation — sent to discover routers
//! - **Type 134**: Router Advertisement — processed for SLAAC
//! - **Type 135**: Neighbor Solicitation (NDP)
//! - **Type 136**: Neighbor Advertisement (NDP)
//!
//! ## ICMPv6 message format
//!
//! ```text
//! Type (8) | Code (8) | Checksum (16)
//! Message body ...
//! ```
//!
//! All ICMPv6 checksums use the IPv6 pseudo-header (RFC 8200 section 8.1).
//!
//! ## NDP Neighbor Cache
//!
//! Maintains a small cache mapping IPv6 link-local addresses to MAC
//! addresses, populated by Neighbor Advertisement messages.  Limited to
//! 32 entries with LRU eviction.
//!
//! ## SLAAC — Stateless Address Autoconfiguration (RFC 4862)
//!
//! Processes Router Advertisements to auto-configure global IPv6 addresses.
//! When an RA contains a Prefix Information option with the Autonomous (A)
//! flag set, a global address is constructed from the prefix + modified
//! EUI-64 interface ID.  Up to 4 global addresses can be configured.
//! Also extracts RDNSS (RFC 8106) for DNS server discovery.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::virtio::net::MacAddress;

use super::ipv6::{self, Ipv6Addr, Ipv6Packet, NH_ICMPV6};

// ---------------------------------------------------------------------------
// ICMPv6 types
// ---------------------------------------------------------------------------

/// Destination Unreachable.
const ICMPV6_DEST_UNREACHABLE: u8 = 1;
/// Packet Too Big.
const ICMPV6_PACKET_TOO_BIG: u8 = 2;
/// Time Exceeded.
const ICMPV6_TIME_EXCEEDED: u8 = 3;
/// Echo Request.
const ICMPV6_ECHO_REQUEST: u8 = 128;
/// Echo Reply.
const ICMPV6_ECHO_REPLY: u8 = 129;
/// Router Solicitation (NDP).
const ICMPV6_ROUTER_SOLICITATION: u8 = 133;
/// Router Advertisement (NDP).
const ICMPV6_ROUTER_ADVERTISEMENT: u8 = 134;
/// Neighbor Solicitation (NDP).
const ICMPV6_NEIGHBOR_SOLICITATION: u8 = 135;
/// Neighbor Advertisement (NDP).
const ICMPV6_NEIGHBOR_ADVERTISEMENT: u8 = 136;

/// ICMPv6 header minimum size (type + code + checksum).
const ICMPV6_HEADER_SIZE: usize = 4;

/// Ping6 identifier (fixed for our kernel).
const PING6_ID: u16 = 0x6789;

/// Maximum outstanding ping6 entries.
const MAX_OUTSTANDING: usize = 16;

// ---------------------------------------------------------------------------
// ICMPv6 Echo tracking
// ---------------------------------------------------------------------------

/// Next sequence number for ping6.
#[allow(dead_code)] // Used by ping6() public API.
static PING6_SEQ: AtomicU16 = AtomicU16::new(1);

/// Last received ping6 reply sequence number.
static LAST_REPLY_SEQ: AtomicU16 = AtomicU16::new(0);

/// Last measured ping6 RTT in nanoseconds.
static LAST_RTT_NS: AtomicU64 = AtomicU64::new(0);

/// An outstanding ping6 awaiting a reply.
#[derive(Debug, Clone, Copy)]
struct Ping6Slot {
    active: bool,
    seq: u16,
    sent_ns: u64,
}

impl Ping6Slot {
    const fn empty() -> Self {
        Self { active: false, seq: 0, sent_ns: 0 }
    }
}

/// Table of outstanding ping6 entries.
static OUTSTANDING: Mutex<[Ping6Slot; MAX_OUTSTANDING]> =
    Mutex::new([Ping6Slot::empty(); MAX_OUTSTANDING]);

/// Record an outstanding ping6.
fn record_outstanding(seq: u16) {
    let now = crate::hrtimer::now_ns();
    let mut table = OUTSTANDING.lock();

    for slot in table.iter_mut() {
        if !slot.active {
            *slot = Ping6Slot { active: true, seq, sent_ns: now };
            return;
        }
    }

    // All full — evict the oldest.
    let mut oldest_idx = 0;
    let mut oldest_time = u64::MAX;
    for (i, slot) in table.iter().enumerate() {
        if slot.sent_ns < oldest_time {
            oldest_time = slot.sent_ns;
            oldest_idx = i;
        }
    }
    if let Some(slot) = table.get_mut(oldest_idx) {
        *slot = Ping6Slot { active: true, seq, sent_ns: now };
    }
}

/// Match a reply to an outstanding ping6 and return the RTT in nanoseconds.
fn match_outstanding(seq: u16) -> Option<u64> {
    let now = crate::hrtimer::now_ns();
    let mut table = OUTSTANDING.lock();
    for slot in table.iter_mut() {
        if slot.active && slot.seq == seq {
            let rtt = now.wrapping_sub(slot.sent_ns);
            slot.active = false;
            return Some(rtt);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// NDP Neighbor Cache
// ---------------------------------------------------------------------------

/// Maximum neighbor cache entries.
const MAX_NEIGHBOR_ENTRIES: usize = 32;

/// A neighbor cache entry (IPv6 address → MAC address).
#[derive(Debug, Clone, Copy)]
struct NeighborEntry {
    /// Whether this entry is in use.
    active: bool,
    /// IPv6 address (typically link-local).
    ip: Ipv6Addr,
    /// Resolved MAC address.
    mac: MacAddress,
    /// Timestamp of last update (ns), for LRU eviction.
    last_update_ns: u64,
}

impl NeighborEntry {
    const fn empty() -> Self {
        Self {
            active: false,
            ip: Ipv6Addr::UNSPECIFIED,
            mac: MacAddress([0; 6]),
            last_update_ns: 0,
        }
    }
}

/// The neighbor cache.
static NEIGHBOR_CACHE: Mutex<[NeighborEntry; MAX_NEIGHBOR_ENTRIES]> =
    Mutex::new([NeighborEntry::empty(); MAX_NEIGHBOR_ENTRIES]);

/// Look up a MAC address in the neighbor cache.
pub fn neighbor_lookup(ip: &Ipv6Addr) -> Option<MacAddress> {
    let cache = NEIGHBOR_CACHE.lock();
    for entry in cache.iter() {
        if entry.active && entry.ip == *ip {
            return Some(entry.mac);
        }
    }
    None
}

/// Insert or update a neighbor cache entry.
fn neighbor_update(ip: Ipv6Addr, mac: MacAddress) {
    let now = crate::hrtimer::now_ns();
    let mut cache = NEIGHBOR_CACHE.lock();

    // Update existing entry.
    for entry in cache.iter_mut() {
        if entry.active && entry.ip == ip {
            entry.mac = mac;
            entry.last_update_ns = now;
            return;
        }
    }

    // Find an empty slot.
    for entry in cache.iter_mut() {
        if !entry.active {
            *entry = NeighborEntry { active: true, ip, mac, last_update_ns: now };
            return;
        }
    }

    // All full — evict the oldest (LRU).
    let mut oldest_idx = 0;
    let mut oldest_time = u64::MAX;
    for (i, entry) in cache.iter().enumerate() {
        if entry.last_update_ns < oldest_time {
            oldest_time = entry.last_update_ns;
            oldest_idx = i;
        }
    }
    if let Some(entry) = cache.get_mut(oldest_idx) {
        *entry = NeighborEntry { active: true, ip, mac, last_update_ns: now };
    }
}

/// Return the number of active entries in the neighbor cache (for diagnostics).
#[allow(dead_code)] // Public API for network diagnostics.
pub fn neighbor_cache_count() -> usize {
    let cache = NEIGHBOR_CACHE.lock();
    cache.iter().filter(|e| e.active).count()
}

// ---------------------------------------------------------------------------
// SLAAC — Stateless Address Autoconfiguration (RFC 4862)
// ---------------------------------------------------------------------------

/// NDP option type: Prefix Information (RFC 4861 §4.6.2).
const NDP_OPT_PREFIX_INFO: u8 = 3;

/// NDP option type: RDNSS — Recursive DNS Server (RFC 8106 §5.1).
const NDP_OPT_RDNSS: u8 = 25;

/// Maximum number of SLAAC-configured global addresses.
const MAX_SLAAC_ADDRS: usize = 4;

/// Maximum number of RDNSS entries from Router Advertisements.
const MAX_RDNSS: usize = 2;

/// A SLAAC-configured global IPv6 address.
#[derive(Debug, Clone, Copy)]
struct SlaacAddr {
    /// Whether this slot is in use.
    active: bool,
    /// The configured global address.
    addr: Ipv6Addr,
    /// Prefix length (typically 64).
    prefix_len: u8,
    /// Valid lifetime (seconds), 0xFFFFFFFF = infinite.
    valid_lifetime: u32,
    /// Preferred lifetime (seconds), 0xFFFFFFFF = infinite.
    preferred_lifetime: u32,
    /// When this address was configured (monotonic ns).
    configured_ns: u64,
}

impl SlaacAddr {
    const fn empty() -> Self {
        Self {
            active: false,
            addr: Ipv6Addr::UNSPECIFIED,
            prefix_len: 0,
            valid_lifetime: 0,
            preferred_lifetime: 0,
            configured_ns: 0,
        }
    }

    /// Whether this address has expired based on valid_lifetime.
    fn is_expired(&self, now_ns: u64) -> bool {
        if self.valid_lifetime == 0xFFFF_FFFF {
            return false; // Infinite lifetime.
        }
        let elapsed_s = now_ns.wrapping_sub(self.configured_ns) / 1_000_000_000;
        elapsed_s > u64::from(self.valid_lifetime)
    }
}

/// Parsed Prefix Information option from a Router Advertisement.
#[derive(Debug, Clone, Copy)]
struct PrefixInfo {
    /// Prefix length (bits).
    prefix_len: u8,
    /// On-link flag (L).
    #[allow(dead_code)] // Stored for completeness.
    on_link: bool,
    /// Autonomous flag (A) — allows SLAAC.
    autonomous: bool,
    /// Valid lifetime (seconds).
    valid_lifetime: u32,
    /// Preferred lifetime (seconds).
    preferred_lifetime: u32,
    /// The prefix (128 bits, only prefix_len bits significant).
    prefix: Ipv6Addr,
}

/// Parsed RDNSS option from a Router Advertisement.
#[derive(Debug, Clone, Copy)]
struct RdnssInfo {
    /// Lifetime (seconds), 0 means remove.
    #[allow(dead_code)]
    lifetime: u32,
    /// DNS server IPv6 address.
    addr: Ipv6Addr,
}

/// SLAAC state: configured addresses and RDNSS servers.
struct SlaacState {
    addrs: [SlaacAddr; MAX_SLAAC_ADDRS],
    addr_count: usize,
    /// RDNSS servers from Router Advertisements.
    rdnss: [Ipv6Addr; MAX_RDNSS],
    rdnss_count: usize,
    /// Router's link-local address (from RA source).
    router_ll: Ipv6Addr,
    /// Whether we have received at least one RA.
    ra_received: bool,
}

impl SlaacState {
    const fn new() -> Self {
        Self {
            addrs: [SlaacAddr::empty(); MAX_SLAAC_ADDRS],
            addr_count: 0,
            rdnss: [Ipv6Addr::UNSPECIFIED; MAX_RDNSS],
            rdnss_count: 0,
            router_ll: Ipv6Addr::UNSPECIFIED,
            ra_received: false,
        }
    }
}

static SLAAC_STATE: Mutex<SlaacState> = Mutex::new(SlaacState::new());

/// Return the first active SLAAC global address (if any).
#[allow(dead_code)] // Public API — called by kshell and net diagnostics.
pub fn slaac_global_addr() -> Option<Ipv6Addr> {
    let state = SLAAC_STATE.lock();
    let now = crate::hrtimer::now_ns();
    for entry in &state.addrs {
        if entry.active && !entry.is_expired(now) {
            return Some(entry.addr);
        }
    }
    None
}

/// Return all active SLAAC addresses (for diagnostics).
#[allow(dead_code)] // Public API — called by kshell net/ifconfig.
pub fn slaac_addresses() -> ([(Ipv6Addr, u8); MAX_SLAAC_ADDRS], usize) {
    let state = SLAAC_STATE.lock();
    let now = crate::hrtimer::now_ns();
    let mut result = [(Ipv6Addr::UNSPECIFIED, 0u8); MAX_SLAAC_ADDRS];
    let mut count = 0;
    for entry in &state.addrs {
        if entry.active && !entry.is_expired(now) {
            if let Some(slot) = result.get_mut(count) {
                *slot = (entry.addr, entry.prefix_len);
                count = count.wrapping_add(1);
            }
        }
    }
    (result, count)
}

/// Return the first RDNSS server from RA (if any).
#[allow(dead_code)] // Public API for future DNS-over-IPv6.
pub fn slaac_rdnss() -> Option<Ipv6Addr> {
    let state = SLAAC_STATE.lock();
    if state.rdnss_count > 0 {
        return Some(state.rdnss[0]);
    }
    None
}

/// Check whether the given address is one of our SLAAC global addresses.
fn is_our_slaac_addr(addr: &Ipv6Addr) -> bool {
    let state = SLAAC_STATE.lock();
    let now = crate::hrtimer::now_ns();
    for entry in &state.addrs {
        if entry.active && !entry.is_expired(now) && entry.addr == *addr {
            return true;
        }
    }
    false
}

/// Whether a Router Advertisement has been received.
#[allow(dead_code)] // Public API for network status.
pub fn ra_received() -> bool {
    SLAAC_STATE.lock().ra_received
}

/// The router's link-local address from the most recent RA.
#[allow(dead_code)] // Public API.
pub fn slaac_router() -> Ipv6Addr {
    SLAAC_STATE.lock().router_ll
}

// ---------------------------------------------------------------------------
// ICMPv6 checksum helpers
// ---------------------------------------------------------------------------

/// Verify the ICMPv6 checksum using the IPv6 pseudo-header.
fn verify_checksum(src: &Ipv6Addr, dst: &Ipv6Addr, data: &[u8]) -> bool {
    ipv6::verify_transport_checksum(src, dst, NH_ICMPV6, data)
}

/// Build an ICMPv6 message with a computed checksum.
///
/// `msg` must have the checksum field (bytes 2-3) set to zero.
/// Returns the message with the checksum filled in.
fn finalize_checksum(src: &Ipv6Addr, dst: &Ipv6Addr, mut msg: Vec<u8>) -> Vec<u8> {
    // Ensure checksum field is zero before computing.
    if msg.len() >= 4 {
        msg[2] = 0;
        msg[3] = 0;
    }
    let cksum = ipv6::compute_transport_checksum(src, dst, NH_ICMPV6, &msg);
    if msg.len() >= 4 {
        msg[2] = (cksum >> 8) as u8;
        msg[3] = cksum as u8;
    }
    msg
}

// ---------------------------------------------------------------------------
// ICMPv6 processing
// ---------------------------------------------------------------------------

/// Process an incoming ICMPv6 packet.
pub fn process_icmpv6(ip_packet: &Ipv6Packet<'_>) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < ICMPV6_HEADER_SIZE {
        return Ok(());
    }

    // Verify checksum (mandatory for all ICMPv6 per RFC 4443).
    if !verify_checksum(&ip_packet.src, &ip_packet.dst, data) {
        crate::serial_println!(
            "[icmpv6] Dropped packet from {} -- bad checksum",
            ip_packet.src
        );
        return Ok(());
    }

    let icmpv6_type = data[0];
    let _code = data[1];

    match icmpv6_type {
        ICMPV6_ECHO_REQUEST => {
            handle_echo_request(ip_packet, data)?;
        }
        ICMPV6_ECHO_REPLY => {
            handle_echo_reply(ip_packet, data);
        }
        ICMPV6_ROUTER_SOLICITATION => {
            // We're a host, not a router — ignore RS.
        }
        ICMPV6_ROUTER_ADVERTISEMENT => {
            handle_router_advertisement(ip_packet, data);
        }
        ICMPV6_NEIGHBOR_SOLICITATION => {
            handle_neighbor_solicitation(ip_packet, data)?;
        }
        ICMPV6_NEIGHBOR_ADVERTISEMENT => {
            handle_neighbor_advertisement(data);
        }
        ICMPV6_DEST_UNREACHABLE => {
            crate::serial_println!(
                "[icmpv6] Destination unreachable from {} (code {})",
                ip_packet.src, _code
            );
        }
        ICMPV6_PACKET_TOO_BIG => {
            if data.len() >= 8 {
                let mtu = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                crate::serial_println!(
                    "[icmpv6] Packet Too Big from {}: MTU={}",
                    ip_packet.src, mtu
                );
            }
        }
        ICMPV6_TIME_EXCEEDED => {
            crate::serial_println!(
                "[icmpv6] Time Exceeded from {} (code {})",
                ip_packet.src, _code
            );
        }
        _ => {
            // Unknown type — silently ignore.
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Echo Request/Reply handling
// ---------------------------------------------------------------------------

/// Handle an incoming Echo Request — send an Echo Reply.
#[allow(clippy::arithmetic_side_effects)]
fn handle_echo_request(ip_packet: &Ipv6Packet<'_>, data: &[u8]) -> KernelResult<()> {
    if data.len() < 8 {
        return Ok(());
    }

    // Build Echo Reply: same data but type changed to 129.
    let mut reply = Vec::from(data);
    reply[0] = ICMPV6_ECHO_REPLY;

    // Our source address (link-local from our MAC).
    let our_mac = super::interface::mac();
    let our_ip = Ipv6Addr::from_mac_link_local(&our_mac);

    let reply = finalize_checksum(&our_ip, &ip_packet.src, reply);
    ipv6::send_raw(our_ip, ip_packet.src, NH_ICMPV6, 64, &reply)
}

/// Handle an incoming Echo Reply — match to outstanding ping6.
fn handle_echo_reply(_ip_packet: &Ipv6Packet<'_>, data: &[u8]) {
    if data.len() < 8 {
        return;
    }

    let id = u16::from_be_bytes([data[4], data[5]]);
    let seq = u16::from_be_bytes([data[6], data[7]]);

    if id != PING6_ID {
        return; // Not our ping6.
    }

    if let Some(rtt_ns) = match_outstanding(seq) {
        LAST_RTT_NS.store(rtt_ns, Ordering::Release);

        #[allow(clippy::arithmetic_side_effects)]
        let rtt_us = rtt_ns / 1000;
        if rtt_us >= 1000 {
            #[allow(clippy::arithmetic_side_effects)]
            let rtt_ms = rtt_us / 1000;
            crate::serial_println!(
                "[icmpv6] Echo reply from {} seq={} rtt={} ms",
                _ip_packet.src, seq, rtt_ms
            );
        } else {
            crate::serial_println!(
                "[icmpv6] Echo reply from {} seq={} rtt={} us",
                _ip_packet.src, seq, rtt_us
            );
        }
    } else {
        crate::serial_println!(
            "[icmpv6] Echo reply from {} seq={}",
            _ip_packet.src, seq
        );
    }

    LAST_REPLY_SEQ.store(seq, Ordering::Release);
}

// ---------------------------------------------------------------------------
// NDP: Neighbor Solicitation / Advertisement
// ---------------------------------------------------------------------------

/// Handle an incoming Neighbor Solicitation (type 135).
///
/// NDP NS format (after ICMPv6 header):
/// - Reserved (4 bytes)
/// - Target Address (16 bytes)
/// - Options (variable): Source Link-Layer Address (type 1)
///
/// If the target is our address, we respond with a Neighbor Advertisement.
#[allow(clippy::arithmetic_side_effects)]
fn handle_neighbor_solicitation(ip_packet: &Ipv6Packet<'_>, data: &[u8]) -> KernelResult<()> {
    // Minimum: ICMPv6 header (4) + reserved (4) + target (16) = 24 bytes.
    if data.len() < 24 {
        return Ok(());
    }

    let mut target = [0u8; 16];
    target.copy_from_slice(&data[8..24]);
    let target_addr = Ipv6Addr(target);

    // Check if we're the target (link-local or any SLAAC global address).
    let our_mac = super::interface::mac();
    let our_ip = Ipv6Addr::from_mac_link_local(&our_mac);
    let is_ours = target_addr == our_ip || is_our_slaac_addr(&target_addr);
    if !is_ours {
        return Ok(()); // Not for us.
    }

    // Extract the source link-layer address from options (if present).
    // This lets us update our neighbor cache with the solicitor's MAC.
    if let Some(src_mac) = parse_ndp_option_slla(&data[24..]) {
        if !ip_packet.src.is_unspecified() {
            neighbor_update(ip_packet.src, src_mac);
        }
    }

    crate::serial_println!(
        "[icmpv6] Neighbor Solicitation from {} for {}",
        ip_packet.src, target_addr
    );

    // Send Neighbor Advertisement.
    send_neighbor_advertisement(ip_packet.src, our_ip, &our_mac)
}

/// Handle an incoming Neighbor Advertisement (type 136).
///
/// NDP NA format (after ICMPv6 header):
/// - R|S|O flags + reserved (4 bytes)
/// - Target Address (16 bytes)
/// - Options: Target Link-Layer Address (type 2)
fn handle_neighbor_advertisement(data: &[u8]) {
    // Minimum: ICMPv6 header (4) + flags (4) + target (16) = 24 bytes.
    if data.len() < 24 {
        return;
    }

    let mut target = [0u8; 16];
    target.copy_from_slice(&data[8..24]);
    let target_addr = Ipv6Addr(target);

    // Extract the target link-layer address from options.
    if let Some(target_mac) = parse_ndp_option_tlla(&data[24..]) {
        neighbor_update(target_addr, target_mac);
        crate::serial_println!(
            "[icmpv6] Neighbor Advertisement: {} is at {}",
            target_addr, target_mac
        );
    }
}

/// Parse the Source Link-Layer Address option (type 1) from NDP options.
///
/// NDP option format: Type (1 byte) + Length (1 byte, in 8-octet units) + data.
/// For Source/Target Link-Layer Address: length = 1 (8 bytes: type+len+6 MAC).
fn parse_ndp_option_slla(options: &[u8]) -> Option<MacAddress> {
    parse_ndp_lla_option(options, 1)
}

/// Parse the Target Link-Layer Address option (type 2) from NDP options.
fn parse_ndp_option_tlla(options: &[u8]) -> Option<MacAddress> {
    parse_ndp_lla_option(options, 2)
}

/// Parse a link-layer address option of the given type from NDP options.
#[allow(clippy::arithmetic_side_effects)]
fn parse_ndp_lla_option(mut options: &[u8], target_type: u8) -> Option<MacAddress> {
    while options.len() >= 2 {
        let opt_type = options[0];
        let opt_len = options[1] as usize;

        // Length is in 8-octet units.  0 is invalid (would cause infinite loop).
        if opt_len == 0 {
            return None;
        }

        let total = opt_len * 8;
        if options.len() < total {
            return None;
        }

        if opt_type == target_type && total >= 8 {
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&options[2..8]);
            return Some(MacAddress(mac));
        }

        options = &options[total..];
    }
    None
}

// ---------------------------------------------------------------------------
// NDP: Sending Neighbor Solicitation / Advertisement
// ---------------------------------------------------------------------------

/// Send a Neighbor Solicitation for the given target address.
///
/// Uses the solicited-node multicast address as the IPv6 destination
/// and includes our MAC as the Source Link-Layer Address option.
#[allow(dead_code)] // Public API — called by ipv6::send() for NDP resolution.
#[allow(clippy::arithmetic_side_effects)]
pub fn send_neighbor_solicitation(target: Ipv6Addr) -> KernelResult<()> {
    let our_mac = super::interface::mac();
    let our_ip = Ipv6Addr::from_mac_link_local(&our_mac);
    let dst = target.solicited_node_multicast();

    // Build the NS message:
    // Type (135) + Code (0) + Checksum (2) + Reserved (4) + Target (16) + SLLA option (8) = 32
    let mut msg = Vec::with_capacity(32);
    msg.push(ICMPV6_NEIGHBOR_SOLICITATION); // Type
    msg.push(0);                             // Code
    msg.extend_from_slice(&[0, 0]);          // Checksum placeholder
    msg.extend_from_slice(&[0, 0, 0, 0]);    // Reserved
    msg.extend_from_slice(&target.0);         // Target Address

    // Source Link-Layer Address option (type 1, length 1 = 8 bytes).
    msg.push(1); // Option type: Source LLA
    msg.push(1); // Length: 1 (8 bytes)
    msg.extend_from_slice(&our_mac.0);

    let msg = finalize_checksum(&our_ip, &dst, msg);
    ipv6::send_raw(our_ip, dst, NH_ICMPV6, 255, &msg)
}

/// Send a Neighbor Advertisement in response to a solicitation.
///
/// Flags: Solicited (S) + Override (O) set, Router (R) clear.
#[allow(clippy::arithmetic_side_effects)]
fn send_neighbor_advertisement(
    dst: Ipv6Addr,
    our_ip: Ipv6Addr,
    our_mac: &MacAddress,
) -> KernelResult<()> {
    // Build the NA message:
    // Type (136) + Code (0) + Checksum (2) + Flags+Reserved (4) + Target (16) + TLLA option (8) = 32
    let mut msg = Vec::with_capacity(32);
    msg.push(ICMPV6_NEIGHBOR_ADVERTISEMENT); // Type
    msg.push(0);                              // Code
    msg.extend_from_slice(&[0, 0]);           // Checksum placeholder

    // Flags: R=0, S=1, O=1, rest reserved (0).
    // Byte layout: R(1) S(1) O(1) Reserved(29 bits).
    // S=1, O=1 → 0x60 in the first byte, rest 0.
    msg.push(0x60);
    msg.extend_from_slice(&[0, 0, 0]); // Remaining 3 bytes of flags+reserved

    msg.extend_from_slice(&our_ip.0); // Target Address (our address)

    // Target Link-Layer Address option (type 2, length 1 = 8 bytes).
    msg.push(2); // Option type: Target LLA
    msg.push(1); // Length: 1 (8 bytes)
    msg.extend_from_slice(&our_mac.0);

    let msg = finalize_checksum(&our_ip, &dst, msg);
    ipv6::send_raw(our_ip, dst, NH_ICMPV6, 255, &msg)
}

// ---------------------------------------------------------------------------
// NDP: Router Solicitation / Advertisement (RFC 4861 §4.1–4.2)
// ---------------------------------------------------------------------------

/// Send a Router Solicitation (ICMPv6 type 133).
///
/// Sent to the all-routers multicast address (ff02::2) to prompt
/// routers to send a Router Advertisement.  This is the first step
/// in SLAAC (RFC 4862).
///
/// RS format (after ICMPv6 header):
/// - Reserved (4 bytes)
/// - Options: Source Link-Layer Address (type 1)
#[allow(dead_code)] // Public API — called from kshell or net::init on IPv6 network.
#[allow(clippy::arithmetic_side_effects)]
pub fn send_router_solicitation() -> KernelResult<()> {
    let our_mac = super::interface::mac();
    let our_ip = Ipv6Addr::from_mac_link_local(&our_mac);
    let dst = Ipv6Addr::ALL_ROUTERS_LINK_LOCAL; // ff02::2

    // Build RS message:
    // Type (133) + Code (0) + Checksum (2) + Reserved (4) + SLLA option (8) = 16
    let mut msg = Vec::with_capacity(16);
    msg.push(ICMPV6_ROUTER_SOLICITATION); // Type
    msg.push(0);                           // Code
    msg.extend_from_slice(&[0, 0]);        // Checksum placeholder
    msg.extend_from_slice(&[0, 0, 0, 0]);  // Reserved

    // Source Link-Layer Address option (type 1, length 1 = 8 bytes).
    msg.push(1); // Option type: Source LLA
    msg.push(1); // Length: 1 (8 bytes)
    msg.extend_from_slice(&our_mac.0);

    let msg = finalize_checksum(&our_ip, &dst, msg);

    crate::serial_println!(
        "[icmpv6] Sending Router Solicitation from {} to {}",
        our_ip, dst
    );

    ipv6::send_raw(our_ip, dst, NH_ICMPV6, 255, &msg)
}

/// Handle an incoming Router Advertisement (type 134).
///
/// RA format (after ICMPv6 header):
/// - Cur Hop Limit (1 byte)
/// - M|O flags + reserved (1 byte)
/// - Router Lifetime (2 bytes)
/// - Reachable Time (4 bytes)
/// - Retrans Timer (4 bytes)
/// - Options: Prefix Information (type 3), RDNSS (type 25), etc.
///
/// Extracts prefix information and configures global addresses via
/// SLAAC (RFC 4862).  Also extracts RDNSS for DNS server discovery.
#[allow(clippy::arithmetic_side_effects)]
fn handle_router_advertisement(ip_packet: &Ipv6Packet<'_>, data: &[u8]) {
    // RA must come from a link-local address.
    if !ip_packet.src.is_link_local() {
        crate::serial_println!(
            "[icmpv6] RA from non-link-local {} — ignored",
            ip_packet.src
        );
        return;
    }

    // Minimum RA: ICMPv6 header (4) + cur_hop (1) + flags (1) +
    // router_lifetime (2) + reachable (4) + retrans (4) = 16 bytes.
    if data.len() < 16 {
        return;
    }

    let _cur_hop_limit = data[4];
    let flags = data[5];
    let _managed = (flags & 0x80) != 0; // M flag — managed (DHCPv6).
    let _other = (flags & 0x40) != 0;   // O flag — other config (DHCPv6 for options).
    let router_lifetime = u16::from_be_bytes([data[6], data[7]]);
    let _reachable_time = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let _retrans_timer = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);

    crate::serial_println!(
        "[icmpv6] Router Advertisement from {}: hop={}, lifetime={}s, flags=M:{} O:{}",
        ip_packet.src, _cur_hop_limit, router_lifetime,
        if _managed { "1" } else { "0" },
        if _other { "1" } else { "0" }
    );

    // Extract the router's MAC from Source LLA option.
    if let Some(router_mac) = parse_ndp_option_slla(&data[16..]) {
        neighbor_update(ip_packet.src, router_mac);
    }

    // Parse RA options for Prefix Information and RDNSS.
    let mut prefixes: [Option<PrefixInfo>; 4] = [None; 4];
    let mut prefix_count = 0usize;
    let mut rdnss_addrs: [Option<RdnssInfo>; MAX_RDNSS] = [None; MAX_RDNSS];
    let mut rdnss_count = 0usize;

    parse_ra_options(
        &data[16..],
        &mut prefixes,
        &mut prefix_count,
        &mut rdnss_addrs,
        &mut rdnss_count,
    );

    // Configure addresses from Autonomous prefixes.
    let our_mac = super::interface::mac();
    let mut state = SLAAC_STATE.lock();
    state.ra_received = true;
    state.router_ll = ip_packet.src;

    for i in 0..prefix_count {
        if let Some(pi) = prefixes[i] {
            if !pi.autonomous {
                continue; // Not for SLAAC.
            }
            if pi.prefix_len != 64 {
                // SLAAC requires a /64 prefix (RFC 4862 §5.5.3).
                crate::serial_println!(
                    "[icmpv6] Prefix /{} not /64 — skipping SLAAC",
                    pi.prefix_len
                );
                continue;
            }
            if pi.prefix.is_link_local() {
                continue; // Don't SLAAC with link-local prefix.
            }

            // Build global address: prefix (64 bits) + EUI-64 interface ID (64 bits).
            let global = build_slaac_address(&pi.prefix, &our_mac);

            crate::serial_println!(
                "[icmpv6] SLAAC: {} (prefix /{}, valid={}s, preferred={}s)",
                global, pi.prefix_len, pi.valid_lifetime, pi.preferred_lifetime
            );

            // Insert into SLAAC state (update if same prefix, else add new).
            slaac_insert(
                &mut state, global, pi.prefix_len,
                pi.valid_lifetime, pi.preferred_lifetime,
            );
        }
    }

    // Store RDNSS servers.
    let mut rdnss_idx = 0usize;
    for i in 0..rdnss_count {
        if let Some(ri) = rdnss_addrs[i] {
            if rdnss_idx < MAX_RDNSS {
                state.rdnss[rdnss_idx] = ri.addr;
                rdnss_idx = rdnss_idx.wrapping_add(1);
                crate::serial_println!(
                    "[icmpv6] RDNSS: {}",
                    ri.addr
                );
            }
        }
    }
    state.rdnss_count = rdnss_idx;
}

/// Build a SLAAC global address from a /64 prefix and MAC address.
///
/// Uses modified EUI-64 (same as link-local generation) for the
/// interface identifier (low 64 bits).
fn build_slaac_address(prefix: &Ipv6Addr, mac: &MacAddress) -> Ipv6Addr {
    let mut addr = [0u8; 16];
    // Copy prefix (high 64 bits).
    addr[..8].copy_from_slice(&prefix.0[..8]);
    // Modified EUI-64 interface ID (low 64 bits).
    addr[8] = mac.0[0] ^ 0x02; // Flip U/L bit.
    addr[9] = mac.0[1];
    addr[10] = mac.0[2];
    addr[11] = 0xFF;
    addr[12] = 0xFE;
    addr[13] = mac.0[3];
    addr[14] = mac.0[4];
    addr[15] = mac.0[5];
    Ipv6Addr(addr)
}

/// Insert or update a SLAAC address in the state table.
fn slaac_insert(
    state: &mut SlaacState,
    addr: Ipv6Addr,
    prefix_len: u8,
    valid_lifetime: u32,
    preferred_lifetime: u32,
) {
    let now = crate::hrtimer::now_ns();

    // Update existing entry with same address.
    for entry in state.addrs.iter_mut() {
        if entry.active && entry.addr == addr {
            entry.valid_lifetime = valid_lifetime;
            entry.preferred_lifetime = preferred_lifetime;
            entry.configured_ns = now;
            return;
        }
    }

    // Find an empty or expired slot.
    for entry in state.addrs.iter_mut() {
        if !entry.active || entry.is_expired(now) {
            *entry = SlaacAddr {
                active: true,
                addr,
                prefix_len,
                valid_lifetime,
                preferred_lifetime,
                configured_ns: now,
            };
            if state.addr_count < MAX_SLAAC_ADDRS {
                state.addr_count = state.addr_count.wrapping_add(1);
            }
            return;
        }
    }

    // All slots full — log and skip.
    crate::serial_println!(
        "[icmpv6] SLAAC table full, cannot add {}",
        addr
    );
}

/// Parse Router Advertisement options.
///
/// Extracts Prefix Information (type 3) and RDNSS (type 25) options.
#[allow(clippy::arithmetic_side_effects)]
fn parse_ra_options(
    mut opts: &[u8],
    prefixes: &mut [Option<PrefixInfo>; 4],
    prefix_count: &mut usize,
    rdnss: &mut [Option<RdnssInfo>; MAX_RDNSS],
    rdnss_count: &mut usize,
) {
    while opts.len() >= 2 {
        let opt_type = opts[0];
        let opt_len = opts[1] as usize;

        // Length is in 8-octet units.  0 is invalid.
        if opt_len == 0 {
            return;
        }

        let total = opt_len * 8;
        if opts.len() < total {
            return;
        }

        match opt_type {
            NDP_OPT_PREFIX_INFO if total >= 32 => {
                // Prefix Information: 32 bytes total.
                // Offset 2: Prefix Length (1 byte)
                // Offset 3: L|A|Reserved flags (1 byte)
                // Offset 4: Valid Lifetime (4 bytes)
                // Offset 8: Preferred Lifetime (4 bytes)
                // Offset 12: Reserved2 (4 bytes)
                // Offset 16: Prefix (16 bytes)
                if *prefix_count < prefixes.len() {
                    let prefix_len = opts[2];
                    let flags = opts[3];
                    let on_link = (flags & 0x80) != 0;
                    let autonomous = (flags & 0x40) != 0;
                    let valid_lifetime = u32::from_be_bytes([
                        opts[4], opts[5], opts[6], opts[7],
                    ]);
                    let preferred_lifetime = u32::from_be_bytes([
                        opts[8], opts[9], opts[10], opts[11],
                    ]);
                    let mut prefix_bytes = [0u8; 16];
                    prefix_bytes.copy_from_slice(&opts[16..32]);

                    prefixes[*prefix_count] = Some(PrefixInfo {
                        prefix_len,
                        on_link,
                        autonomous,
                        valid_lifetime,
                        preferred_lifetime,
                        prefix: Ipv6Addr(prefix_bytes),
                    });
                    *prefix_count += 1;
                }
            }
            NDP_OPT_RDNSS if total >= 24 => {
                // RDNSS: minimum 24 bytes (header 8 + 1 address 16).
                // Offset 2: Reserved (2 bytes)
                // Offset 4: Lifetime (4 bytes)
                // Offset 8: DNS server addresses (16 bytes each)
                let lifetime = u32::from_be_bytes([
                    opts[4], opts[5], opts[6], opts[7],
                ]);
                // Number of addresses = (total - 8) / 16.
                let addr_bytes = total - 8;
                let num_addrs = addr_bytes / 16;
                for a in 0..num_addrs {
                    if *rdnss_count >= rdnss.len() {
                        break;
                    }
                    let off = 8 + a * 16;
                    if off + 16 <= total {
                        let mut addr_buf = [0u8; 16];
                        addr_buf.copy_from_slice(&opts[off..off + 16]);
                        rdnss[*rdnss_count] = Some(RdnssInfo {
                            lifetime,
                            addr: Ipv6Addr(addr_buf),
                        });
                        *rdnss_count += 1;
                    }
                }
            }
            _ => {} // Skip unknown options.
        }

        opts = &opts[total..];
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Send an ICMPv6 Echo Request (ping6) to the given address.
///
/// Returns the sequence number used.
#[allow(dead_code)] // Public API — will be used from kshell ping6 command.
#[allow(clippy::arithmetic_side_effects)]
pub fn ping6(dst: Ipv6Addr) -> KernelResult<u16> {
    let seq = PING6_SEQ.fetch_add(1, Ordering::Relaxed);
    let our_mac = super::interface::mac();
    let our_ip = Ipv6Addr::from_mac_link_local(&our_mac);

    let payload = b"ping6 from kernel!";
    let total = 8 + payload.len();
    let mut msg = Vec::with_capacity(total);
    msg.push(ICMPV6_ECHO_REQUEST); // Type
    msg.push(0);                    // Code
    msg.extend_from_slice(&[0, 0]); // Checksum placeholder
    msg.extend_from_slice(&PING6_ID.to_be_bytes()); // ID
    msg.extend_from_slice(&seq.to_be_bytes());       // Seq
    msg.extend_from_slice(payload);

    let msg = finalize_checksum(&our_ip, &dst, msg);
    record_outstanding(seq);
    ipv6::send_raw(our_ip, dst, NH_ICMPV6, 64, &msg)?;
    Ok(seq)
}

/// Wait for a ping6 reply with the given sequence number.
///
/// Returns `Some(rtt_ns)` on success, `None` on timeout.
#[allow(dead_code)] // Public API — will be used from kshell ping6 command.
pub fn wait_reply_rtt(seq: u16, timeout_polls: u32) -> Option<u64> {
    for _ in 0..timeout_polls {
        super::poll();

        if LAST_REPLY_SEQ.load(Ordering::Acquire) == seq {
            return Some(LAST_RTT_NS.load(Ordering::Acquire));
        }

        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// ICMPv6 unit tests — exercises echo request/reply building, checksum
/// verification, NDP option parsing, and neighbor cache operations.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[icmpv6] Running ICMPv6 self-test...");

    test_echo_request_checksum()?;
    test_echo_reply_type()?;
    test_checksum_verification()?;
    test_ndp_option_parsing()?;
    test_neighbor_cache()?;
    test_neighbor_solicitation_build()?;
    test_neighbor_advertisement_build()?;
    test_ping6_tracking()?;
    test_ra_option_parsing()?;
    test_slaac_address_build()?;
    test_slaac_state()?;

    crate::serial_println!("[icmpv6] ICMPv6 self-test PASSED (11 tests)");
    Ok(())
}

/// Test that a built Echo Request has a valid checksum.
#[allow(clippy::arithmetic_side_effects)]
fn test_echo_request_checksum() -> KernelResult<()> {
    let src = Ipv6Addr::LOOPBACK;
    let dst = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

    let payload = b"test echo";
    let total = 8 + payload.len();
    let mut msg = Vec::with_capacity(total);
    msg.push(ICMPV6_ECHO_REQUEST);
    msg.push(0);
    msg.extend_from_slice(&[0, 0]); // Checksum placeholder
    msg.extend_from_slice(&0x1234u16.to_be_bytes());
    msg.extend_from_slice(&0x0001u16.to_be_bytes());
    msg.extend_from_slice(payload);

    let msg = finalize_checksum(&src, &dst, msg);

    if msg[0] != ICMPV6_ECHO_REQUEST {
        crate::serial_println!("[icmpv6]   FAIL: type = {}", msg[0]);
        return Err(KernelError::InternalError);
    }

    // Verify checksum.
    if !verify_checksum(&src, &dst, &msg) {
        crate::serial_println!("[icmpv6]   FAIL: echo request bad checksum");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   echo request checksum: OK");
    Ok(())
}

/// Test that changing type to Echo Reply produces valid checksum after recompute.
#[allow(clippy::arithmetic_side_effects)]
fn test_echo_reply_type() -> KernelResult<()> {
    let src = Ipv6Addr::LOOPBACK;
    let dst = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);

    let mut msg = Vec::with_capacity(12);
    msg.push(ICMPV6_ECHO_REQUEST);
    msg.push(0);
    msg.extend_from_slice(&[0, 0]);
    msg.extend_from_slice(&0xABCDu16.to_be_bytes());
    msg.extend_from_slice(&0x0002u16.to_be_bytes());
    msg.extend_from_slice(b"data");

    let msg = finalize_checksum(&src, &dst, msg);

    // Change to reply.
    let mut reply = msg;
    reply[0] = ICMPV6_ECHO_REPLY;
    let reply = finalize_checksum(&dst, &src, reply);

    if reply[0] != ICMPV6_ECHO_REPLY {
        crate::serial_println!("[icmpv6]   FAIL: reply type = {}", reply[0]);
        return Err(KernelError::InternalError);
    }

    if !verify_checksum(&dst, &src, &reply) {
        crate::serial_println!("[icmpv6]   FAIL: reply checksum invalid");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   echo reply type: OK");
    Ok(())
}

/// Test checksum verification detects corruption.
#[allow(clippy::arithmetic_side_effects)]
fn test_checksum_verification() -> KernelResult<()> {
    let src = Ipv6Addr::LOOPBACK;
    let dst = Ipv6Addr::LOOPBACK;

    let mut msg = Vec::with_capacity(12);
    msg.push(ICMPV6_ECHO_REQUEST);
    msg.push(0);
    msg.extend_from_slice(&[0, 0]);
    msg.extend_from_slice(&0x1111u16.to_be_bytes());
    msg.extend_from_slice(&0x2222u16.to_be_bytes());
    msg.extend_from_slice(b"cksum");

    let mut msg = finalize_checksum(&src, &dst, msg);

    // Verify valid.
    if !verify_checksum(&src, &dst, &msg) {
        crate::serial_println!("[icmpv6]   FAIL: valid message rejected");
        return Err(KernelError::InternalError);
    }

    // Corrupt a byte.
    if let Some(b) = msg.get_mut(6) {
        *b ^= 0xFF;
    }
    if verify_checksum(&src, &dst, &msg) {
        crate::serial_println!("[icmpv6]   FAIL: corrupted message accepted");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   checksum verification: OK");
    Ok(())
}

/// Test NDP option parsing (Source/Target Link-Layer Address).
fn test_ndp_option_parsing() -> KernelResult<()> {
    // Valid SLLA option: type=1, length=1, MAC=AA:BB:CC:DD:EE:FF.
    let options = [1, 1, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    match parse_ndp_option_slla(&options) {
        Some(mac) => {
            if mac.0 != [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF] {
                crate::serial_println!("[icmpv6]   FAIL: SLLA MAC = {:?}", mac.0);
                return Err(KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[icmpv6]   FAIL: SLLA not found");
            return Err(KernelError::InternalError);
        }
    }

    // Valid TLLA option: type=2.
    let options = [2, 1, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
    match parse_ndp_option_tlla(&options) {
        Some(mac) => {
            if mac.0 != [0x11, 0x22, 0x33, 0x44, 0x55, 0x66] {
                crate::serial_println!("[icmpv6]   FAIL: TLLA MAC = {:?}", mac.0);
                return Err(KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[icmpv6]   FAIL: TLLA not found");
            return Err(KernelError::InternalError);
        }
    }

    // Wrong type: looking for SLLA (1) but only TLLA (2) present.
    if parse_ndp_option_slla(&[2, 1, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66]).is_some() {
        crate::serial_println!("[icmpv6]   FAIL: SLLA found when only TLLA present");
        return Err(KernelError::InternalError);
    }

    // Empty options.
    if parse_ndp_option_slla(&[]).is_some() {
        crate::serial_println!("[icmpv6]   FAIL: found option in empty data");
        return Err(KernelError::InternalError);
    }

    // Invalid length=0 (must not loop forever).
    if parse_ndp_option_slla(&[1, 0, 0, 0, 0, 0, 0, 0]).is_some() {
        crate::serial_println!("[icmpv6]   FAIL: accepted option with length=0");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   NDP option parsing: OK");
    Ok(())
}

/// Test neighbor cache insert, lookup, and update.
fn test_neighbor_cache() -> KernelResult<()> {
    let ip = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFE, 0xAA, 0xBB]);
    let mac1 = MacAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    let mac2 = MacAddress([0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF]);

    // Insert.
    neighbor_update(ip, mac1);
    match neighbor_lookup(&ip) {
        Some(mac) if mac.0 == mac1.0 => {}
        other => {
            crate::serial_println!("[icmpv6]   FAIL: lookup after insert = {:?}", other.map(|m| m.0));
            return Err(KernelError::InternalError);
        }
    }

    // Update.
    neighbor_update(ip, mac2);
    match neighbor_lookup(&ip) {
        Some(mac) if mac.0 == mac2.0 => {}
        other => {
            crate::serial_println!("[icmpv6]   FAIL: lookup after update = {:?}", other.map(|m| m.0));
            return Err(KernelError::InternalError);
        }
    }

    // Non-existent entry.
    let other_ip = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x99]);
    if neighbor_lookup(&other_ip).is_some() {
        crate::serial_println!("[icmpv6]   FAIL: found non-existent entry");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   neighbor cache: OK");
    Ok(())
}

/// Test that send_neighbor_solicitation builds a valid message.
///
/// We can't test actual sending (no NIC), but we verify the message
/// structure by building it manually and checking the checksum.
#[allow(clippy::arithmetic_side_effects)]
fn test_neighbor_solicitation_build() -> KernelResult<()> {
    let target = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    let our_mac = MacAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    let our_ip = Ipv6Addr::from_mac_link_local(&our_mac);
    let dst = target.solicited_node_multicast();

    // Build the NS manually (same as send_neighbor_solicitation).
    let mut msg = Vec::with_capacity(32);
    msg.push(ICMPV6_NEIGHBOR_SOLICITATION);
    msg.push(0);
    msg.extend_from_slice(&[0, 0]);       // Checksum
    msg.extend_from_slice(&[0, 0, 0, 0]); // Reserved
    msg.extend_from_slice(&target.0);
    msg.push(1); // SLLA option type
    msg.push(1); // Length
    msg.extend_from_slice(&our_mac.0);

    let msg = finalize_checksum(&our_ip, &dst, msg);

    // Verify structure.
    if msg[0] != ICMPV6_NEIGHBOR_SOLICITATION {
        crate::serial_println!("[icmpv6]   FAIL: NS type = {}", msg[0]);
        return Err(KernelError::InternalError);
    }
    if msg.len() != 32 {
        crate::serial_println!("[icmpv6]   FAIL: NS length = {}", msg.len());
        return Err(KernelError::InternalError);
    }

    // Verify checksum.
    if !verify_checksum(&our_ip, &dst, &msg) {
        crate::serial_println!("[icmpv6]   FAIL: NS bad checksum");
        return Err(KernelError::InternalError);
    }

    // Verify target address in message body.
    let mut target_in_msg = [0u8; 16];
    target_in_msg.copy_from_slice(&msg[8..24]);
    if Ipv6Addr(target_in_msg) != target {
        crate::serial_println!("[icmpv6]   FAIL: NS target mismatch");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   neighbor solicitation build: OK");
    Ok(())
}

/// Test that Neighbor Advertisement message structure is correct.
#[allow(clippy::arithmetic_side_effects)]
fn test_neighbor_advertisement_build() -> KernelResult<()> {
    let our_mac = MacAddress([0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF]);
    let our_ip = Ipv6Addr::from_mac_link_local(&our_mac);
    let dst = Ipv6Addr([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

    // Build manually.
    let mut msg = Vec::with_capacity(32);
    msg.push(ICMPV6_NEIGHBOR_ADVERTISEMENT);
    msg.push(0);
    msg.extend_from_slice(&[0, 0]);       // Checksum
    msg.push(0x60);                        // S+O flags
    msg.extend_from_slice(&[0, 0, 0]);     // Rest of flags
    msg.extend_from_slice(&our_ip.0);      // Target
    msg.push(2);                           // TLLA option type
    msg.push(1);                           // Length
    msg.extend_from_slice(&our_mac.0);

    let msg = finalize_checksum(&our_ip, &dst, msg);

    if msg[0] != ICMPV6_NEIGHBOR_ADVERTISEMENT {
        crate::serial_println!("[icmpv6]   FAIL: NA type = {}", msg[0]);
        return Err(KernelError::InternalError);
    }

    // S and O flags set.
    if msg[4] != 0x60 {
        crate::serial_println!("[icmpv6]   FAIL: NA flags = {:#04x}", msg[4]);
        return Err(KernelError::InternalError);
    }

    if !verify_checksum(&our_ip, &dst, &msg) {
        crate::serial_println!("[icmpv6]   FAIL: NA bad checksum");
        return Err(KernelError::InternalError);
    }

    // Verify TLLA option.
    let tlla = parse_ndp_option_tlla(&msg[24..]);
    match tlla {
        Some(mac) if mac.0 == our_mac.0 => {}
        _ => {
            crate::serial_println!("[icmpv6]   FAIL: NA TLLA mismatch");
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[icmpv6]   neighbor advertisement build: OK");
    Ok(())
}

/// Test ping6 outstanding tracking.
fn test_ping6_tracking() -> KernelResult<()> {
    // Record an outstanding ping6 with seq=888.
    record_outstanding(888);

    match match_outstanding(888) {
        Some(rtt) => {
            if rtt > 1_000_000_000 {
                crate::serial_println!("[icmpv6]   FAIL: RTT {} ns too large", rtt);
                return Err(KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[icmpv6]   FAIL: outstanding ping6 not found");
            return Err(KernelError::InternalError);
        }
    }

    // Second match returns None (consumed).
    if match_outstanding(888).is_some() {
        crate::serial_println!("[icmpv6]   FAIL: consumed slot matched again");
        return Err(KernelError::InternalError);
    }

    // Non-existent seq.
    if match_outstanding(65535).is_some() {
        crate::serial_println!("[icmpv6]   FAIL: non-existent seq matched");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   ping6 tracking: OK");
    Ok(())
}

/// Test RA option parsing (Prefix Information + RDNSS).
#[allow(clippy::arithmetic_side_effects)]
fn test_ra_option_parsing() -> KernelResult<()> {
    // Build a synthetic RA options blob with:
    // 1. Prefix Information (type 3, length 4 = 32 bytes):
    //    - /64 prefix 2001:db8:1:: with A=1, L=1
    //    - Valid lifetime: 7200s, Preferred: 3600s
    // 2. RDNSS (type 25, length 3 = 24 bytes):
    //    - Lifetime: 1800s
    //    - One DNS address: 2001:4860:4860::8888

    let mut opts = Vec::new();

    // Prefix Information option: type=3, length=4 (32 bytes).
    opts.push(NDP_OPT_PREFIX_INFO); // Type
    opts.push(4);                    // Length (4 * 8 = 32 bytes)
    opts.push(64);                   // Prefix length
    opts.push(0xC0);                 // Flags: L=1, A=1
    opts.extend_from_slice(&7200u32.to_be_bytes());  // Valid lifetime
    opts.extend_from_slice(&3600u32.to_be_bytes());  // Preferred lifetime
    opts.extend_from_slice(&[0, 0, 0, 0]);           // Reserved2
    // Prefix: 2001:db8:1::
    opts.extend_from_slice(&[
        0x20, 0x01, 0x0d, 0xb8, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);

    // RDNSS option: type=25, length=3 (24 bytes).
    opts.push(NDP_OPT_RDNSS);       // Type
    opts.push(3);                    // Length (3 * 8 = 24 bytes)
    opts.extend_from_slice(&[0, 0]); // Reserved
    opts.extend_from_slice(&1800u32.to_be_bytes()); // Lifetime
    // DNS address: 2001:4860:4860::8888
    opts.extend_from_slice(&[
        0x20, 0x01, 0x48, 0x60, 0x48, 0x60, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x88,
    ]);

    let mut prefixes: [Option<PrefixInfo>; 4] = [None; 4];
    let mut prefix_count = 0;
    let mut rdnss_addrs: [Option<RdnssInfo>; MAX_RDNSS] = [None; MAX_RDNSS];
    let mut rdnss_count = 0;

    parse_ra_options(
        &opts,
        &mut prefixes,
        &mut prefix_count,
        &mut rdnss_addrs,
        &mut rdnss_count,
    );

    // Verify prefix info.
    if prefix_count != 1 {
        crate::serial_println!("[icmpv6]   FAIL: prefix_count = {}", prefix_count);
        return Err(KernelError::InternalError);
    }
    if let Some(pi) = prefixes[0] {
        if pi.prefix_len != 64 {
            crate::serial_println!("[icmpv6]   FAIL: prefix_len = {}", pi.prefix_len);
            return Err(KernelError::InternalError);
        }
        if !pi.autonomous {
            crate::serial_println!("[icmpv6]   FAIL: A flag not set");
            return Err(KernelError::InternalError);
        }
        if pi.valid_lifetime != 7200 {
            crate::serial_println!("[icmpv6]   FAIL: valid_lifetime = {}", pi.valid_lifetime);
            return Err(KernelError::InternalError);
        }
        if pi.preferred_lifetime != 3600 {
            crate::serial_println!("[icmpv6]   FAIL: preferred_lifetime = {}", pi.preferred_lifetime);
            return Err(KernelError::InternalError);
        }
        let expected_prefix = Ipv6Addr([
            0x20, 0x01, 0x0d, 0xb8, 0x00, 0x01, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        if pi.prefix != expected_prefix {
            crate::serial_println!("[icmpv6]   FAIL: prefix = {}", pi.prefix);
            return Err(KernelError::InternalError);
        }
    } else {
        crate::serial_println!("[icmpv6]   FAIL: prefix not parsed");
        return Err(KernelError::InternalError);
    }

    // Verify RDNSS.
    if rdnss_count != 1 {
        crate::serial_println!("[icmpv6]   FAIL: rdnss_count = {}", rdnss_count);
        return Err(KernelError::InternalError);
    }
    if let Some(ri) = rdnss_addrs[0] {
        let expected_dns = Ipv6Addr([
            0x20, 0x01, 0x48, 0x60, 0x48, 0x60, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x88,
        ]);
        if ri.addr != expected_dns {
            crate::serial_println!("[icmpv6]   FAIL: rdnss addr = {}", ri.addr);
            return Err(KernelError::InternalError);
        }
    } else {
        crate::serial_println!("[icmpv6]   FAIL: rdnss not parsed");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   RA option parsing: OK");
    Ok(())
}

/// Test SLAAC address construction from prefix + MAC.
fn test_slaac_address_build() -> KernelResult<()> {
    let prefix = Ipv6Addr([
        0x20, 0x01, 0x0d, 0xb8, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    let mac = MacAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);

    let addr = build_slaac_address(&prefix, &mac);

    // Expected: 2001:db8:1::5054:ff:fe12:3456
    // High 64 bits from prefix, low 64 bits from EUI-64:
    // 52:54:00 → 50:54:00 (flip U/L bit) → 5054:00ff:fe12:3456
    let expected = Ipv6Addr([
        0x20, 0x01, 0x0d, 0xb8, 0x00, 0x01, 0x00, 0x00,
        0x50, 0x54, 0x00, 0xFF, 0xFE, 0x12, 0x34, 0x56,
    ]);

    if addr != expected {
        crate::serial_println!("[icmpv6]   FAIL: SLAAC address = {}", addr);
        crate::serial_println!("[icmpv6]   expected:             {}", expected);
        return Err(KernelError::InternalError);
    }

    // Verify the prefix is preserved.
    for i in 0..8 {
        if addr.0[i] != prefix.0[i] {
            crate::serial_println!(
                "[icmpv6]   FAIL: prefix byte {} mismatch: {} vs {}",
                i, addr.0[i], prefix.0[i]
            );
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[icmpv6]   SLAAC address build: OK");
    Ok(())
}

/// Test SLAAC state insertion and lookup.
fn test_slaac_state() -> KernelResult<()> {
    let mut state = SlaacState::new();
    let addr = Ipv6Addr([
        0x20, 0x01, 0x0d, 0xb8, 0x00, 0x01, 0x00, 0x00,
        0x50, 0x54, 0x00, 0xFF, 0xFE, 0x12, 0x34, 0x56,
    ]);

    // Initially empty.
    if state.addr_count != 0 {
        crate::serial_println!("[icmpv6]   FAIL: initial addr_count != 0");
        return Err(KernelError::InternalError);
    }
    if state.ra_received {
        crate::serial_println!("[icmpv6]   FAIL: initial ra_received");
        return Err(KernelError::InternalError);
    }

    // Insert an address.
    slaac_insert(&mut state, addr, 64, 7200, 3600);
    if state.addr_count != 1 {
        crate::serial_println!("[icmpv6]   FAIL: addr_count after insert = {}", state.addr_count);
        return Err(KernelError::InternalError);
    }
    if !state.addrs[0].active {
        crate::serial_println!("[icmpv6]   FAIL: first entry not active");
        return Err(KernelError::InternalError);
    }
    if state.addrs[0].addr != addr {
        crate::serial_println!("[icmpv6]   FAIL: stored addr mismatch");
        return Err(KernelError::InternalError);
    }
    if state.addrs[0].prefix_len != 64 {
        crate::serial_println!("[icmpv6]   FAIL: prefix_len = {}", state.addrs[0].prefix_len);
        return Err(KernelError::InternalError);
    }

    // Update the same address (should not increase count).
    slaac_insert(&mut state, addr, 64, 14400, 7200);
    if state.addr_count != 1 {
        crate::serial_println!("[icmpv6]   FAIL: addr_count after update = {}", state.addr_count);
        return Err(KernelError::InternalError);
    }
    if state.addrs[0].valid_lifetime != 14400 {
        crate::serial_println!("[icmpv6]   FAIL: valid_lifetime not updated");
        return Err(KernelError::InternalError);
    }

    // Insert a different address.
    let addr2 = Ipv6Addr([
        0x20, 0x01, 0x0d, 0xb8, 0x00, 0x02, 0x00, 0x00,
        0x50, 0x54, 0x00, 0xFF, 0xFE, 0x12, 0x34, 0x56,
    ]);
    slaac_insert(&mut state, addr2, 64, 7200, 3600);
    if state.addr_count != 2 {
        crate::serial_println!("[icmpv6]   FAIL: addr_count after 2nd insert = {}", state.addr_count);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[icmpv6]   SLAAC state: OK");
    Ok(())
}
