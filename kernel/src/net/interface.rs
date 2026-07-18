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

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::sync::Mutex;

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
// Dual-stack IP address
// ---------------------------------------------------------------------------

/// A dual-stack IP address (IPv4 or IPv6).
///
/// Used by protocol implementations that support both address families
/// (e.g., TCP, UDP sockets).  Allows a single connection structure to
/// hold either address type without duplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpAddr {
    /// An IPv4 address.
    V4(Ipv4Addr),
    /// An IPv6 address.
    V6(super::ipv6::Ipv6Addr),
}

impl IpAddr {
    /// The unspecified IPv4 address (0.0.0.0).
    pub const UNSPECIFIED_V4: Self = Self::V4(Ipv4Addr::UNSPECIFIED);

    /// The unspecified IPv6 address (::).
    pub const UNSPECIFIED_V6: Self = Self::V6(super::ipv6::Ipv6Addr::UNSPECIFIED);

    /// Check if this is an IPv4 address.
    pub fn is_v4(self) -> bool {
        matches!(self, Self::V4(_))
    }

    /// Check if this is an IPv6 address.
    pub fn is_v6(self) -> bool {
        matches!(self, Self::V6(_))
    }

    /// Get the IPv4 address, if this is IPv4.
    pub fn as_v4(self) -> Option<Ipv4Addr> {
        match self {
            Self::V4(a) => Some(a),
            Self::V6(_) => None,
        }
    }

    /// Get the IPv6 address, if this is IPv6.
    pub fn as_v6(self) -> Option<super::ipv6::Ipv6Addr> {
        match self {
            Self::V4(_) => None,
            Self::V6(a) => Some(a),
        }
    }

    /// Check if this is the unspecified address (for either family).
    pub fn is_unspecified(self) -> bool {
        match self {
            Self::V4(a) => a.is_unspecified(),
            Self::V6(a) => a.is_unspecified(),
        }
    }

    /// Check if this is a multicast address (for either family).
    pub fn is_multicast(self) -> bool {
        match self {
            Self::V4(a) => a.is_multicast(),
            Self::V6(a) => a.is_multicast(),
        }
    }

    /// Check if this is a loopback address.
    pub fn is_loopback(self) -> bool {
        match self {
            Self::V4(a) => a.0 == [127, 0, 0, 1],
            Self::V6(a) => a.is_loopback(),
        }
    }
}

impl From<Ipv4Addr> for IpAddr {
    fn from(v4: Ipv4Addr) -> Self {
        Self::V4(v4)
    }
}

impl From<super::ipv6::Ipv6Addr> for IpAddr {
    fn from(v6: super::ipv6::Ipv6Addr) -> Self {
        Self::V6(v6)
    }
}

impl fmt::Display for IpAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V4(a) => write!(f, "{}", a),
            Self::V6(a) => write!(f, "{}", a),
        }
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

/// Bring the interface administratively up or down.
///
/// This is the write side of [`is_up`], used by `ifconfig <if> up/down` and
/// `ip link set <if> up/down` via `SYS_NET_IF_CONFIG`. It flips the `up` flag
/// on the physical NIC (root namespace) and, if the netns subsystem is
/// initialized, syncs the flag into the root network namespace so
/// namespace-aware queries stay consistent.
///
/// Note: this reflects the *administrative* state only — it does not power the
/// NIC hardware down. Bringing the interface down stops the stack from treating
/// it as usable for new traffic; addresses are retained so a subsequent `up`
/// restores connectivity without reconfiguration (matching Linux semantics).
pub fn set_up(up: bool) {
    IFACE.lock().up = up;
    crate::serial_println!("[net] Interface administratively {}", if up { "up" } else { "down" });

    // Keep the root namespace's view consistent. No-op if netns is not yet
    // initialized (boot ordering: net::init runs before netns::init).
    if crate::netns::is_initialized() {
        // Ignore the "namespace missing" error: the root namespace always
        // exists once netns is initialized, and if it somehow does not, the
        // global IFACE flag above is still authoritative for the root ns
        // (ns_is_up falls back to is_up() for ROOT_NS).
        let _ = crate::netns::set_interface_up(crate::netns::ROOT_NS, up);
    }
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
#[allow(dead_code)] // Public API.
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
        Some(cfg) => {
            // Use the veth endpoint's MAC if one is assigned to this
            // namespace; fall back to the physical NIC MAC otherwise.
            let iface_mac = super::veth::find_endpoint_for_ns(ns_id)
                .and_then(|(pair_id, end_id)| super::veth::mac(pair_id, end_id))
                .map(crate::virtio::net::MacAddress)
                .unwrap_or_else(mac);
            InterfaceInfo {
                up: cfg.up,
                mac: iface_mac,
                ip: from_netns_ip(cfg.ip),
                subnet_mask: from_netns_ip(cfg.subnet_mask),
                gateway: from_netns_ip(cfg.gateway),
                dns: from_netns_ip(cfg.dns),
            }
        }
        None => InterfaceInfo::default(),
    }
}

/// Get the MAC address for a specific network namespace.
///
/// For the root namespace, returns the physical NIC's MAC.  For child
/// namespaces, returns the MAC of the veth endpoint assigned to that
/// namespace (the interface the namespace actually receives frames on),
/// falling back to the physical NIC MAC if no veth endpoint is assigned.
///
/// Used by the RX path (`ethernet::process_frame`) to decide whether an
/// incoming frame is addressed to the interface in a given namespace.
pub fn ns_mac(ns_id: crate::netns::NetNsId) -> MacAddress {
    if ns_id == crate::netns::ROOT_NS {
        return mac();
    }
    super::veth::find_endpoint_for_ns(ns_id)
        .and_then(|(pair_id, end_id)| super::veth::mac(pair_id, end_id))
        .map(crate::virtio::net::MacAddress)
        .unwrap_or_else(mac)
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

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Network interface unit tests — exercises Ipv4Addr methods and
/// InterfaceInfo defaults.
pub fn self_test() -> crate::error::KernelResult<()> {
    crate::serial_println!("[interface] Running self-test...");

    test_ipv4_addr_constructors()?;
    test_ipv4_addr_u32_roundtrip()?;
    test_ipv4_addr_classification()?;
    test_ipv4_same_subnet()?;
    test_interface_info_default()?;
    test_write_primitives()?;

    crate::serial_println!("[interface] Self-test PASSED (6 tests)");
    Ok(())
}

/// Test the interface write primitives (`configure` + `set_up`) that back the
/// `SYS_NET_IF_CONFIG` syscall. Snapshots the live config, mutates it, asserts
/// the changes are visible via `info()`/`is_up()`, then restores the original
/// state so the self-test does not disrupt real networking configured earlier
/// in boot (DHCP, static config).
fn test_write_primitives() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let orig = info();
    let orig_up = is_up();

    // Apply a distinctive test configuration.
    let ip = Ipv4Addr::new(10, 77, 88, 99);
    let mask = Ipv4Addr::new(255, 255, 255, 0);
    let gw = Ipv4Addr::new(10, 77, 88, 1);
    let dns = Ipv4Addr::new(9, 9, 9, 9);
    configure(ip, mask, gw, dns);

    let after = info();
    if after.ip != ip || after.subnet_mask != mask || after.gateway != gw || after.dns != dns {
        crate::serial_println!("[interface]   FAIL: configure() did not apply all fields");
        // Best-effort restore before returning the error.
        configure(orig.ip, orig.subnet_mask, orig.gateway, orig.dns);
        set_up(orig_up);
        return Err(KernelError::InternalError);
    }

    // Toggle the administrative up/down flag.
    set_up(false);
    if is_up() {
        crate::serial_println!("[interface]   FAIL: set_up(false) did not bring interface down");
        configure(orig.ip, orig.subnet_mask, orig.gateway, orig.dns);
        set_up(orig_up);
        return Err(KernelError::InternalError);
    }
    set_up(true);
    if !is_up() {
        crate::serial_println!("[interface]   FAIL: set_up(true) did not bring interface up");
        configure(orig.ip, orig.subnet_mask, orig.gateway, orig.dns);
        set_up(orig_up);
        return Err(KernelError::InternalError);
    }

    // Restore the original live configuration.
    configure(orig.ip, orig.subnet_mask, orig.gateway, orig.dns);
    set_up(orig_up);

    crate::serial_println!("[interface]   write primitives (configure/set_up): OK");
    Ok(())
}

/// Test Ipv4Addr constructors and constants.
fn test_ipv4_addr_constructors() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let addr = Ipv4Addr::new(192, 168, 1, 100);
    if addr.0 != [192, 168, 1, 100] {
        crate::serial_println!("[interface]   FAIL: new() octets");
        return Err(KernelError::InternalError);
    }

    if Ipv4Addr::UNSPECIFIED.0 != [0, 0, 0, 0] {
        crate::serial_println!("[interface]   FAIL: UNSPECIFIED");
        return Err(KernelError::InternalError);
    }
    if Ipv4Addr::BROADCAST.0 != [255, 255, 255, 255] {
        crate::serial_println!("[interface]   FAIL: BROADCAST");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[interface]   ipv4 constructors: OK");
    Ok(())
}

/// Test to_u32 / from_u32 roundtrip.
fn test_ipv4_addr_u32_roundtrip() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let addr = Ipv4Addr::new(10, 0, 1, 255);
    let val = addr.to_u32();
    // 10.0.1.255 in network order = 0x0A0001FF.
    if val != 0x0A00_01FF {
        crate::serial_println!(
            "[interface]   FAIL: to_u32 = {:#010x}", val
        );
        return Err(KernelError::InternalError);
    }

    let back = Ipv4Addr::from_u32(val);
    if back != addr {
        crate::serial_println!("[interface]   FAIL: from_u32 roundtrip");
        return Err(KernelError::InternalError);
    }

    // 0xFFFFFFFF → 255.255.255.255.
    let bcast = Ipv4Addr::from_u32(0xFFFF_FFFF);
    if bcast != Ipv4Addr::BROADCAST {
        crate::serial_println!("[interface]   FAIL: from_u32(0xFFFFFFFF)");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[interface]   ipv4 u32 roundtrip: OK");
    Ok(())
}

/// Test is_unspecified, is_broadcast, is_multicast.
fn test_ipv4_addr_classification() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    if !Ipv4Addr::UNSPECIFIED.is_unspecified() {
        crate::serial_println!("[interface]   FAIL: UNSPECIFIED.is_unspecified()");
        return Err(KernelError::InternalError);
    }
    if Ipv4Addr::new(10, 0, 0, 1).is_unspecified() {
        crate::serial_println!("[interface]   FAIL: 10.0.0.1 should not be unspecified");
        return Err(KernelError::InternalError);
    }

    if !Ipv4Addr::BROADCAST.is_broadcast() {
        crate::serial_println!("[interface]   FAIL: BROADCAST.is_broadcast()");
        return Err(KernelError::InternalError);
    }

    // 224.0.0.1 is multicast (all hosts).
    if !Ipv4Addr::new(224, 0, 0, 1).is_multicast() {
        crate::serial_println!("[interface]   FAIL: 224.0.0.1 not multicast");
        return Err(KernelError::InternalError);
    }
    // 239.255.255.255 is multicast (last multicast address).
    if !Ipv4Addr::new(239, 255, 255, 255).is_multicast() {
        crate::serial_println!("[interface]   FAIL: 239.x not multicast");
        return Err(KernelError::InternalError);
    }
    // 240.0.0.0 is NOT multicast (class E reserved).
    if Ipv4Addr::new(240, 0, 0, 0).is_multicast() {
        crate::serial_println!("[interface]   FAIL: 240.0.0.0 should not be multicast");
        return Err(KernelError::InternalError);
    }
    // 10.0.0.1 is NOT multicast.
    if Ipv4Addr::new(10, 0, 0, 1).is_multicast() {
        crate::serial_println!("[interface]   FAIL: 10.0.0.1 should not be multicast");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[interface]   ipv4 classification: OK");
    Ok(())
}

/// Test same_subnet().
fn test_ipv4_same_subnet() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let mask = Ipv4Addr::new(255, 255, 255, 0);

    // Same /24 subnet.
    let a = Ipv4Addr::new(192, 168, 1, 10);
    let b = Ipv4Addr::new(192, 168, 1, 20);
    if !a.same_subnet(b, mask) {
        crate::serial_println!("[interface]   FAIL: same subnet not detected");
        return Err(KernelError::InternalError);
    }

    // Different subnet.
    let c = Ipv4Addr::new(192, 168, 2, 10);
    if a.same_subnet(c, mask) {
        crate::serial_println!("[interface]   FAIL: different subnet not detected");
        return Err(KernelError::InternalError);
    }

    // /16 mask.
    let mask16 = Ipv4Addr::new(255, 255, 0, 0);
    if !a.same_subnet(c, mask16) {
        crate::serial_println!("[interface]   FAIL: /16 same subnet");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[interface]   same_subnet: OK");
    Ok(())
}

/// Test InterfaceInfo default values.
fn test_interface_info_default() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    let info = InterfaceInfo::default();
    if info.up {
        crate::serial_println!("[interface]   FAIL: default should be down");
        return Err(KernelError::InternalError);
    }
    if !info.ip.is_unspecified() {
        crate::serial_println!("[interface]   FAIL: default IP should be 0.0.0.0");
        return Err(KernelError::InternalError);
    }
    if info.mac.0 != [0; 6] {
        crate::serial_println!("[interface]   FAIL: default MAC should be zero");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[interface]   InterfaceInfo default: OK");
    Ok(())
}
