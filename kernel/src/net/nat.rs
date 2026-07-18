//! NAT (Network Address Translation) for container networking.
//!
//! Provides SNAT/masquerade functionality that allows containers with
//! private IP addresses to communicate with the outside world through
//! the host's public IP address.
//!
//! ## How it works
//!
//! 1. Container sends packet (src=10.88.0.2:1234, dst=93.184.216.34:80)
//! 2. NAT rewrites source: (src=host_ip:NAT_PORT, dst=93.184.216.34:80)
//! 3. Reply arrives: (src=93.184.216.34:80, dst=host_ip:NAT_PORT)
//! 4. NAT rewrites destination: (src=93.184.216.34:80, dst=10.88.0.2:1234)
//!
//! ## Port mapping table
//!
//! Each NAT entry maps:
//!   (original_src_ip, original_src_port, protocol, dst_ip, dst_port)
//!   → (nat_port, namespace_id)
//!
//! NAT ports are allocated from range 32768-60999 (ephemeral range).
//!
//! ## Limitations
//!
//! - IPv4 only (IPv6 doesn't normally need NAT)
//! - TCP and UDP only (no ICMP NAT yet)
//! - Maximum 256 concurrent NAT mappings
//! - No port forwarding / DNAT (future work)

use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::netns::NetNsId;
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum concurrent NAT entries.
const MAX_NAT_ENTRIES: usize = 256;

/// Start of NAT ephemeral port range.
const NAT_PORT_START: u16 = 32768;

/// End of NAT ephemeral port range (inclusive).
const NAT_PORT_END: u16 = 60999;

/// Entry lifetime in seconds (5 minutes for TCP, 30s for UDP).
const TCP_TIMEOUT_SECS: u32 = 300;
const UDP_TIMEOUT_SECS: u32 = 30;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Protocol for NAT tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatProto {
    Tcp,
    Udp,
}

/// A single NAT mapping entry.
#[derive(Debug, Clone, Copy)]
struct NatEntry {
    /// Whether this slot is in use.
    active: bool,
    /// Protocol (TCP or UDP).
    proto: NatProto,
    /// Original source IP (container's private IP).
    orig_src_ip: Ipv4Addr,
    /// Original source port (container's port).
    orig_src_port: u16,
    /// Destination IP (remote server).
    dst_ip: Ipv4Addr,
    /// Destination port (remote service).
    dst_port: u16,
    /// The NAT port allocated on the host side.
    nat_port: u16,
    /// Network namespace of the originating container.
    ns_id: NetNsId,
    /// Remaining lifetime in seconds (decremented by tick).
    ttl: u32,
}

/// NAT table state.
struct NatTable {
    entries: [NatEntry; MAX_NAT_ENTRIES],
    /// Next port to try (round-robin allocation).
    next_port: u16,
    /// Whether NAT is globally enabled.
    enabled: bool,
    /// Total translations performed.
    translations_out: u64,
    /// Total reverse translations performed.
    translations_in: u64,
    /// Total entries created.
    entries_created: u64,
    /// Total entries expired.
    entries_expired: u64,
}

/// Public statistics for diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct NatStats {
    pub enabled: bool,
    pub active_entries: usize,
    pub max_entries: usize,
    pub translations_out: u64,
    pub translations_in: u64,
    pub entries_created: u64,
    pub entries_expired: u64,
}

/// Result of a NAT lookup (for reverse translation).
#[derive(Debug, Clone, Copy)]
pub struct NatMapping {
    pub orig_src_ip: Ipv4Addr,
    pub orig_src_port: u16,
    pub ns_id: NetNsId,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static NAT: Mutex<NatTable> = Mutex::new(NatTable {
    entries: [NatEntry {
        active: false,
        proto: NatProto::Tcp,
        orig_src_ip: Ipv4Addr::UNSPECIFIED,
        orig_src_port: 0,
        dst_ip: Ipv4Addr::UNSPECIFIED,
        dst_port: 0,
        nat_port: 0,
        ns_id: 0,
        ttl: 0,
    }; MAX_NAT_ENTRIES],
    next_port: NAT_PORT_START,
    enabled: false,
    translations_out: 0,
    translations_in: 0,
    entries_created: 0,
    entries_expired: 0,
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Enable NAT globally.
pub fn enable() {
    NAT.lock().enabled = true;
    crate::serial_println!("[nat] NAT/masquerade enabled");
}

/// Disable NAT globally.
pub fn disable() {
    NAT.lock().enabled = false;
    crate::serial_println!("[nat] NAT/masquerade disabled");
}

/// Check if NAT is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    NAT.lock().enabled
}

/// Translate an outgoing packet from a container namespace.
///
/// If the source namespace is non-root and NAT is enabled, creates or
/// reuses a NAT mapping and returns the NAT port to use as the new
/// source port. Returns `None` if NAT is not needed (root NS) or not
/// enabled.
///
/// # Arguments
/// - `ns_id`: The originating namespace (0 = root, no NAT needed)
/// - `proto`: TCP or UDP
/// - `src_ip`: Container's private source IP
/// - `src_port`: Container's source port
/// - `dst_ip`: Remote destination IP
/// - `dst_port`: Remote destination port
pub fn translate_outgoing(
    ns_id: NetNsId,
    proto: NatProto,
    src_ip: Ipv4Addr,
    src_port: u16,
    dst_ip: Ipv4Addr,
    dst_port: u16,
) -> Option<u16> {
    // Root namespace doesn't need NAT.
    if ns_id == crate::netns::ROOT_NS {
        return None;
    }

    let mut table = NAT.lock();
    if !table.enabled {
        return None;
    }

    // Check for existing mapping (reuse if same 5-tuple).
    let existing = table.entries.iter().find(|entry| {
        entry.active
            && entry.proto == proto
            && entry.ns_id == ns_id
            && entry.orig_src_ip == src_ip
            && entry.orig_src_port == src_port
            && entry.dst_ip == dst_ip
            && entry.dst_port == dst_port
    }).map(|e| e.nat_port);

    if let Some(port) = existing {
        table.translations_out = table.translations_out.wrapping_add(1);
        return Some(port);
    }

    // Allocate a new NAT port and entry.
    let nat_port = alloc_port(&mut table, proto)?;

    // Find a free slot.
    let slot = table.entries.iter().position(|e| !e.active)?;

    let ttl = match proto {
        NatProto::Tcp => TCP_TIMEOUT_SECS,
        NatProto::Udp => UDP_TIMEOUT_SECS,
    };

    table.entries[slot] = NatEntry {
        active: true,
        proto,
        orig_src_ip: src_ip,
        orig_src_port: src_port,
        dst_ip,
        dst_port,
        nat_port,
        ns_id,
        ttl,
    };

    table.translations_out = table.translations_out.wrapping_add(1);
    table.entries_created = table.entries_created.wrapping_add(1);

    Some(nat_port)
}

/// Look up an incoming packet's NAT mapping by (proto, nat_port, remote_ip, remote_port).
///
/// Returns the original source IP/port and namespace if a mapping exists.
/// Used for reverse-translating replies back to the container.
pub fn translate_incoming(
    proto: NatProto,
    nat_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
) -> Option<NatMapping> {
    let mut table = NAT.lock();
    if !table.enabled {
        return None;
    }

    // Find matching entry by index to avoid borrow conflicts.
    let found = (0..MAX_NAT_ENTRIES).find(|&i| {
        let e = &table.entries[i];
        e.active
            && e.proto == proto
            && e.nat_port == nat_port
            && e.dst_ip == remote_ip
            && e.dst_port == remote_port
    });

    if let Some(idx) = found {
        // Refresh TTL on activity.
        table.entries[idx].ttl = match proto {
            NatProto::Tcp => TCP_TIMEOUT_SECS,
            NatProto::Udp => UDP_TIMEOUT_SECS,
        };
        table.translations_in = table.translations_in.wrapping_add(1);
        return Some(NatMapping {
            orig_src_ip: table.entries[idx].orig_src_ip,
            orig_src_port: table.entries[idx].orig_src_port,
            ns_id: table.entries[idx].ns_id,
        });
    }

    None
}

/// Periodic tick: decrement TTL and expire old entries.
///
/// Call once per second from the network polling loop.
pub fn tick() {
    let mut table = NAT.lock();
    if !table.enabled {
        return;
    }

    let mut expired_count: u64 = 0;
    for i in 0..MAX_NAT_ENTRIES {
        if table.entries[i].active {
            if table.entries[i].ttl == 0 {
                table.entries[i].active = false;
                expired_count = expired_count.wrapping_add(1);
            } else {
                table.entries[i].ttl = table.entries[i].ttl.wrapping_sub(1);
            }
        }
    }
    table.entries_expired = table.entries_expired.wrapping_add(expired_count);
}

/// Remove all NAT entries for a specific namespace.
///
/// Called when a container is deleted to clean up stale mappings.
pub fn flush_namespace(ns_id: NetNsId) {
    let mut table = NAT.lock();
    let mut flushed: u64 = 0;
    for i in 0..MAX_NAT_ENTRIES {
        if table.entries[i].active && table.entries[i].ns_id == ns_id {
            table.entries[i].active = false;
            flushed = flushed.wrapping_add(1);
        }
    }
    table.entries_expired = table.entries_expired.wrapping_add(flushed);
}

/// Get NAT statistics.
#[must_use]
pub fn stats() -> NatStats {
    let table = NAT.lock();
    let active = table.entries.iter().filter(|e| e.active).count();
    NatStats {
        enabled: table.enabled,
        active_entries: active,
        max_entries: MAX_NAT_ENTRIES,
        translations_out: table.translations_out,
        translations_in: table.translations_in,
        entries_created: table.entries_created,
        entries_expired: table.entries_expired,
    }
}

/// A NAT table entry snapshot: (protocol, original source IP, original source
/// port, destination IP, destination port, translated NAT port, namespace,
/// remaining TTL).
pub type NatEntryInfo = (NatProto, Ipv4Addr, u16, Ipv4Addr, u16, u16, NetNsId, u32);

/// List all active NAT entries (for diagnostics / kshell `nat list`).
pub fn list_entries() -> Vec<NatEntryInfo> {
    let table = NAT.lock();
    let mut result = Vec::new();
    for entry in table.entries.iter() {
        if entry.active {
            result.push((
                entry.proto,
                entry.orig_src_ip,
                entry.orig_src_port,
                entry.dst_ip,
                entry.dst_port,
                entry.nat_port,
                entry.ns_id,
                entry.ttl,
            ));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Port forwarding (DNAT)
// ---------------------------------------------------------------------------

/// Maximum static port-forward rules.
const MAX_PORT_FORWARDS: usize = 32;

/// A static port-forwarding rule (DNAT).
///
/// Maps (proto, host_port) → (container_ip, container_port, ns_id).
/// When a packet arrives at host_port from the outside, it is forwarded
/// to the container's IP:port within the specified namespace.
#[derive(Debug, Clone, Copy)]
struct PortForward {
    active: bool,
    proto: NatProto,
    /// External port on the host.
    host_port: u16,
    /// Internal container IP to forward to.
    container_ip: Ipv4Addr,
    /// Internal container port to forward to.
    container_port: u16,
    /// Namespace ID of the target container.
    ns_id: NetNsId,
}

/// Port forwarding table.
static PORT_FORWARDS: Mutex<[PortForward; MAX_PORT_FORWARDS]> = Mutex::new(
    [PortForward {
        active: false,
        proto: NatProto::Tcp,
        host_port: 0,
        container_ip: Ipv4Addr::UNSPECIFIED,
        container_port: 0,
        ns_id: 0,
    }; MAX_PORT_FORWARDS]
);

/// Port-forward lookup result.
#[derive(Debug, Clone, Copy)]
pub struct PortForwardTarget {
    pub container_ip: Ipv4Addr,
    pub container_port: u16,
    pub ns_id: NetNsId,
}

/// Add a port-forwarding rule.
///
/// When packets arrive at `host_port`, they will be forwarded to
/// `container_ip:container_port` in namespace `ns_id`.
///
/// # Errors
/// - `OutOfMemory` if the forwarding table is full.
/// - `InvalidArgument` if a rule for this (proto, host_port) already exists.
pub fn add_port_forward(
    proto: NatProto,
    host_port: u16,
    container_ip: Ipv4Addr,
    container_port: u16,
    ns_id: NetNsId,
) -> KernelResult<()> {
    let mut rules = PORT_FORWARDS.lock();

    // Check for duplicates.
    let exists = rules.iter().any(|r| {
        r.active && r.proto == proto && r.host_port == host_port
    });
    if exists {
        return Err(KernelError::InvalidArgument);
    }

    // Find free slot.
    let slot = rules.iter().position(|r| !r.active)
        .ok_or(KernelError::OutOfMemory)?;

    rules[slot] = PortForward {
        active: true,
        proto,
        host_port,
        container_ip,
        container_port,
        ns_id,
    };

    crate::serial_println!(
        "[nat] Port forward: {:?} :{} → {}:{} (ns={})",
        proto, host_port, container_ip, container_port, ns_id
    );

    Ok(())
}

/// Remove a port-forwarding rule.
pub fn remove_port_forward(proto: NatProto, host_port: u16) -> KernelResult<()> {
    let mut rules = PORT_FORWARDS.lock();
    let slot = rules.iter().position(|r| {
        r.active && r.proto == proto && r.host_port == host_port
    }).ok_or(KernelError::NotFound)?;
    rules[slot].active = false;
    Ok(())
}

/// Remove all port-forwarding rules for a namespace.
///
/// Called when a container is deleted.
pub fn flush_port_forwards(ns_id: NetNsId) {
    let mut rules = PORT_FORWARDS.lock();
    for rule in rules.iter_mut() {
        if rule.active && rule.ns_id == ns_id {
            rule.active = false;
        }
    }
}

/// Look up a port-forward rule for an incoming packet.
///
/// Returns the container target if a rule exists for (proto, host_port).
pub fn lookup_port_forward(proto: NatProto, host_port: u16) -> Option<PortForwardTarget> {
    let rules = PORT_FORWARDS.lock();
    rules.iter().find(|r| {
        r.active && r.proto == proto && r.host_port == host_port
    }).map(|r| PortForwardTarget {
        container_ip: r.container_ip,
        container_port: r.container_port,
        ns_id: r.ns_id,
    })
}

/// List all active port-forwarding rules (for diagnostics).
pub fn list_port_forwards() -> Vec<(NatProto, u16, Ipv4Addr, u16, NetNsId)> {
    let rules = PORT_FORWARDS.lock();
    let mut result = Vec::new();
    for rule in rules.iter() {
        if rule.active {
            result.push((
                rule.proto,
                rule.host_port,
                rule.container_ip,
                rule.container_port,
                rule.ns_id,
            ));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Allocate a unique NAT port (round-robin in ephemeral range).
fn alloc_port(table: &mut NatTable, proto: NatProto) -> Option<u16> {
    let range_size = NAT_PORT_END.wrapping_sub(NAT_PORT_START).wrapping_add(1);

    // Try each port in the range exactly once.
    for _ in 0..range_size {
        let candidate = table.next_port;

        // Advance (with wrap).
        table.next_port = if table.next_port >= NAT_PORT_END {
            NAT_PORT_START
        } else {
            table.next_port.wrapping_add(1)
        };

        // Check for collision with existing entries of the same protocol.
        let in_use = table.entries.iter().any(|e| {
            e.active && e.proto == proto && e.nat_port == candidate
        });

        if !in_use {
            return Some(candidate);
        }
    }

    // All ports exhausted.
    None
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// NAT self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[nat] Running self-test...");
    let mut passed: u32 = 0;

    // Test 1: Enable/disable.
    {
        assert!(!is_enabled());
        enable();
        assert!(is_enabled());
        disable();
        assert!(!is_enabled());
        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Enable/disable: OK");
    }

    // Test 2: Root namespace doesn't get NAT'd.
    {
        enable();
        let result = translate_outgoing(
            crate::netns::ROOT_NS,
            NatProto::Tcp,
            Ipv4Addr::new(10, 0, 2, 15),
            1234,
            Ipv4Addr::new(93, 184, 216, 34),
            80,
        );
        assert!(result.is_none(), "root NS should not be NAT'd");
        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Root NS bypass: OK");
    }

    // Test 3: Non-root namespace gets NAT port.
    {
        let ns: NetNsId = 42;
        let result = translate_outgoing(
            ns,
            NatProto::Tcp,
            Ipv4Addr::new(10, 88, 0, 2),
            5000,
            Ipv4Addr::new(93, 184, 216, 34),
            80,
        );
        assert!(result.is_some(), "non-root should get NAT port");
        let nat_port = result.unwrap();
        assert!(nat_port >= NAT_PORT_START && nat_port <= NAT_PORT_END,
            "NAT port should be in ephemeral range");

        // Same 5-tuple reuses the same port.
        let result2 = translate_outgoing(
            ns,
            NatProto::Tcp,
            Ipv4Addr::new(10, 88, 0, 2),
            5000,
            Ipv4Addr::new(93, 184, 216, 34),
            80,
        );
        assert_eq!(result2, Some(nat_port), "same 5-tuple should reuse port");
        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Outgoing translation: OK");
    }

    // Test 4: Reverse translation.
    {
        let ns: NetNsId = 42;
        // Get the NAT port from the entry we created.
        let nat_port = translate_outgoing(
            ns,
            NatProto::Tcp,
            Ipv4Addr::new(10, 88, 0, 2),
            5000,
            Ipv4Addr::new(93, 184, 216, 34),
            80,
        ).unwrap();

        // Simulate reply from 93.184.216.34:80 → host:nat_port.
        let mapping = translate_incoming(
            NatProto::Tcp,
            nat_port,
            Ipv4Addr::new(93, 184, 216, 34),
            80,
        );
        assert!(mapping.is_some(), "reverse lookup should find entry");
        let m = mapping.unwrap();
        assert_eq!(m.orig_src_ip, Ipv4Addr::new(10, 88, 0, 2));
        assert_eq!(m.orig_src_port, 5000);
        assert_eq!(m.ns_id, ns);
        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Incoming reverse translation: OK");
    }

    // Test 5: Wrong remote doesn't match.
    {
        let nat_port = {
            let table = NAT.lock();
            table.entries.iter().find(|e| e.active).map(|e| e.nat_port).unwrap()
        };
        let mapping = translate_incoming(
            NatProto::Tcp,
            nat_port,
            Ipv4Addr::new(1, 2, 3, 4), // wrong remote IP
            80,
        );
        assert!(mapping.is_none(), "wrong remote should not match");
        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Wrong remote rejected: OK");
    }

    // Test 6: Flush namespace.
    {
        let ns: NetNsId = 42;
        let s = stats();
        let before = s.active_entries;
        flush_namespace(ns);
        let s2 = stats();
        assert!(s2.active_entries < before, "flush should remove entries");
        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Flush namespace: OK");
    }

    // Test 7: UDP NAT.
    {
        let ns: NetNsId = 77;
        let result = translate_outgoing(
            ns,
            NatProto::Udp,
            Ipv4Addr::new(10, 88, 0, 3),
            9000,
            Ipv4Addr::new(8, 8, 8, 8),
            53,
        );
        assert!(result.is_some(), "UDP NAT should work");

        let nat_port = result.unwrap();
        let mapping = translate_incoming(
            NatProto::Udp,
            nat_port,
            Ipv4Addr::new(8, 8, 8, 8),
            53,
        );
        assert!(mapping.is_some(), "UDP reverse should work");
        assert_eq!(mapping.unwrap().ns_id, ns);

        flush_namespace(ns);
        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   UDP NAT: OK");
    }

    // Test 8: Stats tracking.
    {
        let s = stats();
        assert!(s.entries_created > 0, "should have created entries");
        assert!(s.translations_out > 0, "should have outgoing translations");
        assert!(s.translations_in > 0, "should have incoming translations");
        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Stats tracking: OK");
    }

    // Cleanup: disable NAT.
    disable();

    // Test 9: Port forwarding — add rule.
    {
        let result = add_port_forward(
            NatProto::Tcp,
            8080,
            Ipv4Addr::new(10, 88, 0, 5),
            80,
            99,
        );
        assert!(result.is_ok(), "should add port forward");

        // Duplicate should fail.
        let dup = add_port_forward(
            NatProto::Tcp,
            8080,
            Ipv4Addr::new(10, 88, 0, 6),
            80,
            100,
        );
        assert!(dup.is_err(), "duplicate host_port should fail");

        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Port forward add: OK");
    }

    // Test 10: Port forwarding — lookup.
    {
        let target = lookup_port_forward(NatProto::Tcp, 8080);
        assert!(target.is_some(), "should find forward rule");
        let t = target.unwrap();
        assert_eq!(t.container_ip, Ipv4Addr::new(10, 88, 0, 5));
        assert_eq!(t.container_port, 80);
        assert_eq!(t.ns_id, 99);

        // Wrong protocol should not match.
        let miss = lookup_port_forward(NatProto::Udp, 8080);
        assert!(miss.is_none(), "wrong proto should not match");

        // Wrong port should not match.
        let miss2 = lookup_port_forward(NatProto::Tcp, 9999);
        assert!(miss2.is_none(), "wrong port should not match");

        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Port forward lookup: OK");
    }

    // Test 11: Port forwarding — remove.
    {
        let result = remove_port_forward(NatProto::Tcp, 8080);
        assert!(result.is_ok(), "should remove forward rule");

        let target = lookup_port_forward(NatProto::Tcp, 8080);
        assert!(target.is_none(), "removed rule should not match");

        // Removing again should fail.
        let dup = remove_port_forward(NatProto::Tcp, 8080);
        assert!(dup.is_err(), "double remove should fail");

        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Port forward remove: OK");
    }

    // Test 12: Port forwarding — flush by namespace.
    {
        let _ = add_port_forward(NatProto::Tcp, 3000, Ipv4Addr::new(10, 88, 0, 7), 3000, 55);
        let _ = add_port_forward(NatProto::Udp, 5353, Ipv4Addr::new(10, 88, 0, 7), 53, 55);
        let _ = add_port_forward(NatProto::Tcp, 4000, Ipv4Addr::new(10, 88, 0, 8), 4000, 66);

        flush_port_forwards(55);

        // ns=55 rules should be gone.
        assert!(lookup_port_forward(NatProto::Tcp, 3000).is_none());
        assert!(lookup_port_forward(NatProto::Udp, 5353).is_none());

        // ns=66 rule should still exist.
        assert!(lookup_port_forward(NatProto::Tcp, 4000).is_some());

        // Cleanup.
        flush_port_forwards(66);

        passed = passed.wrapping_add(1);
        crate::serial_println!("[nat]   Port forward flush: OK");
    }

    crate::serial_println!("[nat] Self-test PASSED ({} tests)", passed);
    Ok(())
}
