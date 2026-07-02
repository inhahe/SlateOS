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

/// Interface-configuration write syscall (`kernel/src/syscall/number.rs`,
/// `SYS_NET_IF_CONFIG`). Root-gated. Reads an 18-byte record from `arg0`
/// (length in `arg1`) whose byte 17 is a per-field mask selecting which of
/// IP/mask/gateway/DNS/up to apply (read-modify-write against the live
/// config). This is the native write path behind `ip addr` and `ip link`.
/// See [`build_config_record`] for the layout.
const SYS_NET_IF_CONFIG: u64 = 856;

/// Field-mask bits for the `SYS_NET_IF_CONFIG` record (byte 17). A set bit
/// means "apply this field"; unset means "leave the current value untouched".
mod cfg_mask {
    /// Apply the IPv4 address (record bytes 0..4).
    pub const IP: u8 = 1 << 0;
    /// Apply the subnet mask (record bytes 4..8).
    pub const MASK: u8 = 1 << 1;
    /// Apply the gateway (record bytes 8..12).
    pub const GATEWAY: u8 = 1 << 2;
    /// Apply the DNS server (record bytes 12..16).
    #[allow(dead_code)] // `ip` uses resolv.conf for DNS, not this field.
    pub const DNS: u8 = 1 << 3;
    /// Apply the up/down flag (record byte 16).
    pub const UP: u8 = 1 << 4;
}

/// Routing-table write/read syscalls (`kernel/src/syscall/number.rs`). These
/// carry *non-default* routes: `SYS_NET_ROUTE_ADD` takes a 16-byte record
/// `[dest(4), mask(4), gateway(4), metric(4, LE)]`, `SYS_NET_ROUTE_DEL` takes
/// an 8-byte `[dest(4), mask(4)]`, and `SYS_NET_ROUTE_LIST` fills a buffer with
/// 16-byte records and returns the count. The *default* route lives in the
/// interface gateway (`SYS_NET_IF_CONFIG`), not here (see design-decisions §52).
const SYS_NET_ROUTE_ADD: u64 = 857;
const SYS_NET_ROUTE_DEL: u64 = 858;
const SYS_NET_ROUTE_LIST: u64 = 859;

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

/// Build the 18-byte `SYS_NET_IF_CONFIG` record from the fields the caller
/// wants to change. Any `None` field is left out of the record and its mask
/// bit stays clear, so the kernel preserves the current value for it
/// (read-modify-write). Pure (no syscall) so it is unit-testable on the host.
///
/// Layout: `[0..4]` ip, `[4..8]` mask, `[8..12]` gateway, `[12..16]` dns,
/// `[16]` up flag, `[17]` field mask (see [`cfg_mask`]).
fn build_config_record(
    ip: Option<[u8; 4]>,
    mask: Option<[u8; 4]>,
    gateway: Option<[u8; 4]>,
    dns: Option<[u8; 4]>,
    up: Option<bool>,
) -> [u8; 18] {
    let mut rec = [0u8; 18];
    let mut field_mask = 0u8;
    if let Some(v) = ip {
        rec[0..4].copy_from_slice(&v);
        field_mask |= cfg_mask::IP;
    }
    if let Some(v) = mask {
        rec[4..8].copy_from_slice(&v);
        field_mask |= cfg_mask::MASK;
    }
    if let Some(v) = gateway {
        rec[8..12].copy_from_slice(&v);
        field_mask |= cfg_mask::GATEWAY;
    }
    if let Some(v) = dns {
        rec[12..16].copy_from_slice(&v);
        field_mask |= cfg_mask::DNS;
    }
    if let Some(u) = up {
        rec[16] = u8::from(u);
        field_mask |= cfg_mask::UP;
    }
    rec[17] = field_mask;
    rec
}

/// Apply an interface configuration via `SYS_NET_IF_CONFIG`. Returns the
/// kernel's signed result (0 on success, negative errno on failure).
fn net_if_config(rec: &[u8; 18]) -> i64 {
    // SAFETY: `rec` is exactly 18 bytes, matching the kernel's REC_SIZE
    // contract; the kernel only reads (never writes) the record.
    unsafe {
        syscall4(
            SYS_NET_IF_CONFIG,
            rec.as_ptr() as u64,
            rec.len() as u64,
            0,
            0,
        )
    }
}

/// Size of a `SYS_NET_ROUTE_ADD` / `SYS_NET_ROUTE_LIST` record.
const ROUTE_REC_SIZE: usize = 16;
/// Size of a `SYS_NET_ROUTE_DEL` record.
const ROUTE_DEL_SIZE: usize = 8;

/// Build the 16-byte `SYS_NET_ROUTE_ADD` / listing record. The metric is stored
/// little-endian to match the kernel's `u32::from_le_bytes` decode. Pure (no
/// syscall) so it is unit-testable on the host.
///
/// Layout: `[0..4]` dest, `[4..8]` mask, `[8..12]` gateway, `[12..16]` metric.
fn build_route_record(dest: [u8; 4], mask: [u8; 4], gateway: [u8; 4], metric: u32) -> [u8; 16] {
    let mut rec = [0u8; 16];
    rec[0..4].copy_from_slice(&dest);
    rec[4..8].copy_from_slice(&mask);
    rec[8..12].copy_from_slice(&gateway);
    rec[12..16].copy_from_slice(&metric.to_le_bytes());
    rec
}

/// Build the 8-byte `SYS_NET_ROUTE_DEL` record. Pure (no syscall).
///
/// Layout: `[0..4]` dest, `[4..8]` mask.
fn build_route_del_record(dest: [u8; 4], mask: [u8; 4]) -> [u8; 8] {
    let mut rec = [0u8; 8];
    rec[0..4].copy_from_slice(&dest);
    rec[4..8].copy_from_slice(&mask);
    rec
}

/// Add a route via `SYS_NET_ROUTE_ADD`. Returns the kernel's signed result
/// (0 on success, negative errno on failure).
fn net_route_add(rec: &[u8; ROUTE_REC_SIZE]) -> i64 {
    // SAFETY: `rec` is exactly `ROUTE_REC_SIZE` bytes, matching the kernel's
    // contract; the kernel only reads (never writes) the record.
    unsafe {
        syscall4(
            SYS_NET_ROUTE_ADD,
            rec.as_ptr() as u64,
            rec.len() as u64,
            0,
            0,
        )
    }
}

/// Delete a route via `SYS_NET_ROUTE_DEL`. Returns the kernel's signed result
/// (0 on success, negative errno on failure).
fn net_route_del(rec: &[u8; ROUTE_DEL_SIZE]) -> i64 {
    // SAFETY: `rec` is exactly `ROUTE_DEL_SIZE` bytes, matching the kernel's
    // contract; the kernel only reads (never writes) the record.
    unsafe {
        syscall4(
            SYS_NET_ROUTE_DEL,
            rec.as_ptr() as u64,
            rec.len() as u64,
            0,
            0,
        )
    }
}

/// List routes via `SYS_NET_ROUTE_LIST`. Fills `buf` with 16-byte records and
/// returns the kernel's signed result (route count on success, negative errno
/// on failure). Read-only (not root-gated).
fn net_route_list(buf: &mut [u8]) -> i64 {
    // SAFETY: `buf` is a valid writable slice of `buf.len()` bytes; the kernel
    // writes at most `buf.len()` bytes (whole 16-byte records only).
    unsafe {
        syscall4(
            SYS_NET_ROUTE_LIST,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
            0,
            0,
        )
    }
}

/// Decode one 16-byte route record into dest/mask/gateway bytes and metric.
/// Pure (no syscall) so it is unit-testable on the host.
fn parse_route_record(rec: &[u8; ROUTE_REC_SIZE]) -> ([u8; 4], [u8; 4], [u8; 4], u32) {
    let mut dest = [0u8; 4];
    dest.copy_from_slice(&rec[0..4]);
    let mut mask = [0u8; 4];
    mask.copy_from_slice(&rec[4..8]);
    let mut gateway = [0u8; 4];
    gateway.copy_from_slice(&rec[8..12]);
    let mut metric_bytes = [0u8; 4];
    metric_bytes.copy_from_slice(&rec[12..16]);
    (dest, mask, gateway, u32::from_le_bytes(metric_bytes))
}

/// Read the caller's netns route table via `SYS_NET_ROUTE_LIST`. Returns an
/// empty vec on any error (e.g. the syscall is unavailable). The kernel caps
/// the table at 32 entries, so a 32-record buffer always suffices.
fn read_routes_from_kernel() -> Vec<RouteEntry> {
    let mut routes = Vec::new();
    let mut buf = [0u8; ROUTE_REC_SIZE * 32];
    let ret = net_route_list(&mut buf);
    if ret <= 0 {
        return routes;
    }
    let count = (ret as usize).min(buf.len() / ROUTE_REC_SIZE);
    for i in 0..count {
        let start = i * ROUTE_REC_SIZE;
        let Some(chunk) = buf.get(start..start + ROUTE_REC_SIZE) else {
            break;
        };
        let mut rec = [0u8; ROUTE_REC_SIZE];
        rec.copy_from_slice(chunk);
        let (dest, mask, gateway, metric) = parse_route_record(&rec);
        routes.push(RouteEntry {
            destination: fmt_ipv4(dest),
            gateway: fmt_ipv4(gateway),
            mask: fmt_ipv4(mask),
            iface: "eth0".to_string(),
            metric,
            flags: String::new(),
        });
    }
    routes
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

    // /proc/net/route(s) are not populated on Slate OS, so the live sources are
    // the kernel route table (SYS_NET_ROUTE_LIST, non-default routes) and the
    // interface record (SYS_NET_IF_INFO, the default route). Merge both.
    if routes.is_empty() {
        routes.extend(read_routes_from_kernel());
        if let Some(info) = query_net_if_info()
            && let Some(route) = route_from_net_if_info(&info)
        {
            routes.push(route);
        }
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

/// Parse a dotted-quad IPv4 string into its 4 network-order bytes, rejecting
/// malformed input (wrong component count or out-of-range octet). Pure, so it
/// is unit-testable on the host.
fn ip_to_bytes(ip: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut out = [0u8; 4];
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = part.parse::<u8>().ok()?;
    }
    Some(out)
}

/// Convert a CIDR prefix length (0..=32) into a 4-byte subnet mask. Returns
/// `None` for out-of-range prefixes. Pure, so it is unit-testable on the host.
fn prefix_to_mask(prefix: u8) -> Option<[u8; 4]> {
    if prefix > 32 {
        return None;
    }
    // Build the mask as a u32 (guarding the `1<<32` overflow via the branch),
    // then split into network-order bytes.
    let bits: u32 = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Some(bits.to_be_bytes())
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

/// Report a failed `SYS_NET_IF_CONFIG` call and exit. `-1` (EPERM) is the most
/// common failure — the caller lacks the `CAP_NET_ADMIN`-class authority.
fn config_fail(msg: &str, ret: i64) -> ! {
    if ret == -1 {
        eprintln!("{msg}: permission denied (need root)");
    } else {
        eprintln!("{msg}: error {ret}");
    }
    process::exit(1);
}

fn cmd_link_set(iface: &str, action: &str) {
    let up = match action {
        "up" => true,
        "down" => false,
        other => {
            eprintln!("unknown action: {other} (expected 'up' or 'down')");
            process::exit(1);
        }
    };

    let rec = build_config_record(None, None, None, None, Some(up));
    let ret = net_if_config(&rec);
    if ret < 0 {
        config_fail(&format!("Failed to set {iface} {action}"), ret);
    }
    println!("{iface}: set {action}");
}

fn cmd_addr_add(addr: &str, iface: &str) {
    // `ip addr add <ip>[/<prefix>] dev <iface>` — default /24 when omitted, as
    // `ip` traditionally does for a bare IPv4 host address.
    let (ip_str, prefix_str) = addr.split_once('/').unwrap_or((addr, "24"));
    let Some(ip_bytes) = ip_to_bytes(ip_str) else {
        eprintln!("Invalid IP address: {ip_str}");
        process::exit(1);
    };
    let prefix: u8 = match prefix_str.parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Invalid prefix length: {prefix_str}");
            process::exit(1);
        }
    };
    let Some(mask_bytes) = prefix_to_mask(prefix) else {
        eprintln!("Prefix length out of range (0..=32): {prefix}");
        process::exit(1);
    };

    // Apply IP and mask together so the derived broadcast stays consistent.
    let rec = build_config_record(Some(ip_bytes), Some(mask_bytes), None, None, None);
    let ret = net_if_config(&rec);
    if ret < 0 {
        config_fail(&format!("Failed to add address {addr} to {iface}"), ret);
    }
    println!("{iface}: added {addr}");
}

fn cmd_addr_del(addr: &str, iface: &str) {
    // The kernel models a single primary address per interface, so "delete"
    // means clearing it: set the IP to 0.0.0.0 (unconfigured).
    let rec = build_config_record(Some([0, 0, 0, 0]), None, None, None, None);
    let ret = net_if_config(&rec);
    if ret < 0 {
        config_fail(&format!("Failed to remove address from {iface}"), ret);
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
    let mut metric: u32 = 0;

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
            "metric" => {
                if i + 1 < args.len() {
                    match args[i + 1].parse::<u32>() {
                        Ok(m) => metric = m,
                        Err(_) => {
                            eprintln!("Invalid metric: {}", args[i + 1]);
                            process::exit(1);
                        }
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    let dev = iface.unwrap_or_else(|| "eth0".to_string());

    let Some(gw_str) = gateway else {
        eprintln!("ip: 'route add' requires 'via <gateway>'");
        process::exit(1);
    };
    let Some(gw_bytes) = ip_to_bytes(&gw_str) else {
        eprintln!("Invalid gateway address: {gw_str}");
        process::exit(1);
    };

    // The default route (0.0.0.0/0) is owned by the interface gateway
    // (SYS_NET_IF_CONFIG), not the route table (design-decisions §52). Every
    // other prefix is a route-table entry written via SYS_NET_ROUTE_ADD.
    if is_default_route(dest) {
        let rec = build_config_record(None, None, Some(gw_bytes), None, None);
        let ret = net_if_config(&rec);
        if ret < 0 {
            config_fail(&format!("Failed to add default route via {gw_str}"), ret);
        }
        println!("Added default route via {gw_str} dev {dev}");
        return;
    }

    // Non-default route: parse `<dest>[/<prefix>]` (bare address => /32 host
    // route, matching iproute2), reject 0.0.0.0/0 (that's the default route).
    let (dest_str, prefix_str) = dest.split_once('/').unwrap_or((dest.as_str(), "32"));
    let Some(dest_bytes) = ip_to_bytes(dest_str) else {
        eprintln!("Invalid destination address: {dest_str}");
        process::exit(1);
    };
    let prefix: u8 = match prefix_str.parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Invalid prefix length: {prefix_str}");
            process::exit(1);
        }
    };
    let Some(mask_bytes) = prefix_to_mask(prefix) else {
        eprintln!("Prefix length out of range (0..=32): {prefix}");
        process::exit(1);
    };

    let rec = build_route_record(dest_bytes, mask_bytes, gw_bytes, metric);
    let ret = net_route_add(&rec);
    if ret < 0 {
        config_fail(&format!("Failed to add route {dest} via {gw_str}"), ret);
    }
    println!("Added route {dest} via {gw_str} dev {dev} metric {metric}");
}

/// True if a route destination refers to the default route (`default` or
/// `0.0.0.0/0`).
fn is_default_route(dest: &str) -> bool {
    dest == "default" || dest == "0.0.0.0/0"
}

fn cmd_route_del(dest: &str) {
    // The default route (0.0.0.0/0) lives in the interface gateway; deleting it
    // clears that gateway (design-decisions §52).
    if is_default_route(dest) {
        let rec = build_config_record(None, None, Some([0, 0, 0, 0]), None, None);
        let ret = net_if_config(&rec);
        if ret < 0 {
            config_fail("Failed to delete default route", ret);
        }
        println!("Deleted default route");
        return;
    }

    // Non-default route: parse `<dest>[/<prefix>]` (bare address => /32) and
    // remove the matching route-table entry via SYS_NET_ROUTE_DEL.
    let (dest_str, prefix_str) = dest.split_once('/').unwrap_or((dest, "32"));
    let Some(dest_bytes) = ip_to_bytes(dest_str) else {
        eprintln!("Invalid destination address: {dest_str}");
        process::exit(1);
    };
    let prefix: u8 = match prefix_str.parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Invalid prefix length: {prefix_str}");
            process::exit(1);
        }
    };
    let Some(mask_bytes) = prefix_to_mask(prefix) else {
        eprintln!("Prefix length out of range (0..=32): {prefix}");
        process::exit(1);
    };

    let rec = build_route_del_record(dest_bytes, mask_bytes);
    let ret = net_route_del(&rec);
    if ret < 0 {
        config_fail(&format!("Failed to delete route {dest}"), ret);
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

    // --- SYS_NET_IF_CONFIG record building ---

    #[test]
    fn test_ip_to_bytes() {
        assert_eq!(ip_to_bytes("10.0.2.15"), Some([10, 0, 2, 15]));
        assert_eq!(ip_to_bytes("0.0.0.0"), Some([0, 0, 0, 0]));
        assert_eq!(ip_to_bytes("255.255.255.255"), Some([255, 255, 255, 255]));
        assert_eq!(ip_to_bytes("10.0.0"), None);
        assert_eq!(ip_to_bytes("10.0.0.256"), None);
        assert_eq!(ip_to_bytes("nope"), None);
    }

    #[test]
    fn test_prefix_to_mask() {
        assert_eq!(prefix_to_mask(0), Some([0, 0, 0, 0]));
        assert_eq!(prefix_to_mask(8), Some([255, 0, 0, 0]));
        assert_eq!(prefix_to_mask(16), Some([255, 255, 0, 0]));
        assert_eq!(prefix_to_mask(24), Some([255, 255, 255, 0]));
        assert_eq!(prefix_to_mask(32), Some([255, 255, 255, 255]));
        assert_eq!(prefix_to_mask(25), Some([255, 255, 255, 128]));
        assert_eq!(prefix_to_mask(33), None);
    }

    #[test]
    fn test_build_config_record_addr_add() {
        // `ip addr add 10.0.2.42/24` -> IP + MASK fields set.
        let rec = build_config_record(
            Some([10, 0, 2, 42]),
            prefix_to_mask(24),
            None,
            None,
            None,
        );
        assert_eq!(&rec[0..4], &[10, 0, 2, 42]);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
        assert_eq!(rec[17], cfg_mask::IP | cfg_mask::MASK);
    }

    #[test]
    fn test_build_config_record_link_updown() {
        let up = build_config_record(None, None, None, None, Some(true));
        assert_eq!(up[16], 1);
        assert_eq!(up[17], cfg_mask::UP);
        let down = build_config_record(None, None, None, None, Some(false));
        assert_eq!(down[16], 0);
        assert_eq!(down[17], cfg_mask::UP);
    }

    #[test]
    fn test_build_config_record_gateway() {
        // `ip route add default via 10.0.2.2` -> GATEWAY field only.
        let rec = build_config_record(None, None, Some([10, 0, 2, 2]), None, None);
        assert_eq!(&rec[8..12], &[10, 0, 2, 2]);
        assert_eq!(rec[17], cfg_mask::GATEWAY);
    }

    #[test]
    fn test_is_default_route() {
        assert!(is_default_route("default"));
        assert!(is_default_route("0.0.0.0/0"));
        assert!(!is_default_route("10.0.0.0/8"));
        assert!(!is_default_route("192.168.1.0/24"));
    }

    #[test]
    fn test_build_route_record() {
        // 203.0.113.0/24 via 10.0.2.250 metric 5.
        let rec = build_route_record([203, 0, 113, 0], [255, 255, 255, 0], [10, 0, 2, 250], 5);
        assert_eq!(&rec[0..4], &[203, 0, 113, 0]);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
        assert_eq!(&rec[8..12], &[10, 0, 2, 250]);
        // Metric is little-endian.
        assert_eq!(&rec[12..16], &[5, 0, 0, 0]);
    }

    #[test]
    fn test_build_route_del_record() {
        let rec = build_route_del_record([203, 0, 113, 0], [255, 255, 255, 0]);
        assert_eq!(&rec[0..4], &[203, 0, 113, 0]);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
    }

    #[test]
    fn test_parse_route_record_roundtrip() {
        let rec = build_route_record([172, 16, 0, 0], [255, 240, 0, 0], [10, 0, 2, 2], 300);
        let (dest, mask, gw, metric) = parse_route_record(&rec);
        assert_eq!(dest, [172, 16, 0, 0]);
        assert_eq!(mask, [255, 240, 0, 0]);
        assert_eq!(gw, [10, 0, 2, 2]);
        assert_eq!(metric, 300);
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
