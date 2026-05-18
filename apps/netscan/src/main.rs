//! OurOS Network Scanner
//!
//! Graphical network scanner / discovery application with:
//! - IP range scanning (CIDR, individual, custom ranges)
//! - Port scanning (well-known, custom ranges, individual)
//! - Host discovery via ping sweep and ARP simulation
//! - Service detection with 100+ port-to-service mappings
//! - Scan profiles (quick, full, custom, stealth)
//! - Results display with expandable host details
//! - Simple network topology visualization
//! - Scan history with diff detection
//! - CSV / JSON export
//! - Bandwidth estimation
//! - Wake-on-LAN magic packet sending
//! - WHOIS lookup for public IPs
//! - Simulated traceroute
//!
//! Uses the guitk library for UI rendering. Network I/O is
//! performed through OurOS syscalls; simulated with representative
//! data for initial development.

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha Theme Colors
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Layout Constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 780.0;
const TITLE_BAR_HEIGHT: f32 = 38.0;
const CONFIG_PANEL_HEIGHT: f32 = 140.0;
const SIDEBAR_WIDTH: f32 = 300.0;
const PROGRESS_BAR_HEIGHT: f32 = 28.0;
const TABLE_HEADER_HEIGHT: f32 = 32.0;
const TABLE_ROW_HEIGHT: f32 = 28.0;
const PADDING: f32 = 12.0;
const BUTTON_HEIGHT: f32 = 32.0;
const INPUT_HEIGHT: f32 = 28.0;
const TAB_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 6.0;
const SMALL_RADIUS: f32 = 4.0;

const MAX_HISTORY_ENTRIES: usize = 50;
const MAX_HOSTS_DISPLAY: usize = 256;

// ============================================================================
// IP Address Types
// ============================================================================

/// A simple IPv4 address representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ipv4Addr {
    pub octets: [u8; 4],
}

impl Ipv4Addr {
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self { octets: [a, b, c, d] }
    }

    pub fn to_u32(self) -> u32 {
        let o = self.octets;
        (o[0] as u32) << 24 | (o[1] as u32) << 16 | (o[2] as u32) << 8 | (o[3] as u32)
    }

    pub fn from_u32(val: u32) -> Self {
        Self {
            octets: [
                ((val >> 24) & 0xFF) as u8,
                ((val >> 16) & 0xFF) as u8,
                ((val >> 8) & 0xFF) as u8,
                (val & 0xFF) as u8,
            ],
        }
    }

    pub fn display(&self) -> String {
        let o = self.octets;
        format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
    }

    /// Check if this is a private/RFC1918 address.
    pub fn is_private(&self) -> bool {
        let o = self.octets;
        // 10.0.0.0/8
        if o[0] == 10 { return true; }
        // 172.16.0.0/12
        if o[0] == 172 && (o[1] >= 16 && o[1] <= 31) { return true; }
        // 192.168.0.0/16
        if o[0] == 192 && o[1] == 168 { return true; }
        false
    }

    /// Check if this is a loopback address.
    pub fn is_loopback(&self) -> bool {
        self.octets[0] == 127
    }

    /// Parse an IPv4 address from a string like "192.168.1.1".
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.trim().split('.').collect();
        if parts.len() != 4 { return None; }
        let mut octets = [0u8; 4];
        for (i, part) in parts.iter().enumerate() {
            let val: u8 = part.parse().ok()?;
            octets[i] = val;
        }
        Some(Self { octets })
    }
}

/// MAC address (6 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MacAddr {
    pub bytes: [u8; 6],
}

impl MacAddr {
    pub const fn new(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8) -> Self {
        Self { bytes: [a, b, c, d, e, f] }
    }

    pub fn display(&self) -> String {
        let b = self.bytes;
        format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            b[0], b[1], b[2], b[3], b[4], b[5])
    }

    /// Generate a deterministic MAC from an IP (for simulation).
    pub fn from_ip_simulated(ip: Ipv4Addr) -> Self {
        let o = ip.octets;
        Self::new(0x00, 0x1A, o[0] ^ 0x3C, o[1], o[2], o[3])
    }
}

// ============================================================================
// CIDR / IP Range
// ============================================================================

/// A CIDR network specification.
#[derive(Clone, Debug)]
pub struct CidrRange {
    pub base: Ipv4Addr,
    pub prefix_len: u8,
}

impl CidrRange {
    /// Number of host addresses in this CIDR range.
    pub fn host_count(&self) -> u32 {
        if self.prefix_len >= 32 { return 1; }
        let bits = 32u32.saturating_sub(self.prefix_len as u32);
        1u32.checked_shl(bits).unwrap_or(0)
    }

    /// Network mask as u32.
    pub fn mask(&self) -> u32 {
        if self.prefix_len == 0 { return 0; }
        if self.prefix_len >= 32 { return 0xFFFF_FFFF; }
        0xFFFF_FFFFu32.checked_shl(32u32.saturating_sub(self.prefix_len as u32))
            .unwrap_or(0)
    }

    /// First address in the range.
    pub fn first_addr(&self) -> Ipv4Addr {
        let network = self.base.to_u32() & self.mask();
        Ipv4Addr::from_u32(network)
    }

    /// Last address in the range.
    pub fn last_addr(&self) -> Ipv4Addr {
        let network = self.base.to_u32() & self.mask();
        let broadcast = network | !self.mask();
        Ipv4Addr::from_u32(broadcast)
    }

    /// Generate all host IPs in the range (excluding network and broadcast for /24+).
    pub fn host_ips(&self) -> Vec<Ipv4Addr> {
        let first = self.first_addr().to_u32();
        let last = self.last_addr().to_u32();
        let mut ips = Vec::new();
        // For small ranges, include all; for /24 or bigger, skip network+broadcast
        let start = if self.prefix_len <= 30 { first.saturating_add(1) } else { first };
        let end = if self.prefix_len <= 30 { last.saturating_sub(1) } else { last };
        let mut current = start;
        while current <= end {
            ips.push(Ipv4Addr::from_u32(current));
            if current == u32::MAX { break; }
            current = current.saturating_add(1);
        }
        ips
    }

    /// Parse a CIDR string like "192.168.1.0/24".
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.trim().split('/').collect();
        if parts.len() != 2 { return None; }
        let base = Ipv4Addr::parse(parts.first()?)?;
        let prefix_len: u8 = parts.get(1)?.parse().ok()?;
        if prefix_len > 32 { return None; }
        Some(Self { base, prefix_len })
    }
}

/// Scan target specification.
#[derive(Clone, Debug)]
pub enum ScanTarget {
    /// Single IP address.
    Single(Ipv4Addr),
    /// CIDR range.
    Cidr(CidrRange),
    /// Explicit range (start..end inclusive).
    Range(Ipv4Addr, Ipv4Addr),
}

impl ScanTarget {
    /// All IPs covered by this target.
    pub fn all_ips(&self) -> Vec<Ipv4Addr> {
        match self {
            Self::Single(ip) => vec![*ip],
            Self::Cidr(cidr) => cidr.host_ips(),
            Self::Range(start, end) => {
                let s = start.to_u32();
                let e = end.to_u32();
                let mut ips = Vec::new();
                let mut current = s;
                while current <= e {
                    ips.push(Ipv4Addr::from_u32(current));
                    if current == u32::MAX { break; }
                    current = current.saturating_add(1);
                }
                ips
            }
        }
    }

    /// Parse from user input. Supports CIDR, range ("x.x.x.x-y.y.y.y"), or single IP.
    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if trimmed.contains('/') {
            CidrRange::parse(trimmed).map(Self::Cidr)
        } else if trimmed.contains('-') {
            let parts: Vec<&str> = trimmed.splitn(2, '-').collect();
            let start = Ipv4Addr::parse(parts.first()?)?;
            let end = Ipv4Addr::parse(parts.get(1)?)?;
            Some(Self::Range(start, end))
        } else {
            Ipv4Addr::parse(trimmed).map(Self::Single)
        }
    }
}

// ============================================================================
// Port Definitions and Service Mappings
// ============================================================================

/// Port range specification.
#[derive(Clone, Debug)]
pub enum PortSpec {
    /// Single port.
    Single(u16),
    /// Inclusive range.
    Range(u16, u16),
    /// List of individual ports.
    List(Vec<u16>),
}

impl PortSpec {
    pub fn all_ports(&self) -> Vec<u16> {
        match self {
            Self::Single(p) => vec![*p],
            Self::Range(start, end) => {
                let mut ports = Vec::new();
                let mut p = *start;
                while p <= *end {
                    ports.push(p);
                    if p == u16::MAX { break; }
                    p = p.saturating_add(1);
                }
                ports
            }
            Self::List(l) => l.clone(),
        }
    }
}

/// A well-known port and its associated service.
#[derive(Clone, Debug)]
pub struct ServiceMapping {
    pub port: u16,
    pub protocol: &'static str,
    pub service: &'static str,
    pub description: &'static str,
}

/// The full service database (100+ entries).
pub fn service_database() -> Vec<ServiceMapping> {
    vec![
        ServiceMapping { port: 1, protocol: "tcp", service: "tcpmux", description: "TCP Port Multiplexer" },
        ServiceMapping { port: 5, protocol: "tcp", service: "rje", description: "Remote Job Entry" },
        ServiceMapping { port: 7, protocol: "tcp", service: "echo", description: "Echo Protocol" },
        ServiceMapping { port: 9, protocol: "tcp", service: "discard", description: "Discard Protocol" },
        ServiceMapping { port: 11, protocol: "tcp", service: "systat", description: "Active Users" },
        ServiceMapping { port: 13, protocol: "tcp", service: "daytime", description: "Daytime Protocol" },
        ServiceMapping { port: 17, protocol: "tcp", service: "qotd", description: "Quote of the Day" },
        ServiceMapping { port: 19, protocol: "tcp", service: "chargen", description: "Character Generator" },
        ServiceMapping { port: 20, protocol: "tcp", service: "ftp-data", description: "FTP Data Transfer" },
        ServiceMapping { port: 21, protocol: "tcp", service: "ftp", description: "FTP Control" },
        ServiceMapping { port: 22, protocol: "tcp", service: "ssh", description: "Secure Shell" },
        ServiceMapping { port: 23, protocol: "tcp", service: "telnet", description: "Telnet" },
        ServiceMapping { port: 25, protocol: "tcp", service: "smtp", description: "Simple Mail Transfer Protocol" },
        ServiceMapping { port: 37, protocol: "tcp", service: "time", description: "Time Protocol" },
        ServiceMapping { port: 42, protocol: "tcp", service: "nameserver", description: "Host Name Server" },
        ServiceMapping { port: 43, protocol: "tcp", service: "whois", description: "WHOIS" },
        ServiceMapping { port: 49, protocol: "tcp", service: "tacacs", description: "TACACS Login Host" },
        ServiceMapping { port: 53, protocol: "tcp", service: "dns", description: "Domain Name System" },
        ServiceMapping { port: 67, protocol: "udp", service: "dhcp-server", description: "DHCP Server" },
        ServiceMapping { port: 68, protocol: "udp", service: "dhcp-client", description: "DHCP Client" },
        ServiceMapping { port: 69, protocol: "udp", service: "tftp", description: "Trivial File Transfer" },
        ServiceMapping { port: 70, protocol: "tcp", service: "gopher", description: "Gopher Protocol" },
        ServiceMapping { port: 79, protocol: "tcp", service: "finger", description: "Finger Protocol" },
        ServiceMapping { port: 80, protocol: "tcp", service: "http", description: "HTTP" },
        ServiceMapping { port: 88, protocol: "tcp", service: "kerberos", description: "Kerberos Authentication" },
        ServiceMapping { port: 102, protocol: "tcp", service: "iso-tsap", description: "ISO-TSAP" },
        ServiceMapping { port: 104, protocol: "tcp", service: "dicom", description: "DICOM Medical Imaging" },
        ServiceMapping { port: 109, protocol: "tcp", service: "pop2", description: "POP Version 2" },
        ServiceMapping { port: 110, protocol: "tcp", service: "pop3", description: "POP Version 3" },
        ServiceMapping { port: 111, protocol: "tcp", service: "sunrpc", description: "Sun RPC / Portmapper" },
        ServiceMapping { port: 113, protocol: "tcp", service: "ident", description: "Identification Protocol" },
        ServiceMapping { port: 115, protocol: "tcp", service: "sftp", description: "Simple File Transfer" },
        ServiceMapping { port: 118, protocol: "tcp", service: "sqlserv", description: "SQL Services" },
        ServiceMapping { port: 119, protocol: "tcp", service: "nntp", description: "Network News Transfer" },
        ServiceMapping { port: 123, protocol: "udp", service: "ntp", description: "Network Time Protocol" },
        ServiceMapping { port: 135, protocol: "tcp", service: "msrpc", description: "Microsoft RPC" },
        ServiceMapping { port: 137, protocol: "udp", service: "netbios-ns", description: "NetBIOS Name Service" },
        ServiceMapping { port: 138, protocol: "udp", service: "netbios-dgm", description: "NetBIOS Datagram" },
        ServiceMapping { port: 139, protocol: "tcp", service: "netbios-ssn", description: "NetBIOS Session" },
        ServiceMapping { port: 143, protocol: "tcp", service: "imap", description: "IMAP" },
        ServiceMapping { port: 161, protocol: "udp", service: "snmp", description: "Simple Network Management" },
        ServiceMapping { port: 162, protocol: "udp", service: "snmp-trap", description: "SNMP Trap" },
        ServiceMapping { port: 177, protocol: "tcp", service: "xdmcp", description: "X Display Manager Control" },
        ServiceMapping { port: 179, protocol: "tcp", service: "bgp", description: "Border Gateway Protocol" },
        ServiceMapping { port: 194, protocol: "tcp", service: "irc", description: "Internet Relay Chat" },
        ServiceMapping { port: 201, protocol: "tcp", service: "at-rtmp", description: "AppleTalk Routing" },
        ServiceMapping { port: 209, protocol: "tcp", service: "qmtp", description: "Quick Mail Transfer" },
        ServiceMapping { port: 213, protocol: "tcp", service: "ipx", description: "IPX over IP" },
        ServiceMapping { port: 220, protocol: "tcp", service: "imap3", description: "IMAP Version 3" },
        ServiceMapping { port: 389, protocol: "tcp", service: "ldap", description: "Lightweight Directory Access" },
        ServiceMapping { port: 427, protocol: "tcp", service: "svrloc", description: "Service Location Protocol" },
        ServiceMapping { port: 443, protocol: "tcp", service: "https", description: "HTTP over TLS" },
        ServiceMapping { port: 445, protocol: "tcp", service: "smb", description: "Server Message Block" },
        ServiceMapping { port: 464, protocol: "tcp", service: "kpasswd", description: "Kerberos Password Change" },
        ServiceMapping { port: 465, protocol: "tcp", service: "smtps", description: "SMTP over TLS" },
        ServiceMapping { port: 500, protocol: "udp", service: "isakmp", description: "IPsec Key Exchange" },
        ServiceMapping { port: 502, protocol: "tcp", service: "modbus", description: "Modbus Protocol" },
        ServiceMapping { port: 514, protocol: "tcp", service: "syslog", description: "Syslog" },
        ServiceMapping { port: 515, protocol: "tcp", service: "lpd", description: "Line Printer Daemon" },
        ServiceMapping { port: 520, protocol: "udp", service: "rip", description: "Routing Information Protocol" },
        ServiceMapping { port: 521, protocol: "udp", service: "ripng", description: "RIPng for IPv6" },
        ServiceMapping { port: 530, protocol: "tcp", service: "rpc", description: "Remote Procedure Call" },
        ServiceMapping { port: 543, protocol: "tcp", service: "klogin", description: "Kerberos Login" },
        ServiceMapping { port: 544, protocol: "tcp", service: "kshell", description: "Kerberos Shell" },
        ServiceMapping { port: 546, protocol: "tcp", service: "dhcpv6-client", description: "DHCPv6 Client" },
        ServiceMapping { port: 547, protocol: "tcp", service: "dhcpv6-server", description: "DHCPv6 Server" },
        ServiceMapping { port: 548, protocol: "tcp", service: "afp", description: "Apple Filing Protocol" },
        ServiceMapping { port: 554, protocol: "tcp", service: "rtsp", description: "Real Time Streaming" },
        ServiceMapping { port: 587, protocol: "tcp", service: "submission", description: "Mail Submission" },
        ServiceMapping { port: 593, protocol: "tcp", service: "http-rpc", description: "HTTP RPC Endpoint Map" },
        ServiceMapping { port: 631, protocol: "tcp", service: "ipp", description: "Internet Printing Protocol" },
        ServiceMapping { port: 636, protocol: "tcp", service: "ldaps", description: "LDAP over TLS" },
        ServiceMapping { port: 639, protocol: "tcp", service: "msdp", description: "Multicast Source Discovery" },
        ServiceMapping { port: 646, protocol: "tcp", service: "ldp", description: "Label Distribution Protocol" },
        ServiceMapping { port: 691, protocol: "tcp", service: "msexch-routing", description: "MS Exchange Routing" },
        ServiceMapping { port: 860, protocol: "tcp", service: "iscsi", description: "iSCSI" },
        ServiceMapping { port: 873, protocol: "tcp", service: "rsync", description: "Rsync File Sync" },
        ServiceMapping { port: 902, protocol: "tcp", service: "vmware-auth", description: "VMware Auth Daemon" },
        ServiceMapping { port: 989, protocol: "tcp", service: "ftps-data", description: "FTPS Data" },
        ServiceMapping { port: 990, protocol: "tcp", service: "ftps", description: "FTPS Control" },
        ServiceMapping { port: 993, protocol: "tcp", service: "imaps", description: "IMAP over TLS" },
        ServiceMapping { port: 995, protocol: "tcp", service: "pop3s", description: "POP3 over TLS" },
        ServiceMapping { port: 1080, protocol: "tcp", service: "socks", description: "SOCKS Proxy" },
        ServiceMapping { port: 1194, protocol: "udp", service: "openvpn", description: "OpenVPN" },
        ServiceMapping { port: 1433, protocol: "tcp", service: "mssql", description: "Microsoft SQL Server" },
        ServiceMapping { port: 1434, protocol: "udp", service: "mssql-monitor", description: "MS SQL Monitor" },
        ServiceMapping { port: 1521, protocol: "tcp", service: "oracle", description: "Oracle Database" },
        ServiceMapping { port: 1701, protocol: "udp", service: "l2tp", description: "L2TP VPN" },
        ServiceMapping { port: 1723, protocol: "tcp", service: "pptp", description: "PPTP VPN" },
        ServiceMapping { port: 1812, protocol: "udp", service: "radius", description: "RADIUS Authentication" },
        ServiceMapping { port: 1813, protocol: "udp", service: "radius-acct", description: "RADIUS Accounting" },
        ServiceMapping { port: 1883, protocol: "tcp", service: "mqtt", description: "MQTT Messaging" },
        ServiceMapping { port: 1900, protocol: "udp", service: "ssdp", description: "SSDP / UPnP" },
        ServiceMapping { port: 2049, protocol: "tcp", service: "nfs", description: "Network File System" },
        ServiceMapping { port: 2082, protocol: "tcp", service: "cpanel", description: "cPanel" },
        ServiceMapping { port: 2083, protocol: "tcp", service: "cpanel-ssl", description: "cPanel SSL" },
        ServiceMapping { port: 2181, protocol: "tcp", service: "zookeeper", description: "Apache ZooKeeper" },
        ServiceMapping { port: 2375, protocol: "tcp", service: "docker", description: "Docker REST API" },
        ServiceMapping { port: 2376, protocol: "tcp", service: "docker-tls", description: "Docker TLS API" },
        ServiceMapping { port: 3306, protocol: "tcp", service: "mysql", description: "MySQL Database" },
        ServiceMapping { port: 3389, protocol: "tcp", service: "rdp", description: "Remote Desktop Protocol" },
        ServiceMapping { port: 3690, protocol: "tcp", service: "svn", description: "Subversion" },
        ServiceMapping { port: 4443, protocol: "tcp", service: "https-alt", description: "HTTPS Alternate" },
        ServiceMapping { port: 5060, protocol: "tcp", service: "sip", description: "Session Initiation Protocol" },
        ServiceMapping { port: 5222, protocol: "tcp", service: "xmpp", description: "XMPP Client" },
        ServiceMapping { port: 5269, protocol: "tcp", service: "xmpp-server", description: "XMPP Server" },
        ServiceMapping { port: 5432, protocol: "tcp", service: "postgresql", description: "PostgreSQL Database" },
        ServiceMapping { port: 5672, protocol: "tcp", service: "amqp", description: "RabbitMQ / AMQP" },
        ServiceMapping { port: 5900, protocol: "tcp", service: "vnc", description: "Virtual Network Computing" },
        ServiceMapping { port: 5984, protocol: "tcp", service: "couchdb", description: "CouchDB" },
        ServiceMapping { port: 6379, protocol: "tcp", service: "redis", description: "Redis" },
        ServiceMapping { port: 6443, protocol: "tcp", service: "k8s-api", description: "Kubernetes API Server" },
        ServiceMapping { port: 6667, protocol: "tcp", service: "irc", description: "IRC (alternate)" },
        ServiceMapping { port: 8080, protocol: "tcp", service: "http-alt", description: "HTTP Alternate" },
        ServiceMapping { port: 8443, protocol: "tcp", service: "https-alt", description: "HTTPS Alternate" },
        ServiceMapping { port: 8883, protocol: "tcp", service: "mqtt-tls", description: "MQTT over TLS" },
        ServiceMapping { port: 9090, protocol: "tcp", service: "prometheus", description: "Prometheus" },
        ServiceMapping { port: 9092, protocol: "tcp", service: "kafka", description: "Apache Kafka" },
        ServiceMapping { port: 9200, protocol: "tcp", service: "elasticsearch", description: "Elasticsearch HTTP" },
        ServiceMapping { port: 9300, protocol: "tcp", service: "elasticsearch-tp", description: "Elasticsearch Transport" },
        ServiceMapping { port: 9418, protocol: "tcp", service: "git", description: "Git Protocol" },
        ServiceMapping { port: 11211, protocol: "tcp", service: "memcached", description: "Memcached" },
        ServiceMapping { port: 27017, protocol: "tcp", service: "mongodb", description: "MongoDB" },
        ServiceMapping { port: 27018, protocol: "tcp", service: "mongodb-shard", description: "MongoDB Shard" },
        ServiceMapping { port: 50000, protocol: "tcp", service: "db2", description: "IBM DB2" },
    ]
}

/// Look up a service name by port number.
pub fn lookup_service(port: u16) -> Option<&'static str> {
    // Use a static-like approach; match on common ports for O(1) lookup of the
    // most frequently queried ports, falling back to the database for the rest.
    match port {
        20 => Some("ftp-data"),
        21 => Some("ftp"),
        22 => Some("ssh"),
        23 => Some("telnet"),
        25 => Some("smtp"),
        53 => Some("dns"),
        67 => Some("dhcp"),
        68 => Some("dhcp"),
        80 => Some("http"),
        88 => Some("kerberos"),
        110 => Some("pop3"),
        111 => Some("sunrpc"),
        119 => Some("nntp"),
        123 => Some("ntp"),
        135 => Some("msrpc"),
        137 => Some("netbios"),
        139 => Some("netbios"),
        143 => Some("imap"),
        161 => Some("snmp"),
        179 => Some("bgp"),
        389 => Some("ldap"),
        443 => Some("https"),
        445 => Some("smb"),
        465 => Some("smtps"),
        514 => Some("syslog"),
        515 => Some("lpd"),
        554 => Some("rtsp"),
        587 => Some("submission"),
        631 => Some("ipp"),
        636 => Some("ldaps"),
        873 => Some("rsync"),
        993 => Some("imaps"),
        995 => Some("pop3s"),
        1080 => Some("socks"),
        1194 => Some("openvpn"),
        1433 => Some("mssql"),
        1521 => Some("oracle"),
        1723 => Some("pptp"),
        1883 => Some("mqtt"),
        2049 => Some("nfs"),
        3306 => Some("mysql"),
        3389 => Some("rdp"),
        5060 => Some("sip"),
        5222 => Some("xmpp"),
        5432 => Some("postgresql"),
        5672 => Some("amqp"),
        5900 => Some("vnc"),
        6379 => Some("redis"),
        6443 => Some("k8s-api"),
        8080 => Some("http-alt"),
        8443 => Some("https-alt"),
        9090 => Some("prometheus"),
        9092 => Some("kafka"),
        9200 => Some("elasticsearch"),
        9418 => Some("git"),
        11211 => Some("memcached"),
        27017 => Some("mongodb"),
        _ => None,
    }
}

/// Common ports for quick scan profile.
pub fn quick_scan_ports() -> Vec<u16> {
    vec![
        21, 22, 23, 25, 53, 80, 110, 111, 135, 139, 143, 443, 445, 993, 995,
        1723, 3306, 3389, 5900, 8080,
    ]
}

/// Well-known ports (0-1023).
pub fn well_known_ports() -> Vec<u16> {
    let mut ports = Vec::with_capacity(1024);
    let mut p: u16 = 0;
    loop {
        ports.push(p);
        if p == 1023 { break; }
        p = p.saturating_add(1);
    }
    ports
}

// ============================================================================
// Scan Configuration
// ============================================================================

/// Scan profile presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanProfile {
    Quick,
    Full,
    Custom,
    Stealth,
}

impl ScanProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Quick => "Quick Scan",
            Self::Full => "Full Scan",
            Self::Custom => "Custom",
            Self::Stealth => "Stealth",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Quick => "Top 20 common ports",
            Self::Full => "All 65535 ports",
            Self::Custom => "User-defined port range",
            Self::Stealth => "SYN scan, randomized order, rate-limited",
        }
    }

    pub fn ports(self) -> Vec<u16> {
        match self {
            Self::Quick => quick_scan_ports(),
            Self::Full => {
                let mut ports = Vec::with_capacity(65535);
                let mut p: u16 = 1;
                loop {
                    ports.push(p);
                    if p == u16::MAX { break; }
                    p = p.saturating_add(1);
                }
                ports
            }
            Self::Custom => Vec::new(), // filled by user
            Self::Stealth => quick_scan_ports(),
        }
    }

    pub const ALL: [ScanProfile; 4] = [
        Self::Quick, Self::Full, Self::Custom, Self::Stealth,
    ];
}

/// Discovery method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryMethod {
    PingSweep,
    ArpDiscovery,
    TcpConnect,
}

impl DiscoveryMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::PingSweep => "Ping Sweep",
            Self::ArpDiscovery => "ARP Discovery",
            Self::TcpConnect => "TCP Connect",
        }
    }
}

/// Full scan configuration.
#[derive(Clone, Debug)]
pub struct ScanConfig {
    pub target_input: String,
    pub port_input: String,
    pub profile: ScanProfile,
    pub discovery_method: DiscoveryMethod,
    pub timeout_ms: u32,
    pub concurrency: u32,
    pub randomize_order: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            target_input: String::from("192.168.1.0/24"),
            port_input: String::new(),
            profile: ScanProfile::Quick,
            discovery_method: DiscoveryMethod::PingSweep,
            timeout_ms: 1000,
            concurrency: 100,
            randomize_order: false,
        }
    }
}

// ============================================================================
// Scan Results
// ============================================================================

/// State of a scanned port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortState {
    Open,
    Closed,
    Filtered,
}

impl PortState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Closed => "Closed",
            Self::Filtered => "Filtered",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Open => GREEN,
            Self::Closed => RED,
            Self::Filtered => YELLOW,
        }
    }
}

/// A single port scan result.
#[derive(Clone, Debug)]
pub struct PortResult {
    pub port: u16,
    pub state: PortState,
    pub service: Option<String>,
    pub banner: Option<String>,
    pub response_ms: f32,
}

/// OS fingerprint guess based on open ports and response characteristics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsGuess {
    Linux,
    Windows,
    MacOS,
    FreeBSD,
    Router,
    Printer,
    IoTDevice,
    Unknown,
}

impl OsGuess {
    pub fn label(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Windows => "Windows",
            Self::MacOS => "macOS",
            Self::FreeBSD => "FreeBSD",
            Self::Router => "Router/Switch",
            Self::Printer => "Printer",
            Self::IoTDevice => "IoT Device",
            Self::Unknown => "Unknown",
        }
    }
}

/// A discovered host with all its scan results.
#[derive(Clone, Debug)]
pub struct HostResult {
    pub ip: Ipv4Addr,
    pub hostname: Option<String>,
    pub mac: Option<MacAddr>,
    pub os_guess: OsGuess,
    pub ports: Vec<PortResult>,
    pub latency_ms: f32,
    pub is_up: bool,
    pub ttl: u8,
    pub vendor: Option<String>,
}

impl HostResult {
    pub fn open_port_count(&self) -> usize {
        self.ports.iter().filter(|p| p.state == PortState::Open).count()
    }

    pub fn display_hostname(&self) -> String {
        self.hostname.clone().unwrap_or_else(|| self.ip.display())
    }
}

/// A traceroute hop.
#[derive(Clone, Debug)]
pub struct TracerouteHop {
    pub hop_number: u8,
    pub ip: Option<Ipv4Addr>,
    pub hostname: Option<String>,
    pub rtt_ms: f32,
    pub timed_out: bool,
}

/// WHOIS information for an IP.
#[derive(Clone, Debug)]
pub struct WhoisInfo {
    pub ip: Ipv4Addr,
    pub org_name: String,
    pub country: String,
    pub cidr: String,
    pub net_name: String,
    pub description: String,
    pub abuse_contact: String,
}

/// A complete scan result snapshot.
#[derive(Clone, Debug)]
pub struct ScanResult {
    pub id: u64,
    pub timestamp: String,
    pub target_description: String,
    pub profile: ScanProfile,
    pub hosts: Vec<HostResult>,
    pub total_ips_scanned: u32,
    pub total_ports_scanned: u32,
    pub duration_secs: f32,
}

impl ScanResult {
    pub fn hosts_up(&self) -> usize {
        self.hosts.iter().filter(|h| h.is_up).count()
    }

    pub fn total_open_ports(&self) -> usize {
        self.hosts.iter().map(|h| h.open_port_count()).sum()
    }
}

// ============================================================================
// Scan Simulation Engine
// ============================================================================

/// Deterministic pseudo-random generator for reproducible simulation.
struct SimRng {
    state: u64,
}

impl SimRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        // xorshift64
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() % 10000) as f32 / 10000.0
    }

    fn next_range(&mut self, min: u32, max: u32) -> u32 {
        let range = max.saturating_sub(min).saturating_add(1);
        if range == 0 { return min; }
        let val = self.next_u64() as u32;
        min.saturating_add(val % range)
    }

    fn next_bool(&mut self, probability: f32) -> bool {
        self.next_f32() < probability
    }
}

/// Generate a simulated hostname for an IP.
fn simulated_hostname(ip: Ipv4Addr, rng: &mut SimRng) -> Option<String> {
    let prefixes = [
        "desktop", "laptop", "server", "printer", "nas", "router",
        "switch", "camera", "phone", "tablet", "tv", "iot",
    ];
    if rng.next_bool(0.7) {
        let idx = (rng.next_u64() as usize) % prefixes.len();
        let prefix = prefixes.get(idx).copied().unwrap_or("host");
        Some(format!("{}-{}", prefix, ip.octets[3]))
    } else {
        None
    }
}

/// Guess OS based on open ports.
fn guess_os(ports: &[PortResult]) -> OsGuess {
    let open_ports: Vec<u16> = ports.iter()
        .filter(|p| p.state == PortState::Open)
        .map(|p| p.port)
        .collect();

    if open_ports.contains(&135) || open_ports.contains(&445) || open_ports.contains(&3389) {
        return OsGuess::Windows;
    }
    if open_ports.contains(&548) {
        return OsGuess::MacOS;
    }
    if open_ports.contains(&631) && open_ports.contains(&9100) {
        return OsGuess::Printer;
    }
    if open_ports.contains(&179) || (open_ports.contains(&23) && open_ports.len() <= 3) {
        return OsGuess::Router;
    }
    if open_ports.contains(&1883) || open_ports.contains(&8883) {
        return OsGuess::IoTDevice;
    }
    if open_ports.contains(&22) || open_ports.contains(&111) {
        return OsGuess::Linux;
    }
    if open_ports.is_empty() {
        return OsGuess::Unknown;
    }
    OsGuess::Linux
}

/// Guess a vendor from a MAC address (simulated OUI lookup).
fn guess_vendor(mac: &MacAddr) -> Option<String> {
    let oui = [mac.bytes[0], mac.bytes[1], mac.bytes[2]];
    match oui {
        [0x00, 0x1A, _] => Some("Cisco Systems".to_string()),
        [0x00, 0x50, 0x56] => Some("VMware".to_string()),
        [0x08, 0x00, 0x27] => Some("Oracle VirtualBox".to_string()),
        [0xDC, 0xA6, 0x32] => Some("Raspberry Pi".to_string()),
        [0xB8, 0x27, 0xEB] => Some("Raspberry Pi".to_string()),
        [0x00, 0x0C, 0x29] => Some("VMware".to_string()),
        [0x00, 0x15, 0x5D] => Some("Microsoft Hyper-V".to_string()),
        [0x00, 0x16, 0x3E] => Some("Xen Virtual".to_string()),
        _ => None,
    }
}

/// Simulate scanning a single host.
fn simulate_host_scan(ip: Ipv4Addr, scan_ports: &[u16], rng: &mut SimRng) -> Option<HostResult> {
    // Determine if host is "up" — give ~60% probability for simulated network
    let is_up = rng.next_bool(0.6);
    if !is_up {
        return None;
    }

    let latency = 0.5 + rng.next_f32() * 50.0;
    let mac = MacAddr::from_ip_simulated(ip);
    let hostname = simulated_hostname(ip, rng);
    let ttl_val = if rng.next_bool(0.5) { 64u8 } else { 128u8 };
    let vendor = guess_vendor(&mac);

    let mut ports = Vec::new();
    for port_ref in scan_ports {
        let port_val = *port_ref;
        // Probability of port being open depends on whether it is a "common" port
        let open_prob = match port_val {
            22 | 80 | 443 => 0.5,
            21 | 25 | 53 | 110 | 143 | 993 | 995 => 0.3,
            135 | 139 | 445 | 3389 => 0.25,
            3306 | 5432 | 6379 | 27017 => 0.15,
            8080 | 8443 | 9090 => 0.2,
            _ => 0.05,
        };

        let state = if rng.next_bool(open_prob) {
            PortState::Open
        } else if rng.next_bool(0.1) {
            PortState::Filtered
        } else {
            PortState::Closed
        };

        if state == PortState::Open || state == PortState::Filtered {
            let service = lookup_service(port_val).map(|s| s.to_string());
            let banner = if state == PortState::Open && rng.next_bool(0.4) {
                Some(simulated_banner(port_val))
            } else {
                None
            };
            let response = latency + rng.next_f32() * 10.0;
            ports.push(PortResult {
                port: port_val,
                state,
                service,
                banner,
                response_ms: response,
            });
        }
    }

    let os_guess = guess_os(&ports);

    Some(HostResult {
        ip,
        hostname,
        mac: Some(mac),
        os_guess,
        ports,
        latency_ms: latency,
        is_up: true,
        ttl: ttl_val,
        vendor,
    })
}

/// Generate a simulated banner for a port.
fn simulated_banner(port: u16) -> String {
    match port {
        22 => "SSH-2.0-OpenSSH_9.6".to_string(),
        21 => "220 Welcome to FTP server".to_string(),
        25 => "220 mail.example.com ESMTP".to_string(),
        80 => "HTTP/1.1 200 OK\r\nServer: nginx/1.24".to_string(),
        110 => "+OK POP3 server ready".to_string(),
        143 => "* OK IMAP server ready".to_string(),
        443 => "TLS 1.3 / HTTP/2".to_string(),
        3306 => "5.7.42-MySQL Community Server".to_string(),
        5432 => "PostgreSQL 16.1".to_string(),
        6379 => "Redis v7.2.4".to_string(),
        8080 => "HTTP/1.1 200 OK\r\nServer: Apache-Coyote".to_string(),
        _ => format!("Service on port {}", port),
    }
}

/// Simulate a traceroute to a destination IP.
fn simulate_traceroute(dest: Ipv4Addr) -> Vec<TracerouteHop> {
    let mut rng = SimRng::new(dest.to_u32() as u64);
    let hop_count = rng.next_range(4, 12) as u8;
    let mut hops = Vec::new();

    let mut hop_num: u8 = 1;
    while hop_num <= hop_count {
        let timed_out = rng.next_bool(0.1);
        if timed_out {
            hops.push(TracerouteHop {
                hop_number: hop_num,
                ip: None,
                hostname: None,
                rtt_ms: 0.0,
                timed_out: true,
            });
        } else {
            let hop_ip = Ipv4Addr::new(
                10u8.saturating_add((rng.next_range(0, 245)) as u8),
                (rng.next_range(0, 255)) as u8,
                (rng.next_range(0, 255)) as u8,
                (rng.next_range(1, 254)) as u8,
            );
            let rtt = (hop_num as f32) * 2.5 + rng.next_f32() * 15.0;
            let hostname = if rng.next_bool(0.5) {
                Some(format!("hop-{}.isp.net", hop_num))
            } else {
                None
            };
            hops.push(TracerouteHop {
                hop_number: hop_num,
                ip: Some(hop_ip),
                hostname,
                rtt_ms: rtt,
                timed_out: false,
            });
        }
        if hop_num == u8::MAX { break; }
        hop_num = hop_num.saturating_add(1);
    }

    // Final hop is the destination itself
    hops.push(TracerouteHop {
        hop_number: hop_num,
        ip: Some(dest),
        hostname: None,
        rtt_ms: (hop_count as f32) * 3.0 + rng.next_f32() * 20.0,
        timed_out: false,
    });

    hops
}

/// Simulate WHOIS lookup for a public IP.
fn simulate_whois(ip: Ipv4Addr) -> WhoisInfo {
    let octet0 = ip.octets[0];
    let (org, country, net) = if octet0 < 100 {
        ("ARIN Regional Registry", "US", "NET-BLOCK-A")
    } else if octet0 < 150 {
        ("RIPE Network Coordination Centre", "EU", "EU-NET-BLOCK")
    } else if octet0 < 200 {
        ("APNIC Regional Registry", "AU", "APNIC-BLOCK")
    } else {
        ("LACNIC Regional Registry", "BR", "LACNIC-BLOCK")
    };

    WhoisInfo {
        ip,
        org_name: org.to_string(),
        country: country.to_string(),
        cidr: format!("{}.0.0.0/8", octet0),
        net_name: net.to_string(),
        description: format!("Network block for {}.x.x.x", octet0),
        abuse_contact: format!("abuse@registry-{}.example.com", octet0),
    }
}

/// Estimate scan duration based on target range and port count.
pub fn estimate_scan_time(host_count: u32, port_count: u32, timeout_ms: u32, concurrency: u32) -> f32 {
    if concurrency == 0 { return 0.0; }
    let total_probes = (host_count as u64).saturating_mul(port_count as u64);
    let batches = total_probes.saturating_add(concurrency as u64 - 1) / (concurrency as u64);
    let time_per_batch_ms = timeout_ms as f32 * 0.3; // average case: 30% of timeout
    (batches as f32 * time_per_batch_ms) / 1000.0
}

/// Generate a Wake-on-LAN magic packet for a MAC address.
pub fn build_wol_packet(mac: &MacAddr) -> Vec<u8> {
    let mut packet = Vec::with_capacity(102);
    // 6 bytes of 0xFF
    for _ in 0..6 {
        packet.push(0xFF);
    }
    // 16 repetitions of the MAC address
    for _ in 0..16 {
        for byte in &mac.bytes {
            packet.push(*byte);
        }
    }
    packet
}

// ============================================================================
// Export Utilities
// ============================================================================

/// Export scan results to CSV format.
pub fn export_csv(result: &ScanResult) -> String {
    let mut csv = String::from("IP,Hostname,MAC,OS,Status,Latency(ms),Open Ports,Services\n");
    for host in &result.hosts {
        let hostname = host.hostname.clone().unwrap_or_default();
        let mac_str = host.mac.map(|m| m.display()).unwrap_or_default();
        let open_ports: Vec<String> = host.ports.iter()
            .filter(|p| p.state == PortState::Open)
            .map(|p| p.port.to_string())
            .collect();
        let services: Vec<String> = host.ports.iter()
            .filter(|p| p.state == PortState::Open)
            .filter_map(|p| p.service.clone())
            .collect();
        csv.push_str(&format!(
            "{},{},{},{},{},{:.1},\"{}\",\"{}\"\n",
            host.ip.display(),
            hostname,
            mac_str,
            host.os_guess.label(),
            if host.is_up { "Up" } else { "Down" },
            host.latency_ms,
            open_ports.join(";"),
            services.join(";"),
        ));
    }
    csv
}

/// Export scan results to JSON format.
pub fn export_json(result: &ScanResult) -> String {
    let mut json = String::from("{\n");
    json.push_str(&format!("  \"scan_id\": {},\n", result.id));
    json.push_str(&format!("  \"timestamp\": \"{}\",\n", result.timestamp));
    json.push_str(&format!("  \"target\": \"{}\",\n", result.target_description));
    json.push_str(&format!("  \"profile\": \"{}\",\n", result.profile.label()));
    json.push_str(&format!("  \"duration_secs\": {:.1},\n", result.duration_secs));
    json.push_str(&format!("  \"total_ips\": {},\n", result.total_ips_scanned));
    json.push_str(&format!("  \"total_ports_scanned\": {},\n", result.total_ports_scanned));
    json.push_str("  \"hosts\": [\n");
    for (i, host) in result.hosts.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str(&format!("      \"ip\": \"{}\",\n", host.ip.display()));
        if let Some(ref hn) = host.hostname {
            json.push_str(&format!("      \"hostname\": \"{}\",\n", hn));
        }
        if let Some(mac) = host.mac {
            json.push_str(&format!("      \"mac\": \"{}\",\n", mac.display()));
        }
        json.push_str(&format!("      \"os\": \"{}\",\n", host.os_guess.label()));
        json.push_str(&format!("      \"is_up\": {},\n", host.is_up));
        json.push_str(&format!("      \"latency_ms\": {:.1},\n", host.latency_ms));
        json.push_str(&format!("      \"ttl\": {},\n", host.ttl));
        json.push_str("      \"ports\": [\n");
        for (j, port) in host.ports.iter().enumerate() {
            json.push_str("        {\n");
            json.push_str(&format!("          \"port\": {},\n", port.port));
            json.push_str(&format!("          \"state\": \"{}\",\n", port.state.label()));
            if let Some(ref svc) = port.service {
                json.push_str(&format!("          \"service\": \"{}\",\n", svc));
            }
            if let Some(ref banner) = port.banner {
                let escaped = banner.replace('\"', "\\\"").replace('\r', "\\r").replace('\n', "\\n");
                json.push_str(&format!("          \"banner\": \"{}\",\n", escaped));
            }
            json.push_str(&format!("          \"response_ms\": {:.1}\n", port.response_ms));
            if i < result.hosts.len().saturating_sub(1) || j < host.ports.len().saturating_sub(1) {
                json.push_str("        },\n");
            } else {
                json.push_str("        }\n");
            }
        }
        json.push_str("      ]\n");
        if i < result.hosts.len().saturating_sub(1) {
            json.push_str("    },\n");
        } else {
            json.push_str("    }\n");
        }
    }
    json.push_str("  ]\n");
    json.push_str("}\n");
    json
}

/// Compare two scan results, returning new and missing hosts.
pub fn diff_scans(old: &ScanResult, new: &ScanResult) -> (Vec<Ipv4Addr>, Vec<Ipv4Addr>) {
    let old_ips: Vec<Ipv4Addr> = old.hosts.iter()
        .filter(|h| h.is_up)
        .map(|h| h.ip)
        .collect();
    let new_ips: Vec<Ipv4Addr> = new.hosts.iter()
        .filter(|h| h.is_up)
        .map(|h| h.ip)
        .collect();

    let added: Vec<Ipv4Addr> = new_ips.iter()
        .filter(|ip| !old_ips.contains(ip))
        .copied()
        .collect();
    let removed: Vec<Ipv4Addr> = old_ips.iter()
        .filter(|ip| !new_ips.contains(ip))
        .copied()
        .collect();

    (added, removed)
}

// ============================================================================
// Application View Tabs
// ============================================================================

/// Top-level view tab in the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewTab {
    Results,
    Topology,
    History,
    Traceroute,
    Whois,
}

impl ViewTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Results => "Results",
            Self::Topology => "Topology",
            Self::History => "History",
            Self::Traceroute => "Traceroute",
            Self::Whois => "WHOIS",
        }
    }

    pub const ALL: [ViewTab; 5] = [
        Self::Results, Self::Topology, Self::History, Self::Traceroute, Self::Whois,
    ];
}

// ============================================================================
// Scan Progress
// ============================================================================

/// Progress tracking for an active scan.
#[derive(Clone, Debug)]
pub struct ScanProgress {
    pub phase: ScanPhase,
    pub hosts_scanned: u32,
    pub total_hosts: u32,
    pub ports_scanned: u32,
    pub total_ports: u32,
    pub hosts_found: u32,
    pub elapsed_secs: f32,
}

impl ScanProgress {
    pub fn fraction(&self) -> f32 {
        let total = self.total_hosts.saturating_mul(self.total_ports.max(1));
        if total == 0 { return 0.0; }
        let done = self.hosts_scanned.saturating_mul(self.total_ports.max(1))
            .saturating_add(self.ports_scanned);
        (done as f32 / total as f32).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanPhase {
    Discovery,
    PortScan,
    ServiceDetection,
    Complete,
}

impl ScanPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discovery => "Host Discovery",
            Self::PortScan => "Port Scanning",
            Self::ServiceDetection => "Service Detection",
            Self::Complete => "Complete",
        }
    }
}

// ============================================================================
// Application State
// ============================================================================

/// The full application state.
pub struct NetScanApp {
    pub config: ScanConfig,
    pub active_tab: ViewTab,
    pub profile_tab_idx: usize,
    pub results: Option<ScanResult>,
    pub history: VecDeque<ScanResult>,
    pub selected_host_idx: Option<usize>,
    pub scan_progress: Option<ScanProgress>,
    pub is_scanning: bool,
    pub scroll_offset: f32,
    pub sidebar_scroll: f32,
    pub traceroute_target: String,
    pub traceroute_result: Option<Vec<TracerouteHop>>,
    pub whois_target: String,
    pub whois_result: Option<WhoisInfo>,
    pub wol_target_mac: String,
    pub wol_sent: bool,
    pub show_export_menu: bool,
    pub history_selected_idx: Option<usize>,
    pub history_compare_idx: Option<usize>,
    pub scan_id_counter: u64,
    pub topology_zoom: f32,
    pub detail_port_scroll: f32,
    pub config_field_focus: usize,
}

impl Default for NetScanApp {
    fn default() -> Self {
        Self {
            config: ScanConfig::default(),
            active_tab: ViewTab::Results,
            profile_tab_idx: 0,
            results: None,
            history: VecDeque::new(),
            selected_host_idx: None,
            scan_progress: None,
            is_scanning: false,
            scroll_offset: 0.0,
            sidebar_scroll: 0.0,
            traceroute_target: String::from("8.8.8.8"),
            traceroute_result: None,
            whois_target: String::from("8.8.8.8"),
            whois_result: None,
            wol_target_mac: String::new(),
            wol_sent: false,
            show_export_menu: false,
            history_selected_idx: None,
            history_compare_idx: None,
            scan_id_counter: 1,
            topology_zoom: 1.0,
            detail_port_scroll: 0.0,
            config_field_focus: 0,
        }
    }
}

impl NetScanApp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a simulated scan with the current configuration.
    pub fn start_scan(&mut self) {
        if self.is_scanning { return; }

        let target = match ScanTarget::parse(&self.config.target_input) {
            Some(t) => t,
            None => return, // Invalid target input
        };

        let ips = target.all_ips();
        if ips.is_empty() { return; }

        let ports = match self.config.profile {
            ScanProfile::Custom => {
                parse_port_spec(&self.config.port_input)
                    .unwrap_or_else(|| quick_scan_ports())
            }
            other => other.ports(),
        };

        // Limit the number of ports to keep simulation fast
        let scan_ports: Vec<u16> = if ports.len() > 1024 {
            ports.into_iter().take(1024).collect()
        } else {
            ports
        };

        let mut rng = SimRng::new(ips.len() as u64 * 31 + scan_ports.len() as u64 * 17);
        let mut hosts = Vec::new();

        for ip in &ips {
            if let Some(host) = simulate_host_scan(*ip, &scan_ports, &mut rng) {
                hosts.push(host);
            }
            if hosts.len() >= MAX_HOSTS_DISPLAY { break; }
        }

        let id = self.scan_id_counter;
        self.scan_id_counter = self.scan_id_counter.saturating_add(1);
        let total_ips = ips.len() as u32;
        let total_ports = scan_ports.len() as u32;
        let est_time = estimate_scan_time(total_ips, total_ports, self.config.timeout_ms, self.config.concurrency);

        let result = ScanResult {
            id,
            timestamp: format!("2026-05-18 12:{:02}:{:02}", (id * 7) % 60, (id * 13) % 60),
            target_description: self.config.target_input.clone(),
            profile: self.config.profile,
            hosts,
            total_ips_scanned: total_ips,
            total_ports_scanned: total_ports,
            duration_secs: est_time,
        };

        // Push to history
        if self.history.len() >= MAX_HISTORY_ENTRIES {
            self.history.pop_back();
        }
        self.history.push_front(result.clone());
        self.results = Some(result);
        self.selected_host_idx = None;
        self.scroll_offset = 0.0;
        self.is_scanning = false;
    }

    /// Run traceroute to the configured target.
    pub fn run_traceroute(&mut self) {
        if let Some(ip) = Ipv4Addr::parse(&self.traceroute_target) {
            self.traceroute_result = Some(simulate_traceroute(ip));
        }
    }

    /// Run WHOIS lookup for the configured target.
    pub fn run_whois(&mut self) {
        if let Some(ip) = Ipv4Addr::parse(&self.whois_target) {
            self.whois_result = Some(simulate_whois(ip));
        }
    }

    /// Send Wake-on-LAN to the entered MAC address.
    pub fn send_wol(&mut self) {
        if let Some(mac) = parse_mac(&self.wol_target_mac) {
            let _packet = build_wol_packet(&mac);
            // In real OS: send via UDP broadcast on port 9
            self.wol_sent = true;
        }
    }

    /// Get the currently selected host, if any.
    pub fn selected_host(&self) -> Option<&HostResult> {
        let idx = self.selected_host_idx?;
        let result = self.results.as_ref()?;
        result.hosts.get(idx)
    }

    /// Handle keyboard events.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) if key.pressed => {
                self.handle_key(key)
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse)
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Ctrl+Enter starts scan
        if key.modifiers.ctrl && key.key == Key::Enter {
            self.start_scan();
            return EventResult::Consumed;
        }

        match key.key {
            Key::F5 => {
                self.start_scan();
                EventResult::Consumed
            }
            Key::Tab if key.modifiers.ctrl => {
                // Cycle tabs
                let tabs = ViewTab::ALL;
                let current_idx = tabs.iter()
                    .position(|t| *t == self.active_tab)
                    .unwrap_or(0);
                let next = (current_idx.saturating_add(1)) % tabs.len();
                self.active_tab = tabs.get(next).copied().unwrap_or(ViewTab::Results);
                EventResult::Consumed
            }
            Key::Escape => {
                self.show_export_menu = false;
                self.selected_host_idx = None;
                EventResult::Consumed
            }
            Key::Up => {
                if let Some(ref idx) = self.selected_host_idx {
                    if *idx > 0 {
                        self.selected_host_idx = Some(idx.saturating_sub(1));
                    }
                }
                EventResult::Consumed
            }
            Key::Down => {
                if let Some(ref result) = self.results {
                    let max_idx = result.hosts.len().saturating_sub(1);
                    match self.selected_host_idx {
                        Some(idx) if idx < max_idx => {
                            self.selected_host_idx = Some(idx.saturating_add(1));
                        }
                        None if !result.hosts.is_empty() => {
                            self.selected_host_idx = Some(0);
                        }
                        _ => {}
                    }
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, mouse: &guitk::event::MouseEvent) -> EventResult {
        let mx = mouse.x;
        let my = mouse.y;

        match &mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                // Check tab clicks
                let tab_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + PADDING;
                if my >= tab_y && my <= tab_y + TAB_HEIGHT {
                    let mut tab_x = PADDING;
                    for tab in &ViewTab::ALL {
                        let tab_w = tab.label().len() as f32 * 8.0 + 24.0;
                        if mx >= tab_x && mx <= tab_x + tab_w {
                            self.active_tab = *tab;
                            return EventResult::Consumed;
                        }
                        tab_x += tab_w + 4.0;
                    }
                }

                // Check profile button clicks
                let profile_y = TITLE_BAR_HEIGHT + PADDING + 32.0;
                if my >= profile_y && my <= profile_y + BUTTON_HEIGHT {
                    let mut px = PADDING;
                    for (i, profile) in ScanProfile::ALL.iter().enumerate() {
                        let pw = profile.label().len() as f32 * 8.0 + 20.0;
                        if mx >= px && mx <= px + pw {
                            self.config.profile = *profile;
                            self.profile_tab_idx = i;
                            return EventResult::Consumed;
                        }
                        px += pw + 6.0;
                    }
                }

                // Check scan button
                let scan_btn_x = WINDOW_WIDTH - 150.0 - PADDING;
                let scan_btn_y = TITLE_BAR_HEIGHT + PADDING;
                if mx >= scan_btn_x && mx <= scan_btn_x + 150.0
                    && my >= scan_btn_y && my <= scan_btn_y + BUTTON_HEIGHT
                {
                    self.start_scan();
                    return EventResult::Consumed;
                }

                // Check host row clicks in results
                if self.active_tab == ViewTab::Results {
                    let table_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + PADDING + TAB_HEIGHT + PADDING + TABLE_HEADER_HEIGHT;
                    if my >= table_y && mx < WINDOW_WIDTH - SIDEBAR_WIDTH {
                        let row_idx = ((my - table_y) / TABLE_ROW_HEIGHT) as usize;
                        if let Some(ref result) = self.results {
                            if row_idx < result.hosts.len() {
                                self.selected_host_idx = Some(row_idx);
                                self.detail_port_scroll = 0.0;
                                return EventResult::Consumed;
                            }
                        }
                    }
                }

                // Check export button
                if self.active_tab == ViewTab::Results && self.results.is_some() {
                    let export_x = WINDOW_WIDTH - SIDEBAR_WIDTH + PADDING;
                    let export_y = WINDOW_HEIGHT - 50.0;
                    if mx >= export_x && mx <= export_x + 120.0
                        && my >= export_y && my <= export_y + BUTTON_HEIGHT
                    {
                        self.show_export_menu = !self.show_export_menu;
                        return EventResult::Consumed;
                    }
                }

                // Traceroute run button
                if self.active_tab == ViewTab::Traceroute {
                    let btn_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + TAB_HEIGHT + PADDING * 3.0 + INPUT_HEIGHT;
                    if my >= btn_y && my <= btn_y + BUTTON_HEIGHT
                        && mx >= PADDING && mx <= PADDING + 120.0
                    {
                        self.run_traceroute();
                        return EventResult::Consumed;
                    }
                }

                // WHOIS run button
                if self.active_tab == ViewTab::Whois {
                    let btn_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + TAB_HEIGHT + PADDING * 3.0 + INPUT_HEIGHT;
                    if my >= btn_y && my <= btn_y + BUTTON_HEIGHT
                        && mx >= PADDING && mx <= PADDING + 120.0
                    {
                        self.run_whois();
                        return EventResult::Consumed;
                    }
                }

                // WOL button
                if self.active_tab == ViewTab::Results {
                    let wol_btn_x = WINDOW_WIDTH - SIDEBAR_WIDTH + PADDING;
                    let wol_btn_y = WINDOW_HEIGHT - 90.0;
                    if mx >= wol_btn_x && mx <= wol_btn_x + 120.0
                        && my >= wol_btn_y && my <= wol_btn_y + BUTTON_HEIGHT
                    {
                        self.send_wol();
                        return EventResult::Consumed;
                    }
                }

                EventResult::Ignored
            }
            MouseEventKind::Scroll { dy, .. } => {
                if self.active_tab == ViewTab::Results {
                    if mx < WINDOW_WIDTH - SIDEBAR_WIDTH {
                        self.scroll_offset = (self.scroll_offset - dy * 20.0).max(0.0);
                    } else {
                        self.detail_port_scroll = (self.detail_port_scroll - dy * 20.0).max(0.0);
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire application into a render tree.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Full-window background
        tree.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0,
            width: WINDOW_WIDTH, height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_title_bar(&mut tree);
        self.render_config_panel(&mut tree);
        self.render_tabs(&mut tree);

        match self.active_tab {
            ViewTab::Results => self.render_results_view(&mut tree),
            ViewTab::Topology => self.render_topology_view(&mut tree),
            ViewTab::History => self.render_history_view(&mut tree),
            ViewTab::Traceroute => self.render_traceroute_view(&mut tree),
            ViewTab::Whois => self.render_whois_view(&mut tree),
        }

        if let Some(ref progress) = self.scan_progress {
            self.render_progress_bar(&mut tree, progress);
        }

        tree
    }

    fn render_title_bar(&self, tree: &mut RenderTree) {
        // Title bar background
        tree.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0,
            width: WINDOW_WIDTH, height: TITLE_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // App icon placeholder (radar circle)
        let icon_cx = 22.0;
        let icon_cy = TITLE_BAR_HEIGHT / 2.0;
        tree.push(RenderCommand::FillRect {
            x: icon_cx - 8.0, y: icon_cy - 8.0,
            width: 16.0, height: 16.0,
            color: BLUE,
            corner_radii: CornerRadii::all(8.0),
        });
        tree.push(RenderCommand::FillRect {
            x: icon_cx - 3.0, y: icon_cy - 3.0,
            width: 6.0, height: 6.0,
            color: GREEN,
            corner_radii: CornerRadii::all(3.0),
        });

        // Title text
        tree.push(RenderCommand::Text {
            x: 40.0, y: 10.0,
            text: "Network Scanner".to_string(),
            color: TEXT_COLOR,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status indicator
        let status_text = if self.is_scanning {
            "Scanning..."
        } else if self.results.is_some() {
            "Ready"
        } else {
            "Idle"
        };
        let status_color = if self.is_scanning { YELLOW } else { GREEN };
        tree.push(RenderCommand::FillRect {
            x: WINDOW_WIDTH - 200.0, y: TITLE_BAR_HEIGHT / 2.0 - 4.0,
            width: 8.0, height: 8.0,
            color: status_color,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 188.0, y: 12.0,
            text: status_text.to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_config_panel(&self, tree: &mut RenderTree) {
        let y = TITLE_BAR_HEIGHT;

        // Panel background
        tree.push(RenderCommand::FillRect {
            x: 0.0, y,
            width: WINDOW_WIDTH, height: CONFIG_PANEL_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Section label
        tree.push(RenderCommand::Text {
            x: PADDING, y: y + 8.0,
            text: "Scan Configuration".to_string(),
            color: LAVENDER,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Profile buttons
        let profile_y = y + 28.0;
        let mut px = PADDING;
        for (i, profile) in ScanProfile::ALL.iter().enumerate() {
            let pw = profile.label().len() as f32 * 8.0 + 20.0;
            let is_selected = i == self.profile_tab_idx;
            let bg = if is_selected { BLUE } else { SURFACE0 };
            let fg = if is_selected { CRUST } else { TEXT_COLOR };
            tree.push(RenderCommand::FillRect {
                x: px, y: profile_y,
                width: pw, height: BUTTON_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            tree.push(RenderCommand::Text {
                x: px + 10.0, y: profile_y + 8.0,
                text: profile.label().to_string(),
                color: fg,
                font_size: 12.0,
                font_weight: if is_selected { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: None,
            });
            px += pw + 6.0;
        }

        // Target input
        let input_y = profile_y + BUTTON_HEIGHT + 10.0;
        tree.push(RenderCommand::Text {
            x: PADDING, y: input_y,
            text: "Target:".to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        self.render_text_field(tree, PADDING + 60.0, input_y - 2.0, 250.0, &self.config.target_input, "192.168.1.0/24");

        // Port input (shown for custom profile)
        if self.config.profile == ScanProfile::Custom {
            tree.push(RenderCommand::Text {
                x: PADDING + 330.0, y: input_y,
                text: "Ports:".to_string(),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            self.render_text_field(tree, PADDING + 380.0, input_y - 2.0, 200.0, &self.config.port_input, "80,443,8080");
        }

        // Discovery method label
        let method_y = input_y + INPUT_HEIGHT + 6.0;
        tree.push(RenderCommand::Text {
            x: PADDING, y: method_y,
            text: format!("Discovery: {}  |  Timeout: {}ms  |  Concurrency: {}",
                self.config.discovery_method.label(),
                self.config.timeout_ms,
                self.config.concurrency,
            ),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Bandwidth estimation
        if let Some(target) = ScanTarget::parse(&self.config.target_input) {
            let host_count = target.all_ips().len() as u32;
            let port_count = match self.config.profile {
                ScanProfile::Quick => 20u32,
                ScanProfile::Full => 65535,
                ScanProfile::Stealth => 20,
                ScanProfile::Custom => {
                    parse_port_spec(&self.config.port_input)
                        .map(|p| p.len() as u32)
                        .unwrap_or(20)
                }
            };
            let est = estimate_scan_time(host_count, port_count, self.config.timeout_ms, self.config.concurrency);
            tree.push(RenderCommand::Text {
                x: PADDING + 450.0, y: method_y,
                text: format!("Est. time: {:.1}s ({} hosts x {} ports)", est, host_count, port_count),
                color: TEAL,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Scan button
        let scan_btn_x = WINDOW_WIDTH - 150.0 - PADDING;
        let scan_btn_y = y + PADDING;
        let btn_color = if self.is_scanning { SURFACE1 } else { GREEN };
        let btn_text_color = if self.is_scanning { SUBTEXT0 } else { CRUST };
        tree.push(RenderCommand::FillRect {
            x: scan_btn_x, y: scan_btn_y,
            width: 150.0, height: BUTTON_HEIGHT,
            color: btn_color,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        tree.push(RenderCommand::Text {
            x: scan_btn_x + 30.0, y: scan_btn_y + 9.0,
            text: if self.is_scanning { "Scanning..." } else { "Start Scan (F5)" }.to_string(),
            color: btn_text_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_text_field(&self, tree: &mut RenderTree, x: f32, y: f32, width: f32, value: &str, placeholder: &str) {
        tree.push(RenderCommand::FillRect {
            x, y,
            width, height: INPUT_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        tree.push(RenderCommand::StrokeRect {
            x, y,
            width, height: INPUT_HEIGHT,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        let display = if value.is_empty() { placeholder } else { value };
        let color = if value.is_empty() { OVERLAY0 } else { TEXT_COLOR };
        tree.push(RenderCommand::Text {
            x: x + 8.0, y: y + 7.0,
            text: display.to_string(),
            color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 16.0),
        });
    }

    fn render_tabs(&self, tree: &mut RenderTree) {
        let tab_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + PADDING;

        // Tab bar background
        tree.push(RenderCommand::FillRect {
            x: 0.0, y: tab_y - 2.0,
            width: WINDOW_WIDTH, height: TAB_HEIGHT + 4.0,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tab_x = PADDING;
        for tab in &ViewTab::ALL {
            let tw = tab.label().len() as f32 * 8.0 + 24.0;
            let is_active = *tab == self.active_tab;
            let bg = if is_active { SURFACE0 } else { Color::TRANSPARENT };
            let fg = if is_active { BLUE } else { SUBTEXT0 };

            tree.push(RenderCommand::FillRect {
                x: tab_x, y: tab_y,
                width: tw, height: TAB_HEIGHT,
                color: bg,
                corner_radii: CornerRadii {
                    top_left: SMALL_RADIUS,
                    top_right: SMALL_RADIUS,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            if is_active {
                // Active tab underline
                tree.push(RenderCommand::FillRect {
                    x: tab_x, y: tab_y + TAB_HEIGHT - 2.0,
                    width: tw, height: 2.0,
                    color: BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            tree.push(RenderCommand::Text {
                x: tab_x + 12.0, y: tab_y + 8.0,
                text: tab.label().to_string(),
                color: fg,
                font_size: 12.0,
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: None,
            });

            tab_x += tw + 4.0;
        }
    }

    fn render_results_view(&self, tree: &mut RenderTree) {
        let content_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + PADDING + TAB_HEIGHT + PADDING;
        let table_width = WINDOW_WIDTH - SIDEBAR_WIDTH;

        // Results summary bar
        if let Some(ref result) = self.results {
            self.render_summary_bar(tree, content_y, result);
        }

        let table_top = content_y + 26.0;

        // Table header
        self.render_table_header(tree, 0.0, table_top, table_width);

        // Table rows
        let rows_y = table_top + TABLE_HEADER_HEIGHT;
        if let Some(ref result) = self.results {
            tree.push(RenderCommand::PushClip {
                x: 0.0, y: rows_y,
                width: table_width,
                height: WINDOW_HEIGHT - rows_y,
            });

            for (i, host) in result.hosts.iter().enumerate() {
                let row_y = rows_y + (i as f32) * TABLE_ROW_HEIGHT - self.scroll_offset;
                if row_y + TABLE_ROW_HEIGHT < rows_y { continue; }
                if row_y > WINDOW_HEIGHT { break; }

                let is_selected = self.selected_host_idx == Some(i);
                self.render_host_row(tree, 0.0, row_y, table_width, host, is_selected, i);
            }

            tree.push(RenderCommand::PopClip);
        } else {
            // Empty state
            tree.push(RenderCommand::Text {
                x: table_width / 2.0 - 80.0, y: rows_y + 80.0,
                text: "No scan results yet".to_string(),
                color: OVERLAY0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: table_width / 2.0 - 120.0, y: rows_y + 100.0,
                text: "Press F5 or click Start Scan to begin".to_string(),
                color: SURFACE2,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Sidebar (host detail or overview)
        self.render_sidebar(tree, table_width, content_y);
    }

    fn render_summary_bar(&self, tree: &mut RenderTree, y: f32, result: &ScanResult) {
        let hosts_up = result.hosts_up();
        let open_ports = result.total_open_ports();

        tree.push(RenderCommand::Text {
            x: PADDING, y,
            text: format!(
                "Scanned {} IPs | {} hosts up | {} open ports | {:.1}s | {}",
                result.total_ips_scanned,
                hosts_up,
                open_ports,
                result.duration_secs,
                result.profile.label(),
            ),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_table_header(&self, tree: &mut RenderTree, x: f32, y: f32, width: f32) {
        tree.push(RenderCommand::FillRect {
            x, y,
            width, height: TABLE_HEADER_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let columns = [
            (PADDING, "Status"),
            (60.0, "IP Address"),
            (210.0, "Hostname"),
            (380.0, "MAC Address"),
            (530.0, "OS"),
            (630.0, "Ports"),
            (690.0, "Latency"),
        ];

        for (col_x, label) in &columns {
            tree.push(RenderCommand::Text {
                x: *col_x, y: y + 9.0,
                text: label.to_string(),
                color: LAVENDER,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_host_row(&self, tree: &mut RenderTree, x: f32, y: f32, width: f32,
                       host: &HostResult, selected: bool, idx: usize) {
        // Row background
        let bg = if selected {
            SURFACE1
        } else if idx % 2 == 0 {
            BASE
        } else {
            Color::rgba(49, 50, 68, 80) // Semi-transparent surface
        };

        tree.push(RenderCommand::FillRect {
            x, y,
            width, height: TABLE_ROW_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::ZERO,
        });

        // Status dot
        let dot_color = if host.is_up { GREEN } else { RED };
        tree.push(RenderCommand::FillRect {
            x: PADDING + 12.0, y: y + TABLE_ROW_HEIGHT / 2.0 - 4.0,
            width: 8.0, height: 8.0,
            color: dot_color,
            corner_radii: CornerRadii::all(4.0),
        });

        // IP Address
        tree.push(RenderCommand::Text {
            x: 60.0, y: y + 7.0,
            text: host.ip.display(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(140.0),
        });

        // Hostname
        let hostname = host.hostname.clone().unwrap_or_else(|| "-".to_string());
        tree.push(RenderCommand::Text {
            x: 210.0, y: y + 7.0,
            text: hostname,
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(160.0),
        });

        // MAC
        let mac_str = host.mac.map(|m| m.display()).unwrap_or_else(|| "-".to_string());
        tree.push(RenderCommand::Text {
            x: 380.0, y: y + 7.0,
            text: mac_str,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(140.0),
        });

        // OS
        tree.push(RenderCommand::Text {
            x: 530.0, y: y + 7.0,
            text: host.os_guess.label().to_string(),
            color: PEACH,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(90.0),
        });

        // Open ports count
        let open_count = host.open_port_count();
        let port_color = if open_count > 5 { YELLOW } else if open_count > 0 { GREEN } else { OVERLAY0 };
        tree.push(RenderCommand::Text {
            x: 630.0, y: y + 7.0,
            text: format!("{}", open_count),
            color: port_color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Latency
        tree.push(RenderCommand::Text {
            x: 690.0, y: y + 7.0,
            text: format!("{:.1}ms", host.latency_ms),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_sidebar(&self, tree: &mut RenderTree, x: f32, top_y: f32) {
        // Sidebar background
        tree.push(RenderCommand::FillRect {
            x, y: top_y,
            width: SIDEBAR_WIDTH, height: WINDOW_HEIGHT - top_y,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Left border
        tree.push(RenderCommand::FillRect {
            x, y: top_y,
            width: 1.0, height: WINDOW_HEIGHT - top_y,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        if let Some(host) = self.selected_host() {
            self.render_host_detail(tree, x + PADDING, top_y + PADDING, host);
        } else {
            // Overview panel
            tree.push(RenderCommand::Text {
                x: x + PADDING, y: top_y + PADDING,
                text: "Host Details".to_string(),
                color: LAVENDER,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: x + PADDING, y: top_y + PADDING + 24.0,
                text: "Select a host to view details".to_string(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - PADDING * 2.0),
            });

            // WOL section
            let wol_y = WINDOW_HEIGHT - 100.0;
            tree.push(RenderCommand::Text {
                x: x + PADDING, y: wol_y,
                text: "Wake-on-LAN".to_string(),
                color: LAVENDER,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            self.render_text_field(tree, x + PADDING, wol_y + 18.0, SIDEBAR_WIDTH - PADDING * 3.0, &self.wol_target_mac, "AA:BB:CC:DD:EE:FF");

            // WOL Send button
            let wol_btn_y = wol_y + 50.0;
            tree.push(RenderCommand::FillRect {
                x: x + PADDING, y: wol_btn_y,
                width: 120.0, height: BUTTON_HEIGHT,
                color: MAUVE,
                corner_radii: CornerRadii::all(SMALL_RADIUS),
            });
            tree.push(RenderCommand::Text {
                x: x + PADDING + 20.0, y: wol_btn_y + 9.0,
                text: if self.wol_sent { "Packet Sent!" } else { "Send WOL" }.to_string(),
                color: CRUST,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Export button
            let export_y = WINDOW_HEIGHT - 50.0;
            if self.results.is_some() {
                tree.push(RenderCommand::FillRect {
                    x: x + PADDING, y: export_y,
                    width: 120.0, height: BUTTON_HEIGHT,
                    color: TEAL,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
                tree.push(RenderCommand::Text {
                    x: x + PADDING + 20.0, y: export_y + 9.0,
                    text: "Export...".to_string(),
                    color: CRUST,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            // Export menu popup
            if self.show_export_menu {
                let menu_y = export_y - 60.0;
                tree.push(RenderCommand::BoxShadow {
                    x: x + PADDING, y: menu_y,
                    width: 120.0, height: 56.0,
                    offset_x: 0.0, offset_y: 2.0,
                    blur: 8.0, spread: 0.0,
                    color: Color::rgba(0, 0, 0, 120),
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
                tree.push(RenderCommand::FillRect {
                    x: x + PADDING, y: menu_y,
                    width: 120.0, height: 56.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(SMALL_RADIUS),
                });
                tree.push(RenderCommand::Text {
                    x: x + PADDING + 10.0, y: menu_y + 8.0,
                    text: "Export as CSV".to_string(),
                    color: TEXT_COLOR,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                tree.push(RenderCommand::Text {
                    x: x + PADDING + 10.0, y: menu_y + 32.0,
                    text: "Export as JSON".to_string(),
                    color: TEXT_COLOR,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }
    }

    fn render_host_detail(&self, tree: &mut RenderTree, x: f32, y: f32, host: &HostResult) {
        let w = SIDEBAR_WIDTH - PADDING * 2.0;

        // Host title
        tree.push(RenderCommand::Text {
            x, y,
            text: host.display_hostname(),
            color: BLUE,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w),
        });

        let mut dy = y + 22.0;

        // Detail fields
        let fields = [
            ("IP Address:", host.ip.display()),
            ("Hostname:", host.hostname.clone().unwrap_or_else(|| "N/A".to_string())),
            ("MAC:", host.mac.map(|m| m.display()).unwrap_or_else(|| "N/A".to_string())),
            ("Vendor:", host.vendor.clone().unwrap_or_else(|| "Unknown".to_string())),
            ("OS Guess:", host.os_guess.label().to_string()),
            ("TTL:", host.ttl.to_string()),
            ("Latency:", format!("{:.1} ms", host.latency_ms)),
            ("Open Ports:", host.open_port_count().to_string()),
        ];

        for (label, value) in &fields {
            tree.push(RenderCommand::Text {
                x, y: dy,
                text: label.to_string(),
                color: OVERLAY0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: x + 85.0, y: dy,
                text: value.to_string(),
                color: TEXT_COLOR,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w - 90.0),
            });
            dy += 18.0;
        }

        // Port list header
        dy += 8.0;
        tree.push(RenderCommand::FillRect {
            x, y: dy,
            width: w, height: 1.0,
            color: SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });
        dy += 6.0;
        tree.push(RenderCommand::Text {
            x, y: dy,
            text: "Port Details".to_string(),
            color: LAVENDER,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        dy += 20.0;

        // Port mini-table header
        tree.push(RenderCommand::Text {
            x, y: dy,
            text: "Port".to_string(),
            color: OVERLAY0, font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: x + 55.0, y: dy,
            text: "State".to_string(),
            color: OVERLAY0, font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: x + 115.0, y: dy,
            text: "Service".to_string(),
            color: OVERLAY0, font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        tree.push(RenderCommand::Text {
            x: x + 200.0, y: dy,
            text: "Response".to_string(),
            color: OVERLAY0, font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        dy += 16.0;

        // Clipped port list
        let clip_height = WINDOW_HEIGHT - dy - 20.0;
        tree.push(RenderCommand::PushClip {
            x, y: dy,
            width: w, height: clip_height.max(0.0),
        });

        for port in &host.ports {
            let port_y = dy - self.detail_port_scroll;
            if port_y + 16.0 < dy { continue; }
            if port_y > dy + clip_height { break; }

            tree.push(RenderCommand::Text {
                x, y: port_y,
                text: port.port.to_string(),
                color: TEXT_COLOR, font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: x + 55.0, y: port_y,
                text: port.state.label().to_string(),
                color: port.state.color(), font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: x + 115.0, y: port_y,
                text: port.service.clone().unwrap_or_else(|| "-".to_string()),
                color: SUBTEXT0, font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(80.0),
            });
            tree.push(RenderCommand::Text {
                x: x + 200.0, y: port_y,
                text: format!("{:.1}ms", port.response_ms),
                color: SUBTEXT0, font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Show banner if present
            if let Some(ref banner) = port.banner {
                let banner_line = if banner.len() > 35 {
                    let end = banner.char_indices()
                        .nth(35)
                        .map(|(i, _)| i)
                        .unwrap_or(banner.len());
                    format!("{}...", &banner[..end])
                } else {
                    banner.clone()
                };
                // We skip rendering banner inline to keep it simple; it appears in tooltip concept
                let _ = banner_line;
            }

            dy += 16.0;
        }

        tree.push(RenderCommand::PopClip);
    }

    fn render_topology_view(&self, tree: &mut RenderTree) {
        let content_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + PADDING + TAB_HEIGHT + PADDING;
        let area_w = WINDOW_WIDTH - PADDING * 2.0;
        let area_h = WINDOW_HEIGHT - content_y - PADDING;

        // Background
        tree.push(RenderCommand::FillRect {
            x: PADDING, y: content_y,
            width: area_w, height: area_h,
            color: CRUST,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        tree.push(RenderCommand::Text {
            x: PADDING + 12.0, y: content_y + 12.0,
            text: "Network Topology".to_string(),
            color: LAVENDER,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        if let Some(ref result) = self.results {
            if result.hosts.is_empty() {
                tree.push(RenderCommand::Text {
                    x: area_w / 2.0 - 40.0, y: content_y + area_h / 2.0,
                    text: "No hosts discovered".to_string(),
                    color: OVERLAY0,
                    font_size: 13.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                return;
            }

            // Central router/gateway node
            let center_x = area_w / 2.0 + PADDING;
            let center_y = content_y + 80.0;
            let gateway_size = 36.0;

            tree.push(RenderCommand::FillRect {
                x: center_x - gateway_size / 2.0, y: center_y - gateway_size / 2.0,
                width: gateway_size, height: gateway_size,
                color: PEACH,
                corner_radii: CornerRadii::all(gateway_size / 2.0),
            });
            tree.push(RenderCommand::Text {
                x: center_x - 20.0, y: center_y + gateway_size / 2.0 + 4.0,
                text: "Gateway".to_string(),
                color: PEACH,
                font_size: 10.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Host nodes arranged in arcs below the gateway
            let host_count = result.hosts.len().min(24); // Cap display
            let start_y = center_y + 70.0;
            let cols = 8usize;
            let spacing_x = (area_w - 80.0) / cols as f32;
            let spacing_y = 65.0;

            for (i, host) in result.hosts.iter().take(host_count).enumerate() {
                let col = i % cols;
                let row = i / cols;
                let node_x = 60.0 + col as f32 * spacing_x;
                let node_y = start_y + row as f32 * spacing_y;
                let node_size = 24.0;

                // Line from gateway to node
                tree.push(RenderCommand::Line {
                    x1: center_x, y1: center_y + gateway_size / 2.0,
                    x2: node_x, y2: node_y - node_size / 2.0,
                    color: SURFACE2,
                    width: 1.0,
                });

                // Node color based on OS
                let node_color = match host.os_guess {
                    OsGuess::Linux => GREEN,
                    OsGuess::Windows => BLUE,
                    OsGuess::MacOS => MAUVE,
                    OsGuess::Router => PEACH,
                    OsGuess::Printer => YELLOW,
                    OsGuess::IoTDevice => TEAL,
                    _ => SURFACE2,
                };

                tree.push(RenderCommand::FillRect {
                    x: node_x - node_size / 2.0, y: node_y - node_size / 2.0,
                    width: node_size, height: node_size,
                    color: node_color,
                    corner_radii: CornerRadii::all(node_size / 2.0),
                });

                // IP label
                let short_ip = format!(".{}", host.ip.octets[3]);
                tree.push(RenderCommand::Text {
                    x: node_x - 12.0, y: node_y + node_size / 2.0 + 2.0,
                    text: short_ip,
                    color: SUBTEXT0,
                    font_size: 9.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Legend
            let legend_y = content_y + area_h - 50.0;
            let legend_items = [
                (GREEN, "Linux"), (BLUE, "Windows"), (MAUVE, "macOS"),
                (PEACH, "Router"), (YELLOW, "Printer"), (TEAL, "IoT"),
            ];
            let mut lx = PADDING + 12.0;
            for (color, label) in &legend_items {
                tree.push(RenderCommand::FillRect {
                    x: lx, y: legend_y,
                    width: 10.0, height: 10.0,
                    color: *color,
                    corner_radii: CornerRadii::all(5.0),
                });
                tree.push(RenderCommand::Text {
                    x: lx + 14.0, y: legend_y - 1.0,
                    text: label.to_string(),
                    color: SUBTEXT0,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                lx += label.len() as f32 * 6.0 + 28.0;
            }
        } else {
            tree.push(RenderCommand::Text {
                x: area_w / 2.0 - 60.0, y: content_y + area_h / 2.0,
                text: "Run a scan to see topology".to_string(),
                color: OVERLAY0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_history_view(&self, tree: &mut RenderTree) {
        let content_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + PADDING + TAB_HEIGHT + PADDING;

        tree.push(RenderCommand::Text {
            x: PADDING, y: content_y,
            text: "Scan History".to_string(),
            color: LAVENDER,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        if self.history.is_empty() {
            tree.push(RenderCommand::Text {
                x: PADDING, y: content_y + 30.0,
                text: "No scan history yet".to_string(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        // History table header
        let header_y = content_y + 24.0;
        tree.push(RenderCommand::FillRect {
            x: PADDING, y: header_y,
            width: WINDOW_WIDTH - PADDING * 2.0, height: TABLE_HEADER_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let hist_cols = [
            (PADDING + 4.0, "#"),
            (PADDING + 40.0, "Timestamp"),
            (PADDING + 210.0, "Target"),
            (PADDING + 400.0, "Profile"),
            (PADDING + 510.0, "Hosts Up"),
            (PADDING + 600.0, "Open Ports"),
            (PADDING + 700.0, "Duration"),
        ];
        for (cx, label) in &hist_cols {
            tree.push(RenderCommand::Text {
                x: *cx, y: header_y + 9.0,
                text: label.to_string(),
                color: LAVENDER,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // History rows
        let rows_y = header_y + TABLE_HEADER_HEIGHT;
        for (i, entry) in self.history.iter().enumerate() {
            let row_y = rows_y + (i as f32) * TABLE_ROW_HEIGHT;
            if row_y > WINDOW_HEIGHT { break; }

            let bg = if i % 2 == 0 { BASE } else { Color::rgba(49, 50, 68, 80) };
            tree.push(RenderCommand::FillRect {
                x: PADDING, y: row_y,
                width: WINDOW_WIDTH - PADDING * 2.0, height: TABLE_ROW_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            tree.push(RenderCommand::Text {
                x: PADDING + 4.0, y: row_y + 7.0,
                text: entry.id.to_string(),
                color: OVERLAY0, font_size: 11.0,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: PADDING + 40.0, y: row_y + 7.0,
                text: entry.timestamp.clone(),
                color: SUBTEXT0, font_size: 11.0,
                font_weight: FontWeightHint::Regular, max_width: Some(160.0),
            });
            tree.push(RenderCommand::Text {
                x: PADDING + 210.0, y: row_y + 7.0,
                text: entry.target_description.clone(),
                color: TEXT_COLOR, font_size: 11.0,
                font_weight: FontWeightHint::Regular, max_width: Some(180.0),
            });
            tree.push(RenderCommand::Text {
                x: PADDING + 400.0, y: row_y + 7.0,
                text: entry.profile.label().to_string(),
                color: PEACH, font_size: 11.0,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: PADDING + 510.0, y: row_y + 7.0,
                text: entry.hosts_up().to_string(),
                color: GREEN, font_size: 11.0,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: PADDING + 600.0, y: row_y + 7.0,
                text: entry.total_open_ports().to_string(),
                color: YELLOW, font_size: 11.0,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: PADDING + 700.0, y: row_y + 7.0,
                text: format!("{:.1}s", entry.duration_secs),
                color: SUBTEXT0, font_size: 11.0,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
        }

        // Diff section
        if self.history.len() >= 2 {
            let diff_y = rows_y + (self.history.len() as f32) * TABLE_ROW_HEIGHT + 20.0;
            if diff_y < WINDOW_HEIGHT - 60.0 {
                let newest = self.history.front();
                let previous = self.history.get(1);
                if let (Some(new_scan), Some(old_scan)) = (newest, previous) {
                    let (added, removed) = diff_scans(old_scan, new_scan);
                    tree.push(RenderCommand::Text {
                        x: PADDING, y: diff_y,
                        text: format!("Diff vs previous: +{} new hosts, -{} missing hosts", added.len(), removed.len()),
                        color: if added.is_empty() && removed.is_empty() { OVERLAY0 } else { YELLOW },
                        font_size: 12.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
            }
        }
    }

    fn render_traceroute_view(&self, tree: &mut RenderTree) {
        let content_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + PADDING + TAB_HEIGHT + PADDING;

        tree.push(RenderCommand::Text {
            x: PADDING, y: content_y,
            text: "Traceroute".to_string(),
            color: LAVENDER,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Target input
        let input_y = content_y + 22.0;
        tree.push(RenderCommand::Text {
            x: PADDING, y: input_y + 5.0,
            text: "Target:".to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        self.render_text_field(tree, PADDING + 60.0, input_y, 200.0, &self.traceroute_target, "8.8.8.8");

        // Run button
        let btn_y = input_y + INPUT_HEIGHT + 8.0;
        tree.push(RenderCommand::FillRect {
            x: PADDING, y: btn_y,
            width: 120.0, height: BUTTON_HEIGHT,
            color: BLUE,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        tree.push(RenderCommand::Text {
            x: PADDING + 16.0, y: btn_y + 9.0,
            text: "Run Traceroute".to_string(),
            color: CRUST,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Results
        if let Some(ref hops) = self.traceroute_result {
            let table_y = btn_y + BUTTON_HEIGHT + 16.0;

            // Header
            tree.push(RenderCommand::FillRect {
                x: PADDING, y: table_y,
                width: WINDOW_WIDTH - PADDING * 2.0, height: TABLE_HEADER_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
            let hop_cols = [
                (PADDING + 4.0, "Hop"),
                (PADDING + 50.0, "IP Address"),
                (PADDING + 220.0, "Hostname"),
                (PADDING + 450.0, "RTT"),
            ];
            for (cx, label) in &hop_cols {
                tree.push(RenderCommand::Text {
                    x: *cx, y: table_y + 9.0,
                    text: label.to_string(),
                    color: LAVENDER, font_size: 11.0,
                    font_weight: FontWeightHint::Bold, max_width: None,
                });
            }

            let rows_y = table_y + TABLE_HEADER_HEIGHT;
            for (i, hop) in hops.iter().enumerate() {
                let row_y = rows_y + (i as f32) * TABLE_ROW_HEIGHT;
                if row_y > WINDOW_HEIGHT { break; }

                let bg = if i % 2 == 0 { BASE } else { Color::rgba(49, 50, 68, 80) };
                tree.push(RenderCommand::FillRect {
                    x: PADDING, y: row_y,
                    width: WINDOW_WIDTH - PADDING * 2.0, height: TABLE_ROW_HEIGHT,
                    color: bg,
                    corner_radii: CornerRadii::ZERO,
                });

                tree.push(RenderCommand::Text {
                    x: PADDING + 4.0, y: row_y + 7.0,
                    text: hop.hop_number.to_string(),
                    color: OVERLAY0, font_size: 11.0,
                    font_weight: FontWeightHint::Regular, max_width: None,
                });

                if hop.timed_out {
                    tree.push(RenderCommand::Text {
                        x: PADDING + 50.0, y: row_y + 7.0,
                        text: "* * * (timed out)".to_string(),
                        color: RED, font_size: 11.0,
                        font_weight: FontWeightHint::Regular, max_width: None,
                    });
                } else {
                    let ip_str = hop.ip.map(|ip| ip.display()).unwrap_or_else(|| "???".to_string());
                    tree.push(RenderCommand::Text {
                        x: PADDING + 50.0, y: row_y + 7.0,
                        text: ip_str,
                        color: TEXT_COLOR, font_size: 11.0,
                        font_weight: FontWeightHint::Regular, max_width: None,
                    });
                    tree.push(RenderCommand::Text {
                        x: PADDING + 220.0, y: row_y + 7.0,
                        text: hop.hostname.clone().unwrap_or_else(|| "-".to_string()),
                        color: SUBTEXT0, font_size: 11.0,
                        font_weight: FontWeightHint::Regular, max_width: Some(220.0),
                    });
                    tree.push(RenderCommand::Text {
                        x: PADDING + 450.0, y: row_y + 7.0,
                        text: format!("{:.1} ms", hop.rtt_ms),
                        color: TEAL, font_size: 11.0,
                        font_weight: FontWeightHint::Regular, max_width: None,
                    });
                }

                // Visual RTT bar
                if !hop.timed_out {
                    let bar_max = 200.0;
                    let bar_width = (hop.rtt_ms / 100.0 * bar_max).clamp(2.0, bar_max);
                    tree.push(RenderCommand::FillRect {
                        x: PADDING + 530.0, y: row_y + 8.0,
                        width: bar_width, height: 12.0,
                        color: Color::rgba(137, 180, 250, 120),
                        corner_radii: CornerRadii::all(2.0),
                    });
                }
            }
        }
    }

    fn render_whois_view(&self, tree: &mut RenderTree) {
        let content_y = TITLE_BAR_HEIGHT + CONFIG_PANEL_HEIGHT + PADDING + TAB_HEIGHT + PADDING;

        tree.push(RenderCommand::Text {
            x: PADDING, y: content_y,
            text: "WHOIS Lookup".to_string(),
            color: LAVENDER,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Target input
        let input_y = content_y + 22.0;
        tree.push(RenderCommand::Text {
            x: PADDING, y: input_y + 5.0,
            text: "IP:".to_string(),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        self.render_text_field(tree, PADDING + 30.0, input_y, 200.0, &self.whois_target, "8.8.8.8");

        // Run button
        let btn_y = input_y + INPUT_HEIGHT + 8.0;
        tree.push(RenderCommand::FillRect {
            x: PADDING, y: btn_y,
            width: 120.0, height: BUTTON_HEIGHT,
            color: MAUVE,
            corner_radii: CornerRadii::all(SMALL_RADIUS),
        });
        tree.push(RenderCommand::Text {
            x: PADDING + 18.0, y: btn_y + 9.0,
            text: "Lookup WHOIS".to_string(),
            color: CRUST,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // WHOIS result display
        if let Some(ref info) = self.whois_result {
            let card_y = btn_y + BUTTON_HEIGHT + 16.0;

            tree.push(RenderCommand::FillRect {
                x: PADDING, y: card_y,
                width: 500.0, height: 220.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            tree.push(RenderCommand::StrokeRect {
                x: PADDING, y: card_y,
                width: 500.0, height: 220.0,
                color: SURFACE1,
                line_width: 1.0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            let fields = [
                ("IP Address:", info.ip.display()),
                ("Organization:", info.org_name.clone()),
                ("Country:", info.country.clone()),
                ("CIDR:", info.cidr.clone()),
                ("Net Name:", info.net_name.clone()),
                ("Description:", info.description.clone()),
                ("Abuse Contact:", info.abuse_contact.clone()),
            ];

            let mut fy = card_y + 14.0;
            for (label, value) in &fields {
                tree.push(RenderCommand::Text {
                    x: PADDING + 14.0, y: fy,
                    text: label.to_string(),
                    color: OVERLAY0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                tree.push(RenderCommand::Text {
                    x: PADDING + 140.0, y: fy,
                    text: value.to_string(),
                    color: TEXT_COLOR,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(340.0),
                });
                fy += 26.0;
            }
        }
    }

    fn render_progress_bar(&self, tree: &mut RenderTree, progress: &ScanProgress) {
        let y = WINDOW_HEIGHT - PROGRESS_BAR_HEIGHT;

        // Background
        tree.push(RenderCommand::FillRect {
            x: 0.0, y,
            width: WINDOW_WIDTH, height: PROGRESS_BAR_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Fill
        let frac = progress.fraction();
        tree.push(RenderCommand::FillRect {
            x: 0.0, y,
            width: WINDOW_WIDTH * frac, height: PROGRESS_BAR_HEIGHT,
            color: Color::rgba(137, 180, 250, 100),
            corner_radii: CornerRadii::ZERO,
        });

        // Text
        tree.push(RenderCommand::Text {
            x: PADDING, y: y + 8.0,
            text: format!("{} - {:.0}% ({} hosts found, {:.1}s elapsed)",
                progress.phase.label(),
                frac * 100.0,
                progress.hosts_found,
                progress.elapsed_secs,
            ),
            color: TEXT_COLOR,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

// ============================================================================
// Parsing Helpers
// ============================================================================

/// Parse a port specification string like "80", "80-100", "80,443,8080".
fn parse_port_spec(s: &str) -> Option<Vec<u16>> {
    let trimmed = s.trim();
    if trimmed.is_empty() { return None; }

    let mut ports = Vec::new();
    for part in trimmed.split(',') {
        let p = part.trim();
        if p.contains('-') {
            let range_parts: Vec<&str> = p.splitn(2, '-').collect();
            let start: u16 = range_parts.first()?.trim().parse().ok()?;
            let end: u16 = range_parts.get(1)?.trim().parse().ok()?;
            let mut current = start;
            while current <= end {
                ports.push(current);
                if current == u16::MAX { break; }
                current = current.saturating_add(1);
            }
        } else {
            let port: u16 = p.parse().ok()?;
            ports.push(port);
        }
    }

    if ports.is_empty() { None } else { Some(ports) }
}

/// Parse a MAC address string like "AA:BB:CC:DD:EE:FF".
fn parse_mac(s: &str) -> Option<MacAddr> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() != 6 { return None; }
    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(MacAddr { bytes })
}

// ============================================================================
// Entry Point
// ============================================================================

fn main() {
    let mut app = NetScanApp::new();

    // Initial scan for demonstration
    app.start_scan();

    // Render one frame
    let _tree = app.render();

    // In real OS: enter event loop with compositor
    // loop {
    //     let event = wait_event();
    //     app.handle_event(&event);
    //     let tree = app.render();
    //     compositor_submit(tree);
    // }
}

// ============================================================================
// Tests (55+)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- IPv4 Parsing and Representation ---

    #[test]
    fn test_ipv4_parse_valid() {
        let ip = Ipv4Addr::parse("192.168.1.1").unwrap();
        assert_eq!(ip.octets, [192, 168, 1, 1]);
    }

    #[test]
    fn test_ipv4_parse_zero() {
        let ip = Ipv4Addr::parse("0.0.0.0").unwrap();
        assert_eq!(ip.octets, [0, 0, 0, 0]);
    }

    #[test]
    fn test_ipv4_parse_max() {
        let ip = Ipv4Addr::parse("255.255.255.255").unwrap();
        assert_eq!(ip.octets, [255, 255, 255, 255]);
    }

    #[test]
    fn test_ipv4_parse_invalid_too_few_octets() {
        assert!(Ipv4Addr::parse("192.168.1").is_none());
    }

    #[test]
    fn test_ipv4_parse_invalid_not_numeric() {
        assert!(Ipv4Addr::parse("abc.def.ghi.jkl").is_none());
    }

    #[test]
    fn test_ipv4_display() {
        let ip = Ipv4Addr::new(10, 0, 0, 1);
        assert_eq!(ip.display(), "10.0.0.1");
    }

    #[test]
    fn test_ipv4_to_u32_and_back() {
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        let val = ip.to_u32();
        let roundtrip = Ipv4Addr::from_u32(val);
        assert_eq!(ip, roundtrip);
    }

    #[test]
    fn test_ipv4_is_private_10() {
        assert!(Ipv4Addr::new(10, 0, 0, 1).is_private());
    }

    #[test]
    fn test_ipv4_is_private_172() {
        assert!(Ipv4Addr::new(172, 16, 0, 1).is_private());
        assert!(!Ipv4Addr::new(172, 15, 0, 1).is_private());
        assert!(Ipv4Addr::new(172, 31, 255, 255).is_private());
        assert!(!Ipv4Addr::new(172, 32, 0, 0).is_private());
    }

    #[test]
    fn test_ipv4_is_private_192_168() {
        assert!(Ipv4Addr::new(192, 168, 0, 1).is_private());
    }

    #[test]
    fn test_ipv4_is_not_private() {
        assert!(!Ipv4Addr::new(8, 8, 8, 8).is_private());
    }

    #[test]
    fn test_ipv4_is_loopback() {
        assert!(Ipv4Addr::new(127, 0, 0, 1).is_loopback());
        assert!(!Ipv4Addr::new(128, 0, 0, 1).is_loopback());
    }

    // --- CIDR ---

    #[test]
    fn test_cidr_parse_24() {
        let cidr = CidrRange::parse("192.168.1.0/24").unwrap();
        assert_eq!(cidr.prefix_len, 24);
        assert_eq!(cidr.host_count(), 256);
    }

    #[test]
    fn test_cidr_parse_32() {
        let cidr = CidrRange::parse("10.0.0.1/32").unwrap();
        assert_eq!(cidr.host_count(), 1);
    }

    #[test]
    fn test_cidr_parse_invalid_prefix() {
        assert!(CidrRange::parse("10.0.0.0/33").is_none());
    }

    #[test]
    fn test_cidr_first_last_24() {
        let cidr = CidrRange::parse("192.168.1.0/24").unwrap();
        assert_eq!(cidr.first_addr(), Ipv4Addr::new(192, 168, 1, 0));
        assert_eq!(cidr.last_addr(), Ipv4Addr::new(192, 168, 1, 255));
    }

    #[test]
    fn test_cidr_host_ips_24() {
        let cidr = CidrRange::parse("192.168.1.0/24").unwrap();
        let ips = cidr.host_ips();
        assert_eq!(ips.len(), 254); // excludes .0 (network) and .255 (broadcast)
        assert_eq!(ips.first().unwrap().octets[3], 1);
        assert_eq!(ips.last().unwrap().octets[3], 254);
    }

    #[test]
    fn test_cidr_mask_24() {
        let cidr = CidrRange::parse("192.168.1.0/24").unwrap();
        assert_eq!(cidr.mask(), 0xFFFFFF00);
    }

    #[test]
    fn test_cidr_mask_16() {
        let cidr = CidrRange::parse("10.0.0.0/16").unwrap();
        assert_eq!(cidr.mask(), 0xFFFF0000);
    }

    // --- ScanTarget ---

    #[test]
    fn test_scan_target_single() {
        let target = ScanTarget::parse("10.0.0.1").unwrap();
        let ips = target.all_ips();
        assert_eq!(ips.len(), 1);
        assert_eq!(ips[0], Ipv4Addr::new(10, 0, 0, 1));
    }

    #[test]
    fn test_scan_target_cidr() {
        let target = ScanTarget::parse("192.168.1.0/24").unwrap();
        let ips = target.all_ips();
        assert_eq!(ips.len(), 254);
    }

    #[test]
    fn test_scan_target_range() {
        let target = ScanTarget::parse("10.0.0.1-10.0.0.5").unwrap();
        let ips = target.all_ips();
        assert_eq!(ips.len(), 5);
    }

    #[test]
    fn test_scan_target_invalid() {
        assert!(ScanTarget::parse("not an ip").is_none());
    }

    // --- MAC ---

    #[test]
    fn test_mac_display() {
        let mac = MacAddr::new(0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF);
        assert_eq!(mac.display(), "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn test_mac_from_ip_simulated() {
        let mac = MacAddr::from_ip_simulated(Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(mac.bytes[0], 0x00);
        assert_eq!(mac.bytes[1], 0x1A);
        assert_eq!(mac.bytes[5], 100);
    }

    #[test]
    fn test_parse_mac_valid() {
        let mac = parse_mac("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(mac.bytes, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_parse_mac_invalid() {
        assert!(parse_mac("invalid").is_none());
        assert!(parse_mac("AA:BB:CC:DD:EE").is_none());
    }

    // --- Port Parsing ---

    #[test]
    fn test_parse_port_spec_single() {
        let ports = parse_port_spec("80").unwrap();
        assert_eq!(ports, vec![80]);
    }

    #[test]
    fn test_parse_port_spec_range() {
        let ports = parse_port_spec("80-83").unwrap();
        assert_eq!(ports, vec![80, 81, 82, 83]);
    }

    #[test]
    fn test_parse_port_spec_list() {
        let ports = parse_port_spec("80,443,8080").unwrap();
        assert_eq!(ports, vec![80, 443, 8080]);
    }

    #[test]
    fn test_parse_port_spec_mixed() {
        let ports = parse_port_spec("22,80-82,443").unwrap();
        assert_eq!(ports, vec![22, 80, 81, 82, 443]);
    }

    #[test]
    fn test_parse_port_spec_empty() {
        assert!(parse_port_spec("").is_none());
    }

    #[test]
    fn test_parse_port_spec_invalid() {
        assert!(parse_port_spec("abc").is_none());
    }

    // --- Service Lookup ---

    #[test]
    fn test_lookup_service_http() {
        assert_eq!(lookup_service(80), Some("http"));
    }

    #[test]
    fn test_lookup_service_ssh() {
        assert_eq!(lookup_service(22), Some("ssh"));
    }

    #[test]
    fn test_lookup_service_https() {
        assert_eq!(lookup_service(443), Some("https"));
    }

    #[test]
    fn test_lookup_service_unknown() {
        assert_eq!(lookup_service(12345), None);
    }

    #[test]
    fn test_service_database_has_100_plus() {
        let db = service_database();
        assert!(db.len() >= 100);
    }

    // --- Scan Profiles ---

    #[test]
    fn test_quick_scan_ports() {
        let ports = quick_scan_ports();
        assert_eq!(ports.len(), 20);
        assert!(ports.contains(&80));
        assert!(ports.contains(&443));
    }

    #[test]
    fn test_well_known_ports() {
        let ports = well_known_ports();
        assert_eq!(ports.len(), 1024);
        assert_eq!(*ports.first().unwrap(), 0u16);
        assert_eq!(*ports.last().unwrap(), 1023u16);
    }

    #[test]
    fn test_profile_labels() {
        assert_eq!(ScanProfile::Quick.label(), "Quick Scan");
        assert_eq!(ScanProfile::Full.label(), "Full Scan");
        assert_eq!(ScanProfile::Custom.label(), "Custom");
        assert_eq!(ScanProfile::Stealth.label(), "Stealth");
    }

    // --- OS Guess ---

    #[test]
    fn test_guess_os_windows() {
        let ports = vec![
            PortResult { port: 135, state: PortState::Open, service: None, banner: None, response_ms: 1.0 },
            PortResult { port: 445, state: PortState::Open, service: None, banner: None, response_ms: 1.0 },
        ];
        assert_eq!(guess_os(&ports), OsGuess::Windows);
    }

    #[test]
    fn test_guess_os_linux() {
        let ports = vec![
            PortResult { port: 22, state: PortState::Open, service: None, banner: None, response_ms: 1.0 },
            PortResult { port: 80, state: PortState::Open, service: None, banner: None, response_ms: 1.0 },
        ];
        assert_eq!(guess_os(&ports), OsGuess::Linux);
    }

    #[test]
    fn test_guess_os_unknown_empty() {
        let ports: Vec<PortResult> = vec![];
        assert_eq!(guess_os(&ports), OsGuess::Unknown);
    }

    // --- Bandwidth Estimation ---

    #[test]
    fn test_estimate_scan_time() {
        let time = estimate_scan_time(254, 20, 1000, 100);
        assert!(time > 0.0);
    }

    #[test]
    fn test_estimate_scan_time_zero_concurrency() {
        let time = estimate_scan_time(254, 20, 1000, 0);
        assert_eq!(time, 0.0);
    }

    // --- WOL Packet ---

    #[test]
    fn test_wol_packet_structure() {
        let mac = MacAddr::new(0x11, 0x22, 0x33, 0x44, 0x55, 0x66);
        let packet = build_wol_packet(&mac);
        assert_eq!(packet.len(), 102);
        // First 6 bytes are 0xFF
        for byte in &packet[..6] {
            assert_eq!(*byte, 0xFF);
        }
        // Next 16 repetitions of MAC
        for rep in 0..16 {
            let offset = 6 + rep * 6;
            assert_eq!(packet[offset], 0x11);
            assert_eq!(packet[offset + 5], 0x66);
        }
    }

    // --- Export ---

    #[test]
    fn test_export_csv_header() {
        let result = ScanResult {
            id: 1,
            timestamp: "2026-05-18".to_string(),
            target_description: "192.168.1.0/24".to_string(),
            profile: ScanProfile::Quick,
            hosts: vec![],
            total_ips_scanned: 254,
            total_ports_scanned: 20,
            duration_secs: 1.0,
        };
        let csv = export_csv(&result);
        assert!(csv.starts_with("IP,Hostname,MAC,OS,Status,Latency(ms),Open Ports,Services\n"));
    }

    #[test]
    fn test_export_csv_with_host() {
        let result = ScanResult {
            id: 1,
            timestamp: "2026-05-18".to_string(),
            target_description: "10.0.0.1".to_string(),
            profile: ScanProfile::Quick,
            hosts: vec![HostResult {
                ip: Ipv4Addr::new(10, 0, 0, 1),
                hostname: Some("server-1".to_string()),
                mac: Some(MacAddr::new(0, 0, 0, 0, 0, 1)),
                os_guess: OsGuess::Linux,
                ports: vec![PortResult {
                    port: 22, state: PortState::Open,
                    service: Some("ssh".to_string()),
                    banner: None, response_ms: 1.5,
                }],
                latency_ms: 1.2,
                is_up: true,
                ttl: 64,
                vendor: None,
            }],
            total_ips_scanned: 1,
            total_ports_scanned: 20,
            duration_secs: 0.5,
        };
        let csv = export_csv(&result);
        assert!(csv.contains("10.0.0.1"));
        assert!(csv.contains("server-1"));
        assert!(csv.contains("Linux"));
    }

    #[test]
    fn test_export_json_structure() {
        let result = ScanResult {
            id: 42,
            timestamp: "2026-05-18".to_string(),
            target_description: "192.168.1.0/24".to_string(),
            profile: ScanProfile::Quick,
            hosts: vec![],
            total_ips_scanned: 254,
            total_ports_scanned: 20,
            duration_secs: 1.0,
        };
        let json = export_json(&result);
        assert!(json.contains("\"scan_id\": 42"));
        assert!(json.contains("\"hosts\": ["));
    }

    // --- Diff ---

    #[test]
    fn test_diff_scans_new_host() {
        let old = ScanResult {
            id: 1, timestamp: String::new(), target_description: String::new(),
            profile: ScanProfile::Quick, hosts: vec![],
            total_ips_scanned: 0, total_ports_scanned: 0, duration_secs: 0.0,
        };
        let new = ScanResult {
            id: 2, timestamp: String::new(), target_description: String::new(),
            profile: ScanProfile::Quick,
            hosts: vec![HostResult {
                ip: Ipv4Addr::new(10, 0, 0, 1),
                hostname: None, mac: None, os_guess: OsGuess::Unknown,
                ports: vec![], latency_ms: 1.0, is_up: true, ttl: 64, vendor: None,
            }],
            total_ips_scanned: 1, total_ports_scanned: 0, duration_secs: 0.0,
        };
        let (added, removed) = diff_scans(&old, &new);
        assert_eq!(added.len(), 1);
        assert_eq!(removed.len(), 0);
    }

    #[test]
    fn test_diff_scans_missing_host() {
        let old = ScanResult {
            id: 1, timestamp: String::new(), target_description: String::new(),
            profile: ScanProfile::Quick,
            hosts: vec![HostResult {
                ip: Ipv4Addr::new(10, 0, 0, 1),
                hostname: None, mac: None, os_guess: OsGuess::Unknown,
                ports: vec![], latency_ms: 1.0, is_up: true, ttl: 64, vendor: None,
            }],
            total_ips_scanned: 1, total_ports_scanned: 0, duration_secs: 0.0,
        };
        let new = ScanResult {
            id: 2, timestamp: String::new(), target_description: String::new(),
            profile: ScanProfile::Quick, hosts: vec![],
            total_ips_scanned: 0, total_ports_scanned: 0, duration_secs: 0.0,
        };
        let (added, removed) = diff_scans(&old, &new);
        assert_eq!(added.len(), 0);
        assert_eq!(removed.len(), 1);
    }

    // --- Traceroute ---

    #[test]
    fn test_simulate_traceroute_not_empty() {
        let hops = simulate_traceroute(Ipv4Addr::new(8, 8, 8, 8));
        assert!(!hops.is_empty());
        // Last hop should be the destination
        let last = hops.last().unwrap();
        assert_eq!(last.ip, Some(Ipv4Addr::new(8, 8, 8, 8)));
    }

    #[test]
    fn test_simulate_traceroute_ascending_hops() {
        let hops = simulate_traceroute(Ipv4Addr::new(1, 1, 1, 1));
        for (i, hop) in hops.iter().enumerate() {
            assert_eq!(hop.hop_number as usize, i + 1);
        }
    }

    // --- WHOIS ---

    #[test]
    fn test_simulate_whois() {
        let info = simulate_whois(Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(info.ip, Ipv4Addr::new(8, 8, 8, 8));
        assert!(!info.org_name.is_empty());
        assert!(!info.country.is_empty());
    }

    // --- Application State ---

    #[test]
    fn test_app_new_defaults() {
        let app = NetScanApp::new();
        assert_eq!(app.active_tab, ViewTab::Results);
        assert!(app.results.is_none());
        assert!(!app.is_scanning);
        assert!(app.history.is_empty());
    }

    #[test]
    fn test_app_start_scan() {
        let mut app = NetScanApp::new();
        app.start_scan();
        assert!(app.results.is_some());
        assert!(!app.history.is_empty());
    }

    #[test]
    fn test_app_scan_populates_hosts() {
        let mut app = NetScanApp::new();
        app.start_scan();
        let result = app.results.as_ref().unwrap();
        assert!(!result.hosts.is_empty());
    }

    #[test]
    fn test_app_scan_history_limit() {
        let mut app = NetScanApp::new();
        for _ in 0..MAX_HISTORY_ENTRIES + 5 {
            app.start_scan();
        }
        assert!(app.history.len() <= MAX_HISTORY_ENTRIES);
    }

    #[test]
    fn test_app_run_traceroute() {
        let mut app = NetScanApp::new();
        app.traceroute_target = "8.8.8.8".to_string();
        app.run_traceroute();
        assert!(app.traceroute_result.is_some());
    }

    #[test]
    fn test_app_run_whois() {
        let mut app = NetScanApp::new();
        app.whois_target = "8.8.8.8".to_string();
        app.run_whois();
        assert!(app.whois_result.is_some());
    }

    #[test]
    fn test_app_render_no_crash() {
        let mut app = NetScanApp::new();
        app.start_scan();
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_topology_no_crash() {
        let mut app = NetScanApp::new();
        app.start_scan();
        app.active_tab = ViewTab::Topology;
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_history_no_crash() {
        let mut app = NetScanApp::new();
        app.start_scan();
        app.active_tab = ViewTab::History;
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_traceroute_no_crash() {
        let mut app = NetScanApp::new();
        app.active_tab = ViewTab::Traceroute;
        app.run_traceroute();
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_render_whois_no_crash() {
        let mut app = NetScanApp::new();
        app.active_tab = ViewTab::Whois;
        app.run_whois();
        let tree = app.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_app_select_host() {
        let mut app = NetScanApp::new();
        app.start_scan();
        app.selected_host_idx = Some(0);
        assert!(app.selected_host().is_some());
    }

    #[test]
    fn test_app_key_f5_starts_scan() {
        let mut app = NetScanApp::new();
        let event = Event::Key(KeyEvent {
            key: Key::F5,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        let result = app.handle_event(&event);
        assert_eq!(result, EventResult::Consumed);
        assert!(app.results.is_some());
    }

    #[test]
    fn test_app_key_escape_clears_selection() {
        let mut app = NetScanApp::new();
        app.start_scan();
        app.selected_host_idx = Some(0);
        app.show_export_menu = true;
        let event = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert!(app.selected_host_idx.is_none());
        assert!(!app.show_export_menu);
    }

    // --- PortSpec ---

    #[test]
    fn test_port_spec_single() {
        let spec = PortSpec::Single(443);
        assert_eq!(spec.all_ports(), vec![443]);
    }

    #[test]
    fn test_port_spec_range() {
        let spec = PortSpec::Range(80, 83);
        assert_eq!(spec.all_ports(), vec![80, 81, 82, 83]);
    }

    #[test]
    fn test_port_spec_list() {
        let spec = PortSpec::List(vec![22, 80, 443]);
        assert_eq!(spec.all_ports(), vec![22, 80, 443]);
    }

    // --- Host Result helpers ---

    #[test]
    fn test_host_result_open_port_count() {
        let host = HostResult {
            ip: Ipv4Addr::new(10, 0, 0, 1),
            hostname: None, mac: None, os_guess: OsGuess::Linux,
            ports: vec![
                PortResult { port: 22, state: PortState::Open, service: None, banner: None, response_ms: 1.0 },
                PortResult { port: 80, state: PortState::Open, service: None, banner: None, response_ms: 1.0 },
                PortResult { port: 443, state: PortState::Closed, service: None, banner: None, response_ms: 1.0 },
            ],
            latency_ms: 1.0, is_up: true, ttl: 64, vendor: None,
        };
        assert_eq!(host.open_port_count(), 2);
    }

    #[test]
    fn test_host_result_display_hostname() {
        let host = HostResult {
            ip: Ipv4Addr::new(10, 0, 0, 1),
            hostname: Some("server-1".to_string()),
            mac: None, os_guess: OsGuess::Linux,
            ports: vec![], latency_ms: 1.0, is_up: true, ttl: 64, vendor: None,
        };
        assert_eq!(host.display_hostname(), "server-1");

        let host2 = HostResult {
            ip: Ipv4Addr::new(10, 0, 0, 2),
            hostname: None,
            mac: None, os_guess: OsGuess::Unknown,
            ports: vec![], latency_ms: 1.0, is_up: true, ttl: 64, vendor: None,
        };
        assert_eq!(host2.display_hostname(), "10.0.0.2");
    }

    // --- ScanResult helpers ---

    #[test]
    fn test_scan_result_hosts_up() {
        let result = ScanResult {
            id: 1, timestamp: String::new(), target_description: String::new(),
            profile: ScanProfile::Quick,
            hosts: vec![
                HostResult {
                    ip: Ipv4Addr::new(10, 0, 0, 1), hostname: None, mac: None,
                    os_guess: OsGuess::Linux, ports: vec![], latency_ms: 1.0,
                    is_up: true, ttl: 64, vendor: None,
                },
                HostResult {
                    ip: Ipv4Addr::new(10, 0, 0, 2), hostname: None, mac: None,
                    os_guess: OsGuess::Unknown, ports: vec![], latency_ms: 1.0,
                    is_up: false, ttl: 64, vendor: None,
                },
            ],
            total_ips_scanned: 2, total_ports_scanned: 20, duration_secs: 0.5,
        };
        assert_eq!(result.hosts_up(), 1);
    }

    // --- ScanProgress ---

    #[test]
    fn test_scan_progress_fraction() {
        let progress = ScanProgress {
            phase: ScanPhase::PortScan,
            hosts_scanned: 5,
            total_hosts: 10,
            ports_scanned: 0,
            total_ports: 20,
            hosts_found: 3,
            elapsed_secs: 1.0,
        };
        let frac = progress.fraction();
        assert!(frac > 0.0 && frac <= 1.0);
    }

    #[test]
    fn test_scan_progress_fraction_zero_total() {
        let progress = ScanProgress {
            phase: ScanPhase::Discovery,
            hosts_scanned: 0, total_hosts: 0,
            ports_scanned: 0, total_ports: 0,
            hosts_found: 0, elapsed_secs: 0.0,
        };
        assert_eq!(progress.fraction(), 0.0);
    }

    // --- Vendor Guess ---

    #[test]
    fn test_vendor_guess_cisco() {
        let mac = MacAddr::new(0x00, 0x1A, 0x00, 0, 0, 0);
        assert_eq!(guess_vendor(&mac), Some("Cisco Systems".to_string()));
    }

    #[test]
    fn test_vendor_guess_vmware() {
        let mac = MacAddr::new(0x00, 0x50, 0x56, 0, 0, 0);
        assert_eq!(guess_vendor(&mac), Some("VMware".to_string()));
    }

    #[test]
    fn test_vendor_guess_unknown() {
        let mac = MacAddr::new(0xFF, 0xFF, 0xFF, 0, 0, 0);
        assert!(guess_vendor(&mac).is_none());
    }

    // --- SimRng ---

    #[test]
    fn test_sim_rng_deterministic() {
        let mut rng1 = SimRng::new(42);
        let mut rng2 = SimRng::new(42);
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_sim_rng_range() {
        let mut rng = SimRng::new(123);
        for _ in 0..100 {
            let val = rng.next_range(10, 20);
            assert!(val >= 10 && val <= 20);
        }
    }
}
