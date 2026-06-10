//! DHCP server for automatic container IP assignment (RFC 2131).
//!
//! Provides a lightweight DHCP server that assigns IPv4 addresses to
//! containers connected via veth pairs.  This eliminates the need for
//! manual IP configuration when creating containers.
//!
//! ## Architecture
//!
//! ```text
//! Container ── veth ──→ host namespace
//!                          │
//!                          └─ DHCP server (this module)
//!                               ├── IP pool management
//!                               ├── lease tracking
//!                               └── DISCOVER/OFFER/REQUEST/ACK
//! ```
//!
//! ## IP Pool
//!
//! Each pool covers a /24 subnet (e.g., 10.88.0.0/24).  Addresses
//! .1 is reserved for the gateway (host side of veth), .2-.254 are
//! available for container assignment.  The default pool is
//! 10.88.0.0/24, matching the container NAT subnet.
//!
//! ## Lease Management
//!
//! Leases default to 3600 seconds (1 hour).  The server tracks
//! MAC→IP bindings and reclaims expired leases.  When a container
//! is deleted, its leases are explicitly released via `release_mac()`.
//!
//! ## Integration
//!
//! The DHCP server processes incoming DHCP packets from veth pairs
//! (detected by destination port 67 in the UDP layer).  It responds
//! via raw ethernet frame construction through the veth TX path.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of leases.
const MAX_LEASES: usize = 64;

/// Maximum number of IP pools.
const MAX_POOLS: usize = 8;

/// Default lease time in seconds.
const DEFAULT_LEASE_SECS: u32 = 3600;

/// DHCP message types.
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8 = 2;
const DHCP_REQUEST: u8 = 3;
#[allow(dead_code)]
const DHCP_DECLINE: u8 = 4;
const DHCP_ACK: u8 = 5;
const DHCP_NAK: u8 = 6;
#[allow(dead_code)]
const DHCP_RELEASE: u8 = 7;

/// DHCP option codes.
const OPT_SUBNET_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_DNS: u8 = 6;
const OPT_LEASE_TIME: u8 = 51;
const OPT_MSG_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_END: u8 = 255;

/// DHCP magic cookie.
const DHCP_MAGIC: [u8; 4] = [99, 130, 83, 99];

/// BOOTP op codes.
const BOOTREQUEST: u8 = 1;
const BOOTREPLY: u8 = 2;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// An IP address pool for a subnet.
struct IpPool {
    /// Whether this pool is active.
    active: bool,
    /// Network address (e.g., 10.88.0.0).
    network: [u8; 4],
    /// Subnet mask (e.g., 255.255.255.0).
    mask: [u8; 4],
    /// Gateway address (e.g., 10.88.0.1).
    gateway: [u8; 4],
    /// DNS server address.
    dns: [u8; 4],
    /// First assignable host address (last octet, e.g., 2).
    range_start: u8,
    /// Last assignable host address (last octet, e.g., 254).
    range_end: u8,
    /// Network namespace this pool serves (0 = all).
    ns_id: u64,
}

/// A DHCP lease binding.
struct Lease {
    /// Whether this slot is in use.
    active: bool,
    /// Client MAC address.
    mac: [u8; 6],
    /// Assigned IP address.
    ip: [u8; 4],
    /// Lease expiry timestamp (ns since boot, 0 = never expires in test).
    expires_ns: u64,
    /// The pool index this lease came from.
    pool_idx: usize,
}

/// Parsed DHCP request from a client.
struct DhcpRequest {
    /// DHCP message type (DISCOVER, REQUEST, etc.).
    msg_type: u8,
    /// Client MAC address (from chaddr).
    client_mac: [u8; 6],
    /// Transaction ID.
    xid: u32,
    /// Requested IP (from option 50, if present).
    requested_ip: Option<[u8; 4]>,
    /// Client's current IP (ciaddr).
    ciaddr: [u8; 4],
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// IP address pools.
static POOLS: spin::Mutex<[IpPool; MAX_POOLS]> = spin::Mutex::new([
    IpPool { active: false, network: [0; 4], mask: [0; 4], gateway: [0; 4], dns: [0; 4], range_start: 0, range_end: 0, ns_id: 0 },
    IpPool { active: false, network: [0; 4], mask: [0; 4], gateway: [0; 4], dns: [0; 4], range_start: 0, range_end: 0, ns_id: 0 },
    IpPool { active: false, network: [0; 4], mask: [0; 4], gateway: [0; 4], dns: [0; 4], range_start: 0, range_end: 0, ns_id: 0 },
    IpPool { active: false, network: [0; 4], mask: [0; 4], gateway: [0; 4], dns: [0; 4], range_start: 0, range_end: 0, ns_id: 0 },
    IpPool { active: false, network: [0; 4], mask: [0; 4], gateway: [0; 4], dns: [0; 4], range_start: 0, range_end: 0, ns_id: 0 },
    IpPool { active: false, network: [0; 4], mask: [0; 4], gateway: [0; 4], dns: [0; 4], range_start: 0, range_end: 0, ns_id: 0 },
    IpPool { active: false, network: [0; 4], mask: [0; 4], gateway: [0; 4], dns: [0; 4], range_start: 0, range_end: 0, ns_id: 0 },
    IpPool { active: false, network: [0; 4], mask: [0; 4], gateway: [0; 4], dns: [0; 4], range_start: 0, range_end: 0, ns_id: 0 },
]);

/// Active leases.
static LEASES: spin::Mutex<[Lease; MAX_LEASES]> = spin::Mutex::new([
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
    EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE, EMPTY_LEASE,
]);

const EMPTY_LEASE: Lease = Lease {
    active: false,
    mac: [0; 6],
    ip: [0; 4],
    expires_ns: 0,
    pool_idx: 0,
};

/// Whether the DHCP server is enabled.
static ENABLED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Pool management
// ---------------------------------------------------------------------------

/// Add an IP pool for DHCP assignment.
///
/// `network` is the base address (e.g., [10,88,0,0]), `mask` the subnet
/// mask, `gateway` the router, `dns` the DNS server, and `range` the
/// start..=end host portion of assignable addresses.
pub fn add_pool(
    network: [u8; 4],
    mask: [u8; 4],
    gateway: [u8; 4],
    dns: [u8; 4],
    range_start: u8,
    range_end: u8,
    ns_id: u64,
) -> KernelResult<usize> {
    let mut pools = POOLS.lock();
    for (i, pool) in pools.iter_mut().enumerate() {
        if !pool.active {
            pool.active = true;
            pool.network = network;
            pool.mask = mask;
            pool.gateway = gateway;
            pool.dns = dns;
            pool.range_start = range_start;
            pool.range_end = range_end;
            pool.ns_id = ns_id;
            serial_println!(
                "[dhcpd] Pool {}: {}.{}.{}.{}/{} range .{}-.{} (ns {})",
                i,
                network[0], network[1], network[2], network[3],
                mask[3], range_start, range_end, ns_id,
            );
            return Ok(i);
        }
    }
    Err(KernelError::OutOfMemory)
}

/// Remove an IP pool and release its leases.
pub fn remove_pool(pool_idx: usize) -> KernelResult<()> {
    let mut pools = POOLS.lock();
    let pool = pools.get_mut(pool_idx).ok_or(KernelError::InvalidArgument)?;
    if !pool.active {
        return Err(KernelError::NotFound);
    }
    pool.active = false;

    // Release all leases from this pool.
    let mut leases = LEASES.lock();
    for lease in leases.iter_mut() {
        if lease.active && lease.pool_idx == pool_idx {
            lease.active = false;
        }
    }

    serial_println!("[dhcpd] Pool {} removed", pool_idx);
    Ok(())
}

/// Add the default container pool (10.88.0.0/24, gateway .1, DNS from host).
pub fn add_default_pool() -> KernelResult<usize> {
    let dns = super::interface::info().dns;
    add_pool(
        [10, 88, 0, 0],
        [255, 255, 255, 0],
        [10, 88, 0, 1],
        dns.0,
        2, 254,
        0, // serve all namespaces
    )
}

/// Enable the DHCP server.
pub fn enable() {
    ENABLED.store(true, core::sync::atomic::Ordering::Relaxed);
    serial_println!("[dhcpd] Server enabled");
}

/// Disable the DHCP server.
pub fn disable() {
    ENABLED.store(false, core::sync::atomic::Ordering::Relaxed);
    serial_println!("[dhcpd] Server disabled");
}

/// Check if the server is enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(core::sync::atomic::Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Lease management
// ---------------------------------------------------------------------------

/// Find an existing lease for a MAC address.
fn find_lease_for_mac(mac: &[u8; 6]) -> Option<usize> {
    let leases = LEASES.lock();
    for (i, lease) in leases.iter().enumerate() {
        if lease.active && lease.mac == *mac {
            return Some(i);
        }
    }
    None
}

/// Find an existing lease for an IP address.
fn find_lease_for_ip(ip: &[u8; 4]) -> Option<usize> {
    let leases = LEASES.lock();
    for (i, lease) in leases.iter().enumerate() {
        if lease.active && lease.ip == *ip {
            return Some(i);
        }
    }
    None
}

/// Check if an IP is already leased.
fn is_ip_leased(ip: &[u8; 4]) -> bool {
    find_lease_for_ip(ip).is_some()
}

/// Allocate an IP from a pool for the given MAC.
///
/// Returns the allocated IP or None if the pool is exhausted.
#[allow(clippy::arithmetic_side_effects)]
fn allocate_ip(pool_idx: usize, mac: &[u8; 6]) -> Option<[u8; 4]> {
    let pools = POOLS.lock();
    let pool = pools.get(pool_idx)?;
    if !pool.active {
        return None;
    }

    // Try each address in the range.
    let mut host = pool.range_start;
    while host <= pool.range_end {
        let ip = [pool.network[0], pool.network[1], pool.network[2], host];
        if !is_ip_leased(&ip) {
            // Found a free address — create the lease.
            let now = crate::hrtimer::now_ns();
            let expires = now.saturating_add(
                (DEFAULT_LEASE_SECS as u64).saturating_mul(1_000_000_000),
            );

            let mut leases = LEASES.lock();
            for lease in leases.iter_mut() {
                if !lease.active {
                    lease.active = true;
                    lease.mac = *mac;
                    lease.ip = ip;
                    lease.expires_ns = expires;
                    lease.pool_idx = pool_idx;
                    return Some(ip);
                }
            }
            // No free lease slots.
            return None;
        }
        host = host.wrapping_add(1);
    }
    None
}

/// Release all leases for a given MAC address.
pub fn release_mac(mac: &[u8; 6]) {
    let mut leases = LEASES.lock();
    for lease in leases.iter_mut() {
        if lease.active && lease.mac == *mac {
            serial_println!(
                "[dhcpd] Released {}.{}.{}.{} (MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
                lease.ip[0], lease.ip[1], lease.ip[2], lease.ip[3],
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
            );
            lease.active = false;
        }
    }
}

/// Expire stale leases.
pub fn tick_expire() {
    let now = crate::hrtimer::now_ns();
    let mut leases = LEASES.lock();
    for lease in leases.iter_mut() {
        if lease.active && lease.expires_ns != 0 && now > lease.expires_ns {
            serial_println!(
                "[dhcpd] Lease expired: {}.{}.{}.{}",
                lease.ip[0], lease.ip[1], lease.ip[2], lease.ip[3],
            );
            lease.active = false;
        }
    }
}

/// Count active leases.
pub fn lease_count() -> usize {
    let leases = LEASES.lock();
    leases.iter().filter(|l| l.active).count()
}

/// A single lease snapshot: (MAC address, assigned IP, expiry in ns).
pub type LeaseInfo = ([u8; 6], [u8; 4], u64);

/// Get info about all active leases.
pub fn lease_list() -> ([LeaseInfo; 16], usize) {
    let leases = LEASES.lock();
    let mut out = [([0u8; 6], [0u8; 4], 0u64); 16];
    let mut count = 0;
    for lease in leases.iter() {
        if lease.active && count < 16 {
            out[count] = (lease.mac, lease.ip, lease.expires_ns);
            count = count.saturating_add(1);
        }
    }
    (out, count)
}

// ---------------------------------------------------------------------------
// DHCP message parsing
// ---------------------------------------------------------------------------

/// Parse a DHCP request from raw UDP payload.
///
/// The caller has already verified this is a UDP packet to port 67.
#[allow(clippy::arithmetic_side_effects)]
fn parse_request(data: &[u8]) -> Option<DhcpRequest> {
    // Minimum DHCP message: 240 bytes (236 fixed + 4 magic cookie).
    if data.len() < 240 {
        return None;
    }

    // Must be BOOTREQUEST (client → server).
    if data[0] != BOOTREQUEST {
        return None;
    }

    // Hardware type must be Ethernet (1), length 6.
    if data[1] != 1 || data[2] != 6 {
        return None;
    }

    let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let ciaddr = [data[12], data[13], data[14], data[15]];
    let mut client_mac = [0u8; 6];
    client_mac.copy_from_slice(&data[28..34]);

    // Verify magic cookie at offset 236.
    if data[236..240] != DHCP_MAGIC {
        return None;
    }

    // Parse options.
    let mut msg_type = 0u8;
    let mut requested_ip = None;
    let mut i = 240;

    while i < data.len() {
        let opt = data[i];
        if opt == OPT_END {
            break;
        }
        if opt == 0 {
            // Padding.
            i += 1;
            continue;
        }
        if i + 1 >= data.len() {
            break;
        }
        let len = data[i + 1] as usize;
        let val_start = i + 2;
        let val_end = val_start + len;
        if val_end > data.len() {
            break;
        }

        match opt {
            OPT_MSG_TYPE if len == 1 => {
                msg_type = data[val_start];
            }
            OPT_REQUESTED_IP if len == 4 => {
                requested_ip = Some([
                    data[val_start],
                    data[val_start + 1],
                    data[val_start + 2],
                    data[val_start + 3],
                ]);
            }
            _ => { /* ignore other options */ }
        }

        i = val_end;
    }

    if msg_type == 0 {
        return None;
    }

    Some(DhcpRequest {
        msg_type,
        client_mac,
        xid,
        requested_ip,
        ciaddr,
    })
}

// ---------------------------------------------------------------------------
// DHCP response building
// ---------------------------------------------------------------------------

/// Build a DHCP response message.
///
/// Returns the raw DHCP payload (to be sent via UDP).
#[allow(clippy::arithmetic_side_effects)]
fn build_response(
    msg_type: u8,
    xid: u32,
    client_mac: &[u8; 6],
    your_ip: &[u8; 4],
    server_ip: &[u8; 4],
    mask: &[u8; 4],
    gateway: &[u8; 4],
    dns: &[u8; 4],
    lease_secs: u32,
) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(300);

    // Fixed fields (236 bytes).
    pkt.push(BOOTREPLY);   // op
    pkt.push(1);           // htype (Ethernet)
    pkt.push(6);           // hlen
    pkt.push(0);           // hops
    pkt.extend_from_slice(&xid.to_be_bytes()); // xid
    pkt.extend_from_slice(&[0, 0]); // secs
    pkt.extend_from_slice(&[0, 0]); // flags
    pkt.extend_from_slice(&[0, 0, 0, 0]); // ciaddr
    pkt.extend_from_slice(your_ip);        // yiaddr
    pkt.extend_from_slice(server_ip);      // siaddr
    pkt.extend_from_slice(&[0, 0, 0, 0]); // giaddr
    pkt.extend_from_slice(client_mac);     // chaddr (first 6 bytes)
    pkt.extend_from_slice(&[0u8; 10]);     // chaddr padding (10 bytes)
    pkt.extend_from_slice(&[0u8; 64]);     // sname
    pkt.extend_from_slice(&[0u8; 128]);    // file

    // Magic cookie.
    pkt.extend_from_slice(&DHCP_MAGIC);

    // Options.
    // Message type.
    pkt.extend_from_slice(&[OPT_MSG_TYPE, 1, msg_type]);

    // Server identifier.
    pkt.extend_from_slice(&[OPT_SERVER_ID, 4]);
    pkt.extend_from_slice(server_ip);

    // Lease time.
    pkt.extend_from_slice(&[OPT_LEASE_TIME, 4]);
    pkt.extend_from_slice(&lease_secs.to_be_bytes());

    // Subnet mask.
    pkt.extend_from_slice(&[OPT_SUBNET_MASK, 4]);
    pkt.extend_from_slice(mask);

    // Router/gateway.
    if *gateway != [0, 0, 0, 0] {
        pkt.extend_from_slice(&[OPT_ROUTER, 4]);
        pkt.extend_from_slice(gateway);
    }

    // DNS server.
    if *dns != [0, 0, 0, 0] {
        pkt.extend_from_slice(&[OPT_DNS, 4]);
        pkt.extend_from_slice(dns);
    }

    // End.
    pkt.push(OPT_END);

    // Pad to minimum 300 bytes.
    while pkt.len() < 300 {
        pkt.push(0);
    }

    pkt
}

/// Build a DHCP NAK response.
fn build_nak(xid: u32, client_mac: &[u8; 6], server_ip: &[u8; 4]) -> Vec<u8> {
    build_response(
        DHCP_NAK, xid, client_mac,
        &[0, 0, 0, 0], server_ip,
        &[0, 0, 0, 0], &[0, 0, 0, 0], &[0, 0, 0, 0],
        0,
    )
}

// ---------------------------------------------------------------------------
// Request handling
// ---------------------------------------------------------------------------

/// Process a DHCP request and return the response payload (if any).
///
/// Called from the UDP layer when a packet arrives on port 67.
#[allow(clippy::arithmetic_side_effects)]
pub fn process_request(data: &[u8]) -> Option<(Vec<u8>, [u8; 4])> {
    if !is_enabled() {
        return None;
    }

    let req = parse_request(data)?;

    serial_println!(
        "[dhcpd] {} from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} (xid {:08x})",
        match req.msg_type {
            DHCP_DISCOVER => "DISCOVER",
            DHCP_REQUEST => "REQUEST",
            DHCP_RELEASE => "RELEASE",
            DHCP_DECLINE => "DECLINE",
            _ => "UNKNOWN",
        },
        req.client_mac[0], req.client_mac[1], req.client_mac[2],
        req.client_mac[3], req.client_mac[4], req.client_mac[5],
        req.xid,
    );

    match req.msg_type {
        DHCP_DISCOVER => handle_discover(&req),
        DHCP_REQUEST => handle_request(&req),
        DHCP_RELEASE => {
            release_mac(&req.client_mac);
            None
        }
        _ => None,
    }
}

/// Handle a DHCP DISCOVER — respond with an OFFER.
fn handle_discover(req: &DhcpRequest) -> Option<(Vec<u8>, [u8; 4])> {
    // Check if client already has a lease (re-discover after reboot).
    if let Some(idx) = find_lease_for_mac(&req.client_mac) {
        let leases = LEASES.lock();
        if let Some(lease) = leases.get(idx) {
            let ip = lease.ip;
            let pool_idx = lease.pool_idx;
            drop(leases);

            let pools = POOLS.lock();
            if let Some(pool) = pools.get(pool_idx) {
                if pool.active {
                    let resp = build_response(
                        DHCP_OFFER, req.xid, &req.client_mac,
                        &ip, &pool.gateway, &pool.mask,
                        &pool.gateway, &pool.dns, DEFAULT_LEASE_SECS,
                    );
                    serial_println!(
                        "[dhcpd] OFFER {}.{}.{}.{} (existing lease)",
                        ip[0], ip[1], ip[2], ip[3],
                    );
                    return Some((resp, [255, 255, 255, 255]));
                }
            }
        }
    }

    // No existing lease — collect pool info then try each pool.
    // We copy pool data out of the lock so allocate_ip can re-lock POOLS.
    let mut pool_infos = [(0usize, [0u8; 4], [0u8; 4], [0u8; 4]); MAX_POOLS];
    let mut active_count = 0;
    {
        let pools = POOLS.lock();
        for (i, pool) in pools.iter().enumerate() {
            if pool.active && active_count < MAX_POOLS {
                pool_infos[active_count] = (i, pool.gateway, pool.mask, pool.dns);
                active_count = active_count.saturating_add(1);
            }
        }
    }

    for j in 0..active_count {
        let (i, gateway, mask, dns) = pool_infos[j];
        if let Some(ip) = allocate_ip(i, &req.client_mac) {
            let resp = build_response(
                DHCP_OFFER, req.xid, &req.client_mac,
                &ip, &gateway, &mask, &gateway, &dns,
                DEFAULT_LEASE_SECS,
            );
            serial_println!(
                "[dhcpd] OFFER {}.{}.{}.{}",
                ip[0], ip[1], ip[2], ip[3],
            );
            return Some((resp, [255, 255, 255, 255]));
        }
        // Pool exhausted, try next.
    }

    serial_println!("[dhcpd] No IPs available for DISCOVER");
    None
}

/// Handle a DHCP REQUEST — respond with ACK or NAK.
fn handle_request(req: &DhcpRequest) -> Option<(Vec<u8>, [u8; 4])> {
    let requested = req.requested_ip.unwrap_or(req.ciaddr);
    if requested == [0, 0, 0, 0] {
        return None;
    }

    // Verify the client has a lease for this IP.
    if let Some(idx) = find_lease_for_mac(&req.client_mac) {
        let mut leases = LEASES.lock();
        if let Some(lease) = leases.get_mut(idx) {
            if lease.ip == requested {
                // Renew the lease.
                let now = crate::hrtimer::now_ns();
                lease.expires_ns = now.saturating_add(
                    (DEFAULT_LEASE_SECS as u64).saturating_mul(1_000_000_000),
                );
                let ip = lease.ip;
                let pool_idx = lease.pool_idx;
                drop(leases);

                let pools = POOLS.lock();
                if let Some(pool) = pools.get(pool_idx) {
                    if pool.active {
                        let resp = build_response(
                            DHCP_ACK, req.xid, &req.client_mac,
                            &ip, &pool.gateway, &pool.mask,
                            &pool.gateway, &pool.dns, DEFAULT_LEASE_SECS,
                        );
                        serial_println!(
                            "[dhcpd] ACK {}.{}.{}.{}",
                            ip[0], ip[1], ip[2], ip[3],
                        );
                        return Some((resp, [255, 255, 255, 255]));
                    }
                }
                return None;
            }
        }
    }

    // Client requests an IP it doesn't have a lease for — NAK.
    // Use the first active pool's gateway as server ID.
    let server_ip = {
        let pools = POOLS.lock();
        pools.iter().find(|p| p.active).map(|p| p.gateway)
    };
    if let Some(server_ip) = server_ip {
        let resp = build_nak(req.xid, &req.client_mac, &server_ip);
        serial_println!(
            "[dhcpd] NAK (requested {}.{}.{}.{}, no lease)",
            requested[0], requested[1], requested[2], requested[3],
        );
        return Some((resp, [255, 255, 255, 255]));
    }

    None
}

// ---------------------------------------------------------------------------
// Pool count / status
// ---------------------------------------------------------------------------

/// Count active pools.
pub fn pool_count() -> usize {
    let pools = POOLS.lock();
    pools.iter().filter(|p| p.active).count()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// DHCP server self-test.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[dhcpd] Running self-test...");

    // Test 1: Add a pool.
    let idx = add_pool(
        [192, 168, 100, 0],
        [255, 255, 255, 0],
        [192, 168, 100, 1],
        [8, 8, 8, 8],
        10, 20,
        0,
    )?;
    assert_eq!(pool_count(), 1);
    serial_println!("[dhcpd]   Add pool: OK");

    // Test 2: Allocate an IP.
    let mac1 = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01];
    let ip1 = allocate_ip(idx, &mac1).expect("alloc failed");
    assert_eq!(ip1, [192, 168, 100, 10]);
    assert_eq!(lease_count(), 1);
    serial_println!("[dhcpd]   Allocate IP: OK");

    // Test 3: Second allocation gets next IP.
    let mac2 = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x02];
    let ip2 = allocate_ip(idx, &mac2).expect("alloc failed");
    assert_eq!(ip2, [192, 168, 100, 11]);
    assert_eq!(lease_count(), 2);
    serial_println!("[dhcpd]   Sequential allocation: OK");

    // Test 4: Find lease by MAC.
    let lease_idx = find_lease_for_mac(&mac1).expect("not found");
    let leases = LEASES.lock();
    assert_eq!(leases[lease_idx].ip, [192, 168, 100, 10]);
    drop(leases);
    serial_println!("[dhcpd]   Find lease by MAC: OK");

    // Test 5: Find lease by IP.
    assert!(find_lease_for_ip(&[192, 168, 100, 10]).is_some());
    assert!(find_lease_for_ip(&[192, 168, 100, 99]).is_none());
    serial_println!("[dhcpd]   Find lease by IP: OK");

    // Test 6: Release by MAC.
    release_mac(&mac1);
    assert_eq!(lease_count(), 1);
    assert!(find_lease_for_mac(&mac1).is_none());
    serial_println!("[dhcpd]   Release by MAC: OK");

    // Test 7: Released IP can be re-allocated.
    let mac3 = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x03];
    let ip3 = allocate_ip(idx, &mac3).expect("alloc failed");
    assert_eq!(ip3, [192, 168, 100, 10]); // Reuses .10
    serial_println!("[dhcpd]   IP reuse after release: OK");

    // Test 8: Parse a DHCP DISCOVER message.
    {
        let mut discover = vec![0u8; 300];
        discover[0] = BOOTREQUEST;
        discover[1] = 1; // htype
        discover[2] = 6; // hlen
        // xid
        discover[4] = 0x12;
        discover[5] = 0x34;
        discover[6] = 0x56;
        discover[7] = 0x78;
        // chaddr
        discover[28..34].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        // Magic cookie
        discover[236..240].copy_from_slice(&DHCP_MAGIC);
        // Option: message type = DISCOVER
        discover[240] = OPT_MSG_TYPE;
        discover[241] = 1;
        discover[242] = DHCP_DISCOVER;
        discover[243] = OPT_END;

        let req = parse_request(&discover).expect("parse failed");
        assert_eq!(req.msg_type, DHCP_DISCOVER);
        assert_eq!(req.xid, 0x12345678);
        assert_eq!(req.client_mac, [0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        serial_println!("[dhcpd]   Parse DISCOVER: OK");
    }

    // Test 9: Build and verify response structure.
    {
        let resp = build_response(
            DHCP_OFFER, 0xAABBCCDD,
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
            &[192, 168, 1, 100],
            &[192, 168, 1, 1],
            &[255, 255, 255, 0],
            &[192, 168, 1, 1],
            &[8, 8, 8, 8],
            3600,
        );
        assert!(resp.len() >= 300);
        assert_eq!(resp[0], BOOTREPLY);
        // yiaddr
        assert_eq!(&resp[16..20], &[192, 168, 1, 100]);
        // magic cookie
        assert_eq!(&resp[236..240], &DHCP_MAGIC);
        serial_println!("[dhcpd]   Build response: OK");
    }

    // Test 10: Process DISCOVER with server enabled.
    {
        enable();
        let mut discover = vec![0u8; 300];
        discover[0] = BOOTREQUEST;
        discover[1] = 1;
        discover[2] = 6;
        discover[4..8].copy_from_slice(&0xDEADBEEFu32.to_be_bytes());
        discover[28..34].copy_from_slice(&[0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA]);
        discover[236..240].copy_from_slice(&DHCP_MAGIC);
        discover[240] = OPT_MSG_TYPE;
        discover[241] = 1;
        discover[242] = DHCP_DISCOVER;
        discover[243] = OPT_END;

        let result = process_request(&discover);
        assert!(result.is_some());
        let (resp, dest) = result.expect("no response");
        assert_eq!(dest, [255, 255, 255, 255]); // broadcast
        assert_eq!(resp[0], BOOTREPLY);
        // Check offer assigns from pool
        assert_eq!(resp[16], 192); // yiaddr network part
        serial_println!("[dhcpd]   Process DISCOVER: OK");
    }

    // Cleanup: remove pool and release all test leases.
    release_mac(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x02]);
    release_mac(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x03]);
    release_mac(&[0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA]);
    let _ = remove_pool(idx);
    disable();

    serial_println!("[dhcpd] Self-test PASSED (10 tests)");
    Ok(())
}
