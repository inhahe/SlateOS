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
//! ## Namespace support
//!
//! The global ARP cache serves the root namespace (physical NIC LAN).
//! Per-namespace ARP caches provide isolated MAC resolution for child
//! namespaces connected via veth pairs.  Each namespace has its own
//! 16-entry cache, operating independently of the global cache.
//!
//! Use `ns_lookup()` / `ns_insert()` / `ns_resolve()` for namespace-
//! aware ARP operations.  For the root namespace (ID 0), these
//! delegate to the global cache.

use crate::sync::Mutex;

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
    // Reject broadcast, multicast, and loopback IPs — these should
    // never appear as ARP cache entries.  Prevents cache pollution
    // from malformed or malicious ARP packets.
    if ip.is_multicast() || ip.is_broadcast() || ip.0[0] == 127 {
        return;
    }

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

/// Process an incoming ARP packet received in a given network namespace.
///
/// `ns_id` is the namespace the frame arrived in (`ROOT_NS` for the
/// physical NIC).  The "is this a request for our IP?" check uses that
/// namespace's interface address, and any reply is sourced from that
/// namespace's address, so ARP for a container's IP is answered as the
/// container rather than as the root host.
pub fn process_arp(data: &[u8], ns_id: crate::netns::NetNsId) -> KernelResult<()> {
    let (operation, sender_mac, sender_ip, _target_mac, target_ip) = parse_arp(data)?;

    // Always learn the sender's MAC from any ARP we see, into the arrival
    // namespace's cache (delegates to the global cache for ROOT_NS).
    if !sender_ip.is_unspecified() {
        ns_insert(ns_id, sender_ip, sender_mac);
    }

    match operation {
        ARP_REQUEST => {
            // Is this a request for our IP in this namespace?
            let our_ip = interface::ns_ip(ns_id);
            if !our_ip.is_unspecified() && target_ip == our_ip {
                // Send ARP reply.
                send_reply(&sender_mac, sender_ip, ns_id)?;
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

/// Send an ARP reply to the given target from a namespace's interface.
///
/// The reply is sourced from `ns_id`'s MAC and IP address and egresses
/// through that namespace's link: the physical NIC in the root namespace,
/// or the container's veth (via [`net::send_frame_ns`]) in a container
/// namespace, so a container answers ARP for its own IP on its
/// user-defined network.
fn send_reply(
    target_mac: &MacAddress,
    target_ip: Ipv4Addr,
    ns_id: crate::netns::NetNsId,
) -> KernelResult<()> {
    let our_mac = interface::ns_mac(ns_id);
    let our_ip = interface::ns_ip(ns_id);

    let arp_data = build_arp(ARP_REPLY, &our_mac, our_ip, target_mac, target_ip);
    let frame = ethernet::build_frame(target_mac, &our_mac, ETHERTYPE_ARP, &arp_data);

    super::send_frame_ns(ns_id, &frame)?;

    Ok(())
}

/// Send an ARP request for the given IP address (root namespace).
pub fn send_request(target_ip: Ipv4Addr) -> KernelResult<()> {
    send_request_ns(crate::netns::ROOT_NS, target_ip)
}

/// Send an ARP request for the given IP address from a network namespace.
///
/// The request is sourced from `ns_id`'s interface MAC and IP, and egresses
/// through that namespace's link: the physical NIC in the root namespace,
/// or the container's veth (via [`net::send_frame_ns`]) in a container
/// namespace so a container can resolve its user-defined-network peers.
pub fn send_request_ns(
    ns_id: crate::netns::NetNsId,
    target_ip: Ipv4Addr,
) -> KernelResult<()> {
    let our_mac = interface::ns_mac(ns_id);
    let our_ip = interface::ns_ip(ns_id);

    let arp_data = build_arp(
        ARP_REQUEST,
        &our_mac,
        our_ip,
        &MacAddress([0; 6]),  // Target MAC unknown — that's what we're asking for.
        target_ip,
    );
    let frame = ethernet::build_frame(&BROADCAST_MAC, &our_mac, ETHERTYPE_ARP, &arp_data);

    super::send_frame_ns(ns_id, &frame)?;

    Ok(())
}

/// Resolve an IP address to a MAC address (root namespace).
pub fn resolve(ip: Ipv4Addr) -> KernelResult<MacAddress> {
    resolve_ns(crate::netns::ROOT_NS, ip)
}

/// Resolve an IP address to a MAC address within a network namespace.
///
/// First checks the (shared) ARP cache.  If not found, sends an ARP
/// request from `ns_id` and polls for a reply (blocking, with timeout).
/// The poll loop drives [`net::poll`], which drains both the physical NIC
/// and the veth/bridge fabric, so a container peer's reply — delivered
/// back through the bridge — is learned into this namespace's cache here.
///
/// Resolution and learning are namespace-scoped: root uses the global
/// `ARP_CACHE`; a container namespace uses its own `NS_ARP` cache (activated
/// by `ns_init` at container-veth setup).  Two container networks that reuse
/// the same subnet/IP therefore no longer collide.
#[allow(clippy::arithmetic_side_effects)]
pub fn resolve_ns(ns_id: crate::netns::NetNsId, ip: Ipv4Addr) -> KernelResult<MacAddress> {
    // Check the arrival namespace's cache first (delegates to the global
    // cache for ROOT_NS).
    if let Some(mac) = ns_lookup(ns_id, ip) {
        return Ok(mac);
    }

    // Broadcast is always ff:ff:ff:ff:ff:ff.
    if ip.is_broadcast() {
        return Ok(BROADCAST_MAC);
    }

    // Send ARP request and wait for reply.
    send_request_ns(ns_id, ip)?;

    // Poll for up to ~1 second (1000 iterations with spin + NIC poll).
    for _ in 0..1000 {
        // Poll the NIC (and veth/bridge) for any incoming frames,
        // including the ARP reply.
        super::poll();

        // Check if the reply arrived (learned into this namespace's cache).
        if let Some(mac) = ns_lookup(ns_id, ip) {
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

// ---------------------------------------------------------------------------
// Per-namespace ARP cache
// ---------------------------------------------------------------------------

/// Maximum per-namespace ARP cache entries.
///
/// Smaller than the global cache — containers typically have fewer
/// neighbors (often just the host-side veth peer + gateway).
const NS_ARP_CACHE_SIZE: usize = 16;

/// Maximum number of namespace ARP caches.
///
/// Matches `netns::MAX_NAMESPACES`.  Slot 0 is unused (root namespace
/// uses the global `ARP_CACHE`).
const NS_ARP_MAX: usize = 64;

/// Per-namespace ARP cache slot.
struct NsArpCache {
    /// Whether this namespace slot has an active ARP cache.
    active: bool,
    /// The cache entries.
    entries: [ArpEntry; NS_ARP_CACHE_SIZE],
}

impl NsArpCache {
    const fn empty() -> Self {
        Self {
            active: false,
            entries: [ArpEntry {
                ip: Ipv4Addr::UNSPECIFIED,
                mac: MacAddress([0; 6]),
                valid: false,
                updated_ns: 0,
            }; NS_ARP_CACHE_SIZE],
        }
    }
}

/// Per-namespace ARP cache table.
static NS_ARP: Mutex<[NsArpCache; NS_ARP_MAX]> = Mutex::new(
    [const { NsArpCache::empty() }; NS_ARP_MAX]
);

/// Initialize the per-namespace ARP cache for a namespace.
///
/// Called when a namespace creates a veth endpoint or when the
/// container networking stack sets up a namespace.  Idempotent —
/// calling on an already-initialized namespace is a no-op.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `ns_id` is the root namespace
///   (root uses the global cache) or out of range.
pub fn ns_init(ns_id: crate::netns::NetNsId) -> KernelResult<()> {
    if ns_id == crate::netns::ROOT_NS {
        return Err(KernelError::InvalidArgument);
    }
    let idx = ns_id as usize;
    let mut table = NS_ARP.lock();
    let cache = table.get_mut(idx).ok_or(KernelError::InvalidArgument)?;
    if !cache.active {
        cache.active = true;
        for entry in cache.entries.iter_mut() {
            entry.valid = false;
        }
    }
    Ok(())
}

/// Destroy the per-namespace ARP cache for a namespace.
///
/// Invalidates all entries.  Called during namespace teardown.
pub fn ns_destroy(ns_id: crate::netns::NetNsId) {
    let idx = ns_id as usize;
    let mut table = NS_ARP.lock();
    if let Some(cache) = table.get_mut(idx) {
        cache.active = false;
        for entry in cache.entries.iter_mut() {
            entry.valid = false;
        }
    }
}

/// Look up a MAC address in a namespace's ARP cache.
///
/// For the root namespace, delegates to the global cache.
/// Returns `None` if not found, expired, or namespace has no cache.
pub fn ns_lookup(ns_id: crate::netns::NetNsId, ip: Ipv4Addr) -> Option<MacAddress> {
    if ns_id == crate::netns::ROOT_NS {
        return lookup(ip);
    }

    let now = crate::hrtimer::now_ns();
    let idx = ns_id as usize;
    let table = NS_ARP.lock();
    let cache = table.get(idx)?;
    if !cache.active {
        return None;
    }

    for entry in cache.entries.iter() {
        if entry.ip == ip && entry.is_fresh(now) {
            return Some(entry.mac);
        }
    }
    None
}

/// Insert or update an entry in a namespace's ARP cache.
///
/// For the root namespace, delegates to the global cache.
/// Silently ignores if the namespace has no ARP cache initialized.
pub fn ns_insert(ns_id: crate::netns::NetNsId, ip: Ipv4Addr, mac: MacAddress) {
    if ns_id == crate::netns::ROOT_NS {
        cache_insert(ip, mac);
        return;
    }

    // Reject broadcast, multicast, and loopback IPs.
    if ip.is_multicast() || ip.is_broadcast() || ip.0[0] == 127 {
        return;
    }

    let now = crate::hrtimer::now_ns();
    let idx = ns_id as usize;
    let mut table = NS_ARP.lock();
    let Some(cache) = table.get_mut(idx) else { return };
    if !cache.active {
        return;
    }

    // Check if already present — update.
    for entry in cache.entries.iter_mut() {
        if entry.valid && entry.ip == ip {
            entry.mac = mac;
            entry.updated_ns = now;
            return;
        }
    }

    // Find an empty or expired slot.
    for entry in cache.entries.iter_mut() {
        if !entry.is_fresh(now) {
            *entry = ArpEntry { ip, mac, valid: true, updated_ns: now };
            return;
        }
    }

    // Cache full — evict the oldest.
    let mut oldest_idx = 0;
    let mut oldest_time = u64::MAX;
    for (i, entry) in cache.entries.iter().enumerate() {
        if entry.updated_ns < oldest_time {
            oldest_time = entry.updated_ns;
            oldest_idx = i;
        }
    }
    if let Some(slot) = cache.entries.get_mut(oldest_idx) {
        *slot = ArpEntry { ip, mac, valid: true, updated_ns: now };
    }
}

/// Flush all entries in a namespace's ARP cache.
///
/// For the root namespace, delegates to the global flush.
pub fn ns_flush(ns_id: crate::netns::NetNsId) {
    if ns_id == crate::netns::ROOT_NS {
        flush_cache();
        return;
    }

    let idx = ns_id as usize;
    let mut table = NS_ARP.lock();
    if let Some(cache) = table.get_mut(idx) {
        if cache.active {
            for entry in cache.entries.iter_mut() {
                entry.valid = false;
            }
        }
    }
}

/// Get a snapshot of a namespace's ARP cache entries.
///
/// For the root namespace, delegates to the global `cache_entries()`.
pub fn ns_cache_entries(
    ns_id: crate::netns::NetNsId,
) -> ([ArpCacheEntry; NS_ARP_CACHE_SIZE], usize) {
    let empty = [ArpCacheEntry {
        ip: Ipv4Addr::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        ttl_secs: 0,
    }; NS_ARP_CACHE_SIZE];

    if ns_id == crate::netns::ROOT_NS {
        // Convert global cache entries to the smaller array.
        let (global, global_count) = cache_entries();
        let mut out = empty;
        let copy_count = global_count.min(NS_ARP_CACHE_SIZE);
        for i in 0..copy_count {
            if let Some(src) = global.get(i) {
                if let Some(dst) = out.get_mut(i) {
                    *dst = *src;
                }
            }
        }
        return (out, copy_count);
    }

    let now = crate::hrtimer::now_ns();
    let idx = ns_id as usize;
    let table = NS_ARP.lock();
    let Some(cache) = table.get(idx) else {
        return (empty, 0);
    };
    if !cache.active {
        return (empty, 0);
    }

    let mut out = empty;
    let mut count: usize = 0;

    for entry in cache.entries.iter() {
        if entry.is_fresh(now) {
            let age_ns = now.saturating_sub(entry.updated_ns);
            let remaining_ns = ARP_ENTRY_LIFETIME_NS.saturating_sub(age_ns);
            if let Some(slot) = out.get_mut(count) {
                *slot = ArpCacheEntry {
                    ip: entry.ip,
                    mac: entry.mac,
                    ttl_secs: remaining_ns / 1_000_000_000,
                };
                count = count.wrapping_add(1);
            }
        }
    }

    (out, count)
}

/// Count entries in a namespace's ARP cache.
#[must_use]
pub fn ns_entry_count(ns_id: crate::netns::NetNsId) -> usize {
    if ns_id == crate::netns::ROOT_NS {
        let (_, count) = cache_entries();
        return count;
    }

    let now = crate::hrtimer::now_ns();
    let idx = ns_id as usize;
    let table = NS_ARP.lock();
    let Some(cache) = table.get(idx) else { return 0 };
    if !cache.active {
        return 0;
    }

    cache.entries.iter().filter(|e| e.is_fresh(now)).count()
}

// ---------------------------------------------------------------------------
// ARP cache diagnostics
// ---------------------------------------------------------------------------

/// A snapshot of one ARP cache entry for diagnostic display.
#[derive(Debug, Clone, Copy)]
pub struct ArpCacheEntry {
    /// Resolved IPv4 address.
    pub ip: Ipv4Addr,
    /// Corresponding Ethernet MAC address.
    pub mac: MacAddress,
    /// Seconds remaining until this entry expires.
    pub ttl_secs: u64,
}

/// Return a snapshot of all valid (non-expired) ARP cache entries.
///
/// Useful for `arp -a` style display in the shell or userspace diagnostics.
/// Entries are returned in cache-slot order (not sorted).
pub fn cache_entries() -> ([ArpCacheEntry; ARP_CACHE_SIZE], usize) {
    let now = crate::hrtimer::now_ns();
    let cache = ARP_CACHE.lock();

    let mut out = [ArpCacheEntry {
        ip: Ipv4Addr::UNSPECIFIED,
        mac: MacAddress([0; 6]),
        ttl_secs: 0,
    }; ARP_CACHE_SIZE];
    let mut count: usize = 0;

    for entry in cache.iter() {
        if entry.is_fresh(now) {
            let age_ns = now.saturating_sub(entry.updated_ns);
            let remaining_ns = ARP_ENTRY_LIFETIME_NS.saturating_sub(age_ns);
            if let Some(slot) = out.get_mut(count) {
                *slot = ArpCacheEntry {
                    ip: entry.ip,
                    mac: entry.mac,
                    ttl_secs: remaining_ns / 1_000_000_000,
                };
                count = count.wrapping_add(1);
            }
        }
    }

    (out, count)
}

/// Flush (invalidate) all ARP cache entries.
///
/// Forces re-resolution on the next send to any IP.  Useful after
/// interface reconfiguration or when a network topology change is
/// suspected.
pub fn flush_cache() {
    let mut cache = ARP_CACHE.lock();
    for entry in cache.iter_mut() {
        entry.valid = false;
    }
    crate::serial_println!("[arp] Cache flushed");
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// ARP unit tests — exercises packet parsing, cache insert/lookup, and
/// rejection of invalid inputs.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[arp] Running ARP self-test...");

    test_parse_valid_request()?;
    test_parse_too_short()?;
    test_parse_wrong_hw_type()?;
    test_build_roundtrip()?;
    test_cache_insert_lookup()?;
    test_cache_reject_multicast()?;
    test_cache_flush()?;

    // Per-namespace ARP tests run separately via ns_self_test()
    // because they require netns::init() which runs later in boot.

    crate::serial_println!("[arp] ARP self-test PASSED");
    Ok(())
}

/// Test parsing a well-formed ARP request.
fn test_parse_valid_request() -> KernelResult<()> {
    let our_mac = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    let our_ip = Ipv4Addr([192, 168, 1, 100]);
    let target_mac = MacAddress([0x00; 6]);
    let target_ip = Ipv4Addr([192, 168, 1, 1]);

    let pkt = build_arp(ARP_REQUEST, &our_mac, our_ip, &target_mac, target_ip);
    let (op, s_mac, s_ip, _t_mac, t_ip) = parse_arp(&pkt)?;

    if op != ARP_REQUEST {
        crate::serial_println!("[arp]   FAIL: expected REQUEST, got {}", op);
        return Err(KernelError::InternalError);
    }
    if s_mac.0 != our_mac.0 || s_ip != our_ip || t_ip != target_ip {
        crate::serial_println!("[arp]   FAIL: parsed fields don't match built packet");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[arp]   parse valid request: OK");
    Ok(())
}

/// Test that a too-short packet is rejected.
fn test_parse_too_short() -> KernelResult<()> {
    let short = [0u8; 10];
    if parse_arp(&short).is_ok() {
        crate::serial_println!("[arp]   FAIL: accepted too-short packet");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[arp]   parse too-short: OK (rejected)");
    Ok(())
}

/// Test that a non-Ethernet hardware type is rejected.
fn test_parse_wrong_hw_type() -> KernelResult<()> {
    let our_mac = MacAddress([0x11; 6]);
    let our_ip = Ipv4Addr([10, 0, 0, 1]);
    let target_mac = MacAddress([0x00; 6]);
    let target_ip = Ipv4Addr([10, 0, 0, 2]);

    let mut pkt = build_arp(ARP_REQUEST, &our_mac, our_ip, &target_mac, target_ip);
    // Corrupt the hardware type field (bytes 0-1).
    pkt[0] = 0xFF;
    pkt[1] = 0xFF;

    if parse_arp(&pkt).is_ok() {
        crate::serial_println!("[arp]   FAIL: accepted non-Ethernet hw type");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[arp]   parse wrong hw type: OK (rejected)");
    Ok(())
}

/// Test that build + parse round-trips correctly for ARP reply.
fn test_build_roundtrip() -> KernelResult<()> {
    let src_mac = MacAddress([0x02, 0x42, 0xAC, 0x11, 0x00, 0x02]);
    let src_ip = Ipv4Addr([172, 17, 0, 2]);
    let dst_mac = MacAddress([0x02, 0x42, 0xAC, 0x11, 0x00, 0x01]);
    let dst_ip = Ipv4Addr([172, 17, 0, 1]);

    let pkt = build_arp(ARP_REPLY, &src_mac, src_ip, &dst_mac, dst_ip);
    let (op, s_mac, s_ip, t_mac, t_ip) = parse_arp(&pkt)?;

    if op != ARP_REPLY || s_mac.0 != src_mac.0 || s_ip != src_ip
        || t_mac.0 != dst_mac.0 || t_ip != dst_ip
    {
        crate::serial_println!("[arp]   FAIL: round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[arp]   build/parse round-trip: OK");
    Ok(())
}

/// Test that cache_insert + lookup works for valid unicast IPs.
fn test_cache_insert_lookup() -> KernelResult<()> {
    let ip = Ipv4Addr([10, 99, 99, 99]);
    let mac = MacAddress([0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]);

    // Should not be in cache initially.
    flush_cache();
    if lookup(ip).is_some() {
        crate::serial_println!("[arp]   FAIL: found entry before insert");
        return Err(KernelError::InternalError);
    }

    cache_insert(ip, mac);

    match lookup(ip) {
        Some(found) if found.0 == mac.0 => {}
        Some(found) => {
            crate::serial_println!("[arp]   FAIL: wrong MAC {:?}", found.0);
            return Err(KernelError::InternalError);
        }
        None => {
            crate::serial_println!("[arp]   FAIL: entry not found after insert");
            return Err(KernelError::InternalError);
        }
    }

    flush_cache();
    crate::serial_println!("[arp]   cache insert/lookup: OK");
    Ok(())
}

/// Test that multicast/broadcast IPs are rejected by cache_insert.
fn test_cache_reject_multicast() -> KernelResult<()> {
    flush_cache();

    // 224.0.0.1 is multicast — should be rejected.
    let mcast_ip = Ipv4Addr([224, 0, 0, 1]);
    let mac = MacAddress([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]);
    cache_insert(mcast_ip, mac);

    if lookup(mcast_ip).is_some() {
        crate::serial_println!("[arp]   FAIL: multicast IP was cached");
        flush_cache();
        return Err(KernelError::InternalError);
    }

    // 127.0.0.1 is loopback — should be rejected.
    let lo_ip = Ipv4Addr([127, 0, 0, 1]);
    cache_insert(lo_ip, mac);

    if lookup(lo_ip).is_some() {
        crate::serial_println!("[arp]   FAIL: loopback IP was cached");
        flush_cache();
        return Err(KernelError::InternalError);
    }

    flush_cache();
    crate::serial_println!("[arp]   cache reject multicast/loopback: OK");
    Ok(())
}

/// Test that flush_cache invalidates all entries.
fn test_cache_flush() -> KernelResult<()> {
    let ip1 = Ipv4Addr([10, 1, 1, 1]);
    let ip2 = Ipv4Addr([10, 2, 2, 2]);
    let mac1 = MacAddress([0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    let mac2 = MacAddress([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);

    flush_cache();
    cache_insert(ip1, mac1);
    cache_insert(ip2, mac2);

    // Both should be present.
    if lookup(ip1).is_none() || lookup(ip2).is_none() {
        crate::serial_println!("[arp]   FAIL: entries missing before flush");
        flush_cache();
        return Err(KernelError::InternalError);
    }

    flush_cache();

    if lookup(ip1).is_some() || lookup(ip2).is_some() {
        crate::serial_println!("[arp]   FAIL: entries survived flush");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[arp]   cache flush: OK");
    Ok(())
}

/// Self-test for per-namespace ARP cache.
///
/// Must be called after `netns::init()`.  Exercises namespace isolation,
/// root delegation, and cache lifecycle.
pub fn ns_self_test() -> KernelResult<()> {
    crate::serial_println!("[arp-ns] Running per-namespace ARP self-test...");

    test_ns_arp_isolation()?;
    test_ns_arp_root_delegates()?;
    test_ns_arp_lifecycle()?;
    test_ns_arp_process_learns_into_ns()?;

    crate::serial_println!("[arp-ns] Per-namespace ARP self-test PASSED (4 tests)");
    Ok(())
}

/// Test that `process_arp` learns a sender's MAC into the *arrival*
/// namespace's cache — not the global cache — closing the D-CNET-NSRX
/// residual: two container networks reusing an IP no longer collide.
fn test_ns_arp_process_learns_into_ns() -> KernelResult<()> {
    let ns = crate::netns::create()?;
    ns_init(ns)?;
    // Start from an empty global cache so a "not in global" assertion is
    // meaningful.
    flush_cache();

    let sender_ip = Ipv4Addr([10, 42, 0, 7]);
    let sender_mac = MacAddress([0x02, 0xDE, 0xAD, 0xBE, 0xEF, 0x01]);
    let our_mac = MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x99]);

    // An ARP reply frame arriving in the container namespace.  (Reply, not
    // request, so process_arp only learns and does not try to egress.)
    let frame = build_arp(
        ARP_REPLY,
        &sender_mac,
        sender_ip,
        &our_mac,
        Ipv4Addr([10, 42, 0, 1]),
    );

    process_arp(&frame, ns)?;

    // Learned into the namespace cache…
    match ns_lookup(ns, sender_ip) {
        Some(m) if m.0 == sender_mac.0 => {}
        other => {
            crate::serial_println!(
                "[arp]   FAIL: process_arp did not learn into ns cache: {:?}",
                other.map(|m| m.0)
            );
            ns_destroy(ns);
            crate::netns::delete(ns)?;
            return Err(KernelError::InternalError);
        }
    }

    // …and NOT into the global cache (no cross-namespace leak).
    if lookup(sender_ip).is_some() {
        crate::serial_println!("[arp]   FAIL: ns ARP leaked into global cache");
        ns_destroy(ns);
        crate::netns::delete(ns)?;
        return Err(KernelError::InternalError);
    }

    ns_destroy(ns);
    crate::netns::delete(ns)?;
    crate::serial_println!("[arp]   ns process_arp learns into ns cache: OK");
    Ok(())
}

/// Test that two namespaces have isolated ARP caches.
fn test_ns_arp_isolation() -> KernelResult<()> {
    // Create two child namespaces.
    let ns1 = crate::netns::create()?;
    let ns2 = crate::netns::create()?;

    // Initialize per-namespace ARP caches.
    ns_init(ns1)?;
    ns_init(ns2)?;

    let ip = Ipv4Addr([10, 0, 0, 5]);
    let mac1 = MacAddress([0xAA, 0x11, 0x22, 0x33, 0x44, 0x55]);
    let mac2 = MacAddress([0xBB, 0x66, 0x77, 0x88, 0x99, 0xAA]);

    // Insert different MACs for the same IP into each namespace.
    ns_insert(ns1, ip, mac1);
    ns_insert(ns2, ip, mac2);

    // Each namespace should see its own MAC.
    let found1 = ns_lookup(ns1, ip);
    let found2 = ns_lookup(ns2, ip);

    match (found1, found2) {
        (Some(f1), Some(f2)) if f1.0 == mac1.0 && f2.0 == mac2.0 => {}
        _ => {
            crate::serial_println!(
                "[arp]   FAIL: ns isolation: ns1={:?} ns2={:?}",
                found1.map(|m| m.0), found2.map(|m| m.0)
            );
            ns_destroy(ns1);
            ns_destroy(ns2);
            crate::netns::delete(ns1)?;
            crate::netns::delete(ns2)?;
            return Err(KernelError::InternalError);
        }
    }

    // Flush ns1 — ns2 should be unaffected.
    ns_flush(ns1);
    if ns_lookup(ns1, ip).is_some() {
        crate::serial_println!("[arp]   FAIL: ns1 not flushed");
        ns_destroy(ns1);
        ns_destroy(ns2);
        crate::netns::delete(ns1)?;
        crate::netns::delete(ns2)?;
        return Err(KernelError::InternalError);
    }
    if ns_lookup(ns2, ip).is_none() {
        crate::serial_println!("[arp]   FAIL: ns2 affected by ns1 flush");
        ns_destroy(ns1);
        ns_destroy(ns2);
        crate::netns::delete(ns1)?;
        crate::netns::delete(ns2)?;
        return Err(KernelError::InternalError);
    }

    // Entry count check.
    if ns_entry_count(ns1) != 0 {
        crate::serial_println!("[arp]   FAIL: ns1 count should be 0");
        ns_destroy(ns1);
        ns_destroy(ns2);
        crate::netns::delete(ns1)?;
        crate::netns::delete(ns2)?;
        return Err(KernelError::InternalError);
    }
    if ns_entry_count(ns2) != 1 {
        crate::serial_println!("[arp]   FAIL: ns2 count should be 1");
        ns_destroy(ns1);
        ns_destroy(ns2);
        crate::netns::delete(ns1)?;
        crate::netns::delete(ns2)?;
        return Err(KernelError::InternalError);
    }

    // Cleanup.
    ns_destroy(ns1);
    ns_destroy(ns2);
    crate::netns::delete(ns1)?;
    crate::netns::delete(ns2)?;

    crate::serial_println!("[arp]   ns ARP isolation: OK");
    Ok(())
}

/// Test that root namespace operations delegate to the global cache.
fn test_ns_arp_root_delegates() -> KernelResult<()> {
    flush_cache();

    let ip = Ipv4Addr([172, 16, 0, 99]);
    let mac = MacAddress([0xCC, 0xDD, 0xEE, 0x11, 0x22, 0x33]);

    // Insert via ns_insert for root.
    ns_insert(crate::netns::ROOT_NS, ip, mac);

    // Should be visible via both ns_lookup (root) and global lookup.
    match ns_lookup(crate::netns::ROOT_NS, ip) {
        Some(f) if f.0 == mac.0 => {}
        other => {
            crate::serial_println!("[arp]   FAIL: root ns_lookup: {:?}", other.map(|m| m.0));
            flush_cache();
            return Err(KernelError::InternalError);
        }
    }
    match lookup(ip) {
        Some(f) if f.0 == mac.0 => {}
        other => {
            crate::serial_println!("[arp]   FAIL: root global lookup: {:?}", other.map(|m| m.0));
            flush_cache();
            return Err(KernelError::InternalError);
        }
    }

    // ns_init for root should fail.
    if ns_init(crate::netns::ROOT_NS).is_ok() {
        crate::serial_println!("[arp]   FAIL: ns_init(root) should fail");
        flush_cache();
        return Err(KernelError::InternalError);
    }

    flush_cache();
    crate::serial_println!("[arp]   ns ARP root delegation: OK");
    Ok(())
}

/// Test ARP cache lifecycle: init, use, destroy, verify gone.
fn test_ns_arp_lifecycle() -> KernelResult<()> {
    let ns = crate::netns::create()?;
    ns_init(ns)?;

    let ip = Ipv4Addr([192, 168, 100, 1]);
    let mac = MacAddress([0x02, 0xFE, 0x0A, 0x00, 0x00, 0x01]);

    ns_insert(ns, ip, mac);
    if ns_lookup(ns, ip).is_none() {
        crate::serial_println!("[arp]   FAIL: entry not found after insert");
        ns_destroy(ns);
        crate::netns::delete(ns)?;
        return Err(KernelError::InternalError);
    }

    // Destroy the ARP cache.
    ns_destroy(ns);

    // Lookup should return None now.
    if ns_lookup(ns, ip).is_some() {
        crate::serial_println!("[arp]   FAIL: entry found after destroy");
        crate::netns::delete(ns)?;
        return Err(KernelError::InternalError);
    }

    // ns_cache_entries should return empty.
    let (_, count) = ns_cache_entries(ns);
    if count != 0 {
        crate::serial_println!("[arp]   FAIL: entries after destroy: {}", count);
        crate::netns::delete(ns)?;
        return Err(KernelError::InternalError);
    }

    // Re-init should work (reuse slot).
    ns_init(ns)?;
    ns_insert(ns, ip, mac);
    if ns_lookup(ns, ip).is_none() {
        crate::serial_println!("[arp]   FAIL: entry not found after re-init");
        ns_destroy(ns);
        crate::netns::delete(ns)?;
        return Err(KernelError::InternalError);
    }

    ns_destroy(ns);
    crate::netns::delete(ns)?;
    crate::serial_println!("[arp]   ns ARP lifecycle: OK");
    Ok(())
}
