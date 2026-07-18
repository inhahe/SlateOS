//! Network Namespaces — per-container network isolation.
//!
//! Provides Linux-style network namespace isolation.  Each network
//! namespace has its own:
//!
//! - **Interface configuration**: independent IP, subnet, gateway, DNS
//! - **Routing table**: per-namespace routing entries
//! - **Firewall state**: per-namespace firewall rules and connection
//!   tracking (see `net::firewall::ns_*` functions)
//! - **ARP cache**: per-namespace MAC resolution (future — requires
//!   virtual ethernet pairs)
//!
//! ## Design
//!
//! Network namespaces allow containers to have completely independent
//! network stacks.  A process in one namespace sees different IP
//! addresses, routing tables, and firewall rules than a process in
//! another namespace.
//!
//! The root namespace (ID 0) always exists and corresponds to the
//! host's physical network stack.  Child namespaces start unconfigured
//! and must have their interfaces set up explicitly (typically via
//! virtual ethernet pairs, though that's a future feature).
//!
//! ## Integration Points
//!
//! - **Socket layer**: each socket is bound to a namespace; lookups
//!   use the namespace's routing table and interface config.
//! - **Process credentials**: `proc/pcb.rs` stores the namespace ID;
//!   fork inherits it, `unshare` creates a new one.
//! - **net/ module**: `net::interface` globals become per-namespace
//!   lookups keyed by the current process's namespace ID.
//!
//! ## References
//!
//! - Linux `net/core/net_namespace.c`
//! - `man 7 network_namespaces`
//! - Design spec: container primitives for Docker support

use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of network namespaces.
pub const MAX_NAMESPACES: usize = 64;

/// Maximum routing table entries per namespace.
pub const MAX_ROUTES: usize = 32;

/// The root (host) network namespace.  Always exists.
pub const ROOT_NS: NetNsId = 0;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a network namespace.
pub type NetNsId = u32;

/// IPv4 address — local copy to avoid depending on net::interface.
///
/// This is intentionally a separate type so netns.rs doesn't pull in
/// the `net` module's globals.  The net module will translate between
/// this and its own `Ipv4Addr` when integrating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    /// The unspecified address (0.0.0.0).
    pub const UNSPECIFIED: Self = Self([0, 0, 0, 0]);

    /// Create from four octets.
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }

    /// Convert to `u32` in network byte order.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn to_u32(self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    /// Check if same subnet.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn same_subnet(self, other: Self, mask: Self) -> bool {
        (self.to_u32() & mask.to_u32()) == (other.to_u32() & mask.to_u32())
    }
}

impl core::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

/// Per-namespace interface configuration.
#[derive(Debug, Clone)]
pub struct NsInterfaceConfig {
    /// Whether the virtual interface is up.
    pub up: bool,
    /// IPv4 address assigned to this namespace.
    pub ip: Ipv4Addr,
    /// Subnet mask.
    pub subnet_mask: Ipv4Addr,
    /// Default gateway.
    pub gateway: Ipv4Addr,
    /// DNS server.
    pub dns: Ipv4Addr,
}

impl Default for NsInterfaceConfig {
    fn default() -> Self {
        Self {
            up: false,
            ip: Ipv4Addr::UNSPECIFIED,
            subnet_mask: Ipv4Addr::UNSPECIFIED,
            gateway: Ipv4Addr::UNSPECIFIED,
            dns: Ipv4Addr::UNSPECIFIED,
        }
    }
}

/// A routing table entry.
///
/// Matches packets by destination network (destination + mask) and
/// forwards them to the gateway.  Lower metric = higher priority.
#[derive(Debug, Clone, Copy)]
pub struct RouteEntry {
    /// Destination network address.
    pub destination: Ipv4Addr,
    /// Destination subnet mask (e.g., 255.255.255.0 for /24).
    pub mask: Ipv4Addr,
    /// Next-hop gateway (0.0.0.0 for directly connected).
    pub gateway: Ipv4Addr,
    /// Route metric (lower = preferred).
    pub metric: u32,
}

// ---------------------------------------------------------------------------
// Per-namespace data
// ---------------------------------------------------------------------------

/// A network namespace.
struct NetNamespace {
    /// Whether this slot is active.
    active: bool,
    /// Interface configuration for this namespace.
    iface: NsInterfaceConfig,
    /// Routing table.
    routes: Vec<RouteEntry>,
    /// Number of processes using this namespace.
    nr_procs: u32,
}

impl NetNamespace {
    fn new_empty() -> Self {
        Self {
            active: false,
            iface: NsInterfaceConfig::default(),
            routes: Vec::new(),
            nr_procs: 0,
        }
    }

    fn init(&mut self) {
        self.active = true;
        self.iface = NsInterfaceConfig::default();
        self.routes.clear();
        self.nr_procs = 0;
    }
}

// ---------------------------------------------------------------------------
// Snapshot type
// ---------------------------------------------------------------------------

/// Read-only snapshot of a network namespace's state.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API — fields read by kshell and syscall handlers.
pub struct NetNsStats {
    /// Namespace ID.
    pub id: NetNsId,
    /// Whether active.
    pub active: bool,
    /// Interface up?
    pub iface_up: bool,
    /// Configured IP.
    pub ip: Ipv4Addr,
    /// Subnet mask.
    pub subnet_mask: Ipv4Addr,
    /// Gateway.
    pub gateway: Ipv4Addr,
    /// DNS.
    pub dns: Ipv4Addr,
    /// Number of routing entries.
    pub route_count: usize,
    /// Process count.
    pub nr_procs: u32,
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

struct NetNsTable {
    namespaces: Vec<NetNamespace>,
    next_id: u32,
}

impl NetNsTable {
    fn new() -> Self {
        let mut namespaces = Vec::with_capacity(MAX_NAMESPACES);
        for _ in 0..MAX_NAMESPACES {
            namespaces.push(NetNamespace::new_empty());
        }
        // Root namespace is always active.  Its interface config will
        // be populated by net::interface::init() via configure_root().
        namespaces[0].active = true;
        Self {
            namespaces,
            next_id: 1,
        }
    }
}

static TABLE: Mutex<Option<NetNsTable>> = Mutex::new(None);

/// Check if the network namespace subsystem has been initialized.
///
/// The `net` module calls this before syncing interface configuration
/// to the root namespace, because `net::init()` runs before `netns::init()`
/// at boot time.
#[must_use]
pub fn is_initialized() -> bool {
    TABLE.lock().is_some()
}

/// Initialize the network namespace subsystem.
pub fn init() {
    let mut table = TABLE.lock();
    *table = Some(NetNsTable::new());
    serial_println!("[netns] Initialized ({} max namespaces)", MAX_NAMESPACES);
}

fn with_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut NetNsTable) -> R,
{
    let mut guard = TABLE.lock();
    let table = guard.as_mut().expect("[netns] not initialized");
    f(table)
}

fn with_table_ref<F, R>(f: F) -> R
where
    F: FnOnce(&NetNsTable) -> R,
{
    let guard = TABLE.lock();
    let table = guard.as_ref().expect("[netns] not initialized");
    f(table)
}

// ---------------------------------------------------------------------------
// Public API: lifecycle
// ---------------------------------------------------------------------------

/// Create a new network namespace.
///
/// The new namespace starts with no interface configuration and an
/// empty routing table.
///
/// # Errors
///
/// - [`KernelError::ResourceExhausted`] if all slots are full.
pub fn create() -> KernelResult<NetNsId> {
    with_table(|table| {
        let start = table.next_id as usize;
        let mut found = None;
        for offset in 0..MAX_NAMESPACES {
            #[allow(clippy::arithmetic_side_effects)]
            let idx = (start + offset) % MAX_NAMESPACES;
            if idx == 0 { continue; } // Skip root.
            if !table.namespaces[idx].active {
                found = Some(idx);
                break;
            }
        }

        let idx = found.ok_or(KernelError::ResourceExhausted)?;

        table.namespaces[idx].init();

        #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
        {
            table.next_id = ((idx + 1) % MAX_NAMESPACES) as u32;
        }

        Ok(idx as NetNsId)
    })
}

/// Delete a network namespace.
///
/// Must have no processes attached.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `id` is root or doesn't exist.
/// - [`KernelError::NotEmpty`] if processes are still attached.
pub fn delete(id: NetNsId) -> KernelResult<()> {
    if id == ROOT_NS {
        return Err(KernelError::InvalidArgument);
    }

    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.namespaces[idx].nr_procs > 0 {
            return Err(KernelError::NotEmpty);
        }

        table.namespaces[idx].active = false;
        table.namespaces[idx].iface = NsInterfaceConfig::default();
        table.namespaces[idx].routes.clear();

        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Public API: interface configuration
// ---------------------------------------------------------------------------

/// Configure the interface for a network namespace.
///
/// For the root namespace, this should be called by the DHCP client
/// or manual configuration to mirror the physical NIC's state.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if namespace doesn't exist.
pub fn configure_interface(
    ns_id: NetNsId,
    ip: Ipv4Addr,
    subnet_mask: Ipv4Addr,
    gateway: Ipv4Addr,
    dns: Ipv4Addr,
) -> KernelResult<()> {
    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }

        let iface = &mut table.namespaces[idx].iface;
        iface.ip = ip;
        iface.subnet_mask = subnet_mask;
        iface.gateway = gateway;
        iface.dns = dns;
        iface.up = true;

        Ok(())
    })
}

/// Set the interface up or down for a namespace.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if namespace doesn't exist.
pub fn set_interface_up(ns_id: NetNsId, up: bool) -> KernelResult<()> {
    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.namespaces[idx].iface.up = up;
        Ok(())
    })
}

/// Get the interface configuration for a namespace.
#[must_use]
pub fn interface_config(ns_id: NetNsId) -> Option<NsInterfaceConfig> {
    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        Some(table.namespaces[idx].iface.clone())
    })
}

// ---------------------------------------------------------------------------
// Public API: routing table
// ---------------------------------------------------------------------------

/// Add a route to a namespace's routing table.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if namespace doesn't exist.
/// - [`KernelError::ResourceExhausted`] if routing table is full.
pub fn add_route(
    ns_id: NetNsId,
    destination: Ipv4Addr,
    mask: Ipv4Addr,
    gateway: Ipv4Addr,
    metric: u32,
) -> KernelResult<()> {
    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.namespaces[idx].routes.len() >= MAX_ROUTES {
            return Err(KernelError::ResourceExhausted);
        }

        table.namespaces[idx].routes.push(RouteEntry {
            destination,
            mask,
            gateway,
            metric,
        });

        Ok(())
    })
}

/// Remove a route from a namespace's routing table.
///
/// Removes the first route matching `destination` and `mask`.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if namespace doesn't exist.
/// - [`KernelError::NotFound`] if no matching route.
pub fn remove_route(
    ns_id: NetNsId,
    destination: Ipv4Addr,
    mask: Ipv4Addr,
) -> KernelResult<()> {
    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }

        let routes = &mut table.namespaces[idx].routes;
        let pos = routes.iter().position(|r| {
            r.destination == destination && r.mask == mask
        });

        match pos {
            Some(i) => {
                routes.remove(i);
                Ok(())
            }
            None => Err(KernelError::NotFound),
        }
    })
}

/// Look up the next-hop gateway for a destination IP in a namespace.
///
/// Performs longest-prefix-match: among all routes whose
/// `destination/mask` covers `dest_ip`, returns the one with the
/// most specific mask (longest prefix).  Ties broken by lowest metric.
///
/// Returns `None` if no route matches.
#[must_use]
pub fn route_lookup(ns_id: NetNsId, dest_ip: Ipv4Addr) -> Option<Ipv4Addr> {
    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }

        let routes = &table.namespaces[idx].routes;
        let mut best: Option<&RouteEntry> = None;

        for route in routes {
            // Check if dest_ip is in this route's network.
            if !dest_ip.same_subnet(route.destination, route.mask) {
                continue;
            }

            // Prefer longer prefix (more specific mask = higher u32 value).
            // On tie, prefer lower metric.
            let dominated = best.is_some_and(|b| {
                let new_prefix = route.mask.to_u32();
                let old_prefix = b.mask.to_u32();
                if new_prefix > old_prefix {
                    false // New route is more specific — not dominated.
                } else if new_prefix < old_prefix {
                    true // Old route is more specific — new is dominated.
                } else {
                    route.metric >= b.metric // Same prefix: lower metric wins.
                }
            });

            if !dominated {
                best = Some(route);
            }
        }

        best.map(|r| r.gateway)
    })
}

/// Get the routing table for a namespace.
#[must_use]
pub fn routes(ns_id: NetNsId) -> Vec<RouteEntry> {
    with_table_ref(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Vec::new();
        }
        table.namespaces[idx].routes.clone()
    })
}

// ---------------------------------------------------------------------------
// Public API: process tracking
// ---------------------------------------------------------------------------

/// Increment the process count for a namespace.
pub fn attach_process(ns_id: NetNsId) -> KernelResult<()> {
    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.namespaces[idx].nr_procs =
            table.namespaces[idx].nr_procs.saturating_add(1);
        Ok(())
    })
}

/// Decrement the process count for a namespace.
pub fn detach_process(ns_id: NetNsId) -> KernelResult<()> {
    with_table(|table| {
        let idx = ns_id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.namespaces[idx].nr_procs =
            table.namespaces[idx].nr_procs.saturating_sub(1);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Public API: queries
// ---------------------------------------------------------------------------

/// Get statistics for a network namespace.
#[must_use]
pub fn stats(id: NetNsId) -> Option<NetNsStats> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_NAMESPACES || !table.namespaces[idx].active {
            return None;
        }
        let ns = &table.namespaces[idx];
        Some(NetNsStats {
            id,
            active: true,
            iface_up: ns.iface.up,
            ip: ns.iface.ip,
            subnet_mask: ns.iface.subnet_mask,
            gateway: ns.iface.gateway,
            dns: ns.iface.dns,
            route_count: ns.routes.len(),
            nr_procs: ns.nr_procs,
        })
    })
}

/// Check if a namespace exists.
#[must_use]
pub fn exists(id: NetNsId) -> bool {
    with_table_ref(|table| {
        let idx = id as usize;
        idx < MAX_NAMESPACES && table.namespaces[idx].active
    })
}

/// Count active namespaces.
#[must_use]
pub fn active_count() -> usize {
    with_table_ref(|table| {
        table.namespaces.iter().filter(|ns| ns.active).count()
    })
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for network namespaces.
pub fn self_test() {
    serial_println!("[netns] Running self-test...");

    // Test 1: Root namespace exists.
    assert!(exists(ROOT_NS));
    assert_eq!(active_count(), 1);
    serial_println!("[netns]   Root exists: OK");

    // Test 2: Create namespace.
    let ns1 = create().expect("create ns1");
    assert!(ns1 > 0);
    assert!(exists(ns1));
    assert_eq!(active_count(), 2);
    serial_println!("[netns]   Create namespace: OK");

    // Test 3: New namespace starts unconfigured.
    let cfg = interface_config(ns1).unwrap();
    assert!(!cfg.up);
    assert_eq!(cfg.ip, Ipv4Addr::UNSPECIFIED);
    serial_println!("[netns]   Default unconfigured: OK");

    // Test 4: Configure interface.
    let ip = Ipv4Addr::new(10, 0, 0, 2);
    let mask = Ipv4Addr::new(255, 255, 255, 0);
    let gw = Ipv4Addr::new(10, 0, 0, 1);
    let dns = Ipv4Addr::new(8, 8, 8, 8);
    configure_interface(ns1, ip, mask, gw, dns).expect("configure");
    let cfg = interface_config(ns1).unwrap();
    assert!(cfg.up);
    assert_eq!(cfg.ip, ip);
    assert_eq!(cfg.subnet_mask, mask);
    assert_eq!(cfg.gateway, gw);
    assert_eq!(cfg.dns, dns);
    serial_println!("[netns]   Configure interface: OK");

    // Test 5: Interface up/down toggle.
    set_interface_up(ns1, false).expect("down");
    assert!(!interface_config(ns1).unwrap().up);
    set_interface_up(ns1, true).expect("up");
    assert!(interface_config(ns1).unwrap().up);
    serial_println!("[netns]   Interface up/down: OK");

    // Test 6: Add routes.
    // Default route: 0.0.0.0/0 → 10.0.0.1, metric 100
    add_route(ns1, Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(0, 0, 0, 0),
              Ipv4Addr::new(10, 0, 0, 1), 100).expect("default route");
    // LAN route: 10.0.0.0/24 → 0.0.0.0 (direct), metric 0
    add_route(ns1, Ipv4Addr::new(10, 0, 0, 0), Ipv4Addr::new(255, 255, 255, 0),
              Ipv4Addr::UNSPECIFIED, 0).expect("lan route");
    assert_eq!(routes(ns1).len(), 2);
    serial_println!("[netns]   Add routes: OK");

    // Test 7: Route lookup — longest prefix match.
    // 10.0.0.5 should match the /24 LAN route (more specific than /0).
    let next = route_lookup(ns1, Ipv4Addr::new(10, 0, 0, 5));
    assert_eq!(next, Some(Ipv4Addr::UNSPECIFIED)); // Direct delivery.

    // 8.8.4.4 should match the default route.
    let next = route_lookup(ns1, Ipv4Addr::new(8, 8, 4, 4));
    assert_eq!(next, Some(Ipv4Addr::new(10, 0, 0, 1)));
    serial_println!("[netns]   Route lookup (longest prefix): OK");

    // Test 8: Remove route.
    remove_route(ns1, Ipv4Addr::new(10, 0, 0, 0), Ipv4Addr::new(255, 255, 255, 0))
        .expect("remove lan route");
    assert_eq!(routes(ns1).len(), 1);
    // Now 10.0.0.5 falls through to default route.
    let next = route_lookup(ns1, Ipv4Addr::new(10, 0, 0, 5));
    assert_eq!(next, Some(Ipv4Addr::new(10, 0, 0, 1)));
    serial_println!("[netns]   Remove route: OK");

    // Test 9: Route not found.
    // After removing all routes, lookup should return None.
    remove_route(ns1, Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(0, 0, 0, 0))
        .expect("remove default");
    assert!(route_lookup(ns1, Ipv4Addr::new(1, 2, 3, 4)).is_none());
    serial_println!("[netns]   No route → None: OK");

    // Test 10: Metric tie-breaking.
    // Two default routes with different metrics.
    add_route(ns1, Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(0, 0, 0, 0),
              Ipv4Addr::new(10, 0, 0, 99), 200).expect("high metric");
    add_route(ns1, Ipv4Addr::new(0, 0, 0, 0), Ipv4Addr::new(0, 0, 0, 0),
              Ipv4Addr::new(10, 0, 0, 1), 50).expect("low metric");
    let next = route_lookup(ns1, Ipv4Addr::new(1, 1, 1, 1));
    assert_eq!(next, Some(Ipv4Addr::new(10, 0, 0, 1))); // Lower metric wins.
    serial_println!("[netns]   Metric tie-breaking: OK");

    // Test 11: Process tracking.
    attach_process(ns1).expect("attach");
    attach_process(ns1).expect("attach");
    let s = stats(ns1).unwrap();
    assert_eq!(s.nr_procs, 2);
    detach_process(ns1).expect("detach");
    let s = stats(ns1).unwrap();
    assert_eq!(s.nr_procs, 1);
    serial_println!("[netns]   Process tracking: OK");

    // Test 12: Cannot delete namespace with processes.
    assert!(delete(ns1).is_err());
    detach_process(ns1).expect("detach last");
    serial_println!("[netns]   Delete non-empty rejected: OK");

    // Test 13: Cannot delete root.
    assert!(delete(ROOT_NS).is_err());
    serial_println!("[netns]   Root delete protection: OK");

    // Test 14: Stats query.
    let s = stats(ns1).unwrap();
    assert!(s.iface_up);
    assert_eq!(s.ip, Ipv4Addr::new(10, 0, 0, 2));
    assert_eq!(s.route_count, 2);
    serial_println!("[netns]   Stats: OK");

    // Test 15: Namespace isolation — different namespaces have
    // independent interface configs.
    let ns2 = create().expect("create ns2");
    configure_interface(ns2, Ipv4Addr::new(192, 168, 1, 100),
                        Ipv4Addr::new(255, 255, 255, 0),
                        Ipv4Addr::new(192, 168, 1, 1),
                        Ipv4Addr::new(1, 1, 1, 1)).expect("configure ns2");
    // ns1 and ns2 have different IPs.
    let cfg1 = interface_config(ns1).unwrap();
    let cfg2 = interface_config(ns2).unwrap();
    assert_eq!(cfg1.ip, Ipv4Addr::new(10, 0, 0, 2));
    assert_eq!(cfg2.ip, Ipv4Addr::new(192, 168, 1, 100));
    serial_println!("[netns]   Namespace isolation: OK");

    // Test 16: Route table full.
    let ns3 = create().expect("create ns3");
    for i in 0..MAX_ROUTES {
        #[allow(clippy::cast_possible_truncation)]
        let octet = (i & 0xFF) as u8;
        add_route(ns3, Ipv4Addr::new(octet, 0, 0, 0),
                  Ipv4Addr::new(255, 0, 0, 0),
                  Ipv4Addr::new(10, 0, 0, 1), 100).expect("fill routes");
    }
    assert!(add_route(ns3, Ipv4Addr::new(250, 0, 0, 0),
                      Ipv4Addr::new(255, 0, 0, 0),
                      Ipv4Addr::new(10, 0, 0, 1), 100).is_err());
    serial_println!("[netns]   Route table full: OK");

    // Test 17: Invalid namespace operations.
    assert!(configure_interface(99, Ipv4Addr::UNSPECIFIED, Ipv4Addr::UNSPECIFIED,
                                Ipv4Addr::UNSPECIFIED, Ipv4Addr::UNSPECIFIED).is_err());
    assert!(add_route(99, Ipv4Addr::UNSPECIFIED, Ipv4Addr::UNSPECIFIED,
                      Ipv4Addr::UNSPECIFIED, 0).is_err());
    assert!(!exists(99));
    serial_println!("[netns]   Invalid namespace rejected: OK");

    // Test 18: Remove non-existent route.
    assert!(remove_route(ns1, Ipv4Addr::new(172, 16, 0, 0),
                         Ipv4Addr::new(255, 255, 0, 0)).is_err());
    serial_println!("[netns]   Remove non-existent route: OK");

    // Cleanup.
    delete(ns1).expect("delete ns1");
    delete(ns2).expect("delete ns2");
    delete(ns3).expect("delete ns3");
    assert_eq!(active_count(), 1);
    serial_println!("[netns]   Cleanup: OK");

    serial_println!("[netns] Self-test PASSED (18 tests)");
}
