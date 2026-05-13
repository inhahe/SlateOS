//! Network interface management.
//!
//! Manages the kernel's view of network interfaces: MAC address,
//! IP address, gateway, subnet mask.  Currently supports a single
//! physical interface backed by the virtio-net (or e1000/rtl8139) device.
//!
//! ## Namespace integration
//!
//! The global `IFACE` state represents the physical NIC and serves as
//! the root network namespace (netns ID 0).  Non-root namespaces have
//! independent interface configuration stored in the `netns` module.
//!
//! Callers that need namespace-aware behavior should use `ns_ip()`,
//! `ns_info()`, and `ns_is_up()` instead of the bare `ip()`, `info()`,
//! `is_up()`.  The `ns_*` functions return the physical NIC config for
//! the root namespace and delegate to `netns::interface_config()` for
//! child namespaces.

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use crate::virtio::net::MacAddress;

// ---------------------------------------------------------------------------
// IPv4 address
// ---------------------------------------------------------------------------

/// An IPv4 address (4 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    /// The unspecified address (0.0.0.0).
    pub const UNSPECIFIED: Self = Self([0, 0, 0, 0]);
    /// The broadcast address (255.255.255.255).
    pub const BROADCAST: Self = Self([255, 255, 255, 255]);

    /// Create an address from four octets.
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }

    /// Convert to a u32 in network byte order (big-endian).
    pub fn to_u32(self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    /// Create from a u32 in network byte order (big-endian).
    pub fn from_u32(val: u32) -> Self {
        Self(val.to_be_bytes())
    }

    /// Check if this is the unspecified address.
    pub fn is_unspecified(self) -> bool {
        self == Self::UNSPECIFIED
    }

    /// Check if this is a broadcast address.
    pub fn is_broadcast(self) -> bool {
        self == Self::BROADCAST
    }

    /// Check if this is a multicast address (224.0.0.0/4, RFC 1112).
    ///
    /// Multicast addresses have the first nibble set to 0xE (224–239).
    pub fn is_multicast(self) -> bool {
        self.0[0] & 0xF0 == 0xE0
    }

    /// Check if this address is in the same subnet as `other`.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn same_subnet(self, other: Self, mask: Self) -> bool {
        let a = self.to_u32() & mask.to_u32();
        let b = other.to_u32() & mask.to_u32();
        a == b
    }
}

impl fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

// ---------------------------------------------------------------------------
// Network interface state
// ---------------------------------------------------------------------------

/// Configuration of a network interface.
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    /// Whether the interface is up.
    pub up: bool,
    /// MAC address.
    pub mac: MacAddress,
    /// IPv4 address.
    pub ip: Ipv4Addr,
    /// Subnet mask.
    pub subnet_mask: Ipv4Addr,
    /// Default gateway.
    pub gateway: Ipv4Addr,
    /// DNS server.
    pub dns: Ipv4Addr,
}

impl Default for InterfaceInfo {
    fn default() -> Self {
        Self {
            up: false,
            mac: MacAddress([0; 6]),
            ip: Ipv4Addr::UNSPECIFIED,
            subnet_mask: Ipv4Addr::UNSPECIFIED,
            gateway: Ipv4Addr::UNSPECIFIED,
            dns: Ipv4Addr::UNSPECIFIED,
        }
    }
}

/// Global network interface state.
static IFACE: Mutex<InterfaceInfo> = Mutex::new(InterfaceInfo {
    up: false,
    mac: MacAddress([0; 6]),
    ip: Ipv4Addr::UNSPECIFIED,
    subnet_mask: Ipv4Addr::UNSPECIFIED,
    gateway: Ipv4Addr::UNSPECIFIED,
    dns: Ipv4Addr::UNSPECIFIED,
});

/// Initialize the network interface from the active NIC.
///
/// Tries virtio-net first, then falls back to e1000.
pub fn init() {
    // Try virtio-net first.
    let mac = crate::virtio::net::with_device(|dev| dev.mac());
    if let Some(mac) = mac {
        let mut iface = IFACE.lock();
        iface.mac = mac;
        iface.up = true;
        crate::serial_println!("[net] Interface up (virtio-net): MAC {}", mac);
        return;
    }

    // Fall back to e1000.
    let mac = crate::e1000::with_device(|dev| dev.mac());
    if let Some(mac) = mac {
        let mut iface = IFACE.lock();
        iface.mac = mac;
        iface.up = true;
        crate::serial_println!("[net] Interface up (e1000): MAC {}", mac);
        return;
    }

    // Fall back to RTL8139.
    let mac = crate::rtl8139::with_device(|dev| dev.mac());
    if let Some(mac) = mac {
        let mut iface = IFACE.lock();
        iface.mac = MacAddress(mac);
        iface.up = true;
        crate::serial_println!("[net] Interface up (rtl8139): MAC {}", MacAddress(mac));
        return;
    }

    crate::serial_println!("[net] No NIC — interface not configured");
}

/// Check if the interface is up.
pub fn is_up() -> bool {
    IFACE.lock().up
}

/// Get the interface MAC address.
pub fn mac() -> MacAddress {
    IFACE.lock().mac
}

/// Get a snapshot of the interface configuration.
pub fn info() -> InterfaceInfo {
    IFACE.lock().clone()
}

/// Get the current IPv4 address.
pub fn ip() -> Ipv4Addr {
    IFACE.lock().ip
}

/// Configure the interface with DHCP results.
///
/// Also syncs the configuration to the root network namespace (netns
/// ID 0) so that namespace-aware code sees the same state.  If the
/// netns subsystem has not been initialized yet (it initializes after
/// the network stack at boot), the sync is skipped — the `ns_*`
/// accessors fall back to the global `IFACE` for the root namespace
/// anyway.
pub fn configure(ip: Ipv4Addr, mask: Ipv4Addr, gateway: Ipv4Addr, dns: Ipv4Addr) {
    {
        let mut iface = IFACE.lock();
        iface.ip = ip;
        iface.subnet_mask = mask;
        iface.gateway = gateway;
        iface.dns = dns;
    }
    crate::serial_println!(
        "[net] Configured: IP {} mask {} gw {} dns {}",
        ip, mask, gateway, dns
    );

    // Sync to the root network namespace so that per-namespace queries
    // return consistent results.  No-op if netns is not yet initialized
    // (boot ordering: net::init runs before netns::init).
    if crate::netns::is_initialized() {
        let _ = crate::netns::configure_interface(
            crate::netns::ROOT_NS,
            to_netns_ip(ip),
            to_netns_ip(mask),
            to_netns_ip(gateway),
            to_netns_ip(dns),
        );
    }

    // Send a gratuitous ARP to announce our IP on the LAN.
    // Failures are non-fatal — best-effort announcement.
    let _ = super::arp::send_gratuitous();
}

// ---------------------------------------------------------------------------
// Namespace-aware accessors
// ---------------------------------------------------------------------------

/// Convert a `netns::Ipv4Addr` to our `Ipv4Addr`.
///
/// Both types are `[u8; 4]` wrappers — netns uses its own copy to
/// avoid a circular dependency on this module.
fn from_netns_ip(ip: crate::netns::Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr(ip.0)
}

/// Convert our `Ipv4Addr` to a `netns::Ipv4Addr`.
fn to_netns_ip(ip: Ipv4Addr) -> crate::netns::Ipv4Addr {
    crate::netns::Ipv4Addr(ip.0)
}

/// Get the IPv4 address for a specific network namespace.
///
/// For the root namespace (ID 0), returns the physical NIC's IP from
/// the global `IFACE`.  For child namespaces, queries the per-namespace
/// configuration in the `netns` module.
///
/// Returns `0.0.0.0` if the namespace does not exist.
pub fn ns_ip(ns_id: crate::netns::NetNsId) -> Ipv4Addr {
    if ns_id == crate::netns::ROOT_NS {
        return ip();
    }
    match crate::netns::interface_config(ns_id) {
        Some(cfg) => from_netns_ip(cfg.ip),
        None => Ipv4Addr::UNSPECIFIED,
    }
}

/// Check if the interface is up for a specific network namespace.
///
/// For the root namespace, checks the physical NIC.  For child
/// namespaces, checks the per-namespace interface state.
pub fn ns_is_up(ns_id: crate::netns::NetNsId) -> bool {
    if ns_id == crate::netns::ROOT_NS {
        return is_up();
    }
    crate::netns::interface_config(ns_id)
        .is_some_and(|cfg| cfg.up)
}

/// Get a snapshot of the interface configuration for a specific
/// network namespace.
///
/// For the root namespace, returns the physical NIC's config.  For
/// child namespaces, returns the virtual interface config with the
/// physical NIC's MAC address (since all namespaces share the same
/// physical NIC until virtual ethernet pairs are implemented).
///
/// Returns a default (all-zeros, down) config if the namespace does
/// not exist.
pub fn ns_info(ns_id: crate::netns::NetNsId) -> InterfaceInfo {
    if ns_id == crate::netns::ROOT_NS {
        return info();
    }
    match crate::netns::interface_config(ns_id) {
        Some(cfg) => InterfaceInfo {
            up: cfg.up,
            mac: mac(), // Physical NIC MAC — no veth pairs yet.
            ip: from_netns_ip(cfg.ip),
            subnet_mask: from_netns_ip(cfg.subnet_mask),
            gateway: from_netns_ip(cfg.gateway),
            dns: from_netns_ip(cfg.dns),
        },
        None => InterfaceInfo::default(),
    }
}

// ---------------------------------------------------------------------------
// Interface statistics (lock-free atomic counters)
// ---------------------------------------------------------------------------

/// Total bytes sent (Ethernet frame payload, excluding Ethernet header).
static TX_BYTES: AtomicU64 = AtomicU64::new(0);
/// Total bytes received.
static RX_BYTES: AtomicU64 = AtomicU64::new(0);
/// Total frames sent.
static TX_PACKETS: AtomicU64 = AtomicU64::new(0);
/// Total frames received.
static RX_PACKETS: AtomicU64 = AtomicU64::new(0);
/// Total send errors.
static TX_ERRORS: AtomicU64 = AtomicU64::new(0);
/// Total frames dropped (invalid, too short, etc.).
static RX_DROPS: AtomicU64 = AtomicU64::new(0);

/// Record a successful frame transmission.
pub fn record_tx(bytes: usize) {
    TX_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    TX_PACKETS.fetch_add(1, Ordering::Relaxed);
}

/// Record a failed frame transmission.
pub fn record_tx_error() {
    TX_ERRORS.fetch_add(1, Ordering::Relaxed);
}

/// Record a successful frame reception.
pub fn record_rx(bytes: usize) {
    RX_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    RX_PACKETS.fetch_add(1, Ordering::Relaxed);
}

/// Record a dropped incoming frame.
pub fn record_rx_drop() {
    RX_DROPS.fetch_add(1, Ordering::Relaxed);
}

/// Snapshot of interface traffic statistics.
#[derive(Debug, Clone, Copy)]
pub struct InterfaceStats {
    /// Total bytes transmitted.
    pub tx_bytes: u64,
    /// Total frames transmitted.
    pub tx_packets: u64,
    /// Total transmit errors.
    pub tx_errors: u64,
    /// Total bytes received.
    pub rx_bytes: u64,
    /// Total frames received.
    pub rx_packets: u64,
    /// Total received frames dropped.
    pub rx_drops: u64,
}

/// Return a snapshot of interface traffic statistics.
///
/// Counters are monotonically increasing and never reset (except on
/// reboot).  Use the difference between two snapshots to compute
/// per-interval rates.
pub fn stats() -> InterfaceStats {
    InterfaceStats {
        tx_bytes: TX_BYTES.load(Ordering::Relaxed),
        tx_packets: TX_PACKETS.load(Ordering::Relaxed),
        tx_errors: TX_ERRORS.load(Ordering::Relaxed),
        rx_bytes: RX_BYTES.load(Ordering::Relaxed),
        rx_packets: RX_PACKETS.load(Ordering::Relaxed),
        rx_drops: RX_DROPS.load(Ordering::Relaxed),
    }
}
