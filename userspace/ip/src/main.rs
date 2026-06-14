//! Slate OS Network Configuration Utility
//!
//! Configure network interfaces, IP addresses, routes, and DNS.
//! Reads live state from /proc/net/ and /sys/class/net/, writes
//! configuration via syscalls.
//!
//! # Usage
//!
//! ```text
//! ip link                        List interfaces
//! ip link set <iface> up|down    Bring interface up/down
//! ip addr                        Show all addresses
//! ip addr show <iface>           Show addresses for interface
//! ip addr add <ip/mask> <iface>  Add address to interface
//! ip addr del <ip/mask> <iface>  Remove address from interface
//! ip route                       Show routing table
//! ip route add <dest> via <gw>   Add a route
//! ip route del <dest>            Remove a route
//! ip neigh                       Show ARP/neighbor table
//! ip dns                         Show DNS servers
//! ip dns add <server>            Add DNS server
//! ip stats <iface>               Show interface statistics
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

/// Read-only interface-info query syscall (`kernel/src/syscall/number.rs`,
/// `SYS_NET_IF_INFO`). Returns a fixed 24-byte record describing the default
/// network interface. This is the live read source: the kernel does not yet
/// populate `/sys/class/net/`, `/proc/net/dev`, or `/proc/net/route`, so the
/// show paths fall back to this syscall (TD18 read-path wiring).
const SYS_NET_IF_INFO: u64 = 842;

// IOCTL sub-commands for network configuration.
const NET_IF_UP: u64 = 1;
const NET_IF_DOWN: u64 = 2;
const NET_IF_SET_IP: u64 = 3;
#[allow(dead_code)] // Used when full ioctl support is wired up.
const NET_IF_SET_MASK: u64 = 4;
#[allow(dead_code)]
const NET_IF_SET_GW: u64 = 5;
const NET_ROUTE_ADD: u64 = 10;
const NET_ROUTE_DEL: u64 = 11;
#[allow(dead_code)] // DNS add/del use resolv.conf directly for now.
const NET_DNS_ADD: u64 = 20;
#[allow(dead_code)]
const NET_DNS_DEL: u64 = 21;

#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issue a network-configuration ioctl.
///
/// Slate OS does not yet expose a net-config syscall. The number these tools
/// previously used (810) is actually `SYS_UDP_BIND` in the kernel, so issuing
/// the syscall here bound a UDP socket to a bogus port (the `cmd` value),
/// leaked the resulting handle, and reported false success. Until the
/// net-config ABI lands (operator decision, TD18), interface/route
/// configuration is unsupported: return `ENOSYS` without touching the kernel
/// rather than corrupting UDP socket state.
fn net_ioctl(_cmd: u64, _iface: &str, _arg: u64) -> i64 {
    -38 // ENOSYS — net-config syscall ABI not yet defined (see note above).
}

// ============================================================================
// Kernel interface-info query (SYS_NET_IF_INFO, read-only)
// ============================================================================

/// Size of the `SYS_NET_IF_INFO` record (must match the kernel's `INFO_SIZE`).
const NET_IF_INFO_SIZE: usize = 24;

/// Decoded form of the kernel's 24-byte `SYS_NET_IF_INFO` record.
///
/// Layout (see `kernel/src/syscall/handlers.rs::sys_net_if_info`):
/// `[0..4]` IPv4 address, `[4..8]` subnet mask, `[8..12]` gateway,
/// `[12..16]` DNS server, `[16..22]` MAC, `[22]` up flag, `[23]` reserved.
struct NetIfInfo {
    ip: [u8; 4],
    mask: [u8; 4],
    gateway: [u8; 4],
    mac: [u8; 6],
    up: bool,
}

/// Decode a raw `SYS_NET_IF_INFO` record. Pure (no syscall) so it is unit-test
/// friendly on the host.
fn parse_net_if_info(rec: &[u8; NET_IF_INFO_SIZE]) -> NetIfInfo {
    let mut ip = [0u8; 4];
    ip.copy_from_slice(&rec[0..4]);
    let mut mask = [0u8; 4];
    mask.copy_from_slice(&rec[4..8]);
    let mut gateway = [0u8; 4];
    gateway.copy_from_slice(&rec[8..12]);
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&rec[16..22]);
    NetIfInfo {
        ip,
        mask,
        gateway,
        mac,
        up: rec[22] != 0,
    }
}

/// True if a 4-byte address is all zeros (i.e. unconfigured).
fn is_zero4(addr: [u8; 4]) -> bool {
    addr == [0, 0, 0, 0]
}

/// Format a 4-byte address as a dotted quad.
fn fmt_ipv4(addr: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3])
}

/// Format a 6-byte MAC address as colon-separated lowercase hex.
fn fmt_mac(mac: [u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

/// Compute the directed broadcast address for an `ip`/`mask` pair:
/// `(ip & mask) | !mask`.
fn compute_broadcast(ip: [u8; 4], mask: [u8; 4]) -> [u8; 4] {
    [
        (ip[0] & mask[0]) | !mask[0],
        (ip[1] & mask[1]) | !mask[1],
        (ip[2] & mask[2]) | !mask[2],
        (ip[3] & mask[3]) | !mask[3],
    ]
}

/// Synthesize an [`InterfaceInfo`] for the default interface from a decoded
/// kernel record. The kernel record carries no name or byte counters, so we use
/// the conventional `eth0` name and leave the counters at zero rather than
/// fabricating traffic statistics.
fn interface_from_net_if_info(info: &NetIfInfo) -> InterfaceInfo {
    let has_ip = !is_zero4(info.ip);
    InterfaceInfo {
        name: "eth0".to_string(),
        state: if info.up { "up" } else { "down" }.to_string(),
        mac: fmt_mac(info.mac),
        mtu: 1500,
        ip_addr: if has_ip { fmt_ipv4(info.ip) } else { String::new() },
        netmask: if has_ip {
            fmt_ipv4(info.mask)
        } else {
            String::new()
        },
        broadcast: if has_ip {
            fmt_ipv4(compute_broadcast(info.ip, info.mask))
        } else {
            String::new()
        },
        rx_bytes: 0,
        rx_packets: 0,
        rx_errors: 0,
        tx_bytes: 0,
        tx_packets: 0,
        tx_errors: 0,
    }
}

/// Synthesize the default route from a decoded kernel record. Returns `None`
/// when no gateway is configured (no default route to report).
fn route_from_net_if_info(info: &NetIfInfo) -> Option<RouteEntry> {
    if is_zero4(info.gateway) {
        return None;
    }
    Some(RouteEntry {
        destination: "0.0.0.0".to_string(),
        gateway: fmt_ipv4(info.gateway),
        mask: "0.0.0.0".to_string(),
        iface: "eth0".to_string(),
        metric: 0,
        flags: String::new(),
    })
}

/// Query the kernel for the default interface's configuration via
/// `SYS_NET_IF_INFO`. Returns `None` if the syscall fails.
fn query_net_if_info() -> Option<NetIfInfo> {
    let mut buf = [0u8; NET_IF_INFO_SIZE];
    // SAFETY: `buf` is exactly NET_IF_INFO_SIZE bytes, satisfying the kernel's
    // minimum-length contract; the kernel writes at most that many bytes.
    let ret = unsafe {
        syscall4(
            SYS_NET_IF_INFO,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            0,
            0,
        )
    };
    if ret < 0 {
        return None;
    }
    Some(parse_net_if_info(&buf))
}

// ============================================================================
// Kernel ARP-table query (SYS_ARP_TABLE, read-only)
// ============================================================================

/// Read-only ARP-cache query syscall (`kernel/src/syscall/number.rs`,
/// `SYS_ARP_TABLE`). The kernel does not populate `/proc/net/arp`, so
/// `ip neigh` falls back to this syscall (TD18 read-path wiring).
const SYS_ARP_TABLE: u64 = 843;

/// Size of one `SYS_ARP_TABLE` record (must match the kernel's `RECORD_SIZE`).
const ARP_RECORD_SIZE: usize = 12;

/// Maximum number of ARP records we ask the kernel to return in one call.
const MAX_ARP_RECORDS: usize = 256;

/// Parse a flat buffer of 12-byte `SYS_ARP_TABLE` records into `NeighEntry`s.
///
/// Each record: `[0..4]` IPv4 (network order = `A.B.C.D`), `[4..10]` MAC,
/// `[10..12]` TTL seconds (u16 LE). A trailing partial record (if any) is
/// ignored by `chunks_exact`. A zero MAC marks an incomplete entry.
fn parse_arp_records(buf: &[u8]) -> Vec<NeighEntry> {
    buf.chunks_exact(ARP_RECORD_SIZE)
        .map(|rec| {
            // rec.len() == ARP_RECORD_SIZE (12) is guaranteed by chunks_exact.
            let ip = fmt_ipv4([rec[0], rec[1], rec[2], rec[3]]);
            let mac = [rec[4], rec[5], rec[6], rec[7], rec[8], rec[9]];
            // TTL (rec[10..12]) is read but not displayed; reserved for future use.
            let complete = mac != [0u8; 6];
            NeighEntry {
                ip,
                mac: fmt_mac(mac),
                // The kernel exposes a single global interface; name it eth0
                // for output compatibility.
                iface: "eth0".to_string(),
                state: if complete { "REACHABLE" } else { "INCOMPLETE" }.to_string(),
            }
        })
        .collect()
}

/// Query the kernel ARP cache via `SYS_ARP_TABLE`. Returns an empty vector on
/// syscall failure (caller already tried `/proc/net/arp`).
fn query_arp_table() -> Vec<NeighEntry> {
    let mut buf = vec![0u8; MAX_ARP_RECORDS * ARP_RECORD_SIZE];
    // SAFETY: `buf` is a valid, writable slice; we pass its pointer and exact
    // byte length. SYS_ARP_TABLE writes at most that many bytes and returns the
    // number of 12-byte records written.
    let ret = unsafe {
        syscall4(
            SYS_ARP_TABLE,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            0,
            0,
        )
    };
    if ret < 0 {
        return Vec::new();
    }
    let count = usize::try_from(ret).unwrap_or(0);
    let byte_len = count.saturating_mul(ARP_RECORD_SIZE).min(buf.len());
    let records = buf.get(..byte_len).unwrap_or(&[]);
    parse_arp_records(records)
}

// ============================================================================
// Data reading from /proc and /sys
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

struct InterfaceInfo {
    name: String,
    state: String,
    mac: String,
    mtu: u32,
    ip_addr: String,
    netmask: String,
    broadcast: String,
    rx_bytes: u64,
    rx_packets: u64,
    rx_errors: u64,
    tx_bytes: u64,
    tx_packets: u64,
    tx_errors: u64,
}

fn read_interfaces() -> Vec<InterfaceInfo> {
    let mut interfaces = Vec::new();

    // Try /sys/class/net/ first.
    if let Ok(entries) = fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let base = format!("/sys/class/net/{name}");

                let state = read_file(&format!("{base}/operstate"))
                    .unwrap_or_else(|| "unknown".to_string());
                let mac = read_file(&format!("{base}/address"))
                    .unwrap_or_else(|| "00:00:00:00:00:00".to_string());
                let mtu = read_file(&format!("{base}/mtu"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1500);

                // Statistics.
                let rx_bytes = read_file(&format!("{base}/statistics/rx_bytes"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let rx_packets = read_file(&format!("{base}/statistics/rx_packets"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let rx_errors = read_file(&format!("{base}/statistics/rx_errors"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let tx_bytes = read_file(&format!("{base}/statistics/tx_bytes"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let tx_packets = read_file(&format!("{base}/statistics/tx_packets"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let tx_errors = read_file(&format!("{base}/statistics/tx_errors"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                // IP address from /proc/net/if_inet.
                let (ip_addr, netmask, broadcast) = get_iface_ip(name);

                interfaces.push(InterfaceInfo {
                    name: name.to_string(),
                    state,
                    mac,
                    mtu,
                    ip_addr,
                    netmask,
                    broadcast,
                    rx_bytes,
                    rx_packets,
                    rx_errors,
                    tx_bytes,
                    tx_packets,
                    tx_errors,
                });
            }
        }
    }

    // Fallback: parse /proc/net/dev.
    if interfaces.is_empty()
        && let Some(content) = read_file("/proc/net/dev") {
            let mut skip_header = 2;
            for line in content.lines() {
                if skip_header > 0 {
                    skip_header -= 1;
                    continue;
                }
                let line = line.trim();
                if let Some((name, stats)) = line.split_once(':') {
                    let name = name.trim();
                    let parts: Vec<u64> = stats.split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();

                    let (ip_addr, netmask, broadcast) = get_iface_ip(name);

                    interfaces.push(InterfaceInfo {
                        name: name.to_string(),
                        state: "unknown".to_string(),
                        mac: String::new(),
                        mtu: 1500,
                        ip_addr,
                        netmask,
                        broadcast,
                        rx_bytes: parts.first().copied().unwrap_or(0),
                        rx_packets: parts.get(1).copied().unwrap_or(0),
                        rx_errors: parts.get(2).copied().unwrap_or(0),
                        tx_bytes: parts.get(8).copied().unwrap_or(0),
                        tx_packets: parts.get(9).copied().unwrap_or(0),
                        tx_errors: parts.get(10).copied().unwrap_or(0),
                    });
                }
            }
        }

    // Last resort: query the kernel directly. The kernel does not yet populate
    // /sys/class/net/ or /proc/net/dev, so without this the show paths would
    // report no interfaces. SYS_NET_IF_INFO yields the default interface's live
    // configuration (TD18 read-path wiring).
    if interfaces.is_empty()
        && let Some(info) = query_net_if_info()
    {
        interfaces.push(interface_from_net_if_info(&info));
    }

    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    interfaces
}

fn get_iface_ip(iface: &str) -> (String, String, String) {
    // Read from /proc/net/if_inet or /proc/net/fib_trie.
    if let Some(content) = read_file("/proc/net/if_inet") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && parts[0] == iface {
                return (
                    parts[1].to_string(),
                    parts.get(2).unwrap_or(&"").to_string(),
                    parts.get(3).unwrap_or(&"").to_string(),
                );
            }
        }
    }

    (String::new(), String::new(), String::new())
}

struct RouteEntry {
    destination: String,
    gateway: String,
    mask: String,
    iface: String,
    metric: u32,
    #[allow(dead_code)] // Available for verbose route display.
    flags: String,
}

fn read_routes() -> Vec<RouteEntry> {
    let mut routes = Vec::new();

    if let Some(content) = read_file("/proc/net/route") {
        let mut first = true;
        for line in content.lines() {
            if first { first = false; continue; }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 8 {
                let dest_hex = u32::from_str_radix(parts[1], 16).unwrap_or(0);
                let gw_hex = u32::from_str_radix(parts[2], 16).unwrap_or(0);
                let mask_hex = u32::from_str_radix(parts[7], 16).unwrap_or(0);

                routes.push(RouteEntry {
                    destination: hex_to_ip(dest_hex),
                    gateway: hex_to_ip(gw_hex),
                    mask: hex_to_ip(mask_hex),
                    iface: parts[0].to_string(),
                    metric: parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0),
                    flags: parts.get(3).unwrap_or(&"").to_string(),
                });
            }
        }
    }

    // Also try our kernel's route format: /proc/net/routes.
    if routes.is_empty()
        && let Some(content) = read_file("/proc/net/routes") {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                // Format: dest/mask via gateway dev iface metric N
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let (dest, mask) = if let Some((d, m)) = parts[0].split_once('/') {
                        (d.to_string(), cidr_to_mask(m))
                    } else {
                        (parts[0].to_string(), "255.255.255.255".to_string())
                    };
                    let gateway = if parts.len() > 2 && parts[1] == "via" {
                        parts[2].to_string()
                    } else {
                        "0.0.0.0".to_string()
                    };
                    let iface = parts.iter()
                        .position(|&s| s == "dev")
                        .and_then(|i| parts.get(i + 1))
                        .unwrap_or(&"?")
                        .to_string();
                    let metric = parts.iter()
                        .position(|&s| s == "metric")
                        .and_then(|i| parts.get(i + 1))
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);

                    routes.push(RouteEntry {
                        destination: dest,
                        gateway,
                        mask,
                        iface,
                        metric,
                        flags: String::new(),
                    });
                }
            }
        }

    // Last resort: synthesize the default route from the kernel's interface
    // record. /proc/net/route(s) are not populated, so this is the only live
    // source for the default gateway (TD18 read-path wiring).
    if routes.is_empty()
        && let Some(info) = query_net_if_info()
        && let Some(route) = route_from_net_if_info(&info)
    {
        routes.push(route);
    }

    routes
}

fn hex_to_ip(hex: u32) -> String {
    // Linux /proc/net/route stores IPs in little-endian hex.
    format!(
        "{}.{}.{}.{}",
        hex & 0xFF,
        (hex >> 8) & 0xFF,
        (hex >> 16) & 0xFF,
        (hex >> 24) & 0xFF,
    )
}

fn cidr_to_mask(cidr: &str) -> String {
    let bits: u32 = cidr.parse().unwrap_or(0);
    if bits == 0 {
        return "0.0.0.0".to_string();
    }
    let mask = !((1u32 << (32 - bits)) - 1);
    format!(
        "{}.{}.{}.{}",
        (mask >> 24) & 0xFF,
        (mask >> 16) & 0xFF,
        (mask >> 8) & 0xFF,
        mask & 0xFF,
    )
}

fn ip_to_u32(ip: &str) -> u32 {
    let parts: Vec<u32> = ip.split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() == 4 {
        (parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]
    } else {
        0
    }
}

struct NeighEntry {
    ip: String,
    mac: String,
    iface: String,
    state: String,
}

fn read_arp() -> Vec<NeighEntry> {
    let mut entries = Vec::new();

    if let Some(content) = read_file("/proc/net/arp") {
        let mut first = true;
        for line in content.lines() {
            if first { first = false; continue; }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 6 {
                entries.push(NeighEntry {
                    ip: parts[0].to_string(),
                    mac: parts[3].to_string(),
                    iface: parts[5].to_string(),
                    state: parts.get(2).unwrap_or(&"?").to_string(),
                });
            }
        }
    }

    // The kernel does not populate /proc/net/arp; fall back to the read-only
    // SYS_ARP_TABLE syscall so `ip neigh` shows the live cache (TD18).
    if entries.is_empty() {
        entries = query_arp_table();
    }

    entries
}

fn read_dns_servers() -> Vec<String> {
    let mut servers = Vec::new();

    if let Some(content) = read_file("/etc/resolv.conf") {
        for line in content.lines() {
            if let Some(ns) = line.strip_prefix("nameserver") {
                let ns = ns.trim();
                if !ns.is_empty() {
                    servers.push(ns.to_string());
                }
            }
        }
    }

    servers
}

// ============================================================================
// Display
// ============================================================================

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn cmd_link_show() {
    let interfaces = read_interfaces();
    if interfaces.is_empty() {
        println!("No network interfaces found.");
        return;
    }

    for (idx, iface) in interfaces.iter().enumerate() {
        let state_color = match iface.state.as_str() {
            "up" => "\x1b[32m",    // green
            "down" => "\x1b[31m",  // red
            _ => "\x1b[33m",       // yellow
        };

        println!(
            "{}: {}: <{}> mtu {}",
            idx + 1,
            iface.name,
            iface.state.to_uppercase(),
            iface.mtu,
        );

        if !iface.mac.is_empty() {
            println!("    link/ether {}", iface.mac);
        }

        print!("    state {state_color}{}\x1b[0m", iface.state);
        println!();
    }
}

fn cmd_addr_show(filter: Option<&str>) {
    let interfaces = read_interfaces();

    for (idx, iface) in interfaces.iter().enumerate() {
        if let Some(f) = filter
            && iface.name != f {
                continue;
            }

        println!(
            "{}: {}: <{}> mtu {}",
            idx + 1,
            iface.name,
            iface.state.to_uppercase(),
            iface.mtu,
        );

        if !iface.mac.is_empty() {
            println!("    link/ether {}", iface.mac);
        }

        if !iface.ip_addr.is_empty() {
            let mask_str = if !iface.netmask.is_empty() {
                format!(" netmask {}", iface.netmask)
            } else {
                String::new()
            };
            let bcast_str = if !iface.broadcast.is_empty() {
                format!(" broadcast {}", iface.broadcast)
            } else {
                String::new()
            };
            println!("    inet {}{mask_str}{bcast_str}", iface.ip_addr);
        }
    }
}

fn cmd_route_show() {
    let routes = read_routes();

    if routes.is_empty() {
        println!("No routes found.");
        return;
    }

    println!("{:<18} {:<18} {:<18} {:<8} {:<8}",
        "Destination", "Gateway", "Netmask", "Iface", "Metric");
    println!("{:<18} {:<18} {:<18} {:<8} {:<8}",
        "-----------", "-------", "-------", "-----", "------");

    for r in &routes {
        let dest = if r.destination == "0.0.0.0" { "default" } else { &r.destination };
        let gw = if r.gateway == "0.0.0.0" { "*" } else { &r.gateway };
        println!("{:<18} {:<18} {:<18} {:<8} {:<8}",
            dest, gw, r.mask, r.iface, r.metric);
    }
}

fn cmd_neigh_show() {
    let entries = read_arp();

    if entries.is_empty() {
        println!("No ARP entries.");
        return;
    }

    println!("{:<18} {:<20} {:<10} State", "Address", "HW Address", "Iface");
    println!("{:<18} {:<20} {:<10} -----", "-------", "----------", "-----");

    for e in &entries {
        println!("{:<18} {:<20} {:<10} {}", e.ip, e.mac, e.iface, e.state);
    }
}

fn cmd_dns_show() {
    let servers = read_dns_servers();
    if servers.is_empty() {
        println!("No DNS servers configured.");
    } else {
        println!("DNS Servers:");
        for s in &servers {
            println!("  {s}");
        }
    }
}

fn cmd_stats(iface_name: &str) {
    let interfaces = read_interfaces();
    let iface = match interfaces.iter().find(|i| i.name == iface_name) {
        Some(i) => i,
        None => {
            eprintln!("Interface '{}' not found", iface_name);
            process::exit(1);
        }
    };

    println!("Interface: {}", iface.name);
    println!("  State:   {}", iface.state);
    println!("  MAC:     {}", iface.mac);
    println!("  MTU:     {}", iface.mtu);
    if !iface.ip_addr.is_empty() {
        println!("  IP:      {}", iface.ip_addr);
    }
    println!();
    println!("  RX:");
    println!("    Bytes:   {} ({})", iface.rx_bytes, format_bytes(iface.rx_bytes));
    println!("    Packets: {}", iface.rx_packets);
    println!("    Errors:  {}", iface.rx_errors);
    println!("  TX:");
    println!("    Bytes:   {} ({})", iface.tx_bytes, format_bytes(iface.tx_bytes));
    println!("    Packets: {}", iface.tx_packets);
    println!("    Errors:  {}", iface.tx_errors);
}

fn cmd_link_set(iface: &str, action: &str) {
    let cmd = match action {
        "up" => NET_IF_UP,
        "down" => NET_IF_DOWN,
        other => {
            eprintln!("unknown action: {other} (expected 'up' or 'down')");
            process::exit(1);
        }
    };

    let ret = net_ioctl(cmd, iface, 0);
    if ret < 0 {
        eprintln!("Failed to set {iface} {action}: error {ret}");
        process::exit(1);
    }
    println!("{iface}: set {action}");
}

fn cmd_addr_add(addr: &str, iface: &str) {
    let (ip_str, _mask_str) = addr.split_once('/').unwrap_or((addr, "24"));
    let ip_val = ip_to_u32(ip_str);

    let ret = net_ioctl(NET_IF_SET_IP, iface, ip_val as u64);
    if ret < 0 {
        eprintln!("Failed to add address {addr} to {iface}: error {ret}");
        process::exit(1);
    }
    println!("{iface}: added {addr}");
}

fn cmd_addr_del(addr: &str, iface: &str) {
    // Setting IP to 0 effectively removes the address.
    let ret = net_ioctl(NET_IF_SET_IP, iface, 0);
    if ret < 0 {
        eprintln!("Failed to remove address from {iface}: error {ret}");
        process::exit(1);
    }
    println!("{iface}: removed {addr}");
}

fn cmd_route_add(args: &[String]) {
    // Parse: ip route add <dest>[/<mask>] via <gateway> [dev <iface>] [metric <n>]
    if args.is_empty() {
        eprintln!("usage: ip route add <dest> via <gateway> [dev <iface>]");
        process::exit(1);
    }

    let dest = &args[0];
    let mut gateway = None;
    let mut iface = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "via" => {
                if i + 1 < args.len() {
                    gateway = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "dev" => {
                if i + 1 < args.len() {
                    iface = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    let gw_str = gateway.unwrap_or_else(|| "0.0.0.0".to_string());
    let dev = iface.unwrap_or_else(|| "eth0".to_string());
    let gw_val = ip_to_u32(&gw_str);

    let ret = net_ioctl(NET_ROUTE_ADD, &dev, gw_val as u64);
    if ret < 0 {
        eprintln!("Failed to add route {dest} via {gw_str}: error {ret}");
        process::exit(1);
    }
    println!("Added route {dest} via {gw_str} dev {dev}");
}

fn cmd_route_del(dest: &str) {
    let ret = net_ioctl(NET_ROUTE_DEL, dest, 0);
    if ret < 0 {
        eprintln!("Failed to delete route {dest}: error {ret}");
        process::exit(1);
    }
    println!("Deleted route {dest}");
}

fn cmd_dns_add(server: &str) {
    // Append to /etc/resolv.conf.
    let line = format!("nameserver {server}\n");
    match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/etc/resolv.conf")
    {
        Ok(mut f) => {
            use std::io::Write;
            if let Err(e) = f.write_all(line.as_bytes()) {
                eprintln!("Failed to write DNS config: {e}");
                process::exit(1);
            }
            println!("Added DNS server {server}");
        }
        Err(e) => {
            eprintln!("Cannot open /etc/resolv.conf: {e}");
            process::exit(1);
        }
    }
}

// ============================================================================
// CLI
// ============================================================================

fn print_usage() {
    println!("Slate OS Network Configuration v0.1.0");
    println!();
    println!("Configure network interfaces, addresses, routes, and DNS.");
    println!();
    println!("USAGE:");
    println!("  ip <object> [command] [args]");
    println!();
    println!("OBJECTS:");
    println!("  link          Network interfaces (up/down, MTU)");
    println!("  addr          IP addresses");
    println!("  route         Routing table");
    println!("  neigh         ARP/neighbor table");
    println!("  dns           DNS servers");
    println!("  stats <if>    Interface statistics");
    println!();
    println!("EXAMPLES:");
    println!("  ip link                          List interfaces");
    println!("  ip link set eth0 up              Bring up eth0");
    println!("  ip addr show eth0                Show eth0 addresses");
    println!("  ip addr add 192.168.1.10/24 eth0 Add address");
    println!("  ip route                         Show routing table");
    println!("  ip route add default via 192.168.1.1");
    println!("  ip neigh                         Show ARP cache");
    println!("  ip dns                           Show DNS servers");
    println!("  ip dns add 8.8.8.8              Add DNS server");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    match args[1].as_str() {
        "link" | "l" => {
            if args.len() >= 4 && args[2] == "set" {
                // ip link set <iface> up|down
                if args.len() < 5 {
                    eprintln!("usage: ip link set <iface> up|down");
                    process::exit(1);
                }
                cmd_link_set(&args[3], &args[4]);
            } else {
                cmd_link_show();
            }
        }
        "addr" | "address" | "a" => {
            if args.len() >= 3 {
                match args[2].as_str() {
                    "show" | "list" => {
                        let filter = args.get(3).map(|s| s.as_str());
                        cmd_addr_show(filter);
                    }
                    "add" => {
                        if args.len() < 5 {
                            eprintln!("usage: ip addr add <ip/mask> <iface>");
                            process::exit(1);
                        }
                        cmd_addr_add(&args[3], &args[4]);
                    }
                    "del" | "delete" => {
                        if args.len() < 5 {
                            eprintln!("usage: ip addr del <ip/mask> <iface>");
                            process::exit(1);
                        }
                        cmd_addr_del(&args[3], &args[4]);
                    }
                    other => {
                        // Treat as interface name filter.
                        cmd_addr_show(Some(other));
                    }
                }
            } else {
                cmd_addr_show(None);
            }
        }
        "route" | "r" => {
            if args.len() >= 3 {
                match args[2].as_str() {
                    "add" => cmd_route_add(&args[3..]),
                    "del" | "delete" => {
                        if args.len() < 4 {
                            eprintln!("usage: ip route del <destination>");
                            process::exit(1);
                        }
                        cmd_route_del(&args[3]);
                    }
                    "show" | "list" => cmd_route_show(),
                    _ => cmd_route_show(),
                }
            } else {
                cmd_route_show();
            }
        }
        "neigh" | "neighbor" | "arp" => {
            cmd_neigh_show();
        }
        "dns" => {
            if args.len() >= 4 && args[2] == "add" {
                cmd_dns_add(&args[3]);
            } else if args.len() >= 4 && (args[2] == "del" || args[2] == "delete") {
                println!("DNS deletion: edit /etc/resolv.conf manually");
            } else {
                cmd_dns_show();
            }
        }
        "stats" | "statistics" => {
            if args.len() < 3 {
                eprintln!("usage: ip stats <interface>");
                process::exit(1);
            }
            cmd_stats(&args[2]);
        }
        "help" | "--help" | "-h" => {
            print_usage();
        }
        other => {
            eprintln!("unknown object: {other}");
            eprintln!("Run 'ip help' for usage.");
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_net_if_info_full() {
        // 10.0.2.15 / 255.255.255.0, gw 10.0.2.2, mac 52:54:00:12:34:56, up.
        let rec: [u8; NET_IF_INFO_SIZE] = [
            10, 0, 2, 15, // ip
            255, 255, 255, 0, // mask
            10, 0, 2, 2, // gateway
            10, 0, 2, 3, // dns
            0x52, 0x54, 0x00, 0x12, 0x34, 0x56, // mac
            1,    // up
            0,    // reserved
        ];
        let info = parse_net_if_info(&rec);
        assert_eq!(info.ip, [10, 0, 2, 15]);
        assert_eq!(info.mask, [255, 255, 255, 0]);
        assert_eq!(info.gateway, [10, 0, 2, 2]);
        assert_eq!(info.mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert!(info.up);
    }

    #[test]
    fn test_parse_net_if_info_down_unconfigured() {
        let rec = [0u8; NET_IF_INFO_SIZE];
        let info = parse_net_if_info(&rec);
        assert!(is_zero4(info.ip));
        assert!(is_zero4(info.gateway));
        assert!(!info.up);
    }

    #[test]
    fn test_fmt_ipv4() {
        assert_eq!(fmt_ipv4([10, 0, 2, 15]), "10.0.2.15");
        assert_eq!(fmt_ipv4([0, 0, 0, 0]), "0.0.0.0");
        assert_eq!(fmt_ipv4([255, 255, 255, 255]), "255.255.255.255");
    }

    #[test]
    fn test_fmt_mac() {
        assert_eq!(
            fmt_mac([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]),
            "52:54:00:12:34:56"
        );
        assert_eq!(fmt_mac([0, 0, 0, 0, 0, 0]), "00:00:00:00:00:00");
    }

    #[test]
    fn test_is_zero4() {
        assert!(is_zero4([0, 0, 0, 0]));
        assert!(!is_zero4([0, 0, 0, 1]));
    }

    #[test]
    fn test_compute_broadcast() {
        assert_eq!(
            compute_broadcast([10, 0, 2, 15], [255, 255, 255, 0]),
            [10, 0, 2, 255]
        );
        assert_eq!(
            compute_broadcast([192, 168, 5, 7], [255, 255, 0, 0]),
            [192, 168, 255, 255]
        );
    }

    #[test]
    fn test_interface_from_net_if_info_up() {
        let info = NetIfInfo {
            ip: [10, 0, 2, 15],
            mask: [255, 255, 255, 0],
            gateway: [10, 0, 2, 2],
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            up: true,
        };
        let iface = interface_from_net_if_info(&info);
        assert_eq!(iface.name, "eth0");
        assert_eq!(iface.state, "up");
        assert_eq!(iface.ip_addr, "10.0.2.15");
        assert_eq!(iface.netmask, "255.255.255.0");
        assert_eq!(iface.broadcast, "10.0.2.255");
        assert_eq!(iface.mac, "52:54:00:12:34:56");
    }

    #[test]
    fn test_interface_from_net_if_info_down_unconfigured() {
        let info = NetIfInfo {
            ip: [0, 0, 0, 0],
            mask: [0, 0, 0, 0],
            gateway: [0, 0, 0, 0],
            mac: [0, 0, 0, 0, 0, 0],
            up: false,
        };
        let iface = interface_from_net_if_info(&info);
        assert_eq!(iface.state, "down");
        assert!(iface.ip_addr.is_empty());
        assert!(iface.netmask.is_empty());
        assert!(iface.broadcast.is_empty());
    }

    #[test]
    fn test_route_from_net_if_info_with_gateway() {
        let info = NetIfInfo {
            ip: [10, 0, 2, 15],
            mask: [255, 255, 255, 0],
            gateway: [10, 0, 2, 2],
            mac: [0, 0, 0, 0, 0, 0],
            up: true,
        };
        let route = route_from_net_if_info(&info).expect("default route");
        assert_eq!(route.destination, "0.0.0.0");
        assert_eq!(route.gateway, "10.0.2.2");
        assert_eq!(route.mask, "0.0.0.0");
        assert_eq!(route.iface, "eth0");
    }

    #[test]
    fn test_route_from_net_if_info_no_gateway() {
        let info = NetIfInfo {
            ip: [10, 0, 2, 15],
            mask: [255, 255, 255, 0],
            gateway: [0, 0, 0, 0],
            mac: [0, 0, 0, 0, 0, 0],
            up: true,
        };
        assert!(route_from_net_if_info(&info).is_none());
    }

    #[test]
    fn test_parse_arp_records_complete() {
        // One record: 192.168.1.1 -> 52:54:00:12:34:56, TTL 30s.
        let rec = [
            192, 168, 1, 1, // IPv4 (network order)
            0x52, 0x54, 0x00, 0x12, 0x34, 0x56, // MAC
            30, 0, // TTL (u16 LE)
        ];
        let entries = parse_arp_records(&rec);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].ip, "192.168.1.1");
        assert_eq!(entries[0].mac, "52:54:00:12:34:56");
        assert_eq!(entries[0].iface, "eth0");
        assert_eq!(entries[0].state, "REACHABLE");
    }

    #[test]
    fn test_parse_arp_records_incomplete() {
        // Zero MAC marks an incomplete entry (resolution pending).
        let rec = [10, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        let entries = parse_arp_records(&rec);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].ip, "10.0.2.2");
        assert_eq!(entries[0].mac, "00:00:00:00:00:00");
        assert_eq!(entries[0].state, "INCOMPLETE");
    }

    #[test]
    fn test_parse_arp_records_multiple_and_partial() {
        // Two full records plus 3 trailing bytes that must be ignored.
        let mut buf = Vec::new();
        buf.extend_from_slice(&[192, 168, 0, 1, 1, 2, 3, 4, 5, 6, 60, 0]);
        buf.extend_from_slice(&[192, 168, 0, 2, 7, 8, 9, 10, 11, 12, 0, 0]);
        buf.extend_from_slice(&[1, 2, 3]); // partial -> ignored
        let entries = parse_arp_records(&buf);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].ip, "192.168.0.1");
        assert_eq!(entries[0].mac, "01:02:03:04:05:06");
        assert_eq!(entries[0].state, "REACHABLE");
        assert_eq!(entries[1].ip, "192.168.0.2");
        assert_eq!(entries[1].state, "REACHABLE");
    }

    #[test]
    fn test_parse_arp_records_empty() {
        assert!(parse_arp_records(&[]).is_empty());
    }
}
