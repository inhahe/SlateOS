//! Network interface management.
//!
//! Manages the kernel's view of network interfaces: MAC address,
//! IP address, gateway, subnet mask.  Currently supports a single
//! interface backed by the virtio-net device.

use core::fmt;

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
pub fn configure(ip: Ipv4Addr, mask: Ipv4Addr, gateway: Ipv4Addr, dns: Ipv4Addr) {
    let mut iface = IFACE.lock();
    iface.ip = ip;
    iface.subnet_mask = mask;
    iface.gateway = gateway;
    iface.dns = dns;
    crate::serial_println!(
        "[net] Configured: IP {} mask {} gw {} dns {}",
        ip, mask, gateway, dns
    );
}
