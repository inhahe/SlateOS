//! ICMP (Internet Control Message Protocol) implementation.
//!
//! Supports ICMP Echo Request/Reply (ping) per RFC 792, plus
//! handling of ICMP error messages:
//!
//! - **Type 0**: Echo Reply — matched to outstanding pings
//! - **Type 3**: Destination Unreachable — logged, notifies TCP
//! - **Type 5**: Redirect — logged with suggested gateway
//! - **Type 8**: Echo Request — generates Echo Reply
//! - **Type 11**: Time Exceeded — logged, notifies TCP
//! - **Type 12**: Parameter Problem — logged, notifies TCP
//!
//! ## Echo Request/Reply format
//!
//! ```text
//! Type (8=request, 0=reply) | Code (0) | Checksum
//! Identifier                | Sequence Number
//! Data ...
//! ```
//!
//! ## Checksum verification
//!
//! All incoming ICMP packets are checksum-verified before processing.
//! Packets with invalid checksums are silently dropped (RFC 792).
//!
//! ## Ping RTT measurement
//!
//! Each outstanding ping records a timestamp.  When the reply arrives,
//! the RTT is computed and reported.  Supports up to 16 concurrent
//! outstanding pings.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use crate::sync::Mutex;

use crate::error::KernelResult;

use super::interface::Ipv4Addr;
use super::ipv4::{self, Ipv4Packet, PROTO_ICMP, PROTO_TCP};

// ---------------------------------------------------------------------------
// ICMP types
// ---------------------------------------------------------------------------

/// Echo Reply.
const ICMP_ECHO_REPLY: u8 = 0;
/// Destination Unreachable.
const ICMP_DEST_UNREACHABLE: u8 = 3;
/// Echo Request.
const ICMP_ECHO_REQUEST: u8 = 8;
/// Redirect.
const ICMP_REDIRECT: u8 = 5;
/// Time Exceeded.
const ICMP_TIME_EXCEEDED: u8 = 11;
/// Parameter Problem.
const ICMP_PARAM_PROBLEM: u8 = 12;

/// ICMP header size (type + code + checksum + id/seq or unused).
const ICMP_HEADER_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// ICMP error rate limiter (RFC 1812 §4.3.2.7)
// ---------------------------------------------------------------------------

/// Minimum interval between ICMP error messages (nanoseconds).
/// 10 ms = 100 errors/second maximum.  This prevents DoS amplification
/// when an attacker floods packets to non-existent ports.
const ICMP_ERROR_INTERVAL_NS: u64 = 10_000_000;

/// Timestamp (ns) of the last ICMP error message sent.
static LAST_ICMP_ERROR_NS: AtomicU64 = AtomicU64::new(0);

/// Check whether we're allowed to send an ICMP error right now.
/// Returns `true` if enough time has passed since the last error.
/// Updates the timestamp atomically to claim the slot.
fn icmp_error_rate_ok() -> bool {
    let now = crate::hrtimer::now_ns();
    let prev = LAST_ICMP_ERROR_NS.load(Ordering::Relaxed);
    if now.saturating_sub(prev) < ICMP_ERROR_INTERVAL_NS {
        return false;
    }
    // Try to claim the slot.  If another CPU beats us, that's fine —
    // we just skip this error (over-suppression is acceptable).
    LAST_ICMP_ERROR_NS
        .compare_exchange(prev, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

/// Ping identifier (fixed for our kernel).
const PING_ID: u16 = 0x1234;

/// Maximum outstanding pings tracked for RTT measurement.
const MAX_OUTSTANDING: usize = 16;

// ---------------------------------------------------------------------------
// Ping tracking
// ---------------------------------------------------------------------------

/// Next sequence number.
static PING_SEQ: AtomicU16 = AtomicU16::new(1);

/// Last received ping reply sequence number.
///
/// Used by the simple `wait_reply()` API.  For RTT-aware pings, use
/// `ping_with_rtt()` + `wait_reply_rtt()` instead.
static LAST_REPLY_SEQ: AtomicU16 = AtomicU16::new(0);

/// Last measured RTT in nanoseconds (0 if no reply received yet).
static LAST_RTT_NS: AtomicU64 = AtomicU64::new(0);

/// An outstanding ping awaiting a reply.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // `dst` stored for future per-ping diagnostics
struct PingSlot {
    /// Whether this slot is in use.
    active: bool,
    /// Sequence number of the outstanding ping.
    seq: u16,
    /// Timestamp (monotonic ns) when the ping was sent.
    sent_ns: u64,
    /// Destination IP (for logging).
    dst: Ipv4Addr,
}

impl PingSlot {
    const fn empty() -> Self {
        Self {
            active: false,
            seq: 0,
            sent_ns: 0,
            dst: Ipv4Addr::UNSPECIFIED,
        }
    }
}

/// Table of outstanding pings for RTT tracking.
static OUTSTANDING: Mutex<[PingSlot; MAX_OUTSTANDING]> =
    Mutex::new([PingSlot::empty(); MAX_OUTSTANDING]);

/// Record an outstanding ping.
fn record_outstanding(seq: u16, dst: Ipv4Addr) {
    let now = crate::hrtimer::now_ns();
    let mut table = OUTSTANDING.lock();

    // Find an empty slot (or reuse the oldest if full).
    for slot in table.iter_mut() {
        if !slot.active {
            *slot = PingSlot { active: true, seq, sent_ns: now, dst };
            return;
        }
    }

    // All slots full — evict the oldest.
    let mut oldest_idx = 0;
    let mut oldest_time = u64::MAX;
    for (i, slot) in table.iter().enumerate() {
        if slot.sent_ns < oldest_time {
            oldest_time = slot.sent_ns;
            oldest_idx = i;
        }
    }
    if let Some(slot) = table.get_mut(oldest_idx) {
        *slot = PingSlot { active: true, seq, sent_ns: now, dst };
    }
}

/// Match a reply to an outstanding ping and return the RTT in nanoseconds.
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
// Traceroute probe tracking
// ---------------------------------------------------------------------------

/// Maximum concurrent traceroute probes.
const MAX_TRACEROUTE_PROBES: usize = 32;

/// Traceroute probe identifier (distinct from PING_ID to avoid conflicts).
const TRACEROUTE_ID: u16 = 0x5678;

/// Traceroute probe next sequence number.
static TRACE_SEQ: AtomicU16 = AtomicU16::new(1);

/// A traceroute probe awaiting a Time Exceeded or Echo Reply.
#[derive(Debug, Clone, Copy)]
struct TraceProbe {
    /// Whether this slot is in use.
    active: bool,
    /// Sequence number.
    seq: u16,
    /// Timestamp when sent (ns).
    sent_ns: u64,
    /// TTL (hop number) used for this probe.
    #[allow(dead_code)] // Spec-defined field.
    ttl: u8,
    /// Set when a reply (Time Exceeded or Echo Reply) is received.
    reply_received: bool,
    /// IP address of the replying router (or destination).
    reply_ip: Ipv4Addr,
    /// RTT in nanoseconds (set when reply received).
    rtt_ns: u64,
    /// True if we received an Echo Reply (reached destination).
    reached_dst: bool,
}

impl TraceProbe {
    const fn empty() -> Self {
        Self {
            active: false,
            seq: 0,
            sent_ns: 0,
            ttl: 0,
            reply_received: false,
            reply_ip: Ipv4Addr::UNSPECIFIED,
            rtt_ns: 0,
            reached_dst: false,
        }
    }
}

/// Table of outstanding traceroute probes.
static TRACE_PROBES: Mutex<[TraceProbe; MAX_TRACEROUTE_PROBES]> =
    Mutex::new([TraceProbe::empty(); MAX_TRACEROUTE_PROBES]);

/// Record an outstanding traceroute probe.
pub fn record_trace_probe(seq: u16, ttl: u8) {
    let now = crate::hrtimer::now_ns();
    let mut table = TRACE_PROBES.lock();

    for slot in table.iter_mut() {
        if !slot.active {
            *slot = TraceProbe {
                active: true,
                seq,
                sent_ns: now,
                ttl,
                reply_received: false,
                reply_ip: Ipv4Addr::UNSPECIFIED,
                rtt_ns: 0,
                reached_dst: false,
            };
            return;
        }
    }

    // All full — evict oldest.
    let mut oldest_idx = 0;
    let mut oldest_time = u64::MAX;
    for (i, slot) in table.iter().enumerate() {
        if slot.sent_ns < oldest_time {
            oldest_time = slot.sent_ns;
            oldest_idx = i;
        }
    }
    if let Some(slot) = table.get_mut(oldest_idx) {
        *slot = TraceProbe {
            active: true,
            seq,
            sent_ns: now,
            ttl,
            reply_received: false,
            reply_ip: Ipv4Addr::UNSPECIFIED,
            rtt_ns: 0,
            reached_dst: false,
        };
    }
}

/// Check if a traceroute probe has received a reply.
///
/// Returns `Some((reply_ip, rtt_ns, reached_dst))` if a reply arrived.
pub fn check_trace_reply(seq: u16) -> Option<(Ipv4Addr, u64, bool)> {
    let mut table = TRACE_PROBES.lock();
    for slot in table.iter_mut() {
        if slot.active && slot.seq == seq && slot.reply_received {
            let result = (slot.reply_ip, slot.rtt_ns, slot.reached_dst);
            slot.active = false; // Consume the probe.
            return Some(result);
        }
    }
    None
}

/// Allocate a traceroute sequence number.
pub fn next_trace_seq() -> u16 {
    TRACE_SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Get the traceroute probe ICMP identifier.
pub fn trace_id() -> u16 {
    TRACEROUTE_ID
}

/// Build an ICMP echo request for traceroute (uses TRACEROUTE_ID).
#[allow(clippy::arithmetic_side_effects)]
pub fn build_trace_echo_request(seq: u16) -> Vec<u8> {
    let payload = b"traceroute probe";
    let total = ICMP_HEADER_SIZE + payload.len();
    let mut pkt = Vec::with_capacity(total);

    pkt.push(ICMP_ECHO_REQUEST);
    pkt.push(0);
    pkt.extend_from_slice(&[0, 0]); // Checksum placeholder.
    pkt.extend_from_slice(&TRACEROUTE_ID.to_be_bytes());
    pkt.extend_from_slice(&seq.to_be_bytes());
    pkt.extend_from_slice(payload);

    let checksum = ipv4::ip_checksum(&pkt);
    pkt[2] = (checksum >> 8) as u8;
    pkt[3] = checksum as u8;

    pkt
}

/// Match a Time Exceeded ICMP error against our traceroute probes.
///
/// The ICMP error payload contains the original IP header + first 8 bytes
/// of the original datagram.  For ICMP echo, bytes 4-5 are the ID and
/// bytes 6-7 are the sequence number.
fn match_trace_time_exceeded(icmp_data: &[u8], from_ip: Ipv4Addr) {
    // ICMP header (8 bytes) + original IP header (≥20 bytes) + 8 bytes original payload.
    if icmp_data.len() < ICMP_HEADER_SIZE + 20 + 8 {
        return;
    }

    let orig_ip = &icmp_data[ICMP_HEADER_SIZE..];
    let ihl = (orig_ip[0] & 0x0F) as usize;
    if ihl < 5 {
        return;
    }

    let proto = *orig_ip.get(9).unwrap_or(&0);
    if proto != PROTO_ICMP {
        return; // Not an ICMP probe.
    }

    // Original ICMP header starts after the IP header.
    let icmp_off = ihl.saturating_mul(4);
    let orig_icmp = match orig_ip.get(icmp_off..) {
        Some(d) if d.len() >= 8 => d,
        _ => return,
    };

    // Check type = Echo Request (8).
    if orig_icmp[0] != ICMP_ECHO_REQUEST {
        return;
    }

    let id = u16::from_be_bytes([orig_icmp[4], orig_icmp[5]]);
    let seq = u16::from_be_bytes([orig_icmp[6], orig_icmp[7]]);

    if id != TRACEROUTE_ID {
        return; // Not our traceroute probe.
    }

    let now = crate::hrtimer::now_ns();
    let mut table = TRACE_PROBES.lock();
    for slot in table.iter_mut() {
        if slot.active && slot.seq == seq && !slot.reply_received {
            slot.reply_received = true;
            slot.reply_ip = from_ip;
            slot.rtt_ns = now.saturating_sub(slot.sent_ns);
            slot.reached_dst = false;
            return;
        }
    }
}

/// Match an Echo Reply against traceroute probes (destination reached).
fn match_trace_echo_reply(from_ip: Ipv4Addr, id: u16, seq: u16) {
    if id != TRACEROUTE_ID {
        return;
    }

    let now = crate::hrtimer::now_ns();
    let mut table = TRACE_PROBES.lock();
    for slot in table.iter_mut() {
        if slot.active && slot.seq == seq && !slot.reply_received {
            slot.reply_received = true;
            slot.reply_ip = from_ip;
            slot.rtt_ns = now.saturating_sub(slot.sent_ns);
            slot.reached_dst = true;
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// ICMP packet building
// ---------------------------------------------------------------------------

/// Build an ICMP echo request.
#[allow(clippy::arithmetic_side_effects)]
fn build_echo_request(seq: u16) -> Vec<u8> {
    let payload = b"ping from kernel!";
    let total = ICMP_HEADER_SIZE + payload.len();
    let mut pkt = Vec::with_capacity(total);

    // Type: Echo Request.
    pkt.push(ICMP_ECHO_REQUEST);
    // Code: 0.
    pkt.push(0);
    // Checksum placeholder.
    pkt.extend_from_slice(&[0, 0]);
    // Identifier.
    pkt.extend_from_slice(&PING_ID.to_be_bytes());
    // Sequence number.
    pkt.extend_from_slice(&seq.to_be_bytes());
    // Payload.
    pkt.extend_from_slice(payload);

    // Compute checksum.
    let checksum = ipv4::ip_checksum(&pkt);
    pkt[2] = (checksum >> 8) as u8;
    pkt[3] = checksum as u8;

    pkt
}

// ---------------------------------------------------------------------------
// Checksum verification
// ---------------------------------------------------------------------------

/// Verify the ICMP checksum.
///
/// The checksum covers the entire ICMP message.  A correct checksum
/// folds to zero when computed over the full message including the
/// checksum field (one's complement property).
fn verify_checksum(data: &[u8]) -> bool {
    ipv4::ip_checksum(data) == 0
}

// ---------------------------------------------------------------------------
// ICMP error → transport notification
// ---------------------------------------------------------------------------

/// Parse the embedded IP+transport header from an ICMP error payload
/// and notify the appropriate transport layer (TCP or UDP).
///
/// RFC 792 requires the ICMP error payload to contain the original IP
/// header plus the first 8 bytes of the original datagram (enough for
/// TCP/UDP port numbers).  We parse just enough to identify the original
/// 4-tuple and protocol, then dispatch.
#[allow(clippy::arithmetic_side_effects)]
fn notify_transport_error(icmp_data: &[u8], icmp_type: u8, icmp_code: u8) {
    // ICMP header is 8 bytes; original IP header starts at offset 8.
    if icmp_data.len() < ICMP_HEADER_SIZE + 20 {
        return; // Not enough data for the embedded IP header.
    }

    let orig_ip = &icmp_data[ICMP_HEADER_SIZE..];

    // Validate it's IPv4 (version 4, IHL ≥ 5).
    let version = orig_ip[0] >> 4;
    let ihl = (orig_ip[0] & 0x0F) as usize;
    if version != 4 || ihl < 5 {
        return;
    }

    let ip_hdr_len = ihl * 4;

    // We need at least the IP header + 8 bytes of transport header.
    if orig_ip.len() < ip_hdr_len + 8 {
        return;
    }

    let protocol = orig_ip[9];
    let src_ip = Ipv4Addr::new(orig_ip[12], orig_ip[13], orig_ip[14], orig_ip[15]);
    let dst_ip = Ipv4Addr::new(orig_ip[16], orig_ip[17], orig_ip[18], orig_ip[19]);

    let transport_hdr = &orig_ip[ip_hdr_len..];

    // PMTUD (RFC 1191): for "Fragmentation Needed" (type 3, code 4),
    // extract the next-hop MTU from ICMP header bytes 6-7.  This
    // tells the sender the maximum packet size the path supports.
    let next_hop_mtu = if icmp_type == ICMP_DEST_UNREACHABLE
        && icmp_code == 4
        && icmp_data.len() >= 8
    {
        let mtu = u16::from_be_bytes([icmp_data[6], icmp_data[7]]);
        if mtu > 0 { Some(mtu) } else { None }
    } else {
        None
    };

    match protocol {
        PROTO_TCP => {
            super::tcp::icmp_error(
                src_ip, dst_ip, transport_hdr,
                icmp_type, icmp_code, next_hop_mtu,
            );
        }
        _ => {
            // UDP and other protocols: just log for now.
            // UDP is connectionless, so there's no connection state to abort.
        }
    }
}

// ---------------------------------------------------------------------------
// ICMP error message helpers
// ---------------------------------------------------------------------------

/// Human-readable Destination Unreachable code.
fn dest_unreachable_reason(code: u8) -> &'static str {
    match code {
        0 => "network unreachable",
        1 => "host unreachable",
        2 => "protocol unreachable",
        3 => "port unreachable",
        4 => "fragmentation needed but DF set",
        5 => "source route failed",
        6 => "destination network unknown",
        7 => "destination host unknown",
        9 => "network administratively prohibited",
        10 => "host administratively prohibited",
        13 => "communication administratively prohibited",
        _ => "unknown",
    }
}

/// Human-readable Redirect code.
fn redirect_reason(code: u8) -> &'static str {
    match code {
        0 => "redirect for network",
        1 => "redirect for host",
        2 => "redirect for TOS and network",
        3 => "redirect for TOS and host",
        _ => "unknown",
    }
}

/// Human-readable Time Exceeded code.
fn time_exceeded_reason(code: u8) -> &'static str {
    match code {
        0 => "TTL exceeded in transit",
        1 => "fragment reassembly time exceeded",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// ICMP processing
// ---------------------------------------------------------------------------

/// Process an incoming ICMP packet received in a given network namespace.
///
/// `ns_id` is the namespace the packet arrived in.  Echo replies are sent
/// from that namespace's interface address (so a ping to a container's IP
/// is answered from the container's namespace, not the root namespace).
pub fn process_icmp(
    ip_packet: &Ipv4Packet<'_>,
    ns_id: crate::netns::NetNsId,
) -> KernelResult<()> {
    let data = ip_packet.payload;
    if data.len() < ICMP_HEADER_SIZE {
        return Ok(());
    }

    // Verify ICMP checksum before processing.
    if !verify_checksum(data) {
        crate::serial_println!(
            "[icmp] Dropped packet from {} — bad checksum",
            ip_packet.src
        );
        return Ok(());
    }

    let icmp_type = data[0];
    let code = data[1];

    match icmp_type {
        ICMP_ECHO_REPLY => {
            handle_echo_reply(ip_packet, data);
        }
        ICMP_ECHO_REQUEST => {
            // Reply to echo requests (respond to pings directed at us).
            let our_ip = super::interface::ns_ip(ns_id);
            if !our_ip.is_unspecified() {
                send_echo_reply(ip_packet, ns_id)?;
            }
        }
        ICMP_DEST_UNREACHABLE => {
            crate::serial_println!(
                "[icmp] Destination unreachable from {}: {} (code {})",
                ip_packet.src,
                dest_unreachable_reason(code),
                code
            );
            // Notify the transport layer that originated the packet.
            // ICMP error payload = original IP header + first 8 bytes
            // of the triggering packet (enough for TCP/UDP port info).
            notify_transport_error(data, icmp_type, code);
        }
        ICMP_REDIRECT => {
            // Type 5: a router tells us to use a different gateway for
            // a given destination.  The better gateway IP is in bytes
            // 4-7 of the ICMP header (the "gateway address" field).
            if data.len() >= 8 {
                let gw = Ipv4Addr::new(data[4], data[5], data[6], data[7]);
                crate::serial_println!(
                    "[icmp] Redirect from {}: use gateway {} (code {}={})",
                    ip_packet.src, gw, code, redirect_reason(code)
                );
                // Note: we don't update routing tables because we only
                // have a single default gateway.  Log for diagnostics.
            }
        }
        ICMP_TIME_EXCEEDED => {
            // Check if this is a reply to a traceroute probe first.
            match_trace_time_exceeded(data, ip_packet.src);

            crate::serial_println!(
                "[icmp] Time exceeded from {}: {} (code {})",
                ip_packet.src,
                time_exceeded_reason(code),
                code
            );
            // Also notify transport for TTL exceeded — a SYN_SENT
            // connection to an unreachable-by-TTL host should abort.
            notify_transport_error(data, icmp_type, code);
        }
        ICMP_PARAM_PROBLEM => {
            // Type 12: the packet we sent has a header problem.
            // Byte 4 of the ICMP message is the pointer to the
            // offending byte in the original header.
            let pointer = if data.len() >= 5 { data[4] } else { 0 };
            crate::serial_println!(
                "[icmp] Parameter problem from {}: pointer={} (code {})",
                ip_packet.src, pointer, code
            );
            notify_transport_error(data, icmp_type, code);
        }
        _ => {
            // Other ICMP types — silently ignore.
        }
    }

    Ok(())
}

/// Handle an ICMP Echo Reply.
fn handle_echo_reply(ip_packet: &Ipv4Packet<'_>, data: &[u8]) {
    if data.len() < 8 {
        return;
    }

    let id = u16::from_be_bytes([data[4], data[5]]);
    let seq = u16::from_be_bytes([data[6], data[7]]);

    // Check if this is a traceroute probe reply (destination reached).
    if id == TRACEROUTE_ID {
        match_trace_echo_reply(ip_packet.src, id, seq);
        return;
    }

    if id != PING_ID {
        return; // Not our ping.
    }

    // Try to match against outstanding pings for RTT.
    // Store RTT *before* seq: the consumer in wait_reply_rtt() observes
    // seq via Acquire, so the preceding Release on RTT guarantees the
    // RTT value is visible when seq becomes visible.
    if let Some(rtt_ns) = match_outstanding(seq) {
        LAST_RTT_NS.store(rtt_ns, Ordering::Release);

        // Format RTT in human-readable units.
        #[allow(clippy::arithmetic_side_effects)]
        let rtt_us = rtt_ns / 1000;
        if rtt_us >= 1000 {
            #[allow(clippy::arithmetic_side_effects)]
            let rtt_ms = rtt_us / 1000;
            crate::serial_println!(
                "[icmp] Echo reply from {} seq={} rtt={} ms",
                ip_packet.src, seq, rtt_ms
            );
        } else {
            crate::serial_println!(
                "[icmp] Echo reply from {} seq={} rtt={} us",
                ip_packet.src, seq, rtt_us
            );
        }
    } else {
        crate::serial_println!(
            "[icmp] Echo reply from {} seq={}",
            ip_packet.src, seq
        );
    }

    // Store seq last — this is the signal that the reply arrived.
    // Uses Release so that the preceding RTT store is visible to
    // consumers that Acquire-load LAST_REPLY_SEQ.
    LAST_REPLY_SEQ.store(seq, Ordering::Release);
}

/// Send an ICMP echo reply in response to a request.
#[allow(clippy::arithmetic_side_effects)]
fn send_echo_reply(
    request_ip: &Ipv4Packet<'_>,
    ns_id: crate::netns::NetNsId,
) -> KernelResult<()> {
    let data = request_ip.payload;
    if data.len() < ICMP_HEADER_SIZE {
        return Ok(());
    }

    let mut reply = Vec::from(data);
    // Change type to Echo Reply.
    reply[0] = ICMP_ECHO_REPLY;
    // Recompute checksum.
    reply[2] = 0;
    reply[3] = 0;
    let checksum = ipv4::ip_checksum(&reply);
    reply[2] = (checksum >> 8) as u8;
    reply[3] = checksum as u8;

    // Reply from the namespace the request arrived in.
    ipv4::send_ns(ns_id, request_ip.src, PROTO_ICMP, &reply)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Send an ICMP echo request (ping) to the given IP address.
///
/// Records the ping for RTT measurement.  Use [`wait_reply_rtt()`] to
/// wait for the reply and retrieve the RTT, or [`wait_reply()`] for
/// the simple boolean API.
///
/// Returns the sequence number used.
pub fn ping(dst: Ipv4Addr) -> KernelResult<u16> {
    let seq = PING_SEQ.fetch_add(1, Ordering::Relaxed);
    let pkt = build_echo_request(seq);
    record_outstanding(seq, dst);
    ipv4::send(dst, PROTO_ICMP, &pkt)?;
    Ok(seq)
}

/// Wait for a ping reply with the given sequence number.
///
/// Polls the NIC for up to `timeout_polls` iterations.
/// Returns `true` if the reply was received.
pub fn wait_reply(seq: u16, timeout_polls: u32) -> bool {
    for _ in 0..timeout_polls {
        super::poll();

        if LAST_REPLY_SEQ.load(Ordering::Acquire) == seq {
            return true;
        }

        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }
    false
}

/// Wait for a ping reply and return the RTT in nanoseconds.
///
/// Polls the NIC for up to `timeout_polls` iterations.
/// Returns `Some(rtt_ns)` on success, `None` on timeout.
pub fn wait_reply_rtt(seq: u16, timeout_polls: u32) -> Option<u64> {
    for _ in 0..timeout_polls {
        super::poll();

        // Acquire-load seq: if we see the new seq, the Release store
        // on LAST_RTT_NS is guaranteed to have completed, so the RTT
        // we read next is the correct value for this ping.
        if LAST_REPLY_SEQ.load(Ordering::Acquire) == seq {
            return Some(LAST_RTT_NS.load(Ordering::Acquire));
        }

        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }
    None
}

/// Get the last measured RTT in nanoseconds.
///
/// Returns 0 if no ping reply has been received yet.
#[allow(dead_code)] // Public API for network diagnostics
pub fn last_rtt_ns() -> u64 {
    LAST_RTT_NS.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// ICMP unit tests — exercises echo request building, checksum verification,
/// traceroute probe building, ping tracking, and reason string lookups.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[icmp] Running ICMP self-test...");

    test_build_echo_request_checksum()?;
    test_verify_checksum_valid()?;
    test_verify_checksum_invalid()?;
    test_build_trace_echo_request()?;
    test_ping_tracking()?;
    test_reason_strings()?;

    crate::serial_println!("[icmp] ICMP self-test PASSED (6 tests)");
    Ok(())
}

/// Test that build_echo_request produces a valid ICMP checksum.
fn test_build_echo_request_checksum() -> KernelResult<()> {
    let pkt = build_echo_request(42);

    // Minimum size: 8 bytes header + payload "ping from kernel!" (17 bytes).
    if pkt.len() < ICMP_HEADER_SIZE {
        crate::serial_println!("[icmp]   FAIL: echo request too short ({})", pkt.len());
        return Err(crate::error::KernelError::InternalError);
    }

    // Type must be Echo Request (8).
    if pkt[0] != ICMP_ECHO_REQUEST {
        crate::serial_println!("[icmp]   FAIL: type = {}, expected {}", pkt[0], ICMP_ECHO_REQUEST);
        return Err(crate::error::KernelError::InternalError);
    }

    // Code must be 0.
    if pkt[1] != 0 {
        crate::serial_println!("[icmp]   FAIL: code = {}", pkt[1]);
        return Err(crate::error::KernelError::InternalError);
    }

    // Identifier must be PING_ID.
    let id = u16::from_be_bytes([pkt[4], pkt[5]]);
    if id != PING_ID {
        crate::serial_println!("[icmp]   FAIL: id = {:#06x}, expected {:#06x}", id, PING_ID);
        return Err(crate::error::KernelError::InternalError);
    }

    // Sequence number must be 42.
    let seq = u16::from_be_bytes([pkt[6], pkt[7]]);
    if seq != 42 {
        crate::serial_println!("[icmp]   FAIL: seq = {}, expected 42", seq);
        return Err(crate::error::KernelError::InternalError);
    }

    // Checksum should be valid (folds to 0).
    if !verify_checksum(&pkt) {
        crate::serial_println!("[icmp]   FAIL: echo request has invalid checksum");
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!("[icmp]   build echo request checksum: OK");
    Ok(())
}

/// Test verify_checksum with a known-valid ICMP packet.
fn test_verify_checksum_valid() -> KernelResult<()> {
    // Build a valid echo request and verify it passes.
    let pkt = build_echo_request(100);
    if !verify_checksum(&pkt) {
        crate::serial_println!("[icmp]   FAIL: valid packet rejected by verify_checksum");
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!("[icmp]   verify checksum (valid): OK");
    Ok(())
}

/// Test verify_checksum rejects a corrupted ICMP packet.
fn test_verify_checksum_invalid() -> KernelResult<()> {
    let mut pkt = build_echo_request(200);

    // Corrupt a payload byte.
    if let Some(b) = pkt.get_mut(10) {
        *b ^= 0xFF;
    }

    if verify_checksum(&pkt) {
        crate::serial_println!("[icmp]   FAIL: corrupted packet passed verify_checksum");
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!("[icmp]   verify checksum (invalid): OK (rejected)");
    Ok(())
}

/// Test that build_trace_echo_request produces valid ICMP.
fn test_build_trace_echo_request() -> KernelResult<()> {
    let pkt = build_trace_echo_request(7);

    if pkt.len() < ICMP_HEADER_SIZE {
        crate::serial_println!("[icmp]   FAIL: trace request too short");
        return Err(crate::error::KernelError::InternalError);
    }

    // Type must be Echo Request (8).
    if pkt[0] != ICMP_ECHO_REQUEST {
        crate::serial_println!("[icmp]   FAIL: trace type = {}", pkt[0]);
        return Err(crate::error::KernelError::InternalError);
    }

    // Identifier must be TRACEROUTE_ID.
    let id = u16::from_be_bytes([pkt[4], pkt[5]]);
    if id != TRACEROUTE_ID {
        crate::serial_println!("[icmp]   FAIL: trace id = {:#06x}", id);
        return Err(crate::error::KernelError::InternalError);
    }

    // Sequence must be 7.
    let seq = u16::from_be_bytes([pkt[6], pkt[7]]);
    if seq != 7 {
        crate::serial_println!("[icmp]   FAIL: trace seq = {}", seq);
        return Err(crate::error::KernelError::InternalError);
    }

    // Checksum must be valid.
    if !verify_checksum(&pkt) {
        crate::serial_println!("[icmp]   FAIL: trace request bad checksum");
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!("[icmp]   build trace echo request: OK");
    Ok(())
}

/// Test record_outstanding + match_outstanding for ping RTT tracking.
fn test_ping_tracking() -> KernelResult<()> {
    let dst = Ipv4Addr([8, 8, 8, 8]);

    // Record an outstanding ping with seq=999.
    record_outstanding(999, dst);

    // Matching should return Some(rtt).
    match match_outstanding(999) {
        Some(rtt) => {
            // RTT should be very small (we just recorded it).
            if rtt > 1_000_000_000 {
                crate::serial_println!("[icmp]   FAIL: RTT {} ns too large", rtt);
                return Err(crate::error::KernelError::InternalError);
            }
        }
        None => {
            crate::serial_println!("[icmp]   FAIL: outstanding ping not found");
            return Err(crate::error::KernelError::InternalError);
        }
    }

    // Second match should return None (slot was consumed).
    if match_outstanding(999).is_some() {
        crate::serial_println!("[icmp]   FAIL: consumed slot matched again");
        return Err(crate::error::KernelError::InternalError);
    }

    // Non-existent seq should return None.
    if match_outstanding(12345).is_some() {
        crate::serial_println!("[icmp]   FAIL: non-existent seq matched");
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!("[icmp]   ping tracking: OK");
    Ok(())
}

/// Test that reason string lookups return expected values.
fn test_reason_strings() -> KernelResult<()> {
    // Destination Unreachable codes.
    if dest_unreachable_reason(0) != "network unreachable" {
        crate::serial_println!("[icmp]   FAIL: wrong reason for code 0");
        return Err(crate::error::KernelError::InternalError);
    }
    if dest_unreachable_reason(3) != "port unreachable" {
        crate::serial_println!("[icmp]   FAIL: wrong reason for code 3");
        return Err(crate::error::KernelError::InternalError);
    }
    if dest_unreachable_reason(4) != "fragmentation needed but DF set" {
        crate::serial_println!("[icmp]   FAIL: wrong reason for code 4");
        return Err(crate::error::KernelError::InternalError);
    }
    if dest_unreachable_reason(255) != "unknown" {
        crate::serial_println!("[icmp]   FAIL: unknown code should return 'unknown'");
        return Err(crate::error::KernelError::InternalError);
    }

    // Time Exceeded codes.
    if time_exceeded_reason(0) != "TTL exceeded in transit" {
        crate::serial_println!("[icmp]   FAIL: wrong time_exceeded reason");
        return Err(crate::error::KernelError::InternalError);
    }

    // Redirect codes.
    if redirect_reason(1) != "redirect for host" {
        crate::serial_println!("[icmp]   FAIL: wrong redirect reason");
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!("[icmp]   reason strings: OK");
    Ok(())
}

/// Send an ICMP Destination Unreachable (Port Unreachable) message.
///
/// Per RFC 792 / RFC 1122 §3.2.2.1, when a UDP datagram arrives at a
/// port with no listener, the host should send back an ICMP type 3
/// code 3 (port unreachable).  The ICMP error payload contains the
/// original IP header plus the first 8 bytes of the triggering UDP
/// datagram.
///
/// `orig_ip_hdr` is the full original IP header (typically 20 bytes).
/// `orig_transport_8` is the first 8 bytes of the transport layer
/// (src_port + dst_port + length + checksum for UDP).
#[allow(clippy::arithmetic_side_effects)]
pub fn send_port_unreachable(
    dst: Ipv4Addr,
    orig_ip_hdr: &[u8],
    orig_transport_8: &[u8],
) -> KernelResult<()> {
    // Rate-limit ICMP error generation (RFC 1812 §4.3.2.7).
    // Under a port-scan or UDP flood, the kernel could otherwise generate
    // one Destination Unreachable per inbound packet, amplifying the attack.
    if !icmp_error_rate_ok() {
        return Ok(());
    }

    // Need the original IP header (≥20 bytes) and 8 bytes of transport.
    if orig_ip_hdr.len() < 20 || orig_transport_8.len() < 8 {
        return Ok(());
    }

    // ICMP Destination Unreachable format:
    //   Type (3) | Code (3=port unreachable) | Checksum
    //   Unused (4 bytes, must be 0)
    //   Original IP header + first 8 bytes of triggering datagram
    let payload_len = orig_ip_hdr.len() + 8; // IP header + 8 bytes transport
    let total = ICMP_HEADER_SIZE + payload_len;
    let mut pkt = Vec::with_capacity(total);

    // Type: Destination Unreachable.
    pkt.push(ICMP_DEST_UNREACHABLE);
    // Code: Port Unreachable.
    pkt.push(3);
    // Checksum placeholder.
    pkt.extend_from_slice(&[0, 0]);
    // Unused (4 bytes).
    pkt.extend_from_slice(&[0, 0, 0, 0]);
    // Original IP header.
    pkt.extend_from_slice(orig_ip_hdr);
    // First 8 bytes of the original transport header.
    pkt.extend_from_slice(&orig_transport_8[..8]);

    // Compute checksum over the entire ICMP message.
    let checksum = ipv4::ip_checksum(&pkt);
    pkt[2] = (checksum >> 8) as u8;
    pkt[3] = checksum as u8;

    ipv4::send(dst, PROTO_ICMP, &pkt)
}
