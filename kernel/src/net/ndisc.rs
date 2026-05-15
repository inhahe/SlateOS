//! Network discovery — ARP scan and host enumeration.
//!
//! Discovers active hosts on the local subnet by sending ARP requests
//! for all IPs in the configured subnet range.  Combines ARP cache
//! information with optional ping probes for a comprehensive view
//! of the local network.
//!
//! ## Usage
//!
//! ```text
//! ndisc scan           — ARP scan the local subnet
//! ndisc scan 10.0.2.0/24  — scan a specific range
//! ndisc hosts          — show discovered hosts (from ARP cache)
//! ndisc probe <IP>     — ARP probe + ping a specific host
//! ```
//!
//! ## Features
//!
//! - ARP-based host discovery (no IP required on target — works for
//!   any device with an Ethernet interface)
//! - Subnet range calculation from interface config
//! - Combines ARP and DNS for richer host identification
//! - Host table with MAC, IP, optional hostname

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum hosts tracked.
const MAX_HOSTS: usize = 256;

/// Poll iterations per ARP probe (wait time for reply).
const ARP_PROBE_POLLS: u32 = 50;

/// Maximum subnet size to scan (/16 = 65536, but we cap at 256 for safety).
const MAX_SCAN_SIZE: u32 = 256;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A discovered host.
#[derive(Debug, Clone)]
pub struct Host {
    /// IP address.
    pub ip: Ipv4Addr,
    /// MAC address (from ARP).
    pub mac: String,
    /// Hostname (from DNS reverse lookup, if available).
    pub hostname: String,
    /// Last seen timestamp (ns).
    pub last_seen_ns: u64,
}

/// Whether a scan is in progress.
static SCANNING: AtomicBool = AtomicBool::new(false);

// Statistics.
static TOTAL_SCANS: AtomicU64 = AtomicU64::new(0);
static TOTAL_PROBES: AtomicU64 = AtomicU64::new(0);
static TOTAL_DISCOVERED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Subnet calculation
// ---------------------------------------------------------------------------

/// Calculate the network address from IP and mask.
pub fn network_addr(ip: Ipv4Addr, mask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr([
        ip.0[0] & mask.0[0],
        ip.0[1] & mask.0[1],
        ip.0[2] & mask.0[2],
        ip.0[3] & mask.0[3],
    ])
}

/// Calculate the broadcast address from IP and mask.
pub fn broadcast_addr(ip: Ipv4Addr, mask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr([
        ip.0[0] | !mask.0[0],
        ip.0[1] | !mask.0[1],
        ip.0[2] | !mask.0[2],
        ip.0[3] | !mask.0[3],
    ])
}

/// Count the number of host addresses in a subnet.
pub fn host_count(mask: Ipv4Addr) -> u32 {
    let mask_u32 = mask.to_u32();
    let host_bits = (!mask_u32).count_ones();
    if host_bits < 2 {
        return 0; // /31 or /32 — no usable host range.
    }
    // Host count = 2^host_bits - 2 (exclude network and broadcast).
    (1u32 << host_bits).saturating_sub(2)
}

/// Generate all host IPs in a subnet (excluding network and broadcast).
pub fn subnet_hosts(net: Ipv4Addr, mask: Ipv4Addr) -> Vec<Ipv4Addr> {
    let net_u32 = net.to_u32();
    let mask_u32 = mask.to_u32();
    let bcast_u32 = net_u32 | !mask_u32;
    let count = host_count(mask).min(MAX_SCAN_SIZE);

    let mut hosts = Vec::with_capacity(count as usize);
    // Start from network + 1, end at broadcast - 1.
    let start = net_u32.saturating_add(1);
    let end = bcast_u32.saturating_sub(1);

    let mut addr = start;
    while addr <= end && hosts.len() < count as usize {
        hosts.push(Ipv4Addr::from_u32(addr));
        addr = addr.saturating_add(1);
    }
    hosts
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Scan result.
#[derive(Debug, Clone)]
pub struct ScanResult {
    /// Hosts discovered.
    pub hosts: Vec<Host>,
    /// Total IPs probed.
    pub probed: u32,
    /// Total responding.
    pub responding: u32,
}

/// ARP scan the local subnet.
///
/// Sends ARP requests for all IPs in the subnet and collects responses.
pub fn scan_subnet() -> KernelResult<ScanResult> {
    let our_ip = super::interface::ip();
    let mask = super::interface::info().subnet_mask;

    if our_ip.is_unspecified() || mask.is_unspecified() {
        return Err(KernelError::NotSupported);
    }

    let net = network_addr(our_ip, mask);
    scan_range(net, mask)
}

/// ARP scan a specific IP range.
pub fn scan_range(net: Ipv4Addr, mask: Ipv4Addr) -> KernelResult<ScanResult> {
    if SCANNING.swap(true, Ordering::Relaxed) {
        return Err(KernelError::DeviceBusy);
    }

    TOTAL_SCANS.fetch_add(1, Ordering::Relaxed);

    let hosts_to_probe = subnet_hosts(net, mask);
    let total = hosts_to_probe.len() as u32;

    // Send ARP requests for all hosts.
    for host_ip in &hosts_to_probe {
        let _ = super::arp::send_request(*host_ip);
        TOTAL_PROBES.fetch_add(1, Ordering::Relaxed);
    }

    // Poll to collect responses.
    for _ in 0..ARP_PROBE_POLLS {
        super::poll();
    }

    // Collect discovered hosts from ARP cache.
    let (arp_entries, arp_count) = super::arp::cache_entries();
    let now = crate::hrtimer::now_ns();

    let mut discovered = Vec::new();
    for i in 0..arp_count {
        if let Some(entry) = arp_entries.get(i) {
            // Try reverse DNS.
            let hostname = super::dns::reverse_resolve(entry.ip)
                .unwrap_or_default();

            discovered.push(Host {
                ip: entry.ip,
                mac: format!("{}", entry.mac),
                hostname,
                last_seen_ns: now,
            });
        }
    }

    let responding = discovered.len() as u32;
    TOTAL_DISCOVERED.fetch_add(responding as u64, Ordering::Relaxed);
    SCANNING.store(false, Ordering::Relaxed);

    Ok(ScanResult {
        hosts: discovered,
        probed: total,
        responding,
    })
}

/// Probe a single IP address (ARP + optional ping).
pub fn probe_host(ip: Ipv4Addr) -> KernelResult<Option<Host>> {
    TOTAL_PROBES.fetch_add(1, Ordering::Relaxed);

    // Send ARP request.
    let _ = super::arp::send_request(ip);

    // Poll for response.
    for _ in 0..ARP_PROBE_POLLS {
        super::poll();
    }

    // Check ARP cache.
    let mac = super::arp::lookup(ip);
    match mac {
        Some(m) => {
            let hostname = super::dns::reverse_resolve(ip)
                .unwrap_or_default();
            let now = crate::hrtimer::now_ns();
            TOTAL_DISCOVERED.fetch_add(1, Ordering::Relaxed);

            Ok(Some(Host {
                ip,
                mac: format!("{}", m),
                hostname,
                last_seen_ns: now,
            }))
        }
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Network discovery statistics.
#[derive(Debug)]
pub struct NdiscStats {
    pub scanning: bool,
    pub total_scans: u64,
    pub total_probes: u64,
    pub total_discovered: u64,
}

/// Get statistics.
pub fn stats() -> NdiscStats {
    NdiscStats {
        scanning: SCANNING.load(Ordering::Relaxed),
        total_scans: TOTAL_SCANS.load(Ordering::Relaxed),
        total_probes: TOTAL_PROBES.load(Ordering::Relaxed),
        total_discovered: TOTAL_DISCOVERED.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/ndisc`.
pub fn procfs_content() -> String {
    let s = stats();
    let mut out = String::with_capacity(256);
    out.push_str("Network Discovery\n");
    out.push_str("=================\n\n");
    out.push_str(&format!("Scanning:    {}\n", if s.scanning { "yes" } else { "no" }));
    out.push_str(&format!("Total scans: {}\n", s.total_scans));
    out.push_str(&format!("Probes sent: {}\n", s.total_probes));
    out.push_str(&format!("Discovered:  {}\n", s.total_discovered));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run network discovery self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ndisc] Running network discovery self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Network address calculation ---
    {
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        let net = network_addr(ip, mask);
        assert!(net == Ipv4Addr::new(192, 168, 1, 0), "network addr /24");

        let ip2 = Ipv4Addr::new(10, 0, 2, 15);
        let mask2 = Ipv4Addr::new(255, 255, 255, 0);
        let net2 = network_addr(ip2, mask2);
        assert!(net2 == Ipv4Addr::new(10, 0, 2, 0), "network addr 10.x");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 1 (network address) PASSED");
    }

    // --- Test 2: Broadcast address calculation ---
    {
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        let bcast = broadcast_addr(ip, mask);
        assert!(bcast == Ipv4Addr::new(192, 168, 1, 255), "broadcast /24");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 2 (broadcast address) PASSED");
    }

    // --- Test 3: Host count ---
    {
        let mask24 = Ipv4Addr::new(255, 255, 255, 0);
        assert!(host_count(mask24) == 254, "/24 host count");

        let mask16 = Ipv4Addr::new(255, 255, 0, 0);
        assert!(host_count(mask16) == 65534, "/16 host count");

        let mask32 = Ipv4Addr::new(255, 255, 255, 255);
        assert!(host_count(mask32) == 0, "/32 host count");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 3 (host count) PASSED");
    }

    // --- Test 4: Subnet host generation ---
    {
        let net = Ipv4Addr::new(192, 168, 1, 0);
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        let hosts = subnet_hosts(net, mask);
        assert!(hosts.len() == 254, "254 hosts in /24");
        assert!(hosts[0] == Ipv4Addr::new(192, 168, 1, 1), "first host");
        assert!(hosts[253] == Ipv4Addr::new(192, 168, 1, 254), "last host");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 4 (subnet hosts) PASSED");
    }

    // --- Test 5: Small subnet ---
    {
        let net = Ipv4Addr::new(10, 0, 0, 0);
        let mask = Ipv4Addr::new(255, 255, 255, 252); // /30
        let hosts = subnet_hosts(net, mask);
        assert!(hosts.len() == 2, "/30 = 2 hosts");
        assert!(hosts[0] == Ipv4Addr::new(10, 0, 0, 1), "first host /30");
        assert!(hosts[1] == Ipv4Addr::new(10, 0, 0, 2), "second host /30");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 5 (small subnet) PASSED");
    }

    // --- Test 6: MAX_SCAN_SIZE cap ---
    {
        let net = Ipv4Addr::new(10, 0, 0, 0);
        let mask = Ipv4Addr::new(255, 255, 0, 0); // /16 = 65534 hosts
        let hosts = subnet_hosts(net, mask);
        assert!(hosts.len() == MAX_SCAN_SIZE as usize, "capped at max");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 6 (scan size cap) PASSED");
    }

    // --- Test 7: Host struct ---
    {
        let host = Host {
            ip: Ipv4Addr::new(10, 0, 2, 1),
            mac: String::from("AA:BB:CC:DD:EE:FF"),
            hostname: String::from("gateway"),
            last_seen_ns: 1000,
        };
        assert!(host.ip == Ipv4Addr::new(10, 0, 2, 1), "host ip");
        assert!(host.mac == "AA:BB:CC:DD:EE:FF", "host mac");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 7 (Host struct) PASSED");
    }

    // --- Test 8: Stats accessible ---
    {
        let s = stats();
        // Verify counter is accessible and u64-typed.
        let _ = s.total_scans;

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 8 (stats) PASSED");
    }

    // --- Test 9: Network/broadcast edge cases ---
    {
        // /31 network (point-to-point link).
        let mask31 = Ipv4Addr::new(255, 255, 255, 254);
        assert!(host_count(mask31) == 0, "/31 no hosts");

        // /8 large network.
        let mask8 = Ipv4Addr::new(255, 0, 0, 0);
        assert!(host_count(mask8) == 16_777_214, "/8 host count");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 9 (edge cases) PASSED");
    }

    // --- Test 10: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("Network Discovery"), "header");
        assert!(content.contains("Total scans:"), "scans field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[ndisc]   test 10 (procfs content) PASSED");
    }

    crate::serial_println!("[ndisc] All {} self-tests PASSED", passed);
    Ok(())
}
