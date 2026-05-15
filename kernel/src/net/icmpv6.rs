//! ICMPv6 (Internet Control Message Protocol for IPv6) implementation.
//!
//! Implements RFC 4443 (ICMPv6) and basic RFC 4861 (NDP) support:
//!
//! - **Type 1**: Destination Unreachable
//! - **Type 2**: Packet Too Big
//! - **Type 3**: Time Exceeded
//! - **Type 128**: Echo Request — generates Echo Reply
//! - **Type 129**: Echo Reply — matched to outstanding pings
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

    // Check if we're the target.
    let our_mac = super::interface::mac();
    let our_ip = Ipv6Addr::from_mac_link_local(&our_mac);
    if target_addr != our_ip {
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

    crate::serial_println!("[icmpv6] ICMPv6 self-test PASSED (8 tests)");
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
