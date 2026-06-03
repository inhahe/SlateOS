#![deny(clippy::all)]
#![allow(dead_code)]
//! OurOS mDNS/DNS-SD Service Discovery Utility
//!
//! Multi-personality binary for mDNS (multicast DNS) and DNS-SD (DNS-based
//! Service Discovery) operations. The personality is selected via the
//! basename of argv[0]:
//!
//! - `avahi-daemon`       mDNS/DNS-SD daemon (default)
//! - `avahi-browse`       browse for services on the network
//! - `avahi-resolve`      resolve hostnames/addresses via mDNS
//! - `avahi-publish`      publish services/hostnames/addresses
//! - `avahi-autoipd`      IPv4 link-local address configuration (169.254.x.x)
//! - `avahi-set-host-name` set the mDNS hostname
//!
//! All data is simulated for development and testing purposes. No actual
//! network I/O is performed; the daemon maintains in-memory service
//! registries and responds with synthetic data.

use std::env;
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// mDNS multicast address for IPv4 (RFC 6762).
const MDNS_MULTICAST_V4: &str = "224.0.0.251";

/// mDNS multicast address for IPv6 (RFC 6762).
const MDNS_MULTICAST_V6: &str = "ff02::fb";

/// mDNS port (RFC 6762).
const MDNS_PORT: u16 = 5353;

/// DNS-SD meta-query service type (RFC 6763).
const DNS_SD_META_QUERY: &str = "_services._dns-sd._udp.local";

/// Default mDNS domain.
const MDNS_DOMAIN: &str = "local";

/// Default hostname for the simulated daemon.
const DEFAULT_HOSTNAME: &str = "ouros-host";

/// Default TTL for mDNS records (seconds).
const DEFAULT_TTL: u32 = 4500;

/// Short TTL for goodbye packets.
const _GOODBYE_TTL: u32 = 0;

/// Maximum mDNS record name length.
const MAX_NAME_LEN: usize = 253;

/// Maximum TXT record data length.
const MAX_TXT_LEN: usize = 8900;

/// Link-local address range start (169.254.1.0).
const LINK_LOCAL_START: u32 = 0xA9FE_0100;

/// Link-local address range end (169.254.254.255).
const LINK_LOCAL_END: u32 = 0xA9FE_FEFF;

/// Number of ARP probes for link-local address selection (RFC 3927).
const ARP_PROBE_COUNT: u32 = 3;

/// ARP probe wait time in milliseconds.
const ARP_PROBE_WAIT_MS: u32 = 1000;

/// ARP announce count (RFC 3927).
const ARP_ANNOUNCE_COUNT: u32 = 2;

/// Maximum conflict retries for link-local address.
const MAX_CONFLICTS: u32 = 10;

/// mDNS cache flush bit in class field.
const CACHE_FLUSH_BIT: u16 = 0x8000;

/// DNS class IN.
const DNS_CLASS_IN: u16 = 1;

/// DNS record type A.
const DNS_TYPE_A: u16 = 1;

/// DNS record type AAAA.
const DNS_TYPE_AAAA: u16 = 28;

/// DNS record type PTR.
const DNS_TYPE_PTR: u16 = 12;

/// DNS record type SRV.
const DNS_TYPE_SRV: u16 = 33;

/// DNS record type TXT.
const DNS_TYPE_TXT: u16 = 16;

/// Maximum number of services in a registry.
const MAX_SERVICES: usize = 1024;

/// Maximum number of cached records.
const MAX_CACHE_ENTRIES: usize = 4096;

/// Daemon state file path.
const _DAEMON_STATE_FILE: &str = "/var/run/avahi-daemon/state";

/// Daemon PID file path.
const _DAEMON_PID_FILE: &str = "/var/run/avahi-daemon/pid";

/// Daemon configuration file path.
const DAEMON_CONF_FILE: &str = "/etc/avahi/avahi-daemon.conf";

/// Services directory.
const _SERVICES_DIR: &str = "/etc/avahi/services";

/// Default interface for mDNS operations.
const _DEFAULT_IFACE: &str = "eth0";

/// Protocol constants.
const PROTO_INET: i32 = 0;
const PROTO_INET6: i32 = 1;
const PROTO_UNSPEC: i32 = -1;

/// Interface constants.
const IF_UNSPEC: i32 = -1;

// ============================================================================
// Error types
// ============================================================================

/// Errors that can occur during mDNS/DNS-SD operations.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AvahiError {
    /// Invalid service name.
    InvalidServiceName(String),
    /// Invalid service type.
    InvalidServiceType(String),
    /// Invalid hostname.
    InvalidHostname(String),
    /// Invalid IP address.
    InvalidAddress(String),
    /// Service not found.
    ServiceNotFound(String),
    /// Record not found.
    RecordNotFound(String),
    /// Registry full.
    RegistryFull,
    /// Cache full.
    CacheFull,
    /// Name collision detected.
    NameCollision(String),
    /// Configuration error.
    ConfigError(String),
    /// I/O error.
    IoError(String),
    /// Protocol error.
    ProtocolError(String),
    /// Timeout.
    Timeout,
    /// Address conflict during link-local configuration.
    AddressConflict(String),
    /// Maximum conflicts reached.
    MaxConflictsReached,
    /// Invalid argument.
    InvalidArgument(String),
    /// Daemon not running.
    DaemonNotRunning,
    /// Permission denied.
    PermissionDenied(String),
}

impl std::fmt::Display for AvahiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidServiceName(s) => write!(f, "Invalid service name: {s}"),
            Self::InvalidServiceType(s) => write!(f, "Invalid service type: {s}"),
            Self::InvalidHostname(s) => write!(f, "Invalid hostname: {s}"),
            Self::InvalidAddress(s) => write!(f, "Invalid address: {s}"),
            Self::ServiceNotFound(s) => write!(f, "Service not found: {s}"),
            Self::RecordNotFound(s) => write!(f, "Record not found: {s}"),
            Self::RegistryFull => write!(f, "Service registry is full"),
            Self::CacheFull => write!(f, "Record cache is full"),
            Self::NameCollision(s) => write!(f, "Name collision: {s}"),
            Self::ConfigError(s) => write!(f, "Configuration error: {s}"),
            Self::IoError(s) => write!(f, "I/O error: {s}"),
            Self::ProtocolError(s) => write!(f, "Protocol error: {s}"),
            Self::Timeout => write!(f, "Operation timed out"),
            Self::AddressConflict(s) => write!(f, "Address conflict: {s}"),
            Self::MaxConflictsReached => write!(f, "Maximum address conflicts reached"),
            Self::InvalidArgument(s) => write!(f, "Invalid argument: {s}"),
            Self::DaemonNotRunning => write!(f, "Avahi daemon is not running"),
            Self::PermissionDenied(s) => write!(f, "Permission denied: {s}"),
        }
    }
}

// ============================================================================
// IPv4 / IPv6 address types
// ============================================================================

/// A simple IPv4 address representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Ipv4Addr {
    _octets: [u8; 4],
}

impl Ipv4Addr {
    fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self {
            _octets: [a, b, c, d],
        }
    }

    fn from_u32(val: u32) -> Self {
        Self {
            _octets: [
                ((val >> 24) & 0xFF) as u8,
                ((val >> 16) & 0xFF) as u8,
                ((val >> 8) & 0xFF) as u8,
                (val & 0xFF) as u8,
            ],
        }
    }

    fn to_u32(self) -> u32 {
        (u32::from(self._octets[0]) << 24)
            | (u32::from(self._octets[1]) << 16)
            | (u32::from(self._octets[2]) << 8)
            | u32::from(self._octets[3])
    }

    fn is_link_local(self) -> bool {
        self._octets[0] == 169 && self._octets[1] == 254
    }

    fn octets(self) -> [u8; 4] {
        self._octets
    }
}

impl std::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self._octets[0], self._octets[1], self._octets[2], self._octets[3]
        )
    }
}

/// A simple IPv6 address representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Ipv6Addr {
    _segments: [u16; 8],
}

impl Ipv6Addr {
    // Mirrors std::net::Ipv6Addr::new's eight-segment constructor; the segment
    // count is inherent to IPv6, so the argument count is intentional.
    #[allow(clippy::too_many_arguments)]
    fn new(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16) -> Self {
        Self {
            _segments: [a, b, c, d, e, f, g, h],
        }
    }

    fn is_link_local(self) -> bool {
        self._segments[0] == 0xFE80
            && self._segments[1] == 0
            && self._segments[2] == 0
            && self._segments[3] == 0
    }

    fn segments(self) -> [u16; 8] {
        self._segments
    }
}

impl std::fmt::Display for Ipv6Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
            self._segments[0],
            self._segments[1],
            self._segments[2],
            self._segments[3],
            self._segments[4],
            self._segments[5],
            self._segments[6],
            self._segments[7],
        )
    }
}

/// A generic IP address (either v4 or v6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IpAddr {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

impl std::fmt::Display for IpAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::V4(addr) => write!(f, "{addr}"),
            Self::V6(addr) => write!(f, "{addr}"),
        }
    }
}

// ============================================================================
// DNS record types
// ============================================================================

/// DNS record data variants.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DnsRecordData {
    /// A record: IPv4 address.
    A(Ipv4Addr),
    /// AAAA record: IPv6 address.
    Aaaa(Ipv6Addr),
    /// PTR record: domain name pointer.
    Ptr(String),
    /// SRV record: service location.
    Srv {
        _priority: u16,
        _weight: u16,
        _port: u16,
        _target: String,
    },
    /// TXT record: key-value text pairs.
    Txt(Vec<String>),
}

impl std::fmt::Display for DnsRecordData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::A(addr) => write!(f, "A {addr}"),
            Self::Aaaa(addr) => write!(f, "AAAA {addr}"),
            Self::Ptr(name) => write!(f, "PTR {name}"),
            Self::Srv {
                _priority,
                _weight,
                _port,
                _target,
            } => write!(f, "SRV {_priority} {_weight} {_port} {_target}"),
            Self::Txt(entries) => {
                write!(f, "TXT")?;
                for entry in entries {
                    write!(f, " \"{entry}\"")?;
                }
                Ok(())
            }
        }
    }
}

/// A complete DNS resource record.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DnsRecord {
    /// Record name (e.g., "_http._tcp.local").
    _name: String,
    /// Record class (typically IN = 1).
    _class: u16,
    /// Time to live in seconds.
    _ttl: u32,
    /// Whether the cache-flush bit is set.
    _cache_flush: bool,
    /// Record data.
    _data: DnsRecordData,
}

impl DnsRecord {
    fn new(name: &str, class: u16, ttl: u32, data: DnsRecordData) -> Self {
        Self {
            _name: name.to_string(),
            _class: class,
            _ttl: ttl,
            _cache_flush: false,
            _data: data,
        }
    }

    fn with_cache_flush(mut self, flush: bool) -> Self {
        self._cache_flush = flush;
        self
    }

    fn record_type_code(&self) -> u16 {
        match &self._data {
            DnsRecordData::A(_) => DNS_TYPE_A,
            DnsRecordData::Aaaa(_) => DNS_TYPE_AAAA,
            DnsRecordData::Ptr(_) => DNS_TYPE_PTR,
            DnsRecordData::Srv { .. } => DNS_TYPE_SRV,
            DnsRecordData::Txt(_) => DNS_TYPE_TXT,
        }
    }

    fn record_type_str(&self) -> &'static str {
        match &self._data {
            DnsRecordData::A(_) => "A",
            DnsRecordData::Aaaa(_) => "AAAA",
            DnsRecordData::Ptr(_) => "PTR",
            DnsRecordData::Srv { .. } => "SRV",
            DnsRecordData::Txt(_) => "TXT",
        }
    }

    fn wire_class(&self) -> u16 {
        if self._cache_flush {
            self._class | CACHE_FLUSH_BIT
        } else {
            self._class
        }
    }
}

impl std::fmt::Display for DnsRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\t{}\t{}\t{}",
            self._name,
            self._ttl,
            self.record_type_str(),
            self._data
        )
    }
}

// ============================================================================
// Service types
// ============================================================================

/// Protocol over which a service is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TransportProtocol {
    Tcp,
    Udp,
}

impl std::fmt::Display for TransportProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp => write!(f, "_tcp"),
            Self::Udp => write!(f, "_udp"),
        }
    }
}

/// A DNS-SD service type (e.g., `_http._tcp`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceType {
    /// Application protocol name (e.g., "_http", "_ssh").
    _protocol: String,
    /// Transport protocol (TCP or UDP).
    _transport: TransportProtocol,
    /// Optional subtype (e.g., "_printer" for `_ipp._tcp,_printer`).
    _subtype: Option<String>,
}

impl ServiceType {
    fn new(protocol: &str, transport: TransportProtocol) -> Self {
        Self {
            _protocol: protocol.to_string(),
            _transport: transport,
            _subtype: None,
        }
    }

    fn with_subtype(mut self, subtype: &str) -> Self {
        self._subtype = Some(subtype.to_string());
        self
    }

    /// Full service type string for DNS-SD queries.
    fn type_string(&self) -> String {
        let base = format!("{}.{}", self._protocol, self._transport);
        if let Some(ref sub) = self._subtype {
            format!("{sub}._sub.{base}")
        } else {
            base
        }
    }

    /// Full service type with domain.
    fn fqdn(&self, domain: &str) -> String {
        format!("{}.{domain}", self.type_string())
    }

    /// Parse a service type string like "_http._tcp" or "_printer._sub._ipp._tcp".
    fn parse(s: &str) -> Result<Self, AvahiError> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() == 2 {
            let transport = match parts[1] {
                "_tcp" => TransportProtocol::Tcp,
                "_udp" => TransportProtocol::Udp,
                other => {
                    return Err(AvahiError::InvalidServiceType(format!(
                        "Unknown transport: {other}"
                    )));
                }
            };
            if !parts[0].starts_with('_') {
                return Err(AvahiError::InvalidServiceType(format!(
                    "Protocol must start with underscore: {}",
                    parts[0]
                )));
            }
            Ok(Self::new(parts[0], transport))
        } else if parts.len() == 4 && parts[1] == "_sub" {
            let transport = match parts[3] {
                "_tcp" => TransportProtocol::Tcp,
                "_udp" => TransportProtocol::Udp,
                other => {
                    return Err(AvahiError::InvalidServiceType(format!(
                        "Unknown transport: {other}"
                    )));
                }
            };
            Ok(Self::new(parts[2], transport).with_subtype(parts[0]))
        } else {
            Err(AvahiError::InvalidServiceType(s.to_string()))
        }
    }
}

impl std::fmt::Display for ServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.type_string())
    }
}

// ============================================================================
// Service entry
// ============================================================================

/// Network interface index and protocol family for a discovered service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InterfaceProto {
    _interface: i32,
    _protocol: i32,
}

impl InterfaceProto {
    fn new(interface: i32, protocol: i32) -> Self {
        Self {
            _interface: interface,
            _protocol: protocol,
        }
    }

    fn unspec() -> Self {
        Self::new(IF_UNSPEC, PROTO_UNSPEC)
    }

    fn description(&self) -> String {
        let iface = if self._interface == IF_UNSPEC {
            "any".to_string()
        } else {
            format!("if{}", self._interface)
        };
        let proto = match self._protocol {
            PROTO_INET => "IPv4",
            PROTO_INET6 => "IPv6",
            _ => "any",
        };
        format!("{iface}/{proto}")
    }
}

/// A registered service entry in the mDNS/DNS-SD registry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceEntry {
    /// Human-readable instance name (e.g., "My Web Server").
    _name: String,
    /// Service type.
    _service_type: ServiceType,
    /// Domain (usually "local").
    _domain: String,
    /// Hostname of the machine providing the service.
    _hostname: String,
    /// Port number.
    _port: u16,
    /// TXT record key-value pairs.
    _txt: Vec<String>,
    /// Interface/protocol on which this service is available.
    _iface_proto: InterfaceProto,
    /// IPv4 address (if resolved).
    _addr_v4: Option<Ipv4Addr>,
    /// IPv6 address (if resolved).
    _addr_v6: Option<Ipv6Addr>,
    /// Whether this entry is currently active.
    _active: bool,
}

impl ServiceEntry {
    fn new(name: &str, service_type: ServiceType, hostname: &str, port: u16) -> Self {
        Self {
            _name: name.to_string(),
            _service_type: service_type,
            _domain: MDNS_DOMAIN.to_string(),
            _hostname: hostname.to_string(),
            _port: port,
            _txt: Vec::new(),
            _iface_proto: InterfaceProto::unspec(),
            _addr_v4: None,
            _addr_v6: None,
            _active: true,
        }
    }

    fn with_txt(mut self, txt: Vec<String>) -> Self {
        self._txt = txt;
        self
    }

    fn with_domain(mut self, domain: &str) -> Self {
        self._domain = domain.to_string();
        self
    }

    fn with_addr_v4(mut self, addr: Ipv4Addr) -> Self {
        self._addr_v4 = Some(addr);
        self
    }

    fn with_addr_v6(mut self, addr: Ipv6Addr) -> Self {
        self._addr_v6 = Some(addr);
        self
    }

    fn with_iface_proto(mut self, ip: InterfaceProto) -> Self {
        self._iface_proto = ip;
        self
    }

    /// Full service instance name (e.g., "My Web Server._http._tcp.local").
    fn instance_name(&self) -> String {
        format!(
            "{}.{}.{}",
            self._name,
            self._service_type.type_string(),
            self._domain
        )
    }

    /// Generate DNS records for this service entry.
    fn to_dns_records(&self) -> Vec<DnsRecord> {
        let mut records = Vec::new();
        let stype_fqdn = self._service_type.fqdn(&self._domain);
        let instance = self.instance_name();

        // PTR record: service type -> instance name
        records.push(DnsRecord::new(
            &stype_fqdn,
            DNS_CLASS_IN,
            DEFAULT_TTL,
            DnsRecordData::Ptr(instance.clone()),
        ));

        // SRV record: instance name -> host:port
        let host_fqdn = format!("{}.{}", self._hostname, self._domain);
        records.push(
            DnsRecord::new(
                &instance,
                DNS_CLASS_IN,
                DEFAULT_TTL / 3,
                DnsRecordData::Srv {
                    _priority: 0,
                    _weight: 0,
                    _port: self._port,
                    _target: host_fqdn.clone(),
                },
            )
            .with_cache_flush(true),
        );

        // TXT record
        records.push(
            DnsRecord::new(
                &instance,
                DNS_CLASS_IN,
                DEFAULT_TTL,
                DnsRecordData::Txt(self._txt.clone()),
            )
            .with_cache_flush(true),
        );

        // A record if IPv4 address is known
        if let Some(v4) = self._addr_v4 {
            records.push(
                DnsRecord::new(
                    &host_fqdn,
                    DNS_CLASS_IN,
                    DEFAULT_TTL / 3,
                    DnsRecordData::A(v4),
                )
                .with_cache_flush(true),
            );
        }

        // AAAA record if IPv6 address is known
        if let Some(v6) = self._addr_v6 {
            records.push(
                DnsRecord::new(
                    &host_fqdn,
                    DNS_CLASS_IN,
                    DEFAULT_TTL / 3,
                    DnsRecordData::Aaaa(v6),
                )
                .with_cache_flush(true),
            );
        }

        records
    }
}

impl std::fmt::Display for ServiceEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}) on {} port {}",
            self._name, self._service_type, self._hostname, self._port
        )?;
        if let Some(v4) = self._addr_v4 {
            write!(f, " [{v4}]")?;
        }
        if let Some(v6) = self._addr_v6 {
            write!(f, " [{v6}]")?;
        }
        Ok(())
    }
}

// ============================================================================
// Browse result
// ============================================================================

/// Event type for browse results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowseEvent {
    /// A new service was discovered.
    New,
    /// A previously discovered service was removed.
    Remove,
    /// Browsing is complete (all cached entries delivered).
    AllForNow,
    /// Cache is exhausted, no more entries.
    CacheExhausted,
}

impl std::fmt::Display for BrowseEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::New => write!(f, "NEW"),
            Self::Remove => write!(f, "REMOVE"),
            Self::AllForNow => write!(f, "ALL_FOR_NOW"),
            Self::CacheExhausted => write!(f, "CACHE_EXHAUSTED"),
        }
    }
}

/// Result of a service browse operation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowseResult {
    _event: BrowseEvent,
    _interface: i32,
    _protocol: i32,
    _name: String,
    _service_type: String,
    _domain: String,
}

impl BrowseResult {
    fn new(
        event: BrowseEvent,
        interface: i32,
        protocol: i32,
        name: &str,
        service_type: &str,
        domain: &str,
    ) -> Self {
        Self {
            _event: event,
            _interface: interface,
            _protocol: protocol,
            _name: name.to_string(),
            _service_type: service_type.to_string(),
            _domain: domain.to_string(),
        }
    }

    fn protocol_str(&self) -> &'static str {
        match self._protocol {
            PROTO_INET => "IPv4",
            PROTO_INET6 => "IPv6",
            _ => "n/a",
        }
    }
}

impl std::fmt::Display for BrowseResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:>3} {} {:>4} {:<40} {:<20} {}",
            if self._interface == IF_UNSPEC {
                "*".to_string()
            } else {
                self._interface.to_string()
            },
            self._event,
            self.protocol_str(),
            self._name,
            self._service_type,
            self._domain,
        )
    }
}

// ============================================================================
// Resolve result
// ============================================================================

/// Result of a service resolve or host/address resolve operation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolveResult {
    _interface: i32,
    _protocol: i32,
    _name: String,
    _service_type: Option<String>,
    _domain: String,
    _hostname: String,
    _port: Option<u16>,
    _address: Option<String>,
    _txt: Vec<String>,
}

impl ResolveResult {
    fn from_service(entry: &ServiceEntry) -> Self {
        let addr_str = entry
            ._addr_v4
            .map(|a| a.to_string())
            .or_else(|| entry._addr_v6.map(|a| a.to_string()));
        Self {
            _interface: entry._iface_proto._interface,
            _protocol: entry._iface_proto._protocol,
            _name: entry._name.clone(),
            _service_type: Some(entry._service_type.type_string()),
            _domain: entry._domain.clone(),
            _hostname: entry._hostname.clone(),
            _port: Some(entry._port),
            _address: addr_str,
            _txt: entry._txt.clone(),
        }
    }

    fn from_hostname(hostname: &str, addr: IpAddr, protocol: i32) -> Self {
        Self {
            _interface: IF_UNSPEC,
            _protocol: protocol,
            _name: hostname.to_string(),
            _service_type: None,
            _domain: MDNS_DOMAIN.to_string(),
            _hostname: hostname.to_string(),
            _port: None,
            _address: Some(addr.to_string()),
            _txt: Vec::new(),
        }
    }
}

impl std::fmt::Display for ResolveResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref stype) = self._service_type {
            writeln!(f, "  name = {}", self._name)?;
            writeln!(f, "  type = {stype}")?;
            writeln!(f, "  domain = {}", self._domain)?;
            writeln!(f, "  hostname = {}", self._hostname)?;
            if let Some(port) = self._port {
                writeln!(f, "  port = {port}")?;
            }
            if let Some(ref addr) = self._address {
                writeln!(f, "  address = {addr}")?;
            }
            if !self._txt.is_empty() {
                write!(f, "  txt = [")?;
                for (i, entry) in self._txt.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{entry}\"")?;
                }
                writeln!(f, "]")?;
            }
        } else {
            write!(f, "{}", self._hostname)?;
            if let Some(ref addr) = self._address {
                write!(f, " -> {addr}")?;
            }
        }
        Ok(())
    }
}

// ============================================================================
// Record cache
// ============================================================================

/// A cached DNS record with expiry tracking.
#[derive(Debug, Clone)]
struct CacheEntry {
    _record: DnsRecord,
    /// Simulated timestamp (monotonic counter) when this entry was added.
    _added_at: u64,
    /// Whether this entry has been verified recently.
    _verified: bool,
}

impl CacheEntry {
    fn new(record: DnsRecord, timestamp: u64) -> Self {
        Self {
            _record: record,
            _added_at: timestamp,
            _verified: false,
        }
    }

    fn is_expired(&self, now: u64) -> bool {
        now.saturating_sub(self._added_at) > u64::from(self._record._ttl)
    }
}

/// DNS record cache for the mDNS resolver.
struct RecordCache {
    _entries: Vec<CacheEntry>,
    _clock: u64,
}

impl RecordCache {
    fn new() -> Self {
        Self {
            _entries: Vec::new(),
            _clock: 0,
        }
    }

    fn tick(&mut self) {
        self._clock = self._clock.saturating_add(1);
    }

    fn insert(&mut self, record: DnsRecord) -> Result<(), AvahiError> {
        // Remove existing record with same name and type
        let rtype = record.record_type_code();
        let name = record._name.clone();
        self._entries
            .retain(|e| !(e._record._name == name && e._record.record_type_code() == rtype));

        if self._entries.len() >= MAX_CACHE_ENTRIES {
            // Evict expired entries first
            let now = self._clock;
            self._entries.retain(|e| !e.is_expired(now));
            if self._entries.len() >= MAX_CACHE_ENTRIES {
                return Err(AvahiError::CacheFull);
            }
        }

        self._entries.push(CacheEntry::new(record, self._clock));
        Ok(())
    }

    fn lookup(&self, name: &str, rtype: u16) -> Vec<&DnsRecord> {
        self._entries
            .iter()
            .filter(|e| {
                e._record._name == name
                    && e._record.record_type_code() == rtype
                    && !e.is_expired(self._clock)
            })
            .map(|e| &e._record)
            .collect()
    }

    fn lookup_by_name(&self, name: &str) -> Vec<&DnsRecord> {
        self._entries
            .iter()
            .filter(|e| e._record._name == name && !e.is_expired(self._clock))
            .map(|e| &e._record)
            .collect()
    }

    fn flush(&mut self) {
        self._entries.clear();
    }

    fn remove_expired(&mut self) {
        let now = self._clock;
        self._entries.retain(|e| !e.is_expired(now));
    }

    fn len(&self) -> usize {
        self._entries.len()
    }

    fn is_empty(&self) -> bool {
        self._entries.is_empty()
    }

    fn all_records(&self) -> Vec<&DnsRecord> {
        self._entries
            .iter()
            .filter(|e| !e.is_expired(self._clock))
            .map(|e| &e._record)
            .collect()
    }
}

// ============================================================================
// Service registry
// ============================================================================

/// Registry for mDNS/DNS-SD services (both local and discovered).
struct ServiceRegistry {
    _services: Vec<ServiceEntry>,
    _hostname: String,
    _domain: String,
}

impl ServiceRegistry {
    fn new(hostname: &str) -> Self {
        Self {
            _services: Vec::new(),
            _hostname: hostname.to_string(),
            _domain: MDNS_DOMAIN.to_string(),
        }
    }

    fn register(&mut self, service: ServiceEntry) -> Result<(), AvahiError> {
        if self._services.len() >= MAX_SERVICES {
            return Err(AvahiError::RegistryFull);
        }
        // Check for name collision
        let instance = service.instance_name();
        for existing in &self._services {
            if existing.instance_name() == instance && existing._active {
                return Err(AvahiError::NameCollision(instance));
            }
        }
        self._services.push(service);
        Ok(())
    }

    fn unregister(&mut self, name: &str, service_type: &str) -> Result<ServiceEntry, AvahiError> {
        let idx = self
            ._services
            .iter()
            .position(|s| s._name == name && s._service_type.type_string() == service_type);
        match idx {
            Some(i) => Ok(self._services.remove(i)),
            None => Err(AvahiError::ServiceNotFound(format!(
                "{name} ({service_type})"
            ))),
        }
    }

    fn browse(&self, service_type: &str) -> Vec<BrowseResult> {
        let mut results = Vec::new();
        for svc in &self._services {
            if svc._active && svc._service_type.type_string() == service_type {
                results.push(BrowseResult::new(
                    BrowseEvent::New,
                    svc._iface_proto._interface,
                    svc._iface_proto._protocol,
                    &svc._name,
                    &svc._service_type.type_string(),
                    &svc._domain,
                ));
            }
        }
        results.push(BrowseResult::new(
            BrowseEvent::AllForNow,
            IF_UNSPEC,
            PROTO_UNSPEC,
            "",
            service_type,
            &self._domain,
        ));
        results
    }

    fn browse_all(&self) -> Vec<BrowseResult> {
        let mut results = Vec::new();
        for svc in &self._services {
            if svc._active {
                results.push(BrowseResult::new(
                    BrowseEvent::New,
                    svc._iface_proto._interface,
                    svc._iface_proto._protocol,
                    &svc._name,
                    &svc._service_type.type_string(),
                    &svc._domain,
                ));
            }
        }
        results.push(BrowseResult::new(
            BrowseEvent::AllForNow,
            IF_UNSPEC,
            PROTO_UNSPEC,
            "",
            "",
            &self._domain,
        ));
        results
    }

    fn browse_service_types(&self) -> Vec<String> {
        let mut types: Vec<String> = self
            ._services
            .iter()
            .filter(|s| s._active)
            .map(|s| s._service_type.type_string())
            .collect();
        types.sort();
        types.dedup();
        types
    }

    fn resolve(&self, name: &str, service_type: &str) -> Result<ResolveResult, AvahiError> {
        for svc in &self._services {
            if svc._active && svc._name == name && svc._service_type.type_string() == service_type {
                return Ok(ResolveResult::from_service(svc));
            }
        }
        Err(AvahiError::ServiceNotFound(format!(
            "{name} ({service_type})"
        )))
    }

    fn find_by_hostname(&self, hostname: &str) -> Vec<&ServiceEntry> {
        self._services
            .iter()
            .filter(|s| s._active && s._hostname == hostname)
            .collect()
    }

    fn find_by_address(&self, addr: &IpAddr) -> Vec<&ServiceEntry> {
        self._services
            .iter()
            .filter(|s| {
                if !s._active {
                    return false;
                }
                match addr {
                    IpAddr::V4(v4) => s._addr_v4.as_ref() == Some(v4),
                    IpAddr::V6(v6) => s._addr_v6.as_ref() == Some(v6),
                }
            })
            .collect()
    }

    fn count(&self) -> usize {
        self._services.iter().filter(|s| s._active).count()
    }

    fn count_all(&self) -> usize {
        self._services.len()
    }

    fn set_hostname(&mut self, hostname: &str) {
        self._hostname = hostname.to_string();
    }

    fn hostname(&self) -> &str {
        &self._hostname
    }

    fn domain(&self) -> &str {
        &self._domain
    }

    fn all_services(&self) -> Vec<&ServiceEntry> {
        self._services.iter().filter(|s| s._active).collect()
    }

    fn deactivate(&mut self, name: &str, service_type: &str) -> bool {
        for svc in &mut self._services {
            if svc._name == name && svc._service_type.type_string() == service_type {
                svc._active = false;
                return true;
            }
        }
        false
    }
}

// ============================================================================
// Daemon configuration
// ============================================================================

/// Configuration for the avahi daemon.
#[derive(Debug, Clone)]
struct DaemonConfig {
    _hostname: String,
    _domain: String,
    _browse_domains: Vec<String>,
    _use_ipv4: bool,
    _use_ipv6: bool,
    _allow_interfaces: Vec<String>,
    _deny_interfaces: Vec<String>,
    _enable_dbus: bool,
    _publish_hinfo: bool,
    _publish_addresses: bool,
    _publish_workstation: bool,
    _publish_domain: bool,
    _check_response_ttl: bool,
    _use_iff_running: bool,
    _enable_reflector: bool,
    _reflect_ipv: bool,
    _cache_entries_max: usize,
    _ratelimit_interval_usec: u64,
    _ratelimit_burst: u32,
}

impl DaemonConfig {
    fn default_config() -> Self {
        Self {
            _hostname: DEFAULT_HOSTNAME.to_string(),
            _domain: MDNS_DOMAIN.to_string(),
            _browse_domains: Vec::new(),
            _use_ipv4: true,
            _use_ipv6: true,
            _allow_interfaces: Vec::new(),
            _deny_interfaces: Vec::new(),
            _enable_dbus: true,
            _publish_hinfo: true,
            _publish_addresses: true,
            _publish_workstation: true,
            _publish_domain: true,
            _check_response_ttl: false,
            _use_iff_running: false,
            _enable_reflector: false,
            _reflect_ipv: false,
            _cache_entries_max: MAX_CACHE_ENTRIES,
            _ratelimit_interval_usec: 1_000_000,
            _ratelimit_burst: 1000,
        }
    }

    fn parse_config_line(&mut self, line: &str) -> Result<(), AvahiError> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            return Ok(());
        }
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(AvahiError::ConfigError(format!(
                "Invalid config line: {line}"
            )));
        }
        let key = parts[0].trim();
        let value = parts[1].trim();
        match key {
            "host-name" => self._hostname = value.to_string(),
            "domain-name" => self._domain = value.to_string(),
            "browse-domains" => {
                self._browse_domains = value.split(',').map(|s| s.trim().to_string()).collect();
            }
            "use-ipv4" => {
                self._use_ipv4 = value == "yes";
            }
            "use-ipv6" => {
                self._use_ipv6 = value == "yes";
            }
            "allow-interfaces" => {
                self._allow_interfaces = value.split(',').map(|s| s.trim().to_string()).collect();
            }
            "deny-interfaces" => {
                self._deny_interfaces = value.split(',').map(|s| s.trim().to_string()).collect();
            }
            "enable-dbus" => {
                self._enable_dbus = value == "yes";
            }
            "publish-hinfo" => {
                self._publish_hinfo = value == "yes";
            }
            "publish-addresses" => {
                self._publish_addresses = value == "yes";
            }
            "publish-workstation" => {
                self._publish_workstation = value == "yes";
            }
            "publish-domain" => {
                self._publish_domain = value == "yes";
            }
            "check-response-ttl" => {
                self._check_response_ttl = value == "yes";
            }
            "use-iff-running" => {
                self._use_iff_running = value == "yes";
            }
            "enable-reflector" => {
                self._enable_reflector = value == "yes";
            }
            "reflect-ipv" => {
                self._reflect_ipv = value == "yes";
            }
            "cache-entries-max" => {
                self._cache_entries_max = value.parse::<usize>().map_err(|e| {
                    AvahiError::ConfigError(format!("Invalid cache-entries-max: {e}"))
                })?;
            }
            "ratelimit-interval-usec" => {
                self._ratelimit_interval_usec = value.parse::<u64>().map_err(|e| {
                    AvahiError::ConfigError(format!("Invalid ratelimit-interval-usec: {e}"))
                })?;
            }
            "ratelimit-burst" => {
                self._ratelimit_burst = value.parse::<u32>().map_err(|e| {
                    AvahiError::ConfigError(format!("Invalid ratelimit-burst: {e}"))
                })?;
            }
            _ => {
                // Unknown keys are silently ignored for forward compatibility.
            }
        }
        Ok(())
    }

    fn load_from_string(&mut self, content: &str) -> Result<(), AvahiError> {
        for line in content.lines() {
            self.parse_config_line(line)?;
        }
        Ok(())
    }
}

// ============================================================================
// Link-local address configuration (avahi-autoipd)
// ============================================================================

/// State machine for IPv4LL (link-local) address selection (RFC 3927).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoIpState {
    /// Initial state, no address selected.
    Init,
    /// Probing: sending ARP probes to check address availability.
    Probing,
    /// Announcing: broadcasting the selected address.
    Announcing,
    /// Running: address is configured and in use.
    Running,
    /// Conflict: address conflict detected, selecting new address.
    Conflict,
    /// Stopped: daemon has been stopped.
    Stopped,
}

/// IPv4LL address selector and manager.
struct AutoIpd {
    _interface: String,
    _state: AutoIpState,
    _selected_addr: Option<Ipv4Addr>,
    _probe_count: u32,
    _announce_count: u32,
    _conflict_count: u32,
    _seed: u32,
}

impl AutoIpd {
    fn new(interface: &str) -> Self {
        Self {
            _interface: interface.to_string(),
            _state: AutoIpState::Init,
            _selected_addr: None,
            _probe_count: 0,
            _announce_count: 0,
            _conflict_count: 0,
            _seed: 0x12345678,
        }
    }

    fn with_seed(mut self, seed: u32) -> Self {
        self._seed = seed;
        self
    }

    /// Simple PRNG for address selection (deterministic for testing).
    fn next_random(&mut self) -> u32 {
        // xorshift32
        self._seed ^= self._seed << 13;
        self._seed ^= self._seed >> 17;
        self._seed ^= self._seed << 5;
        self._seed
    }

    /// Select a random link-local address in the 169.254.1.0 - 169.254.254.255 range.
    fn select_address(&mut self) -> Ipv4Addr {
        let range = LINK_LOCAL_END - LINK_LOCAL_START;
        let offset = self.next_random() % range;
        let addr_u32 = LINK_LOCAL_START + offset;
        Ipv4Addr::from_u32(addr_u32)
    }

    /// Advance the state machine. Returns a description of what happened.
    fn step(&mut self) -> Result<String, AvahiError> {
        match self._state {
            AutoIpState::Init => {
                let addr = self.select_address();
                self._selected_addr = Some(addr);
                self._state = AutoIpState::Probing;
                self._probe_count = 0;
                Ok(format!(
                    "Selected candidate address {addr}, starting probes on {}",
                    self._interface
                ))
            }
            AutoIpState::Probing => {
                self._probe_count += 1;
                if self._probe_count >= ARP_PROBE_COUNT {
                    self._state = AutoIpState::Announcing;
                    self._announce_count = 0;
                    let addr = self
                        ._selected_addr
                        .expect("address should be set during probing");
                    Ok(format!(
                        "Probing complete ({} probes sent), no conflicts for {addr}",
                        ARP_PROBE_COUNT
                    ))
                } else {
                    let addr = self
                        ._selected_addr
                        .expect("address should be set during probing");
                    Ok(format!(
                        "ARP probe {}/{ARP_PROBE_COUNT} for {addr} (wait {ARP_PROBE_WAIT_MS}ms)",
                        self._probe_count
                    ))
                }
            }
            AutoIpState::Announcing => {
                self._announce_count += 1;
                if self._announce_count >= ARP_ANNOUNCE_COUNT {
                    self._state = AutoIpState::Running;
                    let addr = self
                        ._selected_addr
                        .expect("address should be set during announcing");
                    Ok(format!("Address {addr} configured on {}", self._interface))
                } else {
                    let addr = self
                        ._selected_addr
                        .expect("address should be set during announcing");
                    Ok(format!(
                        "ARP announcement {}/{ARP_ANNOUNCE_COUNT} for {addr}",
                        self._announce_count
                    ))
                }
            }
            AutoIpState::Running => Ok(format!(
                "Link-local address {} active on {}",
                self._selected_addr
                    .expect("address should be set when running"),
                self._interface
            )),
            AutoIpState::Conflict => {
                self._conflict_count += 1;
                // RFC 3927 §2.2.1: rate-limit/give up only once the number of
                // conflicts *exceeds* MAX_CONFLICTS. We therefore tolerate
                // exactly MAX_CONFLICTS retries (each picks a fresh candidate)
                // and treat the next conflict as fatal. Using `>=` here would
                // give up one retry early.
                if self._conflict_count > MAX_CONFLICTS {
                    self._state = AutoIpState::Stopped;
                    return Err(AvahiError::MaxConflictsReached);
                }
                let addr = self.select_address();
                self._selected_addr = Some(addr);
                self._state = AutoIpState::Probing;
                self._probe_count = 0;
                Ok(format!(
                    "Conflict detected (attempt {}), trying new address {addr}",
                    self._conflict_count
                ))
            }
            AutoIpState::Stopped => Err(AvahiError::DaemonNotRunning),
        }
    }

    /// Simulate a conflict detection on the current address.
    fn conflict(&mut self) -> Result<String, AvahiError> {
        if self._state == AutoIpState::Stopped {
            return Err(AvahiError::DaemonNotRunning);
        }
        let old_addr = self._selected_addr;
        self._state = AutoIpState::Conflict;
        Ok(format!(
            "Address conflict on {} for address {}",
            self._interface,
            old_addr
                .map(|a| a.to_string())
                .unwrap_or_else(|| "none".to_string()),
        ))
    }

    fn stop(&mut self) {
        self._state = AutoIpState::Stopped;
        self._selected_addr = None;
    }

    /// Run the full link-local address selection process to completion.
    fn run_to_completion(&mut self) -> Result<Ipv4Addr, AvahiError> {
        loop {
            let msg = self.step()?;
            eprintln!("autoipd: {msg}");
            if self._state == AutoIpState::Running {
                return self._selected_addr.ok_or(AvahiError::IoError(
                    "No address after reaching running state".to_string(),
                ));
            }
        }
    }
}

// ============================================================================
// mDNS packet builder (simulated)
// ============================================================================

/// DNS header flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DnsHeaderFlags {
    _qr: bool,   // Query (false) or Response (true)
    _opcode: u8, // 0 = standard query
    _aa: bool,   // Authoritative answer
    _tc: bool,   // Truncated
    _rd: bool,   // Recursion desired
    _ra: bool,   // Recursion available
    _rcode: u8,  // Response code
}

impl DnsHeaderFlags {
    fn query() -> Self {
        Self {
            _qr: false,
            _opcode: 0,
            _aa: false,
            _tc: false,
            _rd: false,
            _ra: false,
            _rcode: 0,
        }
    }

    fn response() -> Self {
        Self {
            _qr: true,
            _opcode: 0,
            _aa: true,
            _tc: false,
            _rd: false,
            _ra: false,
            _rcode: 0,
        }
    }

    fn to_u16(self) -> u16 {
        let mut flags: u16 = 0;
        if self._qr {
            flags |= 0x8000;
        }
        flags |= (u16::from(self._opcode) & 0x0F) << 11;
        if self._aa {
            flags |= 0x0400;
        }
        if self._tc {
            flags |= 0x0200;
        }
        if self._rd {
            flags |= 0x0100;
        }
        if self._ra {
            flags |= 0x0080;
        }
        flags |= u16::from(self._rcode) & 0x0F;
        flags
    }

    fn from_u16(val: u16) -> Self {
        Self {
            _qr: val & 0x8000 != 0,
            _opcode: ((val >> 11) & 0x0F) as u8,
            _aa: val & 0x0400 != 0,
            _tc: val & 0x0200 != 0,
            _rd: val & 0x0100 != 0,
            _ra: val & 0x0080 != 0,
            _rcode: (val & 0x0F) as u8,
        }
    }
}

/// A simulated mDNS/DNS packet.
#[derive(Debug, Clone)]
struct DnsPacket {
    _id: u16,
    _flags: DnsHeaderFlags,
    _questions: Vec<DnsQuestion>,
    _answers: Vec<DnsRecord>,
    _authority: Vec<DnsRecord>,
    _additional: Vec<DnsRecord>,
}

/// A DNS question entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DnsQuestion {
    _name: String,
    _qtype: u16,
    _qclass: u16,
    _unicast_response: bool,
}

impl DnsQuestion {
    fn new(name: &str, qtype: u16) -> Self {
        Self {
            _name: name.to_string(),
            _qtype: qtype,
            _qclass: DNS_CLASS_IN,
            _unicast_response: false,
        }
    }

    fn with_unicast(mut self) -> Self {
        self._unicast_response = true;
        self
    }

    fn wire_class(&self) -> u16 {
        if self._unicast_response {
            self._qclass | 0x8000
        } else {
            self._qclass
        }
    }
}

impl DnsPacket {
    fn new_query(id: u16) -> Self {
        Self {
            _id: id,
            _flags: DnsHeaderFlags::query(),
            _questions: Vec::new(),
            _answers: Vec::new(),
            _authority: Vec::new(),
            _additional: Vec::new(),
        }
    }

    fn new_response(id: u16) -> Self {
        Self {
            _id: id,
            _flags: DnsHeaderFlags::response(),
            _questions: Vec::new(),
            _answers: Vec::new(),
            _authority: Vec::new(),
            _additional: Vec::new(),
        }
    }

    fn add_question(&mut self, q: DnsQuestion) {
        self._questions.push(q);
    }

    fn add_answer(&mut self, r: DnsRecord) {
        self._answers.push(r);
    }

    fn add_authority(&mut self, r: DnsRecord) {
        self._authority.push(r);
    }

    fn add_additional(&mut self, r: DnsRecord) {
        self._additional.push(r);
    }

    /// Serialize to a simulated wire format (just length calculation for simulation).
    fn wire_size(&self) -> usize {
        let mut size = 12; // DNS header
        for q in &self._questions {
            size += q._name.len() + 2 + 4; // name + type + class
        }
        for sections in [&self._answers, &self._authority, &self._additional] {
            for r in sections {
                size += r._name.len() + 2 + 10; // name + type/class/ttl/rdlen
                size += match &r._data {
                    DnsRecordData::A(_) => 4,
                    DnsRecordData::Aaaa(_) => 16,
                    DnsRecordData::Ptr(s) => s.len() + 2,
                    DnsRecordData::Srv { _target, .. } => 6 + _target.len() + 2,
                    DnsRecordData::Txt(entries) => {
                        entries.iter().map(|e| e.len() + 1).sum::<usize>()
                    }
                };
            }
        }
        size
    }

    fn question_count(&self) -> usize {
        self._questions.len()
    }

    fn answer_count(&self) -> usize {
        self._answers.len()
    }

    fn is_query(&self) -> bool {
        !self._flags._qr
    }

    fn is_response(&self) -> bool {
        self._flags._qr
    }
}

// ============================================================================
// Hostname validation and manipulation
// ============================================================================

/// Validate an mDNS hostname.
fn validate_hostname(name: &str) -> Result<(), AvahiError> {
    if name.is_empty() {
        return Err(AvahiError::InvalidHostname(
            "Hostname cannot be empty".to_string(),
        ));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(AvahiError::InvalidHostname(format!(
            "Hostname too long: {} > {MAX_NAME_LEN}",
            name.len()
        )));
    }
    // Check for valid characters (letters, digits, hyphens).
    for (i, ch) in name.chars().enumerate() {
        if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '.' {
            return Err(AvahiError::InvalidHostname(format!(
                "Invalid character '{ch}' at position {i}"
            )));
        }
    }
    // Labels must not start or end with hyphen.
    for label in name.split('.') {
        if label.is_empty() {
            return Err(AvahiError::InvalidHostname(
                "Empty label in hostname".to_string(),
            ));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(AvahiError::InvalidHostname(format!(
                "Label '{label}' starts or ends with hyphen"
            )));
        }
        if label.len() > 63 {
            return Err(AvahiError::InvalidHostname(format!(
                "Label '{label}' exceeds 63 characters"
            )));
        }
    }
    Ok(())
}

/// Validate a service name.
fn validate_service_name(name: &str) -> Result<(), AvahiError> {
    if name.is_empty() {
        return Err(AvahiError::InvalidServiceName(
            "Service name cannot be empty".to_string(),
        ));
    }
    if name.len() > 63 {
        return Err(AvahiError::InvalidServiceName(format!(
            "Service name too long: {} > 63",
            name.len()
        )));
    }
    Ok(())
}

/// Validate TXT record data.
fn validate_txt_records(txt: &[String]) -> Result<(), AvahiError> {
    let total_len: usize = txt.iter().map(|e| e.len() + 1).sum();
    if total_len > MAX_TXT_LEN {
        return Err(AvahiError::InvalidArgument(format!(
            "TXT data too large: {total_len} > {MAX_TXT_LEN}"
        )));
    }
    for entry in txt {
        if entry.len() > 255 {
            return Err(AvahiError::InvalidArgument(format!(
                "TXT entry too long: {} > 255",
                entry.len()
            )));
        }
    }
    Ok(())
}

/// Parse an IPv4 address from a dotted-quad string.
fn parse_ipv4(s: &str) -> Result<Ipv4Addr, AvahiError> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return Err(AvahiError::InvalidAddress(format!(
            "Expected 4 octets, got {}",
            parts.len()
        )));
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        octets[i] = part
            .parse::<u8>()
            .map_err(|_| AvahiError::InvalidAddress(format!("Invalid octet: {part}")))?;
    }
    Ok(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]))
}

/// Parse an IPv6 address from a colon-separated hex string.
fn parse_ipv6(s: &str) -> Result<Ipv6Addr, AvahiError> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 8 {
        return Err(AvahiError::InvalidAddress(format!(
            "Expected 8 segments, got {} (compressed addresses not supported)",
            parts.len()
        )));
    }
    let mut segments = [0u16; 8];
    for (i, part) in parts.iter().enumerate() {
        segments[i] = u16::from_str_radix(part, 16)
            .map_err(|_| AvahiError::InvalidAddress(format!("Invalid hex segment: {part}")))?;
    }
    Ok(Ipv6Addr::new(
        segments[0],
        segments[1],
        segments[2],
        segments[3],
        segments[4],
        segments[5],
        segments[6],
        segments[7],
    ))
}

/// Parse a generic IP address (auto-detect v4 vs v6).
fn parse_ip_address(s: &str) -> Result<IpAddr, AvahiError> {
    if s.contains(':') {
        parse_ipv6(s).map(IpAddr::V6)
    } else {
        parse_ipv4(s).map(IpAddr::V4)
    }
}

/// Create a reverse DNS name for an IPv4 address (for PTR records).
fn ipv4_to_reverse_name(addr: Ipv4Addr) -> String {
    let o = addr.octets();
    format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
}

/// Create a reverse DNS name for an IPv6 address (for PTR records).
fn ipv6_to_reverse_name(addr: Ipv6Addr) -> String {
    let mut nibbles = Vec::with_capacity(32);
    for seg in addr.segments() {
        nibbles.push((seg >> 12) & 0xF);
        nibbles.push((seg >> 8) & 0xF);
        nibbles.push((seg >> 4) & 0xF);
        nibbles.push(seg & 0xF);
    }
    nibbles.reverse();
    let parts: Vec<String> = nibbles.iter().map(|n| format!("{n:x}")).collect();
    format!("{}.ip6.arpa", parts.join("."))
}

/// Construct the mDNS FQDN for a hostname.
fn hostname_fqdn(hostname: &str, domain: &str) -> String {
    format!("{hostname}.{domain}")
}

// ============================================================================
// Simulated service database (pre-populated for browse/resolve demos)
// ============================================================================

/// Create a pre-populated service registry for demonstration purposes.
fn create_demo_registry() -> ServiceRegistry {
    let mut reg = ServiceRegistry::new(DEFAULT_HOSTNAME);

    // HTTP web server
    let http_type = ServiceType::new("_http", TransportProtocol::Tcp);
    let http_svc = ServiceEntry::new("OurOS Web Server", http_type, "ouros-host", 80)
        .with_txt(vec!["path=/".to_string(), "version=1.0".to_string()])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 100))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(http_svc);

    // SSH server
    let ssh_type = ServiceType::new("_ssh", TransportProtocol::Tcp);
    let ssh_svc = ServiceEntry::new("OurOS SSH", ssh_type, "ouros-host", 22)
        .with_txt(vec!["ssh-version=2".to_string()])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 100))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(ssh_svc);

    // IPP printer
    let ipp_type = ServiceType::new("_ipp", TransportProtocol::Tcp);
    let ipp_svc = ServiceEntry::new("Office Printer", ipp_type, "printer-1", 631)
        .with_txt(vec![
            "txtvers=1".to_string(),
            "pdl=application/postscript".to_string(),
            "product=(OurOS LaserJet)".to_string(),
        ])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 50))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(ipp_svc);

    // SMB file sharing
    let smb_type = ServiceType::new("_smb", TransportProtocol::Tcp);
    let smb_svc = ServiceEntry::new("OurOS File Share", smb_type, "ouros-host", 445)
        .with_txt(vec![
            "share=public".to_string(),
            "workgroup=WORKGROUP".to_string(),
        ])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 100))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(smb_svc);

    // SFTP server
    let sftp_type = ServiceType::new("_sftp-ssh", TransportProtocol::Tcp);
    let sftp_svc = ServiceEntry::new("OurOS SFTP", sftp_type, "ouros-host", 22)
        .with_txt(vec!["path=/".to_string()])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 100))
        .with_addr_v6(Ipv6Addr::new(
            0xFE80, 0, 0, 0, 0x1234, 0x5678, 0x9ABC, 0xDEF0,
        ))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(sftp_svc);

    // AirPlay
    let raop_type = ServiceType::new("_raop", TransportProtocol::Tcp);
    let raop_svc = ServiceEntry::new("Living Room Speaker", raop_type, "speaker-1", 7000)
        .with_txt(vec![
            "am=Speaker".to_string(),
            "vs=1.0".to_string(),
            "vn=65537".to_string(),
        ])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 200))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(raop_svc);

    // DNS-SD browsing/meta
    let dns_sd_type = ServiceType::new("_workstation", TransportProtocol::Tcp);
    let workstation = ServiceEntry::new(
        "ouros-host [00:11:22:33:44:55]",
        dns_sd_type,
        "ouros-host",
        9,
    )
    .with_addr_v4(Ipv4Addr::new(192, 168, 1, 100))
    .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(workstation);

    // Another machine on the network
    let http_type2 = ServiceType::new("_http", TransportProtocol::Tcp);
    let http_svc2 = ServiceEntry::new("NAS Web Interface", http_type2, "nas-1", 8080)
        .with_txt(vec!["path=/admin".to_string(), "version=2.1".to_string()])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 150))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(http_svc2);

    // MQTT broker
    let mqtt_type = ServiceType::new("_mqtt", TransportProtocol::Tcp);
    let mqtt_svc = ServiceEntry::new("Home Automation MQTT", mqtt_type, "ouros-host", 1883)
        .with_txt(vec!["protocol=3.1.1".to_string()])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 100))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(mqtt_svc);

    // VNC server
    let vnc_type = ServiceType::new("_rfb", TransportProtocol::Tcp);
    let vnc_svc = ServiceEntry::new("OurOS Remote Desktop", vnc_type, "ouros-host", 5900)
        .with_txt(vec!["display=0".to_string()])
        .with_addr_v4(Ipv4Addr::new(192, 168, 1, 100))
        .with_addr_v6(Ipv6Addr::new(
            0xFE80, 0, 0, 0, 0x1234, 0x5678, 0x9ABC, 0xDEF0,
        ))
        .with_iface_proto(InterfaceProto::new(2, PROTO_INET));
    let _ = reg.register(vnc_svc);

    reg
}

/// Create a pre-populated record cache for demonstration.
fn create_demo_cache() -> RecordCache {
    let mut cache = RecordCache::new();

    // Host A records
    let _ = cache.insert(DnsRecord::new(
        "ouros-host.local",
        DNS_CLASS_IN,
        DEFAULT_TTL,
        DnsRecordData::A(Ipv4Addr::new(192, 168, 1, 100)),
    ));

    let _ = cache.insert(DnsRecord::new(
        "printer-1.local",
        DNS_CLASS_IN,
        DEFAULT_TTL,
        DnsRecordData::A(Ipv4Addr::new(192, 168, 1, 50)),
    ));

    let _ = cache.insert(DnsRecord::new(
        "nas-1.local",
        DNS_CLASS_IN,
        DEFAULT_TTL,
        DnsRecordData::A(Ipv4Addr::new(192, 168, 1, 150)),
    ));

    let _ = cache.insert(DnsRecord::new(
        "speaker-1.local",
        DNS_CLASS_IN,
        DEFAULT_TTL,
        DnsRecordData::A(Ipv4Addr::new(192, 168, 1, 200)),
    ));

    // AAAA record
    let _ = cache.insert(DnsRecord::new(
        "ouros-host.local",
        DNS_CLASS_IN,
        DEFAULT_TTL,
        DnsRecordData::Aaaa(Ipv6Addr::new(
            0xFE80, 0, 0, 0, 0x1234, 0x5678, 0x9ABC, 0xDEF0,
        )),
    ));

    // PTR records for reverse lookup
    let _ = cache.insert(DnsRecord::new(
        "100.1.168.192.in-addr.arpa",
        DNS_CLASS_IN,
        DEFAULT_TTL,
        DnsRecordData::Ptr("ouros-host.local".to_string()),
    ));

    let _ = cache.insert(DnsRecord::new(
        "50.1.168.192.in-addr.arpa",
        DNS_CLASS_IN,
        DEFAULT_TTL,
        DnsRecordData::Ptr("printer-1.local".to_string()),
    ));

    cache
}

// ============================================================================
// Simulated hostname database for resolve operations
// ============================================================================

/// Look up a hostname in the simulated mDNS database.
fn resolve_hostname(hostname: &str) -> Vec<(IpAddr, i32)> {
    let name = if hostname.ends_with(".local") {
        hostname.to_string()
    } else {
        format!("{hostname}.local")
    };
    let mut results = Vec::new();
    match name.as_str() {
        "ouros-host.local" => {
            results.push((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), PROTO_INET));
            results.push((
                IpAddr::V6(Ipv6Addr::new(
                    0xFE80, 0, 0, 0, 0x1234, 0x5678, 0x9ABC, 0xDEF0,
                )),
                PROTO_INET6,
            ));
        }
        "printer-1.local" => {
            results.push((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), PROTO_INET));
        }
        "nas-1.local" => {
            results.push((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 150)), PROTO_INET));
        }
        "speaker-1.local" => {
            results.push((IpAddr::V4(Ipv4Addr::new(192, 168, 1, 200)), PROTO_INET));
        }
        _ => {}
    }
    results
}

/// Reverse-lookup an IP address in the simulated mDNS database.
fn resolve_address(addr: &str) -> Option<String> {
    match addr {
        "192.168.1.100" => Some("ouros-host.local".to_string()),
        "192.168.1.50" => Some("printer-1.local".to_string()),
        "192.168.1.150" => Some("nas-1.local".to_string()),
        "192.168.1.200" => Some("speaker-1.local".to_string()),
        _ => None,
    }
}

// ============================================================================
// Personality: avahi-daemon
// ============================================================================

fn run_daemon(args: &[String]) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("--help");

    match cmd {
        "--help" | "-h" => {
            println!("Usage: avahi-daemon [OPTION...]");
            println!();
            println!("mDNS/DNS-SD daemon for OurOS.");
            println!();
            println!("Options:");
            println!("  -D, --daemonize       Daemonize after startup");
            println!("  -s, --syslog          Log to syslog instead of stderr");
            println!("  -k, --kill            Kill a running daemon");
            println!("  -r, --reload          Reload configuration");
            println!("  -c, --check           Check if daemon is running");
            println!("  -V, --version         Show version");
            println!("  -f, --file=FILE       Load configuration from FILE");
            println!("  --no-rlimits          Don't enforce resource limits");
            println!("  --no-drop-root        Don't drop root privileges");
            println!("  --debug               Enable debug output");
            println!("  -h, --help            Show this help");
            0
        }
        "--version" | "-V" => {
            println!("avahi-daemon 0.8 (OurOS)");
            println!("Compiled with mDNS/DNS-SD support.");
            println!("Features: IPv4, IPv6, D-Bus, DNS-SD, wide-area DNS-SD");
            0
        }
        "--check" | "-c" => {
            // Simulate: daemon is running
            println!("Daemon is running (PID: 1234)");
            0
        }
        "--kill" | "-k" => {
            println!("Sending SIGTERM to running daemon...");
            println!("Daemon stopped.");
            0
        }
        "--reload" | "-r" => {
            println!("Sending SIGHUP to running daemon...");
            println!("Configuration reloaded.");
            0
        }
        "-D" | "--daemonize" | "--no-rlimits" | "--no-drop-root" | "--debug" => {
            run_daemon_main(args)
        }
        "-f" | "--file" => run_daemon_main(args),
        _ => {
            if cmd.starts_with('-') {
                run_daemon_main(args)
            } else {
                eprintln!("avahi-daemon: Unknown command: {cmd}");
                eprintln!("Try 'avahi-daemon --help' for more information.");
                1
            }
        }
    }
}

fn run_daemon_main(args: &[String]) -> i32 {
    let mut config = DaemonConfig::default_config();
    let mut daemonize = false;
    let mut debug = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-D" | "--daemonize" => daemonize = true,
            "--debug" => debug = true,
            "--no-rlimits" | "--no-drop-root" | "-s" | "--syslog" => {
                // Accepted but no-op in simulation.
            }
            "-f" | "--file" => {
                i += 1;
                if i < args.len() {
                    let conf_path = &args[i];
                    match std::fs::read_to_string(conf_path) {
                        Ok(content) => {
                            if let Err(e) = config.load_from_string(&content) {
                                eprintln!("avahi-daemon: {e}");
                                return 1;
                            }
                        }
                        Err(e) => {
                            eprintln!("avahi-daemon: Failed to read config {conf_path}: {e}");
                            return 1;
                        }
                    }
                } else {
                    eprintln!("avahi-daemon: --file requires an argument");
                    return 1;
                }
            }
            other if other.starts_with("--file=") => {
                let conf_path = &other["--file=".len()..];
                match std::fs::read_to_string(conf_path) {
                    Ok(content) => {
                        if let Err(e) = config.load_from_string(&content) {
                            eprintln!("avahi-daemon: {e}");
                            return 1;
                        }
                    }
                    Err(e) => {
                        eprintln!("avahi-daemon: Failed to read config {conf_path}: {e}");
                        return 1;
                    }
                }
            }
            _ => {
                // Unknown flags ignored in simulation.
            }
        }
        i += 1;
    }

    // Try loading default config file.
    if let Ok(content) = std::fs::read_to_string(DAEMON_CONF_FILE) {
        let _ = config.load_from_string(&content);
    }

    if daemonize {
        println!("avahi-daemon: Daemonizing...");
    }
    if debug {
        eprintln!("avahi-daemon: Debug mode enabled");
    }

    println!("avahi-daemon[1234]: Starting mDNS/DNS-SD daemon");
    println!(
        "avahi-daemon[1234]: Hostname: {}.{}",
        config._hostname, config._domain
    );
    println!(
        "avahi-daemon[1234]: IPv4: {}, IPv6: {}",
        if config._use_ipv4 {
            "enabled"
        } else {
            "disabled"
        },
        if config._use_ipv6 {
            "enabled"
        } else {
            "disabled"
        },
    );
    println!("avahi-daemon[1234]: Listening on {MDNS_MULTICAST_V4}:{MDNS_PORT}");
    if config._use_ipv6 {
        println!("avahi-daemon[1234]: Listening on [{MDNS_MULTICAST_V6}]:{MDNS_PORT}");
    }

    // Populate demo services.
    let registry = create_demo_registry();
    println!(
        "avahi-daemon[1234]: {} services registered",
        registry.count()
    );

    // Show registered service types.
    let types = registry.browse_service_types();
    for stype in &types {
        println!("avahi-daemon[1234]: Registered type: {stype}");
    }

    println!("avahi-daemon[1234]: Daemon startup complete");
    println!("avahi-daemon[1234]: (simulation mode — exiting immediately)");
    0
}

// ============================================================================
// Personality: avahi-browse
// ============================================================================

fn run_browse(args: &[String]) -> i32 {
    let cmd = args
        .first()
        .cloned()
        .unwrap_or_else(|| "--help".to_string());
    let cmd_args: Vec<String> = args.iter().skip(1).cloned().collect();

    match cmd.as_str() {
        "--help" | "-h" => {
            println!("Usage: avahi-browse [OPTIONS] <service-type>");
            println!();
            println!("Browse for mDNS/DNS-SD services on the local network.");
            println!();
            println!("Options:");
            println!("  -a, --all             Browse for all service types");
            println!("  -d, --domain=DOMAIN   Browse in specified domain");
            println!("  -v, --verbose         Verbose output");
            println!("  -t, --terminate       Terminate after dumping cached records");
            println!("  -c, --cache           Print cached entries only, don't browse");
            println!("  -l, --ignore-local    Ignore local services");
            println!("  -r, --resolve         Resolve discovered services");
            println!("  -f, --no-fail         Don't fail if the daemon is unavailable");
            println!("  -p, --parsable        Output in parsable format");
            println!("  -k, --no-db-lookup    Don't lookup service type descriptions");
            println!("  -b, --dump-db         Dump service type database");
            println!("  -V, --version         Show version");
            println!("  -h, --help            Show this help");
            println!();
            println!("Service types:");
            println!("  _http._tcp            Web servers");
            println!("  _ssh._tcp             SSH servers");
            println!("  _ipp._tcp             IPP printers");
            println!("  _smb._tcp             SMB file sharing");
            println!("  _raop._tcp            AirPlay");
            0
        }
        "--version" | "-V" => {
            println!("avahi-browse 0.8 (OurOS)");
            0
        }
        "--dump-db" | "-b" => {
            dump_service_type_db();
            0
        }
        "-a" | "--all" => browse_all_services(&cmd_args),
        _ => {
            // Treat as service type to browse
            let mut resolve = false;
            let mut verbose = false;
            let mut parsable = false;
            let mut terminate = false;
            let mut service_type = cmd.clone();
            let mut domain = MDNS_DOMAIN.to_string();

            // Check if the first arg is a flag
            if service_type.starts_with('-') {
                let mut stype_found = false;
                let all_args: Vec<String> = std::iter::once(cmd.clone())
                    .chain(cmd_args.iter().cloned())
                    .collect();
                let mut j = 0;
                while j < all_args.len() {
                    let a = all_args[j].as_str();
                    match a {
                        "-r" | "--resolve" => resolve = true,
                        "-v" | "--verbose" => verbose = true,
                        "-p" | "--parsable" => parsable = true,
                        "-t" | "--terminate" => terminate = true,
                        "-c" | "--cache" => terminate = true,
                        "-l" | "--ignore-local" | "-f" | "--no-fail" | "-k" | "--no-db-lookup" => {}
                        "-d" | "--domain" => {
                            j += 1;
                            if j < all_args.len() {
                                domain = all_args[j].clone();
                            }
                        }
                        other if other.starts_with("--domain=") => {
                            domain = other["--domain=".len()..].to_string();
                        }
                        other if !other.starts_with('-') && !stype_found => {
                            service_type = other.to_string();
                            stype_found = true;
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if !stype_found {
                    eprintln!("avahi-browse: No service type specified");
                    eprintln!("Try 'avahi-browse --help' for more information.");
                    return 1;
                }
            } else {
                // Parse remaining flags
                for a in &cmd_args {
                    match a.as_str() {
                        "-r" | "--resolve" => resolve = true,
                        "-v" | "--verbose" => verbose = true,
                        "-p" | "--parsable" => parsable = true,
                        "-t" | "--terminate" => terminate = true,
                        "-c" | "--cache" => terminate = true,
                        _ => {}
                    }
                }
            }

            browse_service_type(
                &service_type,
                &domain,
                resolve,
                verbose,
                parsable,
                terminate,
            )
        }
    }
}

fn browse_service_type(
    service_type: &str,
    domain: &str,
    resolve: bool,
    verbose: bool,
    parsable: bool,
    _terminate: bool,
) -> i32 {
    // Validate service type
    if ServiceType::parse(service_type).is_err() {
        eprintln!("avahi-browse: Invalid service type: {service_type}");
        return 1;
    }

    if verbose {
        println!("Browsing for {service_type} in domain {domain}...");
    }

    let registry = create_demo_registry();
    let results = registry.browse(service_type);

    for result in &results {
        if parsable {
            println!(
                "{};{};{};{};{};{}",
                result._event,
                result._interface,
                result.protocol_str(),
                result._name,
                result._service_type,
                result._domain,
            );
        } else {
            println!("{result}");
        }

        if resolve && result._event == BrowseEvent::New {
            match registry.resolve(&result._name, service_type) {
                Ok(resolved) => {
                    print!("{resolved}");
                }
                Err(e) => {
                    eprintln!("  Failed to resolve: {e}");
                }
            }
        }
    }

    0
}

fn browse_all_services(args: &[String]) -> i32 {
    let mut resolve = false;
    let mut verbose = false;
    let mut parsable = false;

    for a in args {
        match a.as_str() {
            "-r" | "--resolve" => resolve = true,
            "-v" | "--verbose" => verbose = true,
            "-p" | "--parsable" => parsable = true,
            _ => {}
        }
    }

    if verbose {
        println!("Browsing for all services...");
    }

    let registry = create_demo_registry();
    let results = registry.browse_all();

    for result in &results {
        if parsable {
            println!(
                "{};{};{};{};{};{}",
                result._event,
                result._interface,
                result.protocol_str(),
                result._name,
                result._service_type,
                result._domain,
            );
        } else {
            println!("{result}");
        }

        if resolve && result._event == BrowseEvent::New && !result._service_type.is_empty() {
            match registry.resolve(&result._name, &result._service_type) {
                Ok(resolved) => {
                    print!("{resolved}");
                }
                Err(e) => {
                    eprintln!("  Failed to resolve: {e}");
                }
            }
        }
    }

    0
}

fn dump_service_type_db() {
    let db = service_type_database();
    for (stype, desc) in &db {
        println!("{stype:30} {desc}");
    }
}

/// Return a database mapping service types to descriptions.
fn service_type_database() -> Vec<(&'static str, &'static str)> {
    vec![
        ("_http._tcp", "Web Server (HTTP)"),
        ("_https._tcp", "Secure Web Server (HTTPS)"),
        ("_ssh._tcp", "SSH Remote Login"),
        ("_sftp-ssh._tcp", "SFTP File Transfer"),
        ("_ftp._tcp", "FTP File Transfer"),
        ("_ipp._tcp", "Internet Printing Protocol"),
        ("_ipps._tcp", "Internet Printing Protocol (Secure)"),
        ("_printer._tcp", "UNIX Printer"),
        ("_pdl-datastream._tcp", "PDL Data Stream (Printer)"),
        ("_smb._tcp", "Microsoft Windows Network (SMB)"),
        ("_afpovertcp._tcp", "Apple Filing Protocol (AFP)"),
        ("_nfs._tcp", "Network File System (NFS)"),
        ("_webdav._tcp", "WebDAV"),
        ("_webdavs._tcp", "Secure WebDAV"),
        ("_raop._tcp", "AirPlay/AirTunes"),
        ("_airplay._tcp", "AirPlay Display"),
        ("_daap._tcp", "Digital Audio Access Protocol (iTunes)"),
        ("_dpap._tcp", "Digital Photo Access Protocol (iPhoto)"),
        ("_rfb._tcp", "VNC Remote Desktop"),
        ("_rdp._tcp", "Remote Desktop Protocol"),
        ("_mqtt._tcp", "Message Queuing Telemetry Transport"),
        ("_xmpp-client._tcp", "XMPP Client"),
        ("_xmpp-server._tcp", "XMPP Server"),
        ("_sip._udp", "Session Initiation Protocol"),
        ("_dns-sd._udp", "DNS-SD Service Discovery"),
        ("_workstation._tcp", "Workstation"),
        ("_device-info._tcp", "Device Information"),
        ("_googlecast._tcp", "Google Cast"),
        ("_spotify-connect._tcp", "Spotify Connect"),
        ("_coap._udp", "Constrained Application Protocol"),
    ]
}

// ============================================================================
// Personality: avahi-resolve
// ============================================================================

fn run_resolve(args: &[String]) -> i32 {
    let cmd = args
        .first()
        .cloned()
        .unwrap_or_else(|| "--help".to_string());
    let cmd_args: Vec<String> = args.iter().skip(1).cloned().collect();

    match cmd.as_str() {
        "--help" | "-h" => {
            println!("Usage: avahi-resolve [OPTIONS] <hostname|address>");
            println!();
            println!("Resolve hostnames or addresses using mDNS.");
            println!();
            println!("Options:");
            println!("  -n, --name=NAME       Resolve hostname to address");
            println!("  -a, --address=ADDR    Resolve address to hostname");
            println!("  -v, --verbose         Verbose output");
            println!("  -4                    Use IPv4 only");
            println!("  -6                    Use IPv6 only");
            println!("  -V, --version         Show version");
            println!("  -h, --help            Show this help");
            0
        }
        "--version" | "-V" => {
            println!("avahi-resolve 0.8 (OurOS)");
            0
        }
        "-n" | "--name" => {
            if cmd_args.is_empty() {
                eprintln!("avahi-resolve: --name requires a hostname argument");
                return 1;
            }
            resolve_name_cmd(&cmd_args[0], PROTO_UNSPEC, false)
        }
        "-a" | "--address" => {
            if cmd_args.is_empty() {
                eprintln!("avahi-resolve: --address requires an address argument");
                return 1;
            }
            resolve_address_cmd(&cmd_args[0], false)
        }
        other if other.starts_with("--name=") => {
            let name = &other["--name=".len()..];
            resolve_name_cmd(name, PROTO_UNSPEC, false)
        }
        other if other.starts_with("--address=") => {
            let addr = &other["--address=".len()..];
            resolve_address_cmd(addr, false)
        }
        "-4" => {
            if cmd_args.is_empty() {
                eprintln!("avahi-resolve: No hostname or address specified");
                return 1;
            }
            resolve_with_flags(&cmd_args, PROTO_INET)
        }
        "-6" => {
            if cmd_args.is_empty() {
                eprintln!("avahi-resolve: No hostname or address specified");
                return 1;
            }
            resolve_with_flags(&cmd_args, PROTO_INET6)
        }
        _ => {
            // Try to detect if it's a hostname or address
            if cmd.contains(':') || cmd.chars().all(|c| c.is_ascii_digit() || c == '.') {
                resolve_address_cmd(&cmd, false)
            } else {
                resolve_name_cmd(&cmd, PROTO_UNSPEC, false)
            }
        }
    }
}

fn resolve_with_flags(args: &[String], protocol: i32) -> i32 {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-n" | "--name" => {
                i += 1;
                if i < args.len() {
                    return resolve_name_cmd(&args[i], protocol, false);
                }
                eprintln!("avahi-resolve: --name requires a hostname argument");
                return 1;
            }
            "-a" | "--address" => {
                i += 1;
                if i < args.len() {
                    return resolve_address_cmd(&args[i], false);
                }
                eprintln!("avahi-resolve: --address requires an address argument");
                return 1;
            }
            other if !other.starts_with('-') => {
                if other.contains(':') || other.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return resolve_address_cmd(other, false);
                }
                return resolve_name_cmd(other, protocol, false);
            }
            _ => {}
        }
        i += 1;
    }
    eprintln!("avahi-resolve: No hostname or address specified");
    1
}

fn resolve_name_cmd(hostname: &str, protocol: i32, _verbose: bool) -> i32 {
    let results = resolve_hostname(hostname);
    if results.is_empty() {
        eprintln!("avahi-resolve: Failed to resolve hostname '{hostname}': Timeout reached");
        return 1;
    }

    for (addr, proto) in &results {
        if protocol != PROTO_UNSPEC && *proto != protocol {
            continue;
        }
        let fqdn = if hostname.ends_with(".local") {
            hostname.to_string()
        } else {
            format!("{hostname}.local")
        };
        println!("{fqdn}\t{addr}");
    }
    0
}

fn resolve_address_cmd(address: &str, _verbose: bool) -> i32 {
    match resolve_address(address) {
        Some(hostname) => {
            println!("{address}\t{hostname}");
            0
        }
        None => {
            eprintln!("avahi-resolve: Failed to resolve address '{address}': Timeout reached");
            1
        }
    }
}

// ============================================================================
// Personality: avahi-publish
// ============================================================================

fn run_publish(args: &[String]) -> i32 {
    let cmd = args
        .first()
        .cloned()
        .unwrap_or_else(|| "--help".to_string());
    let cmd_args: Vec<String> = args.iter().skip(1).cloned().collect();

    match cmd.as_str() {
        "--help" | "-h" => {
            println!("Usage: avahi-publish [OPTIONS] <command> ...");
            println!();
            println!("Publish mDNS/DNS-SD services, hostnames, or addresses.");
            println!();
            println!("Commands:");
            println!("  service <name> <type> <port> [TXT ...]");
            println!("                        Publish a service");
            println!("  address <hostname> <address>");
            println!("                        Publish an address mapping");
            println!();
            println!("Options:");
            println!("  -s, --service         Publish a service (same as 'service' command)");
            println!("  -a, --address         Publish an address (same as 'address' command)");
            println!("  -d, --domain=DOMAIN   Publish in domain (default: local)");
            println!("  -H, --host=HOST       Host for the service");
            println!("  -R, --no-reverse      Don't publish reverse address record");
            println!("  -f, --no-fail         Don't fail if daemon unavailable");
            println!("  -v, --verbose         Verbose output");
            println!("  -V, --version         Show version");
            println!("  -h, --help            Show this help");
            0
        }
        "--version" | "-V" => {
            println!("avahi-publish 0.8 (OurOS)");
            0
        }
        "service" | "-s" | "--service" => publish_service_cmd(&cmd_args),
        "address" | "-a" | "--address" => publish_address_cmd(&cmd_args),
        _ => {
            // Check for flags before service type
            let mut all: Vec<String> = std::iter::once(cmd.clone())
                .chain(cmd_args.iter().cloned())
                .collect();
            // Try to find 'service' or 'address' in args
            let has_service = all.iter().any(|a| a == "service");
            let has_address = all.iter().any(|a| a == "address");
            if has_service {
                let idx = all.iter().position(|a| a == "service").unwrap_or(0);
                let sub_args: Vec<String> = all.drain(idx + 1..).collect();
                publish_service_cmd(&sub_args)
            } else if has_address {
                let idx = all.iter().position(|a| a == "address").unwrap_or(0);
                let sub_args: Vec<String> = all.drain(idx + 1..).collect();
                publish_address_cmd(&sub_args)
            } else {
                eprintln!("avahi-publish: Unknown command: {cmd}");
                eprintln!("Try 'avahi-publish --help' for more information.");
                1
            }
        }
    }
}

fn publish_service_cmd(args: &[String]) -> i32 {
    let mut name: Option<String> = None;
    let mut service_type: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut txt_records: Vec<String> = Vec::new();
    let mut domain = MDNS_DOMAIN.to_string();
    let mut host: Option<String> = None;

    let mut i = 0;
    let mut positional = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-d" | "--domain" => {
                i += 1;
                if i < args.len() {
                    domain = args[i].clone();
                }
            }
            "-H" | "--host" => {
                i += 1;
                if i < args.len() {
                    host = Some(args[i].clone());
                }
            }
            "-v" | "--verbose" | "-f" | "--no-fail" | "-R" | "--no-reverse" => {}
            other if other.starts_with("--domain=") => {
                domain = other["--domain=".len()..].to_string();
            }
            other if other.starts_with("--host=") => {
                host = Some(other["--host=".len()..].to_string());
            }
            other if !other.starts_with('-') => match positional {
                0 => {
                    name = Some(other.to_string());
                    positional += 1;
                }
                1 => {
                    service_type = Some(other.to_string());
                    positional += 1;
                }
                2 => {
                    match other.parse::<u16>() {
                        Ok(p) => port = Some(p),
                        Err(_) => {
                            eprintln!("avahi-publish: Invalid port: {other}");
                            return 1;
                        }
                    }
                    positional += 1;
                }
                _ => {
                    txt_records.push(other.to_string());
                }
            },
            _ => {}
        }
        i += 1;
    }

    let name = match name {
        Some(n) => n,
        None => {
            eprintln!("avahi-publish: Service name is required");
            return 1;
        }
    };
    let service_type_str = match service_type {
        Some(s) => s,
        None => {
            eprintln!("avahi-publish: Service type is required");
            return 1;
        }
    };
    let port = match port {
        Some(p) => p,
        None => {
            eprintln!("avahi-publish: Port is required");
            return 1;
        }
    };

    if let Err(e) = validate_service_name(&name) {
        eprintln!("avahi-publish: {e}");
        return 1;
    }

    let stype = match ServiceType::parse(&service_type_str) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("avahi-publish: {e}");
            return 1;
        }
    };

    if let Err(e) = validate_txt_records(&txt_records) {
        eprintln!("avahi-publish: {e}");
        return 1;
    }

    let hostname = host.unwrap_or_else(|| DEFAULT_HOSTNAME.to_string());

    let entry = ServiceEntry::new(&name, stype, &hostname, port)
        .with_domain(&domain)
        .with_txt(txt_records);

    println!("Established under name '{}'", entry._name);
    println!("Service: {} ({})", entry.instance_name(), entry._port);
    println!("Domain: {domain}");
    if !entry._txt.is_empty() {
        println!("TXT: {:?}", entry._txt);
    }
    println!("(simulation mode — press Ctrl+C to unpublish)");
    0
}

fn publish_address_cmd(args: &[String]) -> i32 {
    let mut hostname: Option<String> = None;
    let mut address: Option<String> = None;
    let mut no_reverse = false;

    let mut positional = 0;
    for a in args {
        match a.as_str() {
            "-R" | "--no-reverse" => no_reverse = true,
            "-v" | "--verbose" | "-f" | "--no-fail" => {}
            other if !other.starts_with('-') => match positional {
                0 => {
                    hostname = Some(other.to_string());
                    positional += 1;
                }
                1 => {
                    address = Some(other.to_string());
                    positional += 1;
                }
                _ => {}
            },
            _ => {}
        }
    }

    let hostname = match hostname {
        Some(h) => h,
        None => {
            eprintln!("avahi-publish: Hostname is required");
            return 1;
        }
    };
    let address = match address {
        Some(a) => a,
        None => {
            eprintln!("avahi-publish: Address is required");
            return 1;
        }
    };

    if let Err(e) = validate_hostname(&hostname) {
        eprintln!("avahi-publish: {e}");
        return 1;
    }

    let addr = match parse_ip_address(&address) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("avahi-publish: {e}");
            return 1;
        }
    };

    let fqdn = hostname_fqdn(&hostname, MDNS_DOMAIN);
    println!("Established under name '{fqdn}'");
    println!("Address: {addr}");
    if !no_reverse {
        let reverse = match addr {
            IpAddr::V4(v4) => ipv4_to_reverse_name(v4),
            IpAddr::V6(v6) => ipv6_to_reverse_name(v6),
        };
        println!("Reverse: {reverse} -> {fqdn}");
    }
    println!("(simulation mode — press Ctrl+C to unpublish)");
    0
}

// ============================================================================
// Personality: avahi-autoipd
// ============================================================================

fn run_autoipd(args: &[String]) -> i32 {
    let cmd = args
        .first()
        .cloned()
        .unwrap_or_else(|| "--help".to_string());
    let cmd_args: Vec<String> = args.iter().skip(1).cloned().collect();

    match cmd.as_str() {
        "--help" | "-h" => {
            println!("Usage: avahi-autoipd [OPTIONS] <interface>");
            println!();
            println!("IPv4 link-local address configuration daemon.");
            println!("Automatically assigns a 169.254.x.x address via ARP probing (RFC 3927).");
            println!();
            println!("Options:");
            println!("  -D, --daemonize       Daemonize after startup");
            println!("  -s, --syslog          Log to syslog");
            println!("  -k, --kill            Kill running daemon for interface");
            println!("  -r, --refresh         Refresh IP address");
            println!("  -c, --check           Check if daemon is running");
            println!("  --no-drop-root        Don't drop root privileges");
            println!("  --no-chroot           Don't chroot");
            println!("  -S, --start           Start daemon");
            println!("  -w, --wait            Wait until an address is acquired");
            println!("  -V, --version         Show version");
            println!("  -h, --help            Show this help");
            0
        }
        "--version" | "-V" => {
            println!("avahi-autoipd 0.8 (OurOS)");
            0
        }
        "--kill" | "-k" => {
            let iface = cmd_args.first().map(|s| s.as_str()).unwrap_or("eth0");
            println!("Sending SIGTERM to avahi-autoipd for {iface}...");
            println!("Daemon stopped.");
            0
        }
        "--check" | "-c" => {
            let iface = cmd_args.first().map(|s| s.as_str()).unwrap_or("eth0");
            println!("avahi-autoipd is running for {iface} (PID: 2345)");
            0
        }
        "--refresh" | "-r" => {
            let iface = cmd_args.first().map(|s| s.as_str()).unwrap_or("eth0");
            println!("Sending SIGHUP to avahi-autoipd for {iface}...");
            println!("Refreshing IP address...");
            0
        }
        _ => {
            // Find interface name (first non-flag argument)
            let mut interface: Option<String> = None;
            let mut daemonize = false;
            let mut wait = false;

            let all_args: Vec<String> = std::iter::once(cmd.clone())
                .chain(cmd_args.iter().cloned())
                .collect();

            for a in &all_args {
                match a.as_str() {
                    "-D" | "--daemonize" => daemonize = true,
                    "-w" | "--wait" => wait = true,
                    "-s" | "--syslog" | "--no-drop-root" | "--no-chroot" | "-S" | "--start" => {}
                    other if !other.starts_with('-') && interface.is_none() => {
                        interface = Some(other.to_string());
                    }
                    _ => {}
                }
            }

            let iface = match interface {
                Some(i) => i,
                None => {
                    eprintln!("avahi-autoipd: No interface specified");
                    eprintln!("Try 'avahi-autoipd --help' for more information.");
                    return 1;
                }
            };

            if daemonize {
                println!("avahi-autoipd: Daemonizing...");
            }

            println!("avahi-autoipd[2345]: Starting IPv4LL on interface {iface}");

            let mut autoipd = AutoIpd::new(&iface);
            match autoipd.run_to_completion() {
                Ok(addr) => {
                    println!("avahi-autoipd[2345]: Successfully claimed address {addr}");
                    println!("avahi-autoipd[2345]: Configured {iface} with {addr}/16");
                    if wait {
                        println!("avahi-autoipd[2345]: Address acquired, wait complete");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("avahi-autoipd[2345]: Failed: {e}");
                    1
                }
            }
        }
    }
}

// ============================================================================
// Personality: avahi-set-host-name
// ============================================================================

fn run_set_hostname(args: &[String]) -> i32 {
    let cmd = args
        .first()
        .cloned()
        .unwrap_or_else(|| "--help".to_string());

    match cmd.as_str() {
        "--help" | "-h" => {
            println!("Usage: avahi-set-host-name [OPTIONS] <hostname>");
            println!();
            println!("Set the mDNS hostname for this machine.");
            println!();
            println!("Options:");
            println!("  -v, --verbose         Verbose output");
            println!("  -V, --version         Show version");
            println!("  -h, --help            Show this help");
            0
        }
        "--version" | "-V" => {
            println!("avahi-set-host-name 0.8 (OurOS)");
            0
        }
        _ => {
            // Find hostname (first non-flag argument)
            let mut hostname: Option<String> = None;
            let mut verbose = false;

            let all_args: Vec<String> = std::iter::once(cmd.clone())
                .chain(args.iter().skip(1).cloned())
                .collect();

            for a in &all_args {
                match a.as_str() {
                    "-v" | "--verbose" => verbose = true,
                    other if !other.starts_with('-') && hostname.is_none() => {
                        hostname = Some(other.to_string());
                    }
                    _ => {}
                }
            }

            let hostname = match hostname {
                Some(h) => h,
                None => {
                    eprintln!("avahi-set-host-name: No hostname specified");
                    eprintln!("Try 'avahi-set-host-name --help' for more information.");
                    return 1;
                }
            };

            if let Err(e) = validate_hostname(&hostname) {
                eprintln!("avahi-set-host-name: {e}");
                return 1;
            }

            if verbose {
                println!("Connecting to avahi-daemon...");
                println!("Current hostname: {DEFAULT_HOSTNAME}");
                println!("Setting hostname to: {hostname}");
            }

            println!("Host name successfully changed to {hostname}");
            0
        }
    }
}

// ============================================================================
// DNS-SD query helpers
// ============================================================================

/// Build an mDNS query packet for browsing service types.
fn build_browse_query(service_type: &str, domain: &str) -> DnsPacket {
    let mut pkt = DnsPacket::new_query(0);
    let fqdn = format!("{service_type}.{domain}");
    pkt.add_question(DnsQuestion::new(&fqdn, DNS_TYPE_PTR));
    pkt
}

/// Build an mDNS query packet for resolving a service instance.
fn build_resolve_query(instance_name: &str) -> DnsPacket {
    let mut pkt = DnsPacket::new_query(0);
    pkt.add_question(DnsQuestion::new(instance_name, DNS_TYPE_SRV));
    pkt.add_question(DnsQuestion::new(instance_name, DNS_TYPE_TXT));
    pkt
}

/// Build an mDNS query for a hostname's address.
fn build_host_query(hostname: &str, domain: &str, ipv6: bool) -> DnsPacket {
    let mut pkt = DnsPacket::new_query(0);
    let fqdn = hostname_fqdn(hostname, domain);
    if ipv6 {
        pkt.add_question(DnsQuestion::new(&fqdn, DNS_TYPE_AAAA));
    } else {
        pkt.add_question(DnsQuestion::new(&fqdn, DNS_TYPE_A));
    }
    pkt
}

/// Build a response packet for a service entry.
fn build_service_response(entry: &ServiceEntry) -> DnsPacket {
    let mut pkt = DnsPacket::new_response(0);
    let records = entry.to_dns_records();
    for record in records {
        pkt.add_answer(record);
    }
    pkt
}

/// Build the meta-query response listing all service types.
fn build_meta_query_response(registry: &ServiceRegistry) -> DnsPacket {
    let mut pkt = DnsPacket::new_response(0);
    let types = registry.browse_service_types();
    for stype in &types {
        let fqdn = format!("{stype}.{}", registry.domain());
        pkt.add_answer(DnsRecord::new(
            DNS_SD_META_QUERY,
            DNS_CLASS_IN,
            DEFAULT_TTL,
            DnsRecordData::Ptr(fqdn),
        ));
    }
    pkt
}

/// Build a goodbye packet (TTL=0) for a service being unregistered.
fn build_goodbye_packet(entry: &ServiceEntry) -> DnsPacket {
    let mut pkt = DnsPacket::new_response(0);
    let stype_fqdn = entry._service_type.fqdn(&entry._domain);
    let instance = entry.instance_name();
    pkt.add_answer(DnsRecord::new(
        &stype_fqdn,
        DNS_CLASS_IN,
        0,
        DnsRecordData::Ptr(instance),
    ));
    pkt
}

// ============================================================================
// Name conflict resolution helpers
// ============================================================================

/// Generate an alternative name after a conflict (append or increment suffix).
fn make_alternative_name(name: &str) -> String {
    // Check if name already has a numeric suffix like "Name #2"
    if let Some(pos) = name.rfind(" #") {
        let suffix = &name[pos + 2..];
        if let Ok(n) = suffix.parse::<u32>() {
            return format!("{} #{}", &name[..pos], n + 1);
        }
    }
    format!("{name} #2")
}

/// Probe for name uniqueness (simulated: always succeeds unless name contains "conflict").
fn probe_name(name: &str) -> Result<(), AvahiError> {
    if name.to_lowercase().contains("conflict") {
        Err(AvahiError::NameCollision(name.to_string()))
    } else {
        Ok(())
    }
}

// ============================================================================
// Rate limiter (for mDNS query rate limiting)
// ============================================================================

/// Simple token-bucket rate limiter.
#[derive(Debug, Clone, Copy)]
struct RateLimiter {
    _burst: u32,
    _tokens: u32,
    _interval_usec: u64,
    _last_refill: u64,
}

impl RateLimiter {
    fn new(burst: u32, interval_usec: u64) -> Self {
        Self {
            _burst: burst,
            _tokens: burst,
            _interval_usec: interval_usec,
            _last_refill: 0,
        }
    }

    fn try_acquire(&mut self, now_usec: u64) -> bool {
        // Refill tokens based on elapsed time.
        let elapsed = now_usec.saturating_sub(self._last_refill);
        if elapsed >= self._interval_usec {
            let refills = elapsed / self._interval_usec;
            self._tokens = self._tokens.saturating_add(refills as u32).min(self._burst);
            self._last_refill = now_usec;
        }
        if self._tokens > 0 {
            self._tokens -= 1;
            true
        } else {
            false
        }
    }

    fn available(&self) -> u32 {
        self._tokens
    }
}

// ============================================================================
// DNS-SD TXT record parsing
// ============================================================================

/// Parse a TXT record entry of the form "key=value" into (key, value).
fn parse_txt_entry(entry: &str) -> (&str, Option<&str>) {
    if let Some(eq_pos) = entry.find('=') {
        (&entry[..eq_pos], Some(&entry[eq_pos + 1..]))
    } else {
        (entry, None)
    }
}

/// Encode TXT record entries into wire format (length-prefixed strings).
fn encode_txt_records(entries: &[String]) -> Vec<u8> {
    let mut data = Vec::new();
    for entry in entries {
        let len = entry.len().min(255);
        data.push(len as u8);
        data.extend_from_slice(&entry.as_bytes()[..len]);
    }
    data
}

/// Decode wire-format TXT records back to strings.
fn decode_txt_records(data: &[u8]) -> Vec<String> {
    let mut entries = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let len = data[pos] as usize;
        pos += 1;
        if pos + len > data.len() {
            break;
        }
        if let Ok(s) = std::str::from_utf8(&data[pos..pos + len]) {
            entries.push(s.to_string());
        }
        pos += len;
    }
    entries
}

// ============================================================================
// DNS name encoding/decoding (wire format, RFC 1035)
// ============================================================================

/// Encode a DNS name into wire format (label-length encoding).
fn encode_dns_name(name: &str) -> Vec<u8> {
    let mut data = Vec::new();
    for label in name.split('.') {
        if label.is_empty() {
            continue;
        }
        let len = label.len().min(63);
        data.push(len as u8);
        data.extend_from_slice(&label.as_bytes()[..len]);
    }
    data.push(0); // Root label terminator
    data
}

/// Decode a DNS name from wire format (without compression support).
fn decode_dns_name(data: &[u8], start: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut pos = start;
    loop {
        if pos >= data.len() {
            return None;
        }
        let len = data[pos] as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len > 63 {
            // Compression pointer — not supported in this simple decoder.
            return None;
        }
        pos += 1;
        if pos + len > data.len() {
            return None;
        }
        let label = std::str::from_utf8(&data[pos..pos + len]).ok()?;
        labels.push(label.to_string());
        pos += len;
    }
    Some((labels.join("."), pos))
}

// ============================================================================
// Entry point and personality dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("avahi-daemon");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "avahi-daemon" => run_daemon(&rest),
        "avahi-browse" => run_browse(&rest),
        "avahi-resolve" => run_resolve(&rest),
        "avahi-publish" => run_publish(&rest),
        "avahi-autoipd" => run_autoipd(&rest),
        "avahi-set-host-name" => run_set_hostname(&rest),
        _ => {
            // Default to daemon behavior.
            run_daemon(&rest)
        }
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- IPv4 address tests ---

    #[test]
    fn test_ipv4_new() {
        let addr = Ipv4Addr::new(192, 168, 1, 100);
        assert_eq!(addr.octets(), [192, 168, 1, 100]);
    }

    #[test]
    fn test_ipv4_display() {
        let addr = Ipv4Addr::new(10, 0, 0, 1);
        assert_eq!(addr.to_string(), "10.0.0.1");
    }

    #[test]
    fn test_ipv4_from_u32() {
        let addr = Ipv4Addr::from_u32(0xC0A80164); // 192.168.1.100
        assert_eq!(addr.octets(), [192, 168, 1, 100]);
    }

    #[test]
    fn test_ipv4_to_u32() {
        let addr = Ipv4Addr::new(192, 168, 1, 100);
        assert_eq!(addr.to_u32(), 0xC0A80164);
    }

    #[test]
    fn test_ipv4_roundtrip() {
        let original = 0xAC100A05u32; // 172.16.10.5
        let addr = Ipv4Addr::from_u32(original);
        assert_eq!(addr.to_u32(), original);
    }

    #[test]
    fn test_ipv4_link_local() {
        assert!(Ipv4Addr::new(169, 254, 1, 1).is_link_local());
        assert!(Ipv4Addr::new(169, 254, 254, 255).is_link_local());
        assert!(!Ipv4Addr::new(192, 168, 1, 1).is_link_local());
        assert!(!Ipv4Addr::new(10, 0, 0, 1).is_link_local());
    }

    #[test]
    fn test_ipv4_zero() {
        let addr = Ipv4Addr::new(0, 0, 0, 0);
        assert_eq!(addr.to_string(), "0.0.0.0");
        assert_eq!(addr.to_u32(), 0);
    }

    #[test]
    fn test_ipv4_broadcast() {
        let addr = Ipv4Addr::new(255, 255, 255, 255);
        assert_eq!(addr.to_u32(), 0xFFFF_FFFF);
    }

    #[test]
    fn test_ipv4_eq() {
        let a = Ipv4Addr::new(10, 0, 0, 1);
        let b = Ipv4Addr::new(10, 0, 0, 1);
        let c = Ipv4Addr::new(10, 0, 0, 2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // --- IPv6 address tests ---

    #[test]
    fn test_ipv6_new() {
        let addr = Ipv6Addr::new(0xFE80, 0, 0, 0, 0x1234, 0x5678, 0x9ABC, 0xDEF0);
        assert_eq!(addr.segments()[0], 0xFE80);
        assert_eq!(addr.segments()[7], 0xDEF0);
    }

    #[test]
    fn test_ipv6_display() {
        let addr = Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 1);
        assert_eq!(addr.to_string(), "fe80:0:0:0:0:0:0:1");
    }

    #[test]
    fn test_ipv6_link_local() {
        assert!(Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 1).is_link_local());
        assert!(!Ipv6Addr::new(0x2001, 0xDB8, 0, 0, 0, 0, 0, 1).is_link_local());
    }

    #[test]
    fn test_ipv6_not_link_local_nonzero_segments() {
        assert!(!Ipv6Addr::new(0xFE80, 1, 0, 0, 0, 0, 0, 1).is_link_local());
    }

    #[test]
    fn test_ipv6_eq() {
        let a = Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 1);
        let b = Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 1);
        let c = Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // --- IpAddr tests ---

    #[test]
    fn test_ipaddr_display_v4() {
        let addr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(addr.to_string(), "127.0.0.1");
    }

    #[test]
    fn test_ipaddr_display_v6() {
        let addr = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
        assert_eq!(addr.to_string(), "0:0:0:0:0:0:0:1");
    }

    // --- IP address parsing tests ---

    #[test]
    fn test_parse_ipv4_valid() {
        let addr = parse_ipv4("192.168.1.100").unwrap();
        assert_eq!(addr.octets(), [192, 168, 1, 100]);
    }

    #[test]
    fn test_parse_ipv4_invalid_octets() {
        assert!(parse_ipv4("256.0.0.1").is_err());
    }

    #[test]
    fn test_parse_ipv4_too_few_parts() {
        assert!(parse_ipv4("192.168.1").is_err());
    }

    #[test]
    fn test_parse_ipv4_too_many_parts() {
        assert!(parse_ipv4("192.168.1.1.1").is_err());
    }

    #[test]
    fn test_parse_ipv4_non_numeric() {
        assert!(parse_ipv4("abc.def.ghi.jkl").is_err());
    }

    #[test]
    fn test_parse_ipv6_valid() {
        let addr = parse_ipv6("fe80:0000:0000:0000:1234:5678:9abc:def0").unwrap();
        assert_eq!(addr.segments()[0], 0xFE80);
        assert_eq!(addr.segments()[4], 0x1234);
    }

    #[test]
    fn test_parse_ipv6_invalid() {
        assert!(parse_ipv6("fe80::1").is_err()); // Compressed form not supported
    }

    #[test]
    fn test_parse_ip_auto_v4() {
        let addr = parse_ip_address("10.0.0.1").unwrap();
        assert!(matches!(addr, IpAddr::V4(_)));
    }

    #[test]
    fn test_parse_ip_auto_v6() {
        let addr = parse_ip_address("fe80:0:0:0:0:0:0:1").unwrap();
        assert!(matches!(addr, IpAddr::V6(_)));
    }

    // --- DNS record tests ---

    #[test]
    fn test_dns_record_a() {
        let rec = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            DEFAULT_TTL,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        assert_eq!(rec.record_type_code(), DNS_TYPE_A);
        assert_eq!(rec.record_type_str(), "A");
    }

    #[test]
    fn test_dns_record_aaaa() {
        let rec = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            DEFAULT_TTL,
            DnsRecordData::Aaaa(Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 1)),
        );
        assert_eq!(rec.record_type_code(), DNS_TYPE_AAAA);
        assert_eq!(rec.record_type_str(), "AAAA");
    }

    #[test]
    fn test_dns_record_ptr() {
        let rec = DnsRecord::new(
            "_http._tcp.local",
            DNS_CLASS_IN,
            DEFAULT_TTL,
            DnsRecordData::Ptr("My Server._http._tcp.local".to_string()),
        );
        assert_eq!(rec.record_type_code(), DNS_TYPE_PTR);
    }

    #[test]
    fn test_dns_record_srv() {
        let rec = DnsRecord::new(
            "test._http._tcp.local",
            DNS_CLASS_IN,
            1500,
            DnsRecordData::Srv {
                _priority: 0,
                _weight: 0,
                _port: 80,
                _target: "host.local".to_string(),
            },
        );
        assert_eq!(rec.record_type_code(), DNS_TYPE_SRV);
    }

    #[test]
    fn test_dns_record_txt() {
        let rec = DnsRecord::new(
            "test._http._tcp.local",
            DNS_CLASS_IN,
            DEFAULT_TTL,
            DnsRecordData::Txt(vec!["key=value".to_string()]),
        );
        assert_eq!(rec.record_type_code(), DNS_TYPE_TXT);
    }

    #[test]
    fn test_dns_record_cache_flush() {
        let rec = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            DEFAULT_TTL,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        assert_eq!(rec.wire_class(), DNS_CLASS_IN);
        let rec = rec.with_cache_flush(true);
        assert_eq!(rec.wire_class(), DNS_CLASS_IN | CACHE_FLUSH_BIT);
    }

    #[test]
    fn test_dns_record_display() {
        let rec = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            4500,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        let s = rec.to_string();
        assert!(s.contains("test.local"));
        assert!(s.contains("4500"));
        assert!(s.contains("A"));
        assert!(s.contains("10.0.0.1"));
    }

    #[test]
    fn test_dns_record_data_display_srv() {
        let data = DnsRecordData::Srv {
            _priority: 10,
            _weight: 20,
            _port: 80,
            _target: "host.local".to_string(),
        };
        let s = data.to_string();
        assert!(s.contains("SRV"));
        assert!(s.contains("80"));
        assert!(s.contains("host.local"));
    }

    #[test]
    fn test_dns_record_data_display_txt() {
        let data = DnsRecordData::Txt(vec!["a=b".to_string(), "c=d".to_string()]);
        let s = data.to_string();
        assert!(s.contains("TXT"));
        assert!(s.contains("\"a=b\""));
        assert!(s.contains("\"c=d\""));
    }

    // --- Service type tests ---

    #[test]
    fn test_service_type_new() {
        let st = ServiceType::new("_http", TransportProtocol::Tcp);
        assert_eq!(st.type_string(), "_http._tcp");
    }

    #[test]
    fn test_service_type_udp() {
        let st = ServiceType::new("_dns-sd", TransportProtocol::Udp);
        assert_eq!(st.type_string(), "_dns-sd._udp");
    }

    #[test]
    fn test_service_type_with_subtype() {
        let st = ServiceType::new("_ipp", TransportProtocol::Tcp).with_subtype("_printer");
        assert_eq!(st.type_string(), "_printer._sub._ipp._tcp");
    }

    #[test]
    fn test_service_type_fqdn() {
        let st = ServiceType::new("_http", TransportProtocol::Tcp);
        assert_eq!(st.fqdn("local"), "_http._tcp.local");
    }

    #[test]
    fn test_service_type_parse_tcp() {
        let st = ServiceType::parse("_http._tcp").unwrap();
        assert_eq!(st._protocol, "_http");
        assert_eq!(st._transport, TransportProtocol::Tcp);
        assert!(st._subtype.is_none());
    }

    #[test]
    fn test_service_type_parse_udp() {
        let st = ServiceType::parse("_sip._udp").unwrap();
        assert_eq!(st._protocol, "_sip");
        assert_eq!(st._transport, TransportProtocol::Udp);
    }

    #[test]
    fn test_service_type_parse_subtype() {
        let st = ServiceType::parse("_printer._sub._ipp._tcp").unwrap();
        assert_eq!(st._protocol, "_ipp");
        assert_eq!(st._transport, TransportProtocol::Tcp);
        assert_eq!(st._subtype.as_deref(), Some("_printer"));
    }

    #[test]
    fn test_service_type_parse_invalid() {
        assert!(ServiceType::parse("invalid").is_err());
        assert!(ServiceType::parse("_http._xyz").is_err());
        assert!(ServiceType::parse("http._tcp").is_err());
    }

    #[test]
    fn test_service_type_display() {
        let st = ServiceType::new("_ssh", TransportProtocol::Tcp);
        assert_eq!(st.to_string(), "_ssh._tcp");
    }

    #[test]
    fn test_transport_protocol_display() {
        assert_eq!(TransportProtocol::Tcp.to_string(), "_tcp");
        assert_eq!(TransportProtocol::Udp.to_string(), "_udp");
    }

    // --- Service entry tests ---

    #[test]
    fn test_service_entry_new() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Test Server", stype, "host1", 80);
        assert_eq!(entry._name, "Test Server");
        assert_eq!(entry._port, 80);
        assert!(entry._active);
    }

    #[test]
    fn test_service_entry_instance_name() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("My Web", stype, "host1", 80);
        assert_eq!(entry.instance_name(), "My Web._http._tcp.local");
    }

    #[test]
    fn test_service_entry_with_txt() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry =
            ServiceEntry::new("Test", stype, "host1", 80).with_txt(vec!["path=/".to_string()]);
        assert_eq!(entry._txt, vec!["path=/"]);
    }

    #[test]
    fn test_service_entry_with_domain() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Test", stype, "host1", 80).with_domain("example.com");
        assert_eq!(entry._domain, "example.com");
    }

    #[test]
    fn test_service_entry_with_addr_v4() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let addr = Ipv4Addr::new(10, 0, 0, 1);
        let entry = ServiceEntry::new("Test", stype, "host1", 80).with_addr_v4(addr);
        assert_eq!(entry._addr_v4, Some(addr));
    }

    #[test]
    fn test_service_entry_with_addr_v6() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let addr = Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 1);
        let entry = ServiceEntry::new("Test", stype, "host1", 80).with_addr_v6(addr);
        assert_eq!(entry._addr_v6, Some(addr));
    }

    #[test]
    fn test_service_entry_to_dns_records() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Web", stype, "host1", 80)
            .with_addr_v4(Ipv4Addr::new(10, 0, 0, 1))
            .with_txt(vec!["path=/".to_string()]);
        let records = entry.to_dns_records();
        // Should have PTR, SRV, TXT, A records
        assert!(records.len() >= 4);
        let types: Vec<u16> = records.iter().map(|r| r.record_type_code()).collect();
        assert!(types.contains(&DNS_TYPE_PTR));
        assert!(types.contains(&DNS_TYPE_SRV));
        assert!(types.contains(&DNS_TYPE_TXT));
        assert!(types.contains(&DNS_TYPE_A));
    }

    #[test]
    fn test_service_entry_to_dns_records_v6() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Web", stype, "host1", 80)
            .with_addr_v6(Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 1));
        let records = entry.to_dns_records();
        let types: Vec<u16> = records.iter().map(|r| r.record_type_code()).collect();
        assert!(types.contains(&DNS_TYPE_AAAA));
    }

    #[test]
    fn test_service_entry_display() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry =
            ServiceEntry::new("Web", stype, "host1", 80).with_addr_v4(Ipv4Addr::new(10, 0, 0, 1));
        let s = entry.to_string();
        assert!(s.contains("Web"));
        assert!(s.contains("_http._tcp"));
        assert!(s.contains("host1"));
        assert!(s.contains("80"));
        assert!(s.contains("10.0.0.1"));
    }

    // --- Interface / protocol tests ---

    #[test]
    fn test_interface_proto_unspec() {
        let ip = InterfaceProto::unspec();
        assert_eq!(ip._interface, IF_UNSPEC);
        assert_eq!(ip._protocol, PROTO_UNSPEC);
    }

    #[test]
    fn test_interface_proto_description() {
        let ip = InterfaceProto::new(2, PROTO_INET);
        assert_eq!(ip.description(), "if2/IPv4");
    }

    #[test]
    fn test_interface_proto_description_unspec() {
        let ip = InterfaceProto::unspec();
        assert_eq!(ip.description(), "any/any");
    }

    #[test]
    fn test_interface_proto_description_v6() {
        let ip = InterfaceProto::new(3, PROTO_INET6);
        assert_eq!(ip.description(), "if3/IPv6");
    }

    // --- Browse result tests ---

    #[test]
    fn test_browse_result_new() {
        let br = BrowseResult::new(
            BrowseEvent::New,
            2,
            PROTO_INET,
            "Test",
            "_http._tcp",
            "local",
        );
        assert_eq!(br._event, BrowseEvent::New);
        assert_eq!(br._name, "Test");
    }

    #[test]
    fn test_browse_event_display() {
        assert_eq!(BrowseEvent::New.to_string(), "NEW");
        assert_eq!(BrowseEvent::Remove.to_string(), "REMOVE");
        assert_eq!(BrowseEvent::AllForNow.to_string(), "ALL_FOR_NOW");
        assert_eq!(BrowseEvent::CacheExhausted.to_string(), "CACHE_EXHAUSTED");
    }

    #[test]
    fn test_browse_result_protocol_str() {
        let br = BrowseResult::new(BrowseEvent::New, 0, PROTO_INET, "", "", "");
        assert_eq!(br.protocol_str(), "IPv4");
        let br = BrowseResult::new(BrowseEvent::New, 0, PROTO_INET6, "", "", "");
        assert_eq!(br.protocol_str(), "IPv6");
        let br = BrowseResult::new(BrowseEvent::New, 0, PROTO_UNSPEC, "", "", "");
        assert_eq!(br.protocol_str(), "n/a");
    }

    // --- Resolve result tests ---

    #[test]
    fn test_resolve_result_from_service() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry =
            ServiceEntry::new("Web", stype, "host1", 80).with_addr_v4(Ipv4Addr::new(10, 0, 0, 1));
        let rr = ResolveResult::from_service(&entry);
        assert_eq!(rr._name, "Web");
        assert_eq!(rr._hostname, "host1");
        assert_eq!(rr._port, Some(80));
        assert_eq!(rr._address, Some("10.0.0.1".to_string()));
    }

    #[test]
    fn test_resolve_result_from_hostname() {
        let rr = ResolveResult::from_hostname(
            "host1",
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            PROTO_INET,
        );
        assert_eq!(rr._hostname, "host1");
        assert!(rr._service_type.is_none());
    }

    #[test]
    fn test_resolve_result_display_service() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Web", stype, "host1", 80)
            .with_addr_v4(Ipv4Addr::new(10, 0, 0, 1))
            .with_txt(vec!["path=/".to_string()]);
        let rr = ResolveResult::from_service(&entry);
        let s = rr.to_string();
        assert!(s.contains("name = Web"));
        assert!(s.contains("hostname = host1"));
        assert!(s.contains("port = 80"));
        assert!(s.contains("address = 10.0.0.1"));
    }

    #[test]
    fn test_resolve_result_display_hostname() {
        let rr = ResolveResult::from_hostname(
            "host1",
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            PROTO_INET,
        );
        let s = rr.to_string();
        assert!(s.contains("host1"));
        assert!(s.contains("10.0.0.1"));
    }

    // --- Record cache tests ---

    #[test]
    fn test_cache_new() {
        let cache = RecordCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_insert() {
        let mut cache = RecordCache::new();
        let rec = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            100,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        assert!(cache.insert(rec).is_ok());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_cache_lookup() {
        let mut cache = RecordCache::new();
        let rec = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            100,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        cache.insert(rec).unwrap();
        let results = cache.lookup("test.local", DNS_TYPE_A);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_cache_lookup_miss() {
        let cache = RecordCache::new();
        let results = cache.lookup("nonexistent.local", DNS_TYPE_A);
        assert!(results.is_empty());
    }

    #[test]
    fn test_cache_lookup_by_name() {
        let mut cache = RecordCache::new();
        let rec1 = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            100,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        let rec2 = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            100,
            DnsRecordData::Aaaa(Ipv6Addr::new(0xFE80, 0, 0, 0, 0, 0, 0, 1)),
        );
        cache.insert(rec1).unwrap();
        cache.insert(rec2).unwrap();
        let results = cache.lookup_by_name("test.local");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_cache_expiry() {
        let mut cache = RecordCache::new();
        let rec = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            5, // TTL = 5 ticks
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        cache.insert(rec).unwrap();
        // Advance time past TTL
        for _ in 0..6 {
            cache.tick();
        }
        let results = cache.lookup("test.local", DNS_TYPE_A);
        assert!(results.is_empty());
    }

    #[test]
    fn test_cache_flush() {
        let mut cache = RecordCache::new();
        let rec = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            100,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        cache.insert(rec).unwrap();
        assert!(!cache.is_empty());
        cache.flush();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_remove_expired() {
        let mut cache = RecordCache::new();
        let short = DnsRecord::new(
            "short.local",
            DNS_CLASS_IN,
            2,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        let long = DnsRecord::new(
            "long.local",
            DNS_CLASS_IN,
            1000,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 2)),
        );
        cache.insert(short).unwrap();
        cache.insert(long).unwrap();
        assert_eq!(cache.len(), 2);
        for _ in 0..5 {
            cache.tick();
        }
        cache.remove_expired();
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_cache_all_records() {
        let mut cache = RecordCache::new();
        for i in 0..5u8 {
            let rec = DnsRecord::new(
                &format!("host{i}.local"),
                DNS_CLASS_IN,
                100,
                DnsRecordData::A(Ipv4Addr::new(10, 0, 0, i)),
            );
            cache.insert(rec).unwrap();
        }
        assert_eq!(cache.all_records().len(), 5);
    }

    #[test]
    fn test_cache_replace_duplicate() {
        let mut cache = RecordCache::new();
        let rec1 = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            100,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        );
        let rec2 = DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            100,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 2)),
        );
        cache.insert(rec1).unwrap();
        cache.insert(rec2).unwrap();
        // Should replace, not duplicate
        assert_eq!(cache.len(), 1);
        let results = cache.lookup("test.local", DNS_TYPE_A);
        assert_eq!(results.len(), 1);
    }

    // --- Service registry tests ---

    #[test]
    fn test_registry_new() {
        let reg = ServiceRegistry::new("myhost");
        assert_eq!(reg.hostname(), "myhost");
        assert_eq!(reg.domain(), "local");
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn test_registry_register() {
        let mut reg = ServiceRegistry::new("myhost");
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Web", stype, "myhost", 80);
        assert!(reg.register(entry).is_ok());
        assert_eq!(reg.count(), 1);
    }

    #[test]
    fn test_registry_name_collision() {
        let mut reg = ServiceRegistry::new("myhost");
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry1 = ServiceEntry::new("Web", stype.clone(), "myhost", 80);
        let entry2 = ServiceEntry::new("Web", stype, "myhost", 8080);
        assert!(reg.register(entry1).is_ok());
        assert!(reg.register(entry2).is_err());
    }

    #[test]
    fn test_registry_unregister() {
        let mut reg = ServiceRegistry::new("myhost");
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Web", stype, "myhost", 80);
        reg.register(entry).unwrap();
        assert!(reg.unregister("Web", "_http._tcp").is_ok());
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn test_registry_unregister_not_found() {
        let mut reg = ServiceRegistry::new("myhost");
        assert!(reg.unregister("Nonexistent", "_http._tcp").is_err());
    }

    #[test]
    fn test_registry_browse() {
        let mut reg = ServiceRegistry::new("myhost");
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Web", stype, "myhost", 80);
        reg.register(entry).unwrap();
        let results = reg.browse("_http._tcp");
        // Should have one New + one AllForNow
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]._event, BrowseEvent::New);
        assert_eq!(results[1]._event, BrowseEvent::AllForNow);
    }

    #[test]
    fn test_registry_browse_empty() {
        let reg = ServiceRegistry::new("myhost");
        let results = reg.browse("_ftp._tcp");
        assert_eq!(results.len(), 1); // Just AllForNow
        assert_eq!(results[0]._event, BrowseEvent::AllForNow);
    }

    #[test]
    fn test_registry_browse_all() {
        let mut reg = ServiceRegistry::new("myhost");
        let http = ServiceType::new("_http", TransportProtocol::Tcp);
        let ssh = ServiceType::new("_ssh", TransportProtocol::Tcp);
        reg.register(ServiceEntry::new("Web", http, "myhost", 80))
            .unwrap();
        reg.register(ServiceEntry::new("SSH", ssh, "myhost", 22))
            .unwrap();
        let results = reg.browse_all();
        assert_eq!(results.len(), 3); // 2 New + AllForNow
    }

    #[test]
    fn test_registry_browse_service_types() {
        let mut reg = ServiceRegistry::new("myhost");
        let http = ServiceType::new("_http", TransportProtocol::Tcp);
        let ssh = ServiceType::new("_ssh", TransportProtocol::Tcp);
        reg.register(ServiceEntry::new("Web", http, "myhost", 80))
            .unwrap();
        reg.register(ServiceEntry::new("SSH", ssh, "myhost", 22))
            .unwrap();
        let types = reg.browse_service_types();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&"_http._tcp".to_string()));
        assert!(types.contains(&"_ssh._tcp".to_string()));
    }

    #[test]
    fn test_registry_resolve() {
        let mut reg = ServiceRegistry::new("myhost");
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry =
            ServiceEntry::new("Web", stype, "myhost", 80).with_addr_v4(Ipv4Addr::new(10, 0, 0, 1));
        reg.register(entry).unwrap();
        let result = reg.resolve("Web", "_http._tcp").unwrap();
        assert_eq!(result._name, "Web");
        assert_eq!(result._port, Some(80));
    }

    #[test]
    fn test_registry_resolve_not_found() {
        let reg = ServiceRegistry::new("myhost");
        assert!(reg.resolve("Nonexistent", "_http._tcp").is_err());
    }

    #[test]
    fn test_registry_find_by_hostname() {
        let mut reg = ServiceRegistry::new("myhost");
        let http = ServiceType::new("_http", TransportProtocol::Tcp);
        let ssh = ServiceType::new("_ssh", TransportProtocol::Tcp);
        reg.register(ServiceEntry::new("Web", http, "host-a", 80))
            .unwrap();
        reg.register(ServiceEntry::new("SSH", ssh, "host-b", 22))
            .unwrap();
        let found = reg.find_by_hostname("host-a");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]._name, "Web");
    }

    #[test]
    fn test_registry_find_by_address() {
        let mut reg = ServiceRegistry::new("myhost");
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let addr = Ipv4Addr::new(10, 0, 0, 1);
        let entry = ServiceEntry::new("Web", stype, "host1", 80).with_addr_v4(addr);
        reg.register(entry).unwrap();
        let found = reg.find_by_address(&IpAddr::V4(addr));
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_registry_deactivate() {
        let mut reg = ServiceRegistry::new("myhost");
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        reg.register(ServiceEntry::new("Web", stype, "host1", 80))
            .unwrap();
        assert_eq!(reg.count(), 1);
        assert!(reg.deactivate("Web", "_http._tcp"));
        assert_eq!(reg.count(), 0);
        assert_eq!(reg.count_all(), 1);
    }

    #[test]
    fn test_registry_set_hostname() {
        let mut reg = ServiceRegistry::new("old-host");
        reg.set_hostname("new-host");
        assert_eq!(reg.hostname(), "new-host");
    }

    #[test]
    fn test_registry_all_services() {
        let mut reg = ServiceRegistry::new("host");
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        reg.register(ServiceEntry::new("A", stype.clone(), "host", 80))
            .unwrap();
        reg.register(ServiceEntry::new("B", stype, "host", 8080))
            .unwrap();
        assert_eq!(reg.all_services().len(), 2);
    }

    // --- Daemon config tests ---

    #[test]
    fn test_daemon_config_default() {
        let config = DaemonConfig::default_config();
        assert_eq!(config._hostname, DEFAULT_HOSTNAME);
        assert!(config._use_ipv4);
        assert!(config._use_ipv6);
    }

    #[test]
    fn test_daemon_config_parse_line() {
        let mut config = DaemonConfig::default_config();
        config.parse_config_line("host-name=myhost").unwrap();
        assert_eq!(config._hostname, "myhost");
    }

    #[test]
    fn test_daemon_config_parse_comment() {
        let mut config = DaemonConfig::default_config();
        assert!(config.parse_config_line("# comment").is_ok());
        assert!(config.parse_config_line("").is_ok());
    }

    #[test]
    fn test_daemon_config_parse_section() {
        let mut config = DaemonConfig::default_config();
        assert!(config.parse_config_line("[server]").is_ok());
    }

    #[test]
    fn test_daemon_config_parse_bool() {
        let mut config = DaemonConfig::default_config();
        config.parse_config_line("use-ipv6=no").unwrap();
        assert!(!config._use_ipv6);
        config.parse_config_line("use-ipv6=yes").unwrap();
        assert!(config._use_ipv6);
    }

    #[test]
    fn test_daemon_config_parse_numeric() {
        let mut config = DaemonConfig::default_config();
        config.parse_config_line("cache-entries-max=2048").unwrap();
        assert_eq!(config._cache_entries_max, 2048);
    }

    #[test]
    fn test_daemon_config_parse_invalid_numeric() {
        let mut config = DaemonConfig::default_config();
        assert!(config.parse_config_line("cache-entries-max=abc").is_err());
    }

    #[test]
    fn test_daemon_config_load_from_string() {
        let mut config = DaemonConfig::default_config();
        let content = "\
[server]\n\
host-name=test-host\n\
domain-name=test.local\n\
use-ipv4=yes\n\
use-ipv6=no\n\
\n\
[publish]\n\
publish-hinfo=no\n\
";
        config.load_from_string(content).unwrap();
        assert_eq!(config._hostname, "test-host");
        assert_eq!(config._domain, "test.local");
        assert!(config._use_ipv4);
        assert!(!config._use_ipv6);
        assert!(!config._publish_hinfo);
    }

    #[test]
    fn test_daemon_config_parse_interfaces() {
        let mut config = DaemonConfig::default_config();
        config
            .parse_config_line("allow-interfaces=eth0, wlan0")
            .unwrap();
        assert_eq!(config._allow_interfaces, vec!["eth0", "wlan0"]);
    }

    #[test]
    fn test_daemon_config_parse_browse_domains() {
        let mut config = DaemonConfig::default_config();
        config
            .parse_config_line("browse-domains=0pointer.de, zeroconf.org")
            .unwrap();
        assert_eq!(config._browse_domains, vec!["0pointer.de", "zeroconf.org"]);
    }

    // --- AutoIpd tests ---

    #[test]
    fn test_autoipd_new() {
        let aipd = AutoIpd::new("eth0");
        assert_eq!(aipd._state, AutoIpState::Init);
        assert!(aipd._selected_addr.is_none());
    }

    #[test]
    fn test_autoipd_select_address_is_link_local() {
        let mut aipd = AutoIpd::new("eth0").with_seed(42);
        let addr = aipd.select_address();
        assert!(addr.is_link_local());
    }

    #[test]
    fn test_autoipd_select_address_in_range() {
        let mut aipd = AutoIpd::new("eth0").with_seed(12345);
        for _ in 0..100 {
            let addr = aipd.select_address();
            let val = addr.to_u32();
            assert!(val >= LINK_LOCAL_START);
            assert!(val <= LINK_LOCAL_END);
        }
    }

    #[test]
    fn test_autoipd_step_init_to_probing() {
        let mut aipd = AutoIpd::new("eth0");
        let msg = aipd.step().unwrap();
        assert_eq!(aipd._state, AutoIpState::Probing);
        assert!(aipd._selected_addr.is_some());
        assert!(msg.contains("Selected candidate"));
    }

    #[test]
    fn test_autoipd_step_probing() {
        let mut aipd = AutoIpd::new("eth0");
        aipd.step().unwrap(); // Init -> Probing
        for _ in 0..ARP_PROBE_COUNT {
            aipd.step().unwrap();
        }
        assert_eq!(aipd._state, AutoIpState::Announcing);
    }

    #[test]
    fn test_autoipd_step_announcing() {
        let mut aipd = AutoIpd::new("eth0");
        aipd.step().unwrap(); // Init -> Probing
        for _ in 0..ARP_PROBE_COUNT {
            aipd.step().unwrap();
        }
        for _ in 0..ARP_ANNOUNCE_COUNT {
            aipd.step().unwrap();
        }
        assert_eq!(aipd._state, AutoIpState::Running);
    }

    #[test]
    fn test_autoipd_run_to_completion() {
        let mut aipd = AutoIpd::new("eth0").with_seed(42);
        let addr = aipd.run_to_completion().unwrap();
        assert!(addr.is_link_local());
        assert_eq!(aipd._state, AutoIpState::Running);
    }

    #[test]
    fn test_autoipd_conflict() {
        let mut aipd = AutoIpd::new("eth0");
        aipd.step().unwrap(); // Init -> Probing
        let msg = aipd.conflict().unwrap();
        assert!(msg.contains("conflict"));
        assert_eq!(aipd._state, AutoIpState::Conflict);
        // Step should select new address
        let msg = aipd.step().unwrap();
        assert!(msg.contains("trying new address"));
        assert_eq!(aipd._state, AutoIpState::Probing);
    }

    #[test]
    fn test_autoipd_max_conflicts() {
        let mut aipd = AutoIpd::new("eth0");
        aipd.step().unwrap(); // Init -> Probing
        for _ in 0..MAX_CONFLICTS {
            aipd.conflict().unwrap();
            aipd.step().unwrap(); // Conflict -> Probing
        }
        aipd.conflict().unwrap();
        let result = aipd.step();
        assert!(result.is_err());
    }

    #[test]
    fn test_autoipd_stop() {
        let mut aipd = AutoIpd::new("eth0");
        aipd.step().unwrap();
        aipd.stop();
        assert_eq!(aipd._state, AutoIpState::Stopped);
        assert!(aipd.step().is_err());
    }

    #[test]
    fn test_autoipd_conflict_when_stopped() {
        let mut aipd = AutoIpd::new("eth0");
        aipd.stop();
        assert!(aipd.conflict().is_err());
    }

    #[test]
    fn test_autoipd_deterministic_with_seed() {
        let mut a1 = AutoIpd::new("eth0").with_seed(42);
        let mut a2 = AutoIpd::new("eth0").with_seed(42);
        let addr1 = a1.select_address();
        let addr2 = a2.select_address();
        assert_eq!(addr1, addr2);
    }

    // --- Hostname validation tests ---

    #[test]
    fn test_validate_hostname_valid() {
        assert!(validate_hostname("ouros-host").is_ok());
        assert!(validate_hostname("my-machine").is_ok());
        assert!(validate_hostname("host123").is_ok());
    }

    #[test]
    fn test_validate_hostname_empty() {
        assert!(validate_hostname("").is_err());
    }

    #[test]
    fn test_validate_hostname_too_long() {
        let long = "a".repeat(MAX_NAME_LEN + 1);
        assert!(validate_hostname(&long).is_err());
    }

    #[test]
    fn test_validate_hostname_invalid_char() {
        assert!(validate_hostname("host_name").is_err());
        assert!(validate_hostname("host name").is_err());
        assert!(validate_hostname("host@name").is_err());
    }

    #[test]
    fn test_validate_hostname_hyphen_boundary() {
        assert!(validate_hostname("-host").is_err());
        assert!(validate_hostname("host-").is_err());
    }

    #[test]
    fn test_validate_hostname_label_too_long() {
        let long_label = "a".repeat(64);
        assert!(validate_hostname(&long_label).is_err());
    }

    #[test]
    fn test_validate_hostname_with_dots() {
        assert!(validate_hostname("host.local").is_ok());
    }

    #[test]
    fn test_validate_hostname_empty_label() {
        assert!(validate_hostname("host..local").is_err());
    }

    // --- Service name validation tests ---

    #[test]
    fn test_validate_service_name_valid() {
        assert!(validate_service_name("My Service").is_ok());
        assert!(validate_service_name("a").is_ok());
    }

    #[test]
    fn test_validate_service_name_empty() {
        assert!(validate_service_name("").is_err());
    }

    #[test]
    fn test_validate_service_name_too_long() {
        let long = "a".repeat(64);
        assert!(validate_service_name(&long).is_err());
    }

    // --- TXT record validation tests ---

    #[test]
    fn test_validate_txt_valid() {
        let txt = vec!["key=value".to_string(), "flag".to_string()];
        assert!(validate_txt_records(&txt).is_ok());
    }

    #[test]
    fn test_validate_txt_entry_too_long() {
        let long_entry = "a".repeat(256);
        let txt = vec![long_entry];
        assert!(validate_txt_records(&txt).is_err());
    }

    #[test]
    fn test_validate_txt_empty() {
        let txt: Vec<String> = Vec::new();
        assert!(validate_txt_records(&txt).is_ok());
    }

    // --- Reverse name tests ---

    #[test]
    fn test_ipv4_reverse_name() {
        let addr = Ipv4Addr::new(192, 168, 1, 100);
        assert_eq!(ipv4_to_reverse_name(addr), "100.1.168.192.in-addr.arpa");
    }

    #[test]
    fn test_ipv6_reverse_name() {
        let addr = Ipv6Addr::new(0x2001, 0x0DB8, 0, 0, 0, 0, 0, 1);
        let rev = ipv6_to_reverse_name(addr);
        assert!(rev.ends_with(".ip6.arpa"));
        // Should have 32 hex nibbles separated by dots
        let parts: Vec<&str> = rev.strip_suffix(".ip6.arpa").unwrap().split('.').collect();
        assert_eq!(parts.len(), 32);
    }

    // --- Hostname FQDN tests ---

    #[test]
    fn test_hostname_fqdn() {
        assert_eq!(hostname_fqdn("myhost", "local"), "myhost.local");
    }

    // --- DNS header flags tests ---

    #[test]
    fn test_dns_flags_query() {
        let flags = DnsHeaderFlags::query();
        let val = flags.to_u16();
        assert_eq!(val & 0x8000, 0); // QR bit clear
    }

    #[test]
    fn test_dns_flags_response() {
        let flags = DnsHeaderFlags::response();
        let val = flags.to_u16();
        assert_ne!(val & 0x8000, 0); // QR bit set
        assert_ne!(val & 0x0400, 0); // AA bit set
    }

    #[test]
    fn test_dns_flags_roundtrip() {
        let original = DnsHeaderFlags {
            _qr: true,
            _opcode: 2,
            _aa: true,
            _tc: false,
            _rd: true,
            _ra: false,
            _rcode: 3,
        };
        let val = original.to_u16();
        let decoded = DnsHeaderFlags::from_u16(val);
        assert_eq!(decoded._qr, original._qr);
        assert_eq!(decoded._opcode, original._opcode);
        assert_eq!(decoded._aa, original._aa);
        assert_eq!(decoded._tc, original._tc);
        assert_eq!(decoded._rd, original._rd);
        assert_eq!(decoded._ra, original._ra);
        assert_eq!(decoded._rcode, original._rcode);
    }

    // --- DNS packet tests ---

    #[test]
    fn test_dns_packet_query() {
        let pkt = DnsPacket::new_query(0x1234);
        assert!(pkt.is_query());
        assert!(!pkt.is_response());
        assert_eq!(pkt._id, 0x1234);
    }

    #[test]
    fn test_dns_packet_response() {
        let pkt = DnsPacket::new_response(0);
        assert!(pkt.is_response());
        assert!(!pkt.is_query());
    }

    #[test]
    fn test_dns_packet_add_question() {
        let mut pkt = DnsPacket::new_query(0);
        pkt.add_question(DnsQuestion::new("_http._tcp.local", DNS_TYPE_PTR));
        assert_eq!(pkt.question_count(), 1);
    }

    #[test]
    fn test_dns_packet_add_answer() {
        let mut pkt = DnsPacket::new_response(0);
        pkt.add_answer(DnsRecord::new(
            "test.local",
            DNS_CLASS_IN,
            100,
            DnsRecordData::A(Ipv4Addr::new(10, 0, 0, 1)),
        ));
        assert_eq!(pkt.answer_count(), 1);
    }

    #[test]
    fn test_dns_packet_wire_size() {
        let mut pkt = DnsPacket::new_query(0);
        pkt.add_question(DnsQuestion::new("test.local", DNS_TYPE_A));
        assert!(pkt.wire_size() > 12); // At least header + question
    }

    #[test]
    fn test_dns_question_unicast() {
        let q = DnsQuestion::new("test.local", DNS_TYPE_A).with_unicast();
        assert!(q._unicast_response);
        assert_ne!(q.wire_class() & 0x8000, 0);
    }

    // --- Query builder tests ---

    #[test]
    fn test_build_browse_query() {
        let pkt = build_browse_query("_http._tcp", "local");
        assert!(pkt.is_query());
        assert_eq!(pkt.question_count(), 1);
        assert_eq!(pkt._questions[0]._name, "_http._tcp.local");
        assert_eq!(pkt._questions[0]._qtype, DNS_TYPE_PTR);
    }

    #[test]
    fn test_build_resolve_query() {
        let pkt = build_resolve_query("Test._http._tcp.local");
        assert!(pkt.is_query());
        assert_eq!(pkt.question_count(), 2); // SRV + TXT
    }

    #[test]
    fn test_build_host_query_v4() {
        let pkt = build_host_query("myhost", "local", false);
        assert_eq!(pkt._questions[0]._qtype, DNS_TYPE_A);
    }

    #[test]
    fn test_build_host_query_v6() {
        let pkt = build_host_query("myhost", "local", true);
        assert_eq!(pkt._questions[0]._qtype, DNS_TYPE_AAAA);
    }

    #[test]
    fn test_build_service_response() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry =
            ServiceEntry::new("Web", stype, "host1", 80).with_addr_v4(Ipv4Addr::new(10, 0, 0, 1));
        let pkt = build_service_response(&entry);
        assert!(pkt.is_response());
        assert!(pkt.answer_count() >= 3); // PTR + SRV + TXT + A
    }

    #[test]
    fn test_build_meta_query_response() {
        let mut reg = ServiceRegistry::new("host");
        let http = ServiceType::new("_http", TransportProtocol::Tcp);
        let ssh = ServiceType::new("_ssh", TransportProtocol::Tcp);
        reg.register(ServiceEntry::new("Web", http, "host", 80))
            .unwrap();
        reg.register(ServiceEntry::new("SSH", ssh, "host", 22))
            .unwrap();
        let pkt = build_meta_query_response(&reg);
        assert!(pkt.is_response());
        assert_eq!(pkt.answer_count(), 2);
    }

    #[test]
    fn test_build_goodbye_packet() {
        let stype = ServiceType::new("_http", TransportProtocol::Tcp);
        let entry = ServiceEntry::new("Web", stype, "host1", 80);
        let pkt = build_goodbye_packet(&entry);
        assert!(pkt.is_response());
        assert_eq!(pkt._answers[0]._ttl, 0);
    }

    // --- Name conflict resolution tests ---

    #[test]
    fn test_make_alternative_name() {
        assert_eq!(make_alternative_name("My Service"), "My Service #2");
    }

    #[test]
    fn test_make_alternative_name_increment() {
        assert_eq!(make_alternative_name("My Service #2"), "My Service #3");
        assert_eq!(make_alternative_name("My Service #99"), "My Service #100");
    }

    #[test]
    fn test_probe_name_ok() {
        assert!(probe_name("My Service").is_ok());
    }

    #[test]
    fn test_probe_name_conflict() {
        assert!(probe_name("conflict-service").is_err());
    }

    // --- Rate limiter tests ---

    #[test]
    fn test_rate_limiter_new() {
        let rl = RateLimiter::new(10, 1_000_000);
        assert_eq!(rl.available(), 10);
    }

    #[test]
    fn test_rate_limiter_acquire() {
        let mut rl = RateLimiter::new(3, 1_000_000);
        assert!(rl.try_acquire(0));
        assert!(rl.try_acquire(0));
        assert!(rl.try_acquire(0));
        assert!(!rl.try_acquire(0)); // Exhausted
    }

    #[test]
    fn test_rate_limiter_refill() {
        let mut rl = RateLimiter::new(2, 1_000_000);
        assert!(rl.try_acquire(0));
        assert!(rl.try_acquire(0));
        assert!(!rl.try_acquire(0)); // Exhausted
        assert!(rl.try_acquire(1_000_000)); // Refilled after interval
    }

    #[test]
    fn test_rate_limiter_cap_at_burst() {
        let mut rl = RateLimiter::new(2, 1_000_000);
        // Even after a long time, tokens cap at burst
        assert!(rl.try_acquire(10_000_000));
        assert!(rl.try_acquire(10_000_000));
        assert!(!rl.try_acquire(10_000_000));
    }

    // --- TXT record encoding/decoding tests ---

    #[test]
    fn test_parse_txt_entry_kv() {
        let (key, val) = parse_txt_entry("key=value");
        assert_eq!(key, "key");
        assert_eq!(val, Some("value"));
    }

    #[test]
    fn test_parse_txt_entry_flag() {
        let (key, val) = parse_txt_entry("flag");
        assert_eq!(key, "flag");
        assert_eq!(val, None);
    }

    #[test]
    fn test_parse_txt_entry_empty_value() {
        let (key, val) = parse_txt_entry("key=");
        assert_eq!(key, "key");
        assert_eq!(val, Some(""));
    }

    #[test]
    fn test_encode_txt_records() {
        let entries = vec!["a=b".to_string(), "c=d".to_string()];
        let data = encode_txt_records(&entries);
        assert_eq!(data, vec![3, b'a', b'=', b'b', 3, b'c', b'=', b'd']);
    }

    #[test]
    fn test_decode_txt_records() {
        let data = vec![3, b'a', b'=', b'b', 3, b'c', b'=', b'd'];
        let entries = decode_txt_records(&data);
        assert_eq!(entries, vec!["a=b", "c=d"]);
    }

    #[test]
    fn test_txt_roundtrip() {
        let original = vec![
            "key1=value1".to_string(),
            "key2=value2".to_string(),
            "flag".to_string(),
        ];
        let encoded = encode_txt_records(&original);
        let decoded = decode_txt_records(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_txt_empty() {
        let entries = decode_txt_records(&[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_decode_txt_truncated() {
        // Length says 5 but only 3 bytes follow
        let data = vec![5, b'a', b'b', b'c'];
        let entries = decode_txt_records(&data);
        assert!(entries.is_empty()); // Should stop at truncation
    }

    // --- DNS name encoding/decoding tests ---

    #[test]
    fn test_encode_dns_name() {
        let data = encode_dns_name("test.local");
        // Should be: 4 "test" 5 "local" 0
        assert_eq!(data[0], 4);
        assert_eq!(&data[1..5], b"test");
        assert_eq!(data[5], 5);
        assert_eq!(&data[6..11], b"local");
        assert_eq!(data[11], 0);
    }

    #[test]
    fn test_decode_dns_name() {
        let data = encode_dns_name("test.local");
        let (name, pos) = decode_dns_name(&data, 0).unwrap();
        assert_eq!(name, "test.local");
        assert_eq!(pos, data.len());
    }

    #[test]
    fn test_dns_name_roundtrip() {
        let names = vec!["host.local", "_http._tcp.local", "a.b.c.d.e"];
        for n in names {
            let encoded = encode_dns_name(n);
            let (decoded, _) = decode_dns_name(&encoded, 0).unwrap();
            assert_eq!(decoded, n);
        }
    }

    #[test]
    fn test_decode_dns_name_empty() {
        let data = vec![0]; // Root label only
        let (name, _) = decode_dns_name(&data, 0).unwrap();
        assert_eq!(name, "");
    }

    #[test]
    fn test_decode_dns_name_truncated() {
        let data = vec![5, b'h', b'e']; // Says 5 bytes but only 2 follow
        assert!(decode_dns_name(&data, 0).is_none());
    }

    // --- Simulated resolve tests ---

    #[test]
    fn test_resolve_hostname_known() {
        let results = resolve_hostname("ouros-host");
        assert!(!results.is_empty());
    }

    #[test]
    fn test_resolve_hostname_with_domain() {
        let results = resolve_hostname("ouros-host.local");
        assert!(!results.is_empty());
    }

    #[test]
    fn test_resolve_hostname_unknown() {
        let results = resolve_hostname("nonexistent-host");
        assert!(results.is_empty());
    }

    #[test]
    fn test_resolve_address_known() {
        let result = resolve_address("192.168.1.100");
        assert_eq!(result, Some("ouros-host.local".to_string()));
    }

    #[test]
    fn test_resolve_address_unknown() {
        let result = resolve_address("10.0.0.99");
        assert!(result.is_none());
    }

    // --- Demo registry tests ---

    #[test]
    fn test_demo_registry_has_services() {
        let reg = create_demo_registry();
        assert!(reg.count() > 0);
    }

    #[test]
    fn test_demo_registry_has_http() {
        let reg = create_demo_registry();
        let results = reg.browse("_http._tcp");
        // Should have at least one HTTP service + AllForNow
        assert!(results.len() >= 2);
    }

    #[test]
    fn test_demo_registry_has_ssh() {
        let reg = create_demo_registry();
        let results = reg.browse("_ssh._tcp");
        assert!(results.len() >= 2);
    }

    #[test]
    fn test_demo_registry_resolve_http() {
        let reg = create_demo_registry();
        let result = reg.resolve("OurOS Web Server", "_http._tcp");
        assert!(result.is_ok());
    }

    #[test]
    fn test_demo_cache_has_entries() {
        let cache = create_demo_cache();
        assert!(!cache.is_empty());
    }

    #[test]
    fn test_demo_cache_lookup_host() {
        let cache = create_demo_cache();
        let results = cache.lookup("ouros-host.local", DNS_TYPE_A);
        assert!(!results.is_empty());
    }

    // --- Service type database tests ---

    #[test]
    fn test_service_type_db_not_empty() {
        let db = service_type_database();
        assert!(!db.is_empty());
    }

    #[test]
    fn test_service_type_db_has_http() {
        let db = service_type_database();
        assert!(db.iter().any(|(t, _)| *t == "_http._tcp"));
    }

    // --- AvahiError display tests ---

    #[test]
    fn test_error_display() {
        let e = AvahiError::InvalidServiceName("bad".to_string());
        assert!(e.to_string().contains("bad"));
        let e = AvahiError::Timeout;
        assert!(e.to_string().contains("timed out"));
        let e = AvahiError::RegistryFull;
        assert!(e.to_string().contains("full"));
        let e = AvahiError::DaemonNotRunning;
        assert!(e.to_string().contains("not running"));
    }

    #[test]
    fn test_error_display_all_variants() {
        // Ensure every variant has a meaningful display
        let errors: Vec<AvahiError> = vec![
            AvahiError::InvalidServiceName("x".into()),
            AvahiError::InvalidServiceType("x".into()),
            AvahiError::InvalidHostname("x".into()),
            AvahiError::InvalidAddress("x".into()),
            AvahiError::ServiceNotFound("x".into()),
            AvahiError::RecordNotFound("x".into()),
            AvahiError::RegistryFull,
            AvahiError::CacheFull,
            AvahiError::NameCollision("x".into()),
            AvahiError::ConfigError("x".into()),
            AvahiError::IoError("x".into()),
            AvahiError::ProtocolError("x".into()),
            AvahiError::Timeout,
            AvahiError::AddressConflict("x".into()),
            AvahiError::MaxConflictsReached,
            AvahiError::InvalidArgument("x".into()),
            AvahiError::DaemonNotRunning,
            AvahiError::PermissionDenied("x".into()),
        ];
        for e in errors {
            assert!(!e.to_string().is_empty());
        }
    }

    // --- Personality detection tests ---

    #[test]
    fn test_personality_basename_unix() {
        let s = "/usr/bin/avahi-browse";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        assert_eq!(base, "avahi-browse");
    }

    #[test]
    fn test_personality_basename_windows() {
        let s = "C:\\Program Files\\avahi-daemon.exe";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        assert_eq!(base, "avahi-daemon");
    }

    #[test]
    fn test_personality_no_path() {
        let s = "avahi-resolve";
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        assert_eq!(base, "avahi-resolve");
    }

    #[test]
    fn test_personality_exe_strip() {
        let s = "avahi-publish.exe";
        let base = s.strip_suffix(".exe").unwrap_or(s);
        assert_eq!(base, "avahi-publish");
    }

    // --- Constant verification tests ---

    #[test]
    fn test_mdns_constants() {
        assert_eq!(MDNS_PORT, 5353);
        assert_eq!(MDNS_MULTICAST_V4, "224.0.0.251");
        assert_eq!(MDNS_MULTICAST_V6, "ff02::fb");
    }

    #[test]
    fn test_dns_type_constants() {
        assert_eq!(DNS_TYPE_A, 1);
        assert_eq!(DNS_TYPE_AAAA, 28);
        assert_eq!(DNS_TYPE_PTR, 12);
        assert_eq!(DNS_TYPE_SRV, 33);
        assert_eq!(DNS_TYPE_TXT, 16);
    }

    #[test]
    fn test_dns_class_in() {
        assert_eq!(DNS_CLASS_IN, 1);
    }

    #[test]
    fn test_cache_flush_bit() {
        assert_eq!(CACHE_FLUSH_BIT, 0x8000);
    }

    #[test]
    fn test_link_local_range() {
        let start = Ipv4Addr::from_u32(LINK_LOCAL_START);
        let end = Ipv4Addr::from_u32(LINK_LOCAL_END);
        assert!(start.is_link_local());
        assert!(end.is_link_local());
        const _: () = assert!(LINK_LOCAL_END > LINK_LOCAL_START);
    }

    #[test]
    fn test_default_ttl() {
        assert_eq!(DEFAULT_TTL, 4500);
    }

    #[test]
    fn test_proto_constants() {
        assert_eq!(PROTO_INET, 0);
        assert_eq!(PROTO_INET6, 1);
        assert_eq!(PROTO_UNSPEC, -1);
        assert_eq!(IF_UNSPEC, -1);
    }
}
