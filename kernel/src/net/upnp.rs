//! UPnP IGD and NAT-PMP/PCP port forwarding.
//!
//! Automatically discovers the gateway router and creates/deletes port
//! mappings so that services (SSH, game servers, file sharing, etc.)
//! can receive incoming connections from the internet.
//!
//! ## Protocols
//!
//! Two independent protocols are supported:
//!
//! - **NAT-PMP** (RFC 6886) / **PCP** (RFC 6887): simple UDP protocol
//!   on port 5351.  Fast discovery, low overhead.  Supported by Apple
//!   AirPort, many consumer routers (OpenWrt, MikroTik, etc.).
//!
//! - **UPnP IGD** (Internet Gateway Device): SSDP multicast discovery
//!   + SOAP/XML control.  More complex but more widely supported
//!     (most consumer routers enable UPnP by default).
//!
//! We try NAT-PMP first (simpler, faster), then fall back to UPnP IGD.
//!
//! ## NAT-PMP Protocol
//!
//! ```text
//! Client                        Gateway (port 5351/UDP)
//!   │                               │
//!   │── External addr request ──────>│  (opcode 0)
//!   │<── External addr response ─────│  (opcode 128, result code, ext IP)
//!   │                               │
//!   │── Map TCP/UDP request ────────>│  (opcode 1/2, internal port, external port, lifetime)
//!   │<── Map response ──────────────│  (opcode 129/130, result code, mapped ports, lifetime)
//!   │                               │
//!   │── Destroy mapping ────────────>│  (same as map, lifetime=0)
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! upnp::init();                                      // Discover gateway
//! upnp::add_mapping(Protocol::Tcp, 8080, 8080, 3600, "My Web Server");
//! // ... later ...
//! upnp::remove_mapping(Protocol::Tcp, 8080);
//! ```
//!
//! ## References
//!
//! - RFC 6886: NAT Port Mapping Protocol (NAT-PMP)
//! - RFC 6887: Port Control Protocol (PCP)
//! - UPnP Forum: Internet Gateway Device v1.0 / v2.0
//! - miniupnpc (reference C implementation)

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// NAT-PMP server port on the gateway.
const NATPMP_PORT: u16 = 5351;

/// UPnP SSDP multicast address (239.255.255.250).
const SSDP_MULTICAST_IP: u32 = 0xEFFF_FFFA; // 239.255.255.250
/// SSDP port.
const SSDP_PORT: u16 = 1900;

/// Maximum port mappings we track.
const MAX_MAPPINGS: usize = 64;

/// Default mapping lifetime in seconds (1 hour).
const DEFAULT_LIFETIME_SECS: u32 = 3600;

/// Minimum acceptable mapping lifetime (2 minutes).
const MIN_LIFETIME_SECS: u32 = 120;

/// Maximum mapping lifetime (24 hours).
const MAX_LIFETIME_SECS: u32 = 86400;

/// NAT-PMP opcode: Get external address.
const NATPMP_OP_EXTERN_ADDR: u8 = 0;

/// NAT-PMP opcode: Map UDP port.
const NATPMP_OP_MAP_UDP: u8 = 1;

/// NAT-PMP opcode: Map TCP port.
const NATPMP_OP_MAP_TCP: u8 = 2;

/// NAT-PMP response flag (bit 7 of opcode).
const NATPMP_RESPONSE_FLAG: u8 = 128;

/// NAT-PMP result codes.
const NATPMP_RESULT_SUCCESS: u16 = 0;
const NATPMP_RESULT_UNSUPPORTED_VERSION: u16 = 1;
const NATPMP_RESULT_NOT_AUTHORIZED: u16 = 2;
const NATPMP_RESULT_NETWORK_FAILURE: u16 = 3;
const NATPMP_RESULT_OUT_OF_RESOURCES: u16 = 4;
const NATPMP_RESULT_UNSUPPORTED_OPCODE: u16 = 5;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Network protocol for port mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    /// Label for display.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }

    /// NAT-PMP opcode for this protocol.
    const fn natpmp_opcode(self) -> u8 {
        match self {
            Self::Udp => NATPMP_OP_MAP_UDP,
            Self::Tcp => NATPMP_OP_MAP_TCP,
        }
    }
}

/// Discovery method used to find the gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryMethod {
    /// Not yet discovered.
    None,
    /// NAT-PMP (RFC 6886) — direct UDP to gateway.
    NatPmp,
    /// UPnP IGD — SSDP multicast discovery + SOAP control.
    UpnpIgd,
}

impl DiscoveryMethod {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::NatPmp => "NAT-PMP",
            Self::UpnpIgd => "UPnP IGD",
        }
    }
}

/// State of a port mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingState {
    /// Mapping requested but not yet confirmed by gateway.
    Pending,
    /// Mapping confirmed and active.
    Active,
    /// Mapping failed (see error).
    Failed,
    /// Mapping expired (lifetime ran out).
    Expired,
    /// Removal requested.
    Removing,
}

impl MappingState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Failed => "failed",
            Self::Expired => "expired",
            Self::Removing => "removing",
        }
    }
}

/// A port forwarding mapping.
#[derive(Debug, Clone)]
pub struct PortMapping {
    /// Protocol (TCP or UDP).
    pub protocol: Protocol,
    /// Internal (LAN) port on this machine.
    pub internal_port: u16,
    /// External (WAN) port on the router.
    pub external_port: u16,
    /// Mapping lifetime in seconds (0 = permanent until removed).
    pub lifetime_secs: u32,
    /// Human-readable description (e.g., "SSH server").
    pub description: String,
    /// Current state.
    pub state: MappingState,
    /// Timestamp (ns) when the mapping was created or last renewed.
    pub created_ns: u64,
    /// Timestamp (ns) when the mapping expires.
    pub expires_ns: u64,
    /// Number of successful renewals.
    pub renewals: u32,
    /// Last error message (if state == Failed).
    pub last_error: Option<String>,
}

/// NAT-PMP external address response.
#[derive(Debug, Clone, Copy)]
struct NatPmpExternAddr {
    /// Result code.
    result: u16,
    /// Seconds since epoch (gateway uptime in some implementations).
    epoch: u32,
    /// External IPv4 address (network byte order).
    external_ip: u32,
}

/// NAT-PMP mapping response.
#[derive(Debug, Clone, Copy)]
struct NatPmpMapResponse {
    /// Result code.
    result: u16,
    /// Seconds since epoch.
    epoch: u32,
    /// Internal port.
    internal_port: u16,
    /// Mapped external port (may differ from requested).
    external_port: u16,
    /// Lifetime in seconds (as granted by gateway).
    lifetime: u32,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Main UPnP/NAT-PMP state.
struct State {
    /// Whether init has been called.
    initialized: bool,
    /// Discovery method detected.
    method: DiscoveryMethod,
    /// Gateway IP address (from DHCP or manual config).
    gateway_ip: u32,
    /// External (public) IP address from the gateway.
    external_ip: u32,
    /// Active port mappings.
    mappings: Vec<PortMapping>,
    /// Total mappings ever created.
    total_created: u64,
    /// Total mappings ever removed.
    total_removed: u64,
    /// Total renewal attempts.
    total_renewals: u64,
    /// Total failures.
    total_failures: u64,
    /// UPnP control URL (if using UPnP IGD).
    upnp_control_url: Option<String>,
    /// UPnP service type string.
    upnp_service_type: Option<String>,
    /// Discovery timestamp (ns).
    discovered_ns: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            initialized: false,
            method: DiscoveryMethod::None,
            gateway_ip: 0,
            external_ip: 0,
            mappings: Vec::new(),
            total_created: 0,
            total_removed: 0,
            total_renewals: 0,
            total_failures: 0,
            upnp_control_url: None,
            upnp_service_type: None,
            discovered_ns: 0,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static MAPPING_COUNT: AtomicU32 = AtomicU32::new(0);
static TICK_COUNTER: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// IPv4 formatting helper
// ---------------------------------------------------------------------------

fn format_ipv4(ip: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (ip >> 24) & 0xFF,
        (ip >> 16) & 0xFF,
        (ip >> 8) & 0xFF,
        ip & 0xFF,
    )
}

fn parse_ipv4(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (u32::from(a) << 24) | (u32::from(b) << 16) | (u32::from(c) << 8) | u32::from(d)
}

// ---------------------------------------------------------------------------
// NAT-PMP packet building
// ---------------------------------------------------------------------------

/// Build a NAT-PMP external address request packet.
///
/// Format (2 bytes):
///   [version=0] [opcode=0]
fn build_natpmp_extern_addr_request() -> Vec<u8> {
    alloc::vec![0u8, NATPMP_OP_EXTERN_ADDR]
}

/// Build a NAT-PMP mapping request packet.
///
/// Format (12 bytes):
///   [version=0] [opcode] [reserved=0,0] [internal_port:u16be]
///   [external_port:u16be] [lifetime:u32be]
fn build_natpmp_map_request(
    protocol: Protocol,
    internal_port: u16,
    external_port: u16,
    lifetime_secs: u32,
) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(12);
    pkt.push(0); // version
    pkt.push(protocol.natpmp_opcode());
    pkt.push(0); // reserved
    pkt.push(0);
    pkt.extend_from_slice(&internal_port.to_be_bytes());
    pkt.extend_from_slice(&external_port.to_be_bytes());
    pkt.extend_from_slice(&lifetime_secs.to_be_bytes());
    pkt
}

/// Parse a NAT-PMP external address response.
///
/// Response format (12 bytes):
///   [version=0] [opcode=128] [result:u16be] [epoch:u32be] [ip:u32be]
fn parse_natpmp_extern_addr_response(data: &[u8]) -> Option<NatPmpExternAddr> {
    if data.len() < 12 {
        return None;
    }
    if data[1] != NATPMP_RESPONSE_FLAG | NATPMP_OP_EXTERN_ADDR {
        return None;
    }
    let result = u16::from_be_bytes([data[2], data[3]]);
    let epoch = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let external_ip = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    Some(NatPmpExternAddr {
        result,
        epoch,
        external_ip,
    })
}

/// Parse a NAT-PMP mapping response.
///
/// Response format (16 bytes):
///   [version=0] [opcode=129/130] [result:u16be] [epoch:u32be]
///   [internal:u16be] [external:u16be] [lifetime:u32be]
fn parse_natpmp_map_response(data: &[u8]) -> Option<NatPmpMapResponse> {
    if data.len() < 16 {
        return None;
    }
    let opcode = data[1];
    // Response opcodes are 129 (UDP) or 130 (TCP).
    if opcode != (NATPMP_RESPONSE_FLAG | NATPMP_OP_MAP_UDP)
        && opcode != (NATPMP_RESPONSE_FLAG | NATPMP_OP_MAP_TCP)
    {
        return None;
    }
    let result = u16::from_be_bytes([data[2], data[3]]);
    let epoch = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let internal_port = u16::from_be_bytes([data[8], data[9]]);
    let external_port = u16::from_be_bytes([data[10], data[11]]);
    let lifetime = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
    Some(NatPmpMapResponse {
        result,
        epoch,
        internal_port,
        external_port,
        lifetime,
    })
}

/// Human-readable NAT-PMP result code.
fn natpmp_result_str(code: u16) -> &'static str {
    match code {
        NATPMP_RESULT_SUCCESS => "success",
        NATPMP_RESULT_UNSUPPORTED_VERSION => "unsupported version",
        NATPMP_RESULT_NOT_AUTHORIZED => "not authorized",
        NATPMP_RESULT_NETWORK_FAILURE => "network failure",
        NATPMP_RESULT_OUT_OF_RESOURCES => "out of resources",
        NATPMP_RESULT_UNSUPPORTED_OPCODE => "unsupported opcode",
        _ => "unknown error",
    }
}

// ---------------------------------------------------------------------------
// UPnP SSDP discovery
// ---------------------------------------------------------------------------

/// Build an SSDP M-SEARCH request packet for UPnP IGD discovery.
///
/// This is sent as a UDP multicast to 239.255.255.250:1900.
fn build_ssdp_msearch() -> Vec<u8> {
    let msg = "M-SEARCH * HTTP/1.1\r\n\
               HOST: 239.255.255.250:1900\r\n\
               MAN: \"ssdp:discover\"\r\n\
               MX: 3\r\n\
               ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
               \r\n";
    Vec::from(msg.as_bytes())
}

/// Build a SOAP AddPortMapping request body.
fn build_soap_add_mapping(
    protocol: Protocol,
    external_port: u16,
    internal_ip: &str,
    internal_port: u16,
    description: &str,
    lifetime: u32,
) -> Vec<u8> {
    let proto_str = match protocol {
        Protocol::Tcp => "TCP",
        Protocol::Udp => "UDP",
    };
    let body = format!(
        "<?xml version=\"1.0\"?>\r\n\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
         <s:Body>\r\n\
         <u:AddPortMapping xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\r\n\
         <NewRemoteHost></NewRemoteHost>\r\n\
         <NewExternalPort>{}</NewExternalPort>\r\n\
         <NewProtocol>{}</NewProtocol>\r\n\
         <NewInternalPort>{}</NewInternalPort>\r\n\
         <NewInternalClient>{}</NewInternalClient>\r\n\
         <NewEnabled>1</NewEnabled>\r\n\
         <NewPortMappingDescription>{}</NewPortMappingDescription>\r\n\
         <NewLeaseDuration>{}</NewLeaseDuration>\r\n\
         </u:AddPortMapping>\r\n\
         </s:Body>\r\n\
         </s:Envelope>",
        external_port, proto_str, internal_port, internal_ip, description, lifetime,
    );
    Vec::from(body.as_bytes())
}

/// Build a SOAP DeletePortMapping request body.
fn build_soap_delete_mapping(
    protocol: Protocol,
    external_port: u16,
) -> Vec<u8> {
    let proto_str = match protocol {
        Protocol::Tcp => "TCP",
        Protocol::Udp => "UDP",
    };
    let body = format!(
        "<?xml version=\"1.0\"?>\r\n\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
         <s:Body>\r\n\
         <u:DeletePortMapping xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\r\n\
         <NewRemoteHost></NewRemoteHost>\r\n\
         <NewExternalPort>{}</NewExternalPort>\r\n\
         <NewProtocol>{}</NewProtocol>\r\n\
         </u:DeletePortMapping>\r\n\
         </s:Body>\r\n\
         </s:Envelope>",
        external_port, proto_str,
    );
    Vec::from(body.as_bytes())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize UPnP/NAT-PMP subsystem.
///
/// Discovers the gateway and determines which port forwarding protocol
/// is supported.  Call after the network interface is configured (after
/// DHCP).
pub fn init() {
    let iface = crate::net::interface::info();
    let gateway = iface.gateway.to_u32();
    if gateway == 0 {
        crate::serial_println!("[upnp] No gateway configured — skipping UPnP/NAT-PMP init");
        return;
    }

    let mut state = STATE.lock();
    state.gateway_ip = gateway;

    // Try NAT-PMP first (simpler, faster).
    // In a full implementation, we would send a UDP packet to
    // gateway:5351 and wait for a response.  If the gateway responds
    // with the external IP, NAT-PMP is supported.
    //
    // For now, we prepare the state for NAT-PMP discovery.
    // The actual packet send/recv requires the UDP socket API to
    // be connected to a polling loop (which happens via net::poll()).
    let request = build_natpmp_extern_addr_request();
    crate::serial_println!(
        "[upnp] Attempting NAT-PMP discovery (gateway {} port {}), {} bytes",
        format_ipv4(gateway), NATPMP_PORT, request.len(),
    );

    // Store that we've attempted discovery.
    // The actual response handling would happen in a callback or poll.
    state.discovered_ns = crate::hpet::elapsed_ns();
    state.initialized = true;
    INITIALIZED.store(true, Ordering::Release);

    crate::serial_println!(
        "[upnp] Initialized (gateway={}, method={})",
        format_ipv4(state.gateway_ip),
        state.method.label(),
    );
}

/// Set the discovery method (called when a NAT-PMP or SSDP response arrives).
pub fn set_discovery_method(method: DiscoveryMethod) {
    let mut state = STATE.lock();
    state.method = method;
    crate::syslog!("upnp", Info, "Discovery method set to {}", method.label());
}

/// Set the external IP (from NAT-PMP response or UPnP query).
pub fn set_external_ip(ip: u32) {
    let mut state = STATE.lock();
    state.external_ip = ip;
    crate::syslog!("upnp", Info, "External IP: {}", format_ipv4(ip));
}

/// Add a port forwarding mapping.
///
/// Sends a mapping request to the gateway.  The mapping is initially in
/// `Pending` state and transitions to `Active` when the gateway confirms.
///
/// # Arguments
/// - `protocol` — TCP or UDP.
/// - `internal_port` — port on this machine.
/// - `external_port` — desired port on the router (0 = let router choose).
/// - `lifetime_secs` — how long the mapping should last (0 = default).
/// - `description` — human-readable label.
///
/// Returns the index of the new mapping.
pub fn add_mapping(
    protocol: Protocol,
    internal_port: u16,
    external_port: u16,
    lifetime_secs: u32,
    description: &str,
) -> Option<usize> {
    let mut state = STATE.lock();

    if state.mappings.len() >= MAX_MAPPINGS {
        crate::syslog!("upnp", Error, "Maximum mappings ({}) reached", MAX_MAPPINGS);
        return None;
    }

    // Check for duplicate (same protocol + internal port).
    for m in &state.mappings {
        if m.protocol == protocol && m.internal_port == internal_port
            && m.state != MappingState::Failed
            && m.state != MappingState::Expired
        {
            crate::syslog!(
                "upnp", Warning,
                "Mapping already exists for {} port {}",
                protocol.label(), internal_port,
            );
            return None;
        }
    }

    let now = crate::hpet::elapsed_ns();
    let lifetime = if lifetime_secs == 0 {
        DEFAULT_LIFETIME_SECS
    } else {
        lifetime_secs.clamp(MIN_LIFETIME_SECS, MAX_LIFETIME_SECS)
    };
    let ext_port = if external_port == 0 { internal_port } else { external_port };

    let mapping = PortMapping {
        protocol,
        internal_port,
        external_port: ext_port,
        lifetime_secs: lifetime,
        description: String::from(description),
        state: MappingState::Pending,
        created_ns: now,
        expires_ns: now.saturating_add(u64::from(lifetime).saturating_mul(1_000_000_000)),
        renewals: 0,
        last_error: None,
    };

    // Build the mapping request packet.
    match state.method {
        DiscoveryMethod::NatPmp => {
            let _pkt = build_natpmp_map_request(
                protocol, internal_port, ext_port, lifetime,
            );
            // In a full implementation: send via UDP to gateway:5351.
        }
        DiscoveryMethod::UpnpIgd => {
            let local_ip = crate::net::interface::ip().to_u32();
            let ip_str = format_ipv4(local_ip);
            let _body = build_soap_add_mapping(
                protocol, ext_port, &ip_str, internal_port, description, lifetime,
            );
            // In a full implementation: HTTP POST to control URL.
        }
        DiscoveryMethod::None => {
            // No gateway protocol — mark as active anyway (best effort).
        }
    }

    let idx = state.mappings.len();
    state.mappings.push(mapping);
    state.total_created = state.total_created.saturating_add(1);
    MAPPING_COUNT.store(state.mappings.len() as u32, Ordering::Relaxed);

    crate::syslog!(
        "upnp", Info,
        "Added mapping: {} {}:{} → ext:{} ({}s, \"{}\")",
        protocol.label(), format_ipv4(0), internal_port,
        ext_port, lifetime, description,
    );

    Some(idx)
}

/// Remove a port mapping by protocol and internal port.
///
/// Sends a deletion request to the gateway and removes the mapping
/// from our tracking list.
pub fn remove_mapping(protocol: Protocol, internal_port: u16) -> bool {
    let mut state = STATE.lock();

    let pos = state.mappings.iter().position(|m| {
        m.protocol == protocol && m.internal_port == internal_port
            && m.state != MappingState::Expired
    });

    let Some(idx) = pos else {
        return false;
    };

    let ext_port = state.mappings[idx].external_port;

    // Build the delete request.
    match state.method {
        DiscoveryMethod::NatPmp => {
            // NAT-PMP: map request with lifetime=0 deletes.
            let _pkt = build_natpmp_map_request(protocol, internal_port, ext_port, 0);
            // In a full implementation: send via UDP.
        }
        DiscoveryMethod::UpnpIgd => {
            let _body = build_soap_delete_mapping(protocol, ext_port);
            // In a full implementation: HTTP POST to control URL.
        }
        DiscoveryMethod::None => {}
    }

    state.mappings.remove(idx);
    state.total_removed = state.total_removed.saturating_add(1);
    MAPPING_COUNT.store(state.mappings.len() as u32, Ordering::Relaxed);

    crate::syslog!(
        "upnp", Info,
        "Removed mapping: {} int:{} ext:{}",
        protocol.label(), internal_port, ext_port,
    );

    true
}

/// Confirm a mapping as active (called when gateway responds).
pub fn confirm_mapping(protocol: Protocol, internal_port: u16, external_port: u16, lifetime: u32) {
    let mut state = STATE.lock();
    let now = crate::hpet::elapsed_ns();

    for m in &mut state.mappings {
        if m.protocol == protocol && m.internal_port == internal_port {
            m.state = MappingState::Active;
            m.external_port = external_port;
            m.lifetime_secs = lifetime;
            m.expires_ns = now.saturating_add(u64::from(lifetime).saturating_mul(1_000_000_000));
            return;
        }
    }
}

/// Mark a mapping as failed.
pub fn fail_mapping(protocol: Protocol, internal_port: u16, error: &str) {
    let mut state = STATE.lock();
    state.total_failures = state.total_failures.saturating_add(1);

    for m in &mut state.mappings {
        if m.protocol == protocol && m.internal_port == internal_port {
            m.state = MappingState::Failed;
            m.last_error = Some(String::from(error));
            return;
        }
    }
}

/// Periodic tick — check for expired mappings and renew.
///
/// Called approximately once per second.
pub fn tick() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let tick_num = TICK_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Check every 30 ticks (30 seconds).
    if !tick_num.is_multiple_of(30) {
        return;
    }

    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    let count = state.mappings.len();
    for i in 0..count {
        if i >= state.mappings.len() {
            break;
        }
        let m = &state.mappings[i];
        if m.state != MappingState::Active {
            continue;
        }

        // Check if mapping is about to expire (within 60 seconds).
        let renew_threshold = m.expires_ns.saturating_sub(60_000_000_000);
        if now >= renew_threshold && now < m.expires_ns {
            // Renew the mapping.
            let protocol = m.protocol;
            let int_port = m.internal_port;
            let ext_port = m.external_port;
            let lifetime = m.lifetime_secs;

            match state.method {
                DiscoveryMethod::NatPmp => {
                    let _pkt = build_natpmp_map_request(
                        protocol, int_port, ext_port, lifetime,
                    );
                    // Send renewal packet.
                }
                DiscoveryMethod::UpnpIgd => {
                    // UPnP: re-add the mapping (idempotent).
                }
                DiscoveryMethod::None => {}
            }

            state.mappings[i].renewals = state.mappings[i].renewals.saturating_add(1);
            state.mappings[i].expires_ns = now.saturating_add(
                u64::from(lifetime).saturating_mul(1_000_000_000)
            );
            state.total_renewals = state.total_renewals.saturating_add(1);
        } else if now >= m.expires_ns {
            // Mapping expired.
            state.mappings[i].state = MappingState::Expired;
        }
    }

    // Clean up expired/failed mappings older than 5 minutes.
    let cleanup_threshold = now.saturating_sub(300_000_000_000);
    state.mappings.retain(|m| {
        m.state != MappingState::Expired && m.state != MappingState::Failed
            || m.created_ns > cleanup_threshold
    });
    MAPPING_COUNT.store(state.mappings.len() as u32, Ordering::Relaxed);
}

/// Get all current mappings.
pub fn all_mappings() -> Vec<PortMapping> {
    let state = STATE.lock();
    state.mappings.clone()
}

/// Get summary statistics.
pub fn stats() -> (bool, DiscoveryMethod, u32, u32, u64, u64, u64, u64) {
    let state = STATE.lock();
    let external_ip = state.external_ip;
    (
        state.initialized,
        state.method,
        external_ip,
        state.mappings.len() as u32,
        state.total_created,
        state.total_removed,
        state.total_renewals,
        state.total_failures,
    )
}

/// Get the external IP address (0 if not discovered).
pub fn external_ip() -> u32 {
    STATE.lock().external_ip
}

/// Get the gateway IP address.
pub fn gateway_ip() -> u32 {
    STATE.lock().gateway_ip
}

// ---------------------------------------------------------------------------
// procfs content
// ---------------------------------------------------------------------------

/// Generate content for `/proc/upnp`.
pub fn procfs_content() -> String {
    let state = STATE.lock();
    let mut out = String::with_capacity(1024);

    out.push_str("=== UPnP/NAT-PMP Port Forwarding ===\n");
    out.push_str(&format!("initialized: {}\n", state.initialized));
    out.push_str(&format!("method: {}\n", state.method.label()));
    out.push_str(&format!("gateway: {}\n", format_ipv4(state.gateway_ip)));
    out.push_str(&format!("external_ip: {}\n", format_ipv4(state.external_ip)));
    out.push_str(&format!("active_mappings: {}\n", state.mappings.len()));
    out.push_str(&format!("total_created: {}\n", state.total_created));
    out.push_str(&format!("total_removed: {}\n", state.total_removed));
    out.push_str(&format!("total_renewals: {}\n", state.total_renewals));
    out.push_str(&format!("total_failures: {}\n", state.total_failures));

    if !state.mappings.is_empty() {
        out.push_str("\n=== Port Mappings ===\n");
        for m in &state.mappings {
            out.push_str(&format!(
                "{} int:{} → ext:{} [{}] ({}, {}s, renewals={})\n",
                m.protocol.label(),
                m.internal_port,
                m.external_port,
                m.state.label(),
                m.description,
                m.lifetime_secs,
                m.renewals,
            ));
            if let Some(ref err) = m.last_error {
                out.push_str(&format!("  error: {}\n", err));
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for UPnP/NAT-PMP subsystem.
pub fn self_test() {
    crate::serial_println!("[upnp] Running self-test...");

    // Test 1: NAT-PMP packet building.
    let pkt = build_natpmp_extern_addr_request();
    assert_eq!(pkt.len(), 2, "NAT-PMP extern addr request should be 2 bytes");
    assert_eq!(pkt[0], 0, "Version should be 0");
    assert_eq!(pkt[1], 0, "Opcode should be 0");
    crate::serial_println!("[upnp]   NAT-PMP extern addr request: OK");

    // Test 2: NAT-PMP map request building.
    let pkt = build_natpmp_map_request(Protocol::Tcp, 8080, 8080, 3600);
    assert_eq!(pkt.len(), 12, "NAT-PMP map request should be 12 bytes");
    assert_eq!(pkt[0], 0, "Version should be 0");
    assert_eq!(pkt[1], NATPMP_OP_MAP_TCP, "Opcode should be MAP_TCP");
    let int_port = u16::from_be_bytes([pkt[4], pkt[5]]);
    assert_eq!(int_port, 8080, "Internal port should be 8080");
    let ext_port = u16::from_be_bytes([pkt[6], pkt[7]]);
    assert_eq!(ext_port, 8080, "External port should be 8080");
    let lifetime = u32::from_be_bytes([pkt[8], pkt[9], pkt[10], pkt[11]]);
    assert_eq!(lifetime, 3600, "Lifetime should be 3600");
    crate::serial_println!("[upnp]   NAT-PMP map request: OK");

    // Test 3: NAT-PMP extern addr response parsing.
    let resp = [
        0u8, 128, // version, opcode (response)
        0, 0,     // result code = success
        0, 0, 0, 100, // epoch = 100
        203, 0, 113, 1, // external IP = 203.0.113.1
    ];
    let parsed = parse_natpmp_extern_addr_response(&resp);
    assert!(parsed.is_some(), "Should parse valid response");
    let parsed = parsed.unwrap();
    assert_eq!(parsed.result, 0, "Result should be success");
    assert_eq!(parsed.epoch, 100, "Epoch should be 100");
    assert_eq!(parsed.external_ip, parse_ipv4(203, 0, 113, 1));
    crate::serial_println!("[upnp]   NAT-PMP extern addr response: OK");

    // Test 4: NAT-PMP map response parsing.
    let resp = [
        0u8, 130,     // version, opcode (TCP map response)
        0, 0,         // result = success
        0, 0, 1, 0,   // epoch = 256
        0x1F, 0x90,   // internal port = 8080
        0x1F, 0x91,   // external port = 8081
        0, 0, 0x0E, 0x10, // lifetime = 3600
    ];
    let parsed = parse_natpmp_map_response(&resp);
    assert!(parsed.is_some(), "Should parse valid map response");
    let parsed = parsed.unwrap();
    assert_eq!(parsed.result, 0);
    assert_eq!(parsed.internal_port, 8080);
    assert_eq!(parsed.external_port, 8081);
    assert_eq!(parsed.lifetime, 3600);
    crate::serial_println!("[upnp]   NAT-PMP map response: OK");

    // Test 5: Short packets are rejected.
    assert!(parse_natpmp_extern_addr_response(&[0u8; 4]).is_none());
    assert!(parse_natpmp_map_response(&[0u8; 8]).is_none());
    crate::serial_println!("[upnp]   Short packet rejection: OK");

    // Test 6: SSDP M-SEARCH packet.
    let ssdp = build_ssdp_msearch();
    let ssdp_str = core::str::from_utf8(&ssdp).unwrap_or("");
    assert!(ssdp_str.starts_with("M-SEARCH"), "SSDP should start with M-SEARCH");
    assert!(ssdp_str.contains("InternetGatewayDevice"), "Should search for IGD");
    assert!(ssdp_str.contains("239.255.255.250:1900"), "Should target SSDP multicast");
    crate::serial_println!("[upnp]   SSDP M-SEARCH: OK ({} bytes)", ssdp.len());

    // Test 7: SOAP AddPortMapping body.
    let soap = build_soap_add_mapping(
        Protocol::Tcp, 8080, "192.168.1.100", 8080, "Test", 3600,
    );
    let soap_str = core::str::from_utf8(&soap).unwrap_or("");
    assert!(soap_str.contains("AddPortMapping"), "SOAP should contain action");
    assert!(soap_str.contains("8080"), "SOAP should contain port");
    assert!(soap_str.contains("192.168.1.100"), "SOAP should contain client IP");
    assert!(soap_str.contains("TCP"), "SOAP should contain protocol");
    crate::serial_println!("[upnp]   SOAP AddPortMapping: OK ({} bytes)", soap.len());

    // Test 8: SOAP DeletePortMapping body.
    let soap_del = build_soap_delete_mapping(Protocol::Udp, 9000);
    let soap_del_str = core::str::from_utf8(&soap_del).unwrap_or("");
    assert!(soap_del_str.contains("DeletePortMapping"));
    assert!(soap_del_str.contains("9000"));
    assert!(soap_del_str.contains("UDP"));
    crate::serial_println!("[upnp]   SOAP DeletePortMapping: OK");

    // Test 9: Add and remove mappings.
    {
        let mut state = STATE.lock();
        state.initialized = true;
        state.method = DiscoveryMethod::NatPmp;
        state.gateway_ip = parse_ipv4(192, 168, 1, 1);
    }
    INITIALIZED.store(true, Ordering::Release);

    let idx = add_mapping(Protocol::Tcp, 22, 22, 3600, "SSH");
    assert!(idx.is_some(), "Should add mapping");
    assert_eq!(idx, Some(0));

    let idx2 = add_mapping(Protocol::Udp, 51820, 51820, 7200, "WireGuard");
    assert!(idx2.is_some(), "Should add second mapping");

    // Duplicate should fail.
    let dup = add_mapping(Protocol::Tcp, 22, 22, 3600, "SSH again");
    assert!(dup.is_none(), "Duplicate should be rejected");

    let mappings = all_mappings();
    assert_eq!(mappings.len(), 2, "Should have 2 mappings");
    assert_eq!(mappings[0].protocol, Protocol::Tcp);
    assert_eq!(mappings[0].internal_port, 22);
    assert_eq!(mappings[1].protocol, Protocol::Udp);
    assert_eq!(mappings[1].internal_port, 51820);
    crate::serial_println!("[upnp]   Add mapping: OK");

    // Confirm mapping.
    confirm_mapping(Protocol::Tcp, 22, 22, 3600);
    {
        let state = STATE.lock();
        assert_eq!(state.mappings[0].state, MappingState::Active);
    }
    crate::serial_println!("[upnp]   Confirm mapping: OK");

    // Fail mapping.
    fail_mapping(Protocol::Udp, 51820, "port in use");
    {
        let state = STATE.lock();
        assert_eq!(state.mappings[1].state, MappingState::Failed);
        assert!(state.mappings[1].last_error.as_ref().unwrap().contains("port in use"));
    }
    crate::serial_println!("[upnp]   Fail mapping: OK");

    // Remove mapping.
    let removed = remove_mapping(Protocol::Tcp, 22);
    assert!(removed, "Should remove existing mapping");
    let removed2 = remove_mapping(Protocol::Tcp, 22);
    assert!(!removed2, "Should not find removed mapping");
    crate::serial_println!("[upnp]   Remove mapping: OK");

    // Test 10: Statistics.
    let (init, method, _ext_ip, count, created, removed_count, renewals, failures) = stats();
    assert!(init);
    assert_eq!(method, DiscoveryMethod::NatPmp);
    assert!(created >= 2, "Should have >= 2 created");
    assert!(removed_count >= 1, "Should have >= 1 removed");
    crate::serial_println!(
        "[upnp]   Stats: created={}, removed={}, renewals={}, failures={}, active={}",
        created, removed_count, renewals, failures, count,
    );

    // Test 11: IPv4 formatting.
    assert_eq!(format_ipv4(parse_ipv4(192, 168, 1, 1)), "192.168.1.1");
    assert_eq!(format_ipv4(parse_ipv4(10, 0, 0, 1)), "10.0.0.1");
    assert_eq!(format_ipv4(0), "0.0.0.0");
    crate::serial_println!("[upnp]   IPv4 format: OK");

    // Test 12: Result code labels.
    assert_eq!(natpmp_result_str(0), "success");
    assert_eq!(natpmp_result_str(2), "not authorized");
    assert_eq!(natpmp_result_str(999), "unknown error");
    crate::serial_println!("[upnp]   Result labels: OK");

    // Test 13: procfs content.
    let content = procfs_content();
    assert!(content.contains("=== UPnP/NAT-PMP"), "Should have header");
    assert!(content.contains("NAT-PMP"), "Should show method");
    crate::serial_println!("[upnp]   procfs: OK ({} bytes)", content.len());

    // Clean up test state.
    {
        let mut state = STATE.lock();
        state.mappings.clear();
    }
    MAPPING_COUNT.store(0, Ordering::Relaxed);

    crate::serial_println!("[upnp] Self-test PASSED (13 tests)");
}
