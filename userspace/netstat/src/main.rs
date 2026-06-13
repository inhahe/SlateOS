//! SlateOS `netstat` — network connection and socket statistics.
//!
//! Reads from `/proc/net/{tcp,udp,tcp6,udp6}` and related procfs paths
//! to display active connections, listening sockets, routing tables,
//! interface statistics, and protocol counters.
//!
//! Supports flags: -a, -t, -u, -l, -n, -p, -s, -r, -i, --json, --help.

#![deny(clippy::all)]
#![allow(clippy::manual_range_contains)] // clearer as explicit comparisons in some spots

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::process;

// ---------------------------------------------------------------------------
// TCP state mapping
// ---------------------------------------------------------------------------

/// TCP connection states as encoded in `/proc/net/tcp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum TcpState {
    Established = 1,
    SynSent = 2,
    SynRecv = 3,
    FinWait1 = 4,
    FinWait2 = 5,
    TimeWait = 6,
    Close = 7,
    CloseWait = 8,
    LastAck = 9,
    Listen = 10,
    Closing = 11,
}

impl TcpState {
    fn from_hex(s: &str) -> Option<Self> {
        let val = u8::from_str_radix(s.trim(), 16).ok()?;
        match val {
            1 => Some(Self::Established),
            2 => Some(Self::SynSent),
            3 => Some(Self::SynRecv),
            4 => Some(Self::FinWait1),
            5 => Some(Self::FinWait2),
            6 => Some(Self::TimeWait),
            7 => Some(Self::Close),
            8 => Some(Self::CloseWait),
            9 => Some(Self::LastAck),
            10 => Some(Self::Listen),
            11 => Some(Self::Closing),
            _ => None,
        }
    }
}

impl fmt::Display for TcpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Established => "ESTABLISHED",
            Self::SynSent => "SYN_SENT",
            Self::SynRecv => "SYN_RECV",
            Self::FinWait1 => "FIN_WAIT1",
            Self::FinWait2 => "FIN_WAIT2",
            Self::TimeWait => "TIME_WAIT",
            Self::Close => "CLOSE",
            Self::CloseWait => "CLOSE_WAIT",
            Self::LastAck => "LAST_ACK",
            Self::Listen => "LISTEN",
            Self::Closing => "CLOSING",
        };
        f.write_str(label)
    }
}

// ---------------------------------------------------------------------------
// Parsed connection entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Connection {
    protocol: String,
    local_addr: String,
    local_port: u16,
    remote_addr: String,
    remote_port: u16,
    state: Option<TcpState>,
    tx_queue: u32,
    rx_queue: u32,
    inode: u64,
    #[allow(dead_code)] // Available for permission display.
    uid: u32,
    pid: Option<u32>,
    program: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsed routing entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RouteEntry {
    destination: String,
    gateway: String,
    genmask: String,
    flags: String,
    metric: u32,
    _ref_cnt: u32,
    use_cnt: u32,
    iface: String,
}

// ---------------------------------------------------------------------------
// Parsed interface entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct IfaceEntry {
    name: String,
    mtu: u32,
    rx_bytes: u64,
    rx_packets: u64,
    rx_errors: u64,
    rx_dropped: u64,
    tx_bytes: u64,
    tx_packets: u64,
    tx_errors: u64,
    tx_dropped: u64,
}

// ---------------------------------------------------------------------------
// Protocol statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct ProtoStats {
    entries: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// CLI options
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Options {
    show_tcp: bool,
    show_udp: bool,
    listening_only: bool,
    numeric: bool,
    show_pid: bool,
    show_stats: bool,
    show_route: bool,
    show_iface: bool,
    json_output: bool,
    show_all: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            show_tcp: false,
            show_udp: false,
            listening_only: false,
            numeric: true, // default to numeric; DNS resolution is opt-in
            show_pid: false,
            show_stats: false,
            show_route: false,
            show_iface: false,
            json_output: false,
            show_all: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Address parsing helpers
// ---------------------------------------------------------------------------

/// Parse a hex-encoded IPv4 address from /proc/net/tcp format.
/// Format: `AABBCCDD:PORT` where the IP bytes are in host (little-endian) order.
fn parse_ipv4_addr(hex: &str) -> Option<(String, u16)> {
    let mut parts = hex.split(':');
    let ip_hex = parts.next()?;
    let port_hex = parts.next()?;

    if ip_hex.len() != 8 {
        return None;
    }

    // /proc/net/tcp stores IPv4 as a single 32-bit LE hex value.
    let ip_val = u32::from_str_radix(ip_hex, 16).ok()?;
    let addr = Ipv4Addr::from(ip_val.to_be());
    // The kernel writes the value in host byte order on LE machines,
    // so on x86_64 the bytes are already reversed. Convert by treating
    // the raw u32 as native-endian, then reading octets.
    let octets = [
        (ip_val & 0xFF) as u8,
        ((ip_val >> 8) & 0xFF) as u8,
        ((ip_val >> 16) & 0xFF) as u8,
        ((ip_val >> 24) & 0xFF) as u8,
    ];
    let _ = addr; // replaced by manual octet extraction
    let addr_str = format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]);

    let port = u16::from_str_radix(port_hex, 16).ok()?;
    Some((addr_str, port))
}

/// Parse a hex-encoded IPv6 address from /proc/net/tcp6 format.
/// Format: `AABBCCDD_EEFFGGHH_IIJJKKLL_MMNNOOPP:PORT` (32 hex chars + port).
fn parse_ipv6_addr(hex: &str) -> Option<(String, u16)> {
    let mut parts = hex.split(':');
    let ip_hex = parts.next()?;
    let port_hex = parts.next()?;

    if ip_hex.len() != 32 {
        return None;
    }

    // The kernel stores IPv6 as four 32-bit words in host byte order.
    let mut octets = [0u8; 16];
    for word_idx in 0..4 {
        let start = word_idx * 8;
        let word = u32::from_str_radix(&ip_hex[start..start + 8], 16).ok()?;
        // Each 32-bit word is in little-endian order on x86_64.
        let base = word_idx * 4;
        octets[base] = (word & 0xFF) as u8;
        octets[base + 1] = ((word >> 8) & 0xFF) as u8;
        octets[base + 2] = ((word >> 16) & 0xFF) as u8;
        octets[base + 3] = ((word >> 24) & 0xFF) as u8;
    }

    let addr = Ipv6Addr::from(octets);
    let port = u16::from_str_radix(port_hex, 16).ok()?;

    // Represent IPv4-mapped addresses in dotted form.
    if let Some(v4) = extract_mapped_v4(&addr) {
        Some((format!("::ffff:{v4}"), port))
    } else {
        Some((format!("{addr}"), port))
    }
}

/// If the address is an IPv4-mapped IPv6 address (::ffff:x.x.x.x), return the v4 part.
fn extract_mapped_v4(addr: &Ipv6Addr) -> Option<Ipv4Addr> {
    let segs = addr.segments();
    // ::ffff:x.x.x.x
    if segs[0] == 0
        && segs[1] == 0
        && segs[2] == 0
        && segs[3] == 0
        && segs[4] == 0
        && segs[5] == 0xFFFF
    {
        let hi = segs[6];
        let lo = segs[7];
        Some(Ipv4Addr::new(
            (hi >> 8) as u8,
            (hi & 0xFF) as u8,
            (lo >> 8) as u8,
            (lo & 0xFF) as u8,
        ))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// /proc/net/tcp and /proc/net/udp parser
// ---------------------------------------------------------------------------

fn parse_proc_net_file(path: &str, protocol: &str, is_v6: bool) -> Vec<Connection> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut connections = Vec::new();

    for line in content.lines().skip(1) {
        // skip header
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }

        // fields[1] = local_address, fields[2] = remote_address, fields[3] = state
        let (local_addr, local_port) = if is_v6 {
            match parse_ipv6_addr(fields[1]) {
                Some(v) => v,
                None => continue,
            }
        } else {
            match parse_ipv4_addr(fields[1]) {
                Some(v) => v,
                None => continue,
            }
        };

        let (remote_addr, remote_port) = if is_v6 {
            match parse_ipv6_addr(fields[2]) {
                Some(v) => v,
                None => continue,
            }
        } else {
            match parse_ipv4_addr(fields[2]) {
                Some(v) => v,
                None => continue,
            }
        };

        let state = TcpState::from_hex(fields[3]);

        // fields[4] = tx_queue:rx_queue
        let (tx_queue, rx_queue) = parse_queue_pair(fields[4]);

        // fields[7] = uid
        let uid = fields.get(7).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

        // fields[9] = inode
        let inode = fields.get(9).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

        connections.push(Connection {
            protocol: protocol.to_string(),
            local_addr,
            local_port,
            remote_addr,
            remote_port,
            state,
            tx_queue,
            rx_queue,
            inode,
            uid,
            pid: None,
            program: None,
        });
    }

    connections
}

fn parse_queue_pair(s: &str) -> (u32, u32) {
    let mut parts = s.split(':');
    let tx = parts
        .next()
        .and_then(|v| u32::from_str_radix(v, 16).ok())
        .unwrap_or(0);
    let rx = parts
        .next()
        .and_then(|v| u32::from_str_radix(v, 16).ok())
        .unwrap_or(0);
    (tx, rx)
}

// ---------------------------------------------------------------------------
// PID / program name resolution via /proc/<pid>/fd -> inode mapping
// ---------------------------------------------------------------------------

fn build_inode_to_pid_map() -> HashMap<u64, (u32, String)> {
    let mut map = HashMap::new();

    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return map,
    };

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Read the process command name.
        let comm = fs::read_to_string(format!("/proc/{pid}/comm"))
            .unwrap_or_default()
            .trim()
            .to_string();

        // Scan the fd directory for socket inodes.
        let fd_path = format!("/proc/{pid}/fd");
        let fd_dir = match fs::read_dir(&fd_path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        for fd_entry in fd_dir.flatten() {
            let link = match fs::read_link(fd_entry.path()) {
                Ok(l) => l,
                Err(_) => continue,
            };
            let link_str = link.to_string_lossy().to_string();
            // Socket links look like: socket:[12345]
            if let Some(inode_str) = link_str
                .strip_prefix("socket:[")
                .and_then(|s| s.strip_suffix(']'))
                && let Ok(inode) = inode_str.parse::<u64>() {
                    map.insert(inode, (pid, comm.clone()));
                }
        }
    }

    map
}

// ---------------------------------------------------------------------------
// Routing table parser (/proc/net/route)
// ---------------------------------------------------------------------------

fn parse_route_table() -> Vec<RouteEntry> {
    let content = match fs::read_to_string("/proc/net/route") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut routes = Vec::new();

    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 8 {
            // Also try whitespace-split as a fallback.
            let ws_fields: Vec<&str> = line.split_whitespace().collect();
            if ws_fields.len() >= 8
                && let Some(entry) = parse_route_fields(&ws_fields) {
                    routes.push(entry);
                }
            continue;
        }
        if let Some(entry) = parse_route_fields(&fields) {
            routes.push(entry);
        }
    }

    routes
}

fn parse_route_fields(fields: &[&str]) -> Option<RouteEntry> {
    if fields.len() < 8 {
        return None;
    }

    let iface = fields[0].trim().to_string();
    let destination = hex_to_ipv4_route(fields[1].trim());
    let gateway = hex_to_ipv4_route(fields[2].trim());
    let flags_val = u32::from_str_radix(fields[3].trim(), 16).unwrap_or(0);
    let _ref_cnt = fields[4].trim().parse::<u32>().unwrap_or(0);
    let use_cnt = fields[5].trim().parse::<u32>().unwrap_or(0);
    let metric = fields[6].trim().parse::<u32>().unwrap_or(0);
    let genmask = hex_to_ipv4_route(fields[7].trim());

    let mut flags = String::new();
    if flags_val & 0x0001 != 0 {
        flags.push('U');
    }
    if flags_val & 0x0002 != 0 {
        flags.push('G');
    }
    if flags_val & 0x0004 != 0 {
        flags.push('H');
    }

    Some(RouteEntry {
        destination,
        gateway,
        genmask,
        flags,
        metric,
        _ref_cnt,
        use_cnt,
        iface,
    })
}

fn hex_to_ipv4_route(hex: &str) -> String {
    let val = u32::from_str_radix(hex, 16).unwrap_or(0);
    let a = (val & 0xFF) as u8;
    let b = ((val >> 8) & 0xFF) as u8;
    let c = ((val >> 16) & 0xFF) as u8;
    let d = ((val >> 24) & 0xFF) as u8;
    format!("{a}.{b}.{c}.{d}")
}

// ---------------------------------------------------------------------------
// Interface statistics parser (/proc/net/dev)
// ---------------------------------------------------------------------------

fn parse_iface_stats() -> Vec<IfaceEntry> {
    let content = match fs::read_to_string("/proc/net/dev") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut ifaces = Vec::new();

    // Skip the first two header lines.
    for line in content.lines().skip(2) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Format: "iface: rx_bytes rx_packets rx_errs rx_drop ... tx_bytes tx_packets tx_errs tx_drop ..."
        let colon_pos = match trimmed.find(':') {
            Some(p) => p,
            None => continue,
        };

        let name = trimmed[..colon_pos].trim().to_string();
        let rest = &trimmed[colon_pos + 1..];
        let nums: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();

        if nums.len() < 16 {
            continue;
        }

        ifaces.push(IfaceEntry {
            name,
            mtu: read_iface_mtu(trimmed[..colon_pos].trim()),
            rx_bytes: nums[0],
            rx_packets: nums[1],
            rx_errors: nums[2],
            rx_dropped: nums[3],
            tx_bytes: nums[8],
            tx_packets: nums[9],
            tx_errors: nums[10],
            tx_dropped: nums[11],
        });
    }

    ifaces
}

fn read_iface_mtu(iface_name: &str) -> u32 {
    let path = format!("/sys/class/net/{iface_name}/mtu");
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Protocol statistics (/proc/net/snmp, /proc/net/netstat)
// ---------------------------------------------------------------------------

fn parse_protocol_stats() -> HashMap<String, ProtoStats> {
    let mut all_stats: HashMap<String, ProtoStats> = HashMap::new();

    // /proc/net/snmp contains pairs of lines: header then values.
    for path in &["/proc/net/snmp", "/proc/net/netstat"] {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;
        while i + 1 < lines.len() {
            let header_line = lines[i];
            let value_line = lines[i + 1];

            let headers: Vec<&str> = header_line.split_whitespace().collect();
            let values: Vec<&str> = value_line.split_whitespace().collect();

            if headers.len() >= 2
                && values.len() >= 2
                && headers.len() == values.len()
            {
                // First element is protocol name with colon, e.g. "Tcp:"
                let proto_hdr = headers[0].trim_end_matches(':');
                let proto_val = values[0].trim_end_matches(':');

                if proto_hdr == proto_val {
                    let stats = all_stats
                        .entry(proto_hdr.to_string())
                        .or_default();

                    for j in 1..headers.len() {
                        stats.entries.push((
                            headers[j].to_string(),
                            values[j].to_string(),
                        ));
                    }
                }
            }

            i += 2;
        }
    }

    all_stats
}

// ---------------------------------------------------------------------------
// Display: service/port name lookup (common ports only, no external db)
// ---------------------------------------------------------------------------

fn port_to_service(port: u16) -> Option<&'static str> {
    match port {
        20 => Some("ftp-data"),
        21 => Some("ftp"),
        22 => Some("ssh"),
        23 => Some("telnet"),
        25 => Some("smtp"),
        53 => Some("domain"),
        67 => Some("bootps"),
        68 => Some("bootpc"),
        80 => Some("http"),
        110 => Some("pop3"),
        119 => Some("nntp"),
        123 => Some("ntp"),
        143 => Some("imap"),
        161 => Some("snmp"),
        162 => Some("snmptrap"),
        389 => Some("ldap"),
        443 => Some("https"),
        465 => Some("smtps"),
        514 => Some("syslog"),
        587 => Some("submission"),
        636 => Some("ldaps"),
        993 => Some("imaps"),
        995 => Some("pop3s"),
        1080 => Some("socks"),
        1433 => Some("ms-sql"),
        1521 => Some("oracle"),
        3306 => Some("mysql"),
        3389 => Some("rdp"),
        5432 => Some("postgresql"),
        5900 => Some("vnc"),
        6379 => Some("redis"),
        8080 => Some("http-alt"),
        8443 => Some("https-alt"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Address formatting
// ---------------------------------------------------------------------------

fn format_addr(addr: &str, port: u16, numeric: bool) -> String {
    let port_display = if numeric {
        format!("{port}")
    } else {
        port_to_service(port)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{port}"))
    };

    let addr_display = if (addr == "0.0.0.0" || addr == "::") && !numeric {
        "*".to_string()
    } else {
        addr.to_string()
    };

    format!("{addr_display}:{port_display}")
}

// ---------------------------------------------------------------------------
// JSON output helpers
// ---------------------------------------------------------------------------

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn connections_to_json(connections: &[Connection]) -> String {
    let mut out = String::from("[\n");
    for (i, c) in connections.iter().enumerate() {
        out.push_str("  {\n");
        out.push_str(&format!(
            "    \"protocol\": \"{}\",\n",
            json_escape(&c.protocol)
        ));
        out.push_str(&format!(
            "    \"local_address\": \"{}\",\n",
            json_escape(&c.local_addr)
        ));
        out.push_str(&format!("    \"local_port\": {},\n", c.local_port));
        out.push_str(&format!(
            "    \"remote_address\": \"{}\",\n",
            json_escape(&c.remote_addr)
        ));
        out.push_str(&format!("    \"remote_port\": {},\n", c.remote_port));
        if let Some(ref st) = c.state {
            out.push_str(&format!("    \"state\": \"{st}\",\n"));
        } else {
            out.push_str("    \"state\": null,\n");
        }
        out.push_str(&format!("    \"tx_queue\": {},\n", c.tx_queue));
        out.push_str(&format!("    \"rx_queue\": {},\n", c.rx_queue));
        if let Some(pid) = c.pid {
            out.push_str(&format!("    \"pid\": {pid},\n"));
            let prog = c
                .program
                .as_deref()
                .unwrap_or("-");
            out.push_str(&format!(
                "    \"program\": \"{}\"\n",
                json_escape(prog)
            ));
        } else {
            out.push_str("    \"pid\": null,\n");
            out.push_str("    \"program\": null\n");
        }
        if i + 1 < connections.len() {
            out.push_str("  },\n");
        } else {
            out.push_str("  }\n");
        }
    }
    out.push(']');
    out
}

fn routes_to_json(routes: &[RouteEntry]) -> String {
    let mut out = String::from("[\n");
    for (i, r) in routes.iter().enumerate() {
        out.push_str("  {\n");
        out.push_str(&format!(
            "    \"destination\": \"{}\",\n",
            json_escape(&r.destination)
        ));
        out.push_str(&format!(
            "    \"gateway\": \"{}\",\n",
            json_escape(&r.gateway)
        ));
        out.push_str(&format!(
            "    \"genmask\": \"{}\",\n",
            json_escape(&r.genmask)
        ));
        out.push_str(&format!(
            "    \"flags\": \"{}\",\n",
            json_escape(&r.flags)
        ));
        out.push_str(&format!("    \"metric\": {},\n", r.metric));
        out.push_str(&format!(
            "    \"interface\": \"{}\"\n",
            json_escape(&r.iface)
        ));
        if i + 1 < routes.len() {
            out.push_str("  },\n");
        } else {
            out.push_str("  }\n");
        }
    }
    out.push(']');
    out
}

fn ifaces_to_json(ifaces: &[IfaceEntry]) -> String {
    let mut out = String::from("[\n");
    for (i, iface) in ifaces.iter().enumerate() {
        out.push_str("  {\n");
        out.push_str(&format!(
            "    \"name\": \"{}\",\n",
            json_escape(&iface.name)
        ));
        out.push_str(&format!("    \"mtu\": {},\n", iface.mtu));
        out.push_str(&format!("    \"rx_bytes\": {},\n", iface.rx_bytes));
        out.push_str(&format!("    \"rx_packets\": {},\n", iface.rx_packets));
        out.push_str(&format!("    \"rx_errors\": {},\n", iface.rx_errors));
        out.push_str(&format!("    \"rx_dropped\": {},\n", iface.rx_dropped));
        out.push_str(&format!("    \"tx_bytes\": {},\n", iface.tx_bytes));
        out.push_str(&format!("    \"tx_packets\": {},\n", iface.tx_packets));
        out.push_str(&format!("    \"tx_errors\": {},\n", iface.tx_errors));
        out.push_str(&format!("    \"tx_dropped\": {}\n", iface.tx_dropped));
        if i + 1 < ifaces.len() {
            out.push_str("  },\n");
        } else {
            out.push_str("  }\n");
        }
    }
    out.push(']');
    out
}

fn stats_to_json(all_stats: &HashMap<String, ProtoStats>) -> String {
    let mut out = String::from("{\n");
    let mut keys: Vec<&String> = all_stats.keys().collect();
    keys.sort();
    for (ki, key) in keys.iter().enumerate() {
        let stats = &all_stats[*key];
        out.push_str(&format!("  \"{}\": {{\n", json_escape(key)));
        for (i, (name, val)) in stats.entries.iter().enumerate() {
            out.push_str(&format!(
                "    \"{}\": {}",
                json_escape(name),
                json_escape(val)
            ));
            if i + 1 < stats.entries.len() {
                out.push_str(",\n");
            } else {
                out.push('\n');
            }
        }
        if ki + 1 < keys.len() {
            out.push_str("  },\n");
        } else {
            out.push_str("  }\n");
        }
    }
    out.push('}');
    out
}

// ---------------------------------------------------------------------------
// Display: table-formatted output
// ---------------------------------------------------------------------------

fn print_connections(
    stdout: &mut io::StdoutLock<'_>,
    connections: &[Connection],
    opts: &Options,
) {
    if opts.json_output {
        let _ = writeln!(stdout, "{}", connections_to_json(connections));
        return;
    }

    // Header
    if opts.show_pid {
        let _ = writeln!(
            stdout,
            "{:<6} {:<6} {:<6} {:<25} {:<25} {:<12} PID/Program",
            "Proto", "Recv-Q", "Send-Q", "Local Address", "Foreign Address", "State"
        );
    } else {
        let _ = writeln!(
            stdout,
            "{:<6} {:<6} {:<6} {:<25} {:<25} {:<12}",
            "Proto", "Recv-Q", "Send-Q", "Local Address", "Foreign Address", "State"
        );
    }

    for c in connections {
        let local = format_addr(&c.local_addr, c.local_port, opts.numeric);
        let remote = format_addr(&c.remote_addr, c.remote_port, opts.numeric);
        let state_str = c
            .state
            .as_ref()
            .map(|s| format!("{s}"))
            .unwrap_or_default();

        if opts.show_pid {
            let pid_prog = match (c.pid, c.program.as_deref()) {
                (Some(pid), Some(prog)) => format!("{pid}/{prog}"),
                (Some(pid), None) => format!("{pid}/-"),
                _ => "-".to_string(),
            };
            let _ = writeln!(
                stdout,
                "{:<6} {:<6} {:<6} {:<25} {:<25} {:<12} {}",
                c.protocol, c.rx_queue, c.tx_queue, local, remote, state_str, pid_prog
            );
        } else {
            let _ = writeln!(
                stdout,
                "{:<6} {:<6} {:<6} {:<25} {:<25} {:<12}",
                c.protocol, c.rx_queue, c.tx_queue, local, remote, state_str
            );
        }
    }
}

fn print_route_table(stdout: &mut io::StdoutLock<'_>, routes: &[RouteEntry], json: bool) {
    if json {
        let _ = writeln!(stdout, "{}", routes_to_json(routes));
        return;
    }

    let _ = writeln!(stdout, "Kernel IP routing table");
    let _ = writeln!(
        stdout,
        "{:<18} {:<18} {:<18} {:<6} {:<6} {:<4} Iface",
        "Destination", "Gateway", "Genmask", "Flags", "Metric", "Use"
    );

    for r in routes {
        let _ = writeln!(
            stdout,
            "{:<18} {:<18} {:<18} {:<6} {:<6} {:<4} {}",
            r.destination, r.gateway, r.genmask, r.flags, r.metric, r.use_cnt, r.iface
        );
    }
}

fn print_iface_table(stdout: &mut io::StdoutLock<'_>, ifaces: &[IfaceEntry], json: bool) {
    if json {
        let _ = writeln!(stdout, "{}", ifaces_to_json(ifaces));
        return;
    }

    let _ = writeln!(stdout, "Kernel Interface table");
    let _ = writeln!(
        stdout,
        "{:<12} {:<6} {:<12} {:<12} {:<10} {:<10} {:<12} {:<12} {:<10} {:<10}",
        "Iface", "MTU", "RX-Bytes", "RX-Pkts", "RX-Err", "RX-Drop",
        "TX-Bytes", "TX-Pkts", "TX-Err", "TX-Drop"
    );

    for iface in ifaces {
        let _ = writeln!(
            stdout,
            "{:<12} {:<6} {:<12} {:<12} {:<10} {:<10} {:<12} {:<12} {:<10} {:<10}",
            iface.name,
            iface.mtu,
            iface.rx_bytes,
            iface.rx_packets,
            iface.rx_errors,
            iface.rx_dropped,
            iface.tx_bytes,
            iface.tx_packets,
            iface.tx_errors,
            iface.tx_dropped
        );
    }
}

fn print_protocol_stats(
    stdout: &mut io::StdoutLock<'_>,
    all_stats: &HashMap<String, ProtoStats>,
    json: bool,
) {
    if json {
        let _ = writeln!(stdout, "{}", stats_to_json(all_stats));
        return;
    }

    let mut keys: Vec<&String> = all_stats.keys().collect();
    keys.sort();

    for key in keys {
        let stats = &all_stats[key];
        let _ = writeln!(stdout, "{key}:");
        for (name, val) in &stats.entries {
            let _ = writeln!(stdout, "    {name}: {val}");
        }
        let _ = writeln!(stdout);
    }
}

// ---------------------------------------------------------------------------
// Usage / help
// ---------------------------------------------------------------------------

fn print_usage(stdout: &mut io::StdoutLock<'_>) {
    let _ = writeln!(stdout, "Usage: netstat [OPTIONS]");
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "Display network connections, routing tables, interface statistics,");
    let _ = writeln!(stdout, "and protocol statistics.");
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "Options:");
    let _ = writeln!(stdout, "  -a, --all         Show all sockets (listening and non-listening)");
    let _ = writeln!(stdout, "  -t, --tcp         Show TCP connections only");
    let _ = writeln!(stdout, "  -u, --udp         Show UDP sockets only");
    let _ = writeln!(stdout, "  -l, --listening   Show only listening sockets");
    let _ = writeln!(stdout, "  -n, --numeric     Show numeric addresses (no name resolution)");
    let _ = writeln!(stdout, "  -p, --programs    Show PID and program names");
    let _ = writeln!(stdout, "  -s, --statistics  Show protocol statistics");
    let _ = writeln!(stdout, "  -r, --route       Show the kernel routing table");
    let _ = writeln!(stdout, "  -i, --interfaces  Show interface statistics table");
    let _ = writeln!(stdout, "      --json        Output in JSON format");
    let _ = writeln!(stdout, "  -h, --help        Display this help message");
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "Flags can be combined, e.g.: netstat -tulnp");
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options::default();
    let mut had_protocol_flag = false;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                let stdout_handle = io::stdout();
                let mut stdout = stdout_handle.lock();
                print_usage(&mut stdout);
                process::exit(0);
            }
            "--json" => {
                opts.json_output = true;
            }
            "--all" => {
                opts.show_all = true;
            }
            "--tcp" => {
                opts.show_tcp = true;
                had_protocol_flag = true;
            }
            "--udp" => {
                opts.show_udp = true;
                had_protocol_flag = true;
            }
            "--listening" => {
                opts.listening_only = true;
            }
            "--numeric" => {
                opts.numeric = true;
            }
            "--programs" => {
                opts.show_pid = true;
            }
            "--statistics" => {
                opts.show_stats = true;
            }
            "--route" => {
                opts.show_route = true;
            }
            "--interfaces" => {
                opts.show_iface = true;
            }
            s if s.starts_with('-') && !s.starts_with("--") => {
                // Short flags: can be combined, e.g. -tulnp
                for ch in s[1..].chars() {
                    match ch {
                        'a' => opts.show_all = true,
                        't' => {
                            opts.show_tcp = true;
                            had_protocol_flag = true;
                        }
                        'u' => {
                            opts.show_udp = true;
                            had_protocol_flag = true;
                        }
                        'l' => opts.listening_only = true,
                        'n' => opts.numeric = true,
                        'p' => opts.show_pid = true,
                        's' => opts.show_stats = true,
                        'r' => opts.show_route = true,
                        'i' => opts.show_iface = true,
                        'h' => {
                            let stdout_handle = io::stdout();
                            let mut stdout = stdout_handle.lock();
                            print_usage(&mut stdout);
                            process::exit(0);
                        }
                        other => {
                            return Err(format!("Unknown option: -{other}"));
                        }
                    }
                }
            }
            other => {
                return Err(format!("Unknown argument: {other}"));
            }
        }
    }

    // If no protocol flag was given and we are showing connections,
    // default to showing both TCP and UDP.
    if !had_protocol_flag && !opts.show_stats && !opts.show_route && !opts.show_iface {
        opts.show_tcp = true;
        opts.show_udp = true;
    }

    Ok(opts)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(io::stderr(), "netstat: {e}");
            let _ = writeln!(io::stderr(), "Try 'netstat --help' for more information.");
            process::exit(1);
        }
    };

    let stdout_handle = io::stdout();
    let mut stdout = stdout_handle.lock();

    // -s: protocol statistics
    if opts.show_stats {
        let stats = parse_protocol_stats();
        if stats.is_empty() {
            let _ = writeln!(stdout, "No protocol statistics available.");
        } else {
            print_protocol_stats(&mut stdout, &stats, opts.json_output);
        }
        return;
    }

    // -r: routing table
    if opts.show_route {
        let routes = parse_route_table();
        if routes.is_empty() {
            let _ = writeln!(stdout, "No routing entries found.");
        } else {
            print_route_table(&mut stdout, &routes, opts.json_output);
        }
        return;
    }

    // -i: interface table
    if opts.show_iface {
        let ifaces = parse_iface_stats();
        if ifaces.is_empty() {
            let _ = writeln!(stdout, "No interface statistics available.");
        } else {
            print_iface_table(&mut stdout, &ifaces, opts.json_output);
        }
        return;
    }

    // Default: show connections
    let mut connections = Vec::new();

    if opts.show_tcp {
        connections.extend(parse_proc_net_file("/proc/net/tcp", "tcp", false));
        connections.extend(parse_proc_net_file("/proc/net/tcp6", "tcp6", true));
    }

    if opts.show_udp {
        connections.extend(parse_proc_net_file("/proc/net/udp", "udp", false));
        connections.extend(parse_proc_net_file("/proc/net/udp6", "udp6", true));
    }

    // Filter: listening only
    if opts.listening_only {
        connections.retain(|c| {
            c.state == Some(TcpState::Listen)
                || c.protocol.starts_with("udp")
        });
    } else if !opts.show_all {
        // By default (no -a), show established + listening for TCP,
        // and all UDP sockets.
        connections.retain(|c| {
            c.protocol.starts_with("udp")
                || c.state == Some(TcpState::Established)
                || c.state == Some(TcpState::Listen)
        });
    }

    // Resolve PIDs if requested
    if opts.show_pid {
        let inode_map = build_inode_to_pid_map();
        for conn in &mut connections {
            if let Some((pid, prog)) = inode_map.get(&conn.inode) {
                conn.pid = Some(*pid);
                conn.program = Some(prog.clone());
            }
        }
    }

    if connections.is_empty() {
        if !opts.json_output {
            let _ = writeln!(stdout, "Active Internet connections");
            let _ = writeln!(stdout, "(No connections found)");
        } else {
            let _ = writeln!(stdout, "[]");
        }
    } else {
        if !opts.json_output {
            let _ = writeln!(stdout, "Active Internet connections");
        }
        print_connections(&mut stdout, &connections, &opts);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipv4_addr_loopback() {
        // 0100007F = 127.0.0.1 in LE hex
        let result = parse_ipv4_addr("0100007F:0050");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "127.0.0.1");
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_ipv4_addr_any() {
        let result = parse_ipv4_addr("00000000:0016");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "0.0.0.0");
        assert_eq!(port, 22);
    }

    #[test]
    fn test_parse_ipv4_addr_specific() {
        // 192.168.1.100 = C0.A8.01.64 -> LE hex = 6401A8C0
        let result = parse_ipv4_addr("6401A8C0:1F90");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "192.168.1.100");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_parse_ipv4_addr_invalid() {
        assert!(parse_ipv4_addr("ZZZZ:0050").is_none());
        assert!(parse_ipv4_addr("0100007F").is_none());
        assert!(parse_ipv4_addr("").is_none());
    }

    #[test]
    fn test_parse_ipv6_addr_loopback() {
        // ::1 in /proc format: 00000000000000000000000001000000
        let result = parse_ipv6_addr("00000000000000000000000001000000:0050");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "::1");
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_ipv6_addr_any() {
        let result = parse_ipv6_addr("00000000000000000000000000000000:0016");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "::");
        assert_eq!(port, 22);
    }

    #[test]
    fn test_tcp_state_from_hex() {
        assert_eq!(TcpState::from_hex("01"), Some(TcpState::Established));
        assert_eq!(TcpState::from_hex("0A"), Some(TcpState::Listen));
        assert_eq!(TcpState::from_hex("06"), Some(TcpState::TimeWait));
        assert_eq!(TcpState::from_hex("0B"), Some(TcpState::Closing));
        assert_eq!(TcpState::from_hex("00"), None);
        assert_eq!(TcpState::from_hex("0C"), None);
        assert_eq!(TcpState::from_hex("FF"), None);
    }

    #[test]
    fn test_tcp_state_display() {
        assert_eq!(format!("{}", TcpState::Established), "ESTABLISHED");
        assert_eq!(format!("{}", TcpState::Listen), "LISTEN");
        assert_eq!(format!("{}", TcpState::TimeWait), "TIME_WAIT");
        assert_eq!(format!("{}", TcpState::CloseWait), "CLOSE_WAIT");
        assert_eq!(format!("{}", TcpState::SynSent), "SYN_SENT");
    }

    #[test]
    fn test_format_addr_numeric() {
        assert_eq!(format_addr("127.0.0.1", 80, true), "127.0.0.1:80");
        assert_eq!(format_addr("0.0.0.0", 22, true), "0.0.0.0:22");
    }

    #[test]
    fn test_format_addr_symbolic() {
        assert_eq!(format_addr("0.0.0.0", 80, false), "*:http");
        assert_eq!(format_addr("127.0.0.1", 443, false), "127.0.0.1:https");
        assert_eq!(format_addr("::", 22, false), "*:ssh");
        assert_eq!(format_addr("10.0.0.1", 9999, false), "10.0.0.1:9999");
    }

    #[test]
    fn test_port_to_service() {
        assert_eq!(port_to_service(22), Some("ssh"));
        assert_eq!(port_to_service(80), Some("http"));
        assert_eq!(port_to_service(443), Some("https"));
        assert_eq!(port_to_service(3306), Some("mysql"));
        assert_eq!(port_to_service(12345), None);
    }

    #[test]
    fn test_parse_queue_pair() {
        assert_eq!(parse_queue_pair("00000000:00000000"), (0, 0));
        assert_eq!(parse_queue_pair("00000001:00000002"), (1, 2));
        assert_eq!(parse_queue_pair("0000FFFF:00000100"), (0xFFFF, 0x100));
    }

    #[test]
    fn test_hex_to_ipv4_route() {
        assert_eq!(hex_to_ipv4_route("00000000"), "0.0.0.0");
        assert_eq!(hex_to_ipv4_route("0100A8C0"), "192.168.0.1");
    }

    #[test]
    fn test_json_escape() {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("he\"llo"), "he\\\"llo");
        assert_eq!(json_escape("line\nnew"), "line\\nnew");
        assert_eq!(json_escape("tab\there"), "tab\\there");
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn test_parse_args_defaults() {
        let opts = parse_args(&[]).unwrap();
        assert!(opts.show_tcp);
        assert!(opts.show_udp);
        assert!(!opts.listening_only);
        assert!(opts.numeric);
        assert!(!opts.show_pid);
        assert!(!opts.show_stats);
        assert!(!opts.show_route);
        assert!(!opts.show_iface);
        assert!(!opts.json_output);
    }

    #[test]
    fn test_parse_args_tcp_only() {
        let args = vec!["-t".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.show_tcp);
        assert!(!opts.show_udp);
    }

    #[test]
    fn test_parse_args_combined() {
        let args = vec!["-tulnp".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.show_tcp);
        assert!(opts.show_udp);
        assert!(opts.listening_only);
        assert!(opts.numeric);
        assert!(opts.show_pid);
    }

    #[test]
    fn test_parse_args_long_flags() {
        let args = vec![
            "--tcp".to_string(),
            "--listening".to_string(),
            "--json".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(opts.show_tcp);
        assert!(!opts.show_udp);
        assert!(opts.listening_only);
        assert!(opts.json_output);
    }

    #[test]
    fn test_parse_args_unknown() {
        let args = vec!["-z".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_connections_to_json_empty() {
        let json = connections_to_json(&[]);
        assert_eq!(json, "[\n]");
    }

    #[test]
    fn test_connections_to_json_single() {
        let conn = Connection {
            protocol: "tcp".to_string(),
            local_addr: "127.0.0.1".to_string(),
            local_port: 80,
            remote_addr: "10.0.0.1".to_string(),
            remote_port: 45678,
            state: Some(TcpState::Established),
            tx_queue: 0,
            rx_queue: 0,
            inode: 12345,
            uid: 0,
            pid: Some(1234),
            program: Some("httpd".to_string()),
        };
        let json = connections_to_json(&[conn]);
        assert!(json.contains("\"protocol\": \"tcp\""));
        assert!(json.contains("\"state\": \"ESTABLISHED\""));
        assert!(json.contains("\"pid\": 1234"));
        assert!(json.contains("\"program\": \"httpd\""));
    }

    #[test]
    fn test_extract_mapped_v4() {
        let addr: Ipv6Addr = "::ffff:192.168.1.1".parse().unwrap();
        let v4 = extract_mapped_v4(&addr);
        assert!(v4.is_some());
        assert_eq!(v4.unwrap(), Ipv4Addr::new(192, 168, 1, 1));
    }

    #[test]
    fn test_extract_mapped_v4_not_mapped() {
        let addr: Ipv6Addr = "::1".parse().unwrap();
        assert!(extract_mapped_v4(&addr).is_none());

        let addr2: Ipv6Addr = "fe80::1".parse().unwrap();
        assert!(extract_mapped_v4(&addr2).is_none());
    }
}
