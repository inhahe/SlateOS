//! Slate OS `ifconfig` -- classic network interface configuration utility.
//!
//! Displays and configures network interfaces using the traditional `ifconfig`
//! command syntax. Reads live state from `/sys/class/net/` and `/proc/net/`,
//! falling back to the read-only `SYS_NET_IF_INFO` syscall. Write operations
//! (up/down/set-ip/set-netmask) apply via the root-gated `SYS_NET_IF_CONFIG`
//! syscall. MTU and explicit-broadcast changes are not representable in the
//! kernel interface model and are reported as unsupported rather than silently
//! ignored.
//!
//! # Usage
//!
//! ```text
//! ifconfig                                Show all active interfaces
//! ifconfig -a                             Show all interfaces (including down)
//! ifconfig -s                             Short table output
//! ifconfig eth0                           Show specific interface
//! ifconfig eth0 192.168.1.100             Set IP address
//! ifconfig eth0 netmask 255.255.255.0     Set netmask
//! ifconfig eth0 broadcast 192.168.1.255   Set broadcast address
//! ifconfig eth0 mtu 1500                  Set MTU
//! ifconfig eth0 up                        Bring interface up
//! ifconfig eth0 down                      Bring interface down
//! ```

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

/// Read-only interface-info query syscall (`kernel/src/syscall/number.rs`,
/// `SYS_NET_IF_INFO`). Returns a fixed 24-byte record describing the default
/// network interface. This is the live read source: the kernel does not yet
/// populate `/sys/class/net/` or `/proc/net/dev`, so display mode falls back to
/// this syscall (TD18 read-path wiring).
const SYS_NET_IF_INFO: u64 = 842;

/// Interface-configuration write syscall (`kernel/src/syscall/number.rs`,
/// `SYS_NET_IF_CONFIG`). Root-gated. Reads an 18-byte record from `arg0`
/// (length in `arg1`) whose byte 17 is a per-field mask selecting which of
/// IP/mask/gateway/DNS/up to apply (read-modify-write against the live
/// config). See [`build_config_record`] for the layout.
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
    pub const DNS: u8 = 1 << 3;
    /// Apply the up/down flag (record byte 16).
    pub const UP: u8 = 1 << 4;
}

#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
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
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&rec[16..22]);
    NetIfInfo {
        ip,
        mask,
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
/// kernel record. The kernel record carries no name or byte counters, so we
/// use the conventional `eth0` name (matching the rest of the OS) and leave the
/// counters at zero rather than fabricating traffic statistics.
fn interface_from_net_if_info(info: &NetIfInfo) -> InterfaceInfo {
    let has_ip = !is_zero4(info.ip);
    let ip_addr = if has_ip { fmt_ipv4(info.ip) } else { String::new() };
    let netmask = if has_ip {
        fmt_ipv4(info.mask)
    } else {
        String::new()
    };
    let broadcast = if has_ip {
        fmt_ipv4(compute_broadcast(info.ip, info.mask))
    } else {
        String::new()
    };
    let flags = if info.up {
        iff::UP | iff::BROADCAST | iff::RUNNING | iff::MULTICAST
    } else {
        iff::BROADCAST | iff::MULTICAST
    };
    InterfaceInfo {
        name: "eth0".to_string(),
        flags,
        mtu: 1500,
        mac: fmt_mac(info.mac),
        ip_addr,
        netmask,
        broadcast,
        tx_queuelen: 1000,
        rx_bytes: 0,
        rx_packets: 0,
        rx_errors: 0,
        rx_dropped: 0,
        rx_overruns: 0,
        rx_frame: 0,
        tx_bytes: 0,
        tx_packets: 0,
        tx_errors: 0,
        tx_dropped: 0,
        tx_overruns: 0,
        tx_carrier: 0,
        tx_collisions: 0,
    }
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
// Interface flags
// ============================================================================

/// Standard Linux-compatible interface flags (from `linux/if.h`).
/// We use a u32 bitmask read from `/sys/class/net/<iface>/flags`.
#[allow(dead_code)]
mod iff {
    pub const UP: u32 = 0x1;
    pub const BROADCAST: u32 = 0x2;
    pub const DEBUG: u32 = 0x4;
    pub const LOOPBACK: u32 = 0x8;
    pub const POINTOPOINT: u32 = 0x10;
    pub const NOTRAILERS: u32 = 0x20;
    pub const RUNNING: u32 = 0x40;
    pub const NOARP: u32 = 0x80;
    pub const PROMISC: u32 = 0x100;
    pub const ALLMULTI: u32 = 0x200;
    pub const MASTER: u32 = 0x400;
    pub const SLAVE: u32 = 0x800;
    pub const MULTICAST: u32 = 0x1000;
    pub const PORTSEL: u32 = 0x2000;
    pub const AUTOMEDIA: u32 = 0x4000;
    pub const DYNAMIC: u32 = 0x8000;
}

/// Build a human-readable flags string like "UP,BROADCAST,RUNNING,MULTICAST".
fn flags_to_string(flags: u32) -> String {
    let mut parts = Vec::new();
    if flags & iff::UP != 0 {
        parts.push("UP");
    }
    if flags & iff::BROADCAST != 0 {
        parts.push("BROADCAST");
    }
    if flags & iff::DEBUG != 0 {
        parts.push("DEBUG");
    }
    if flags & iff::LOOPBACK != 0 {
        parts.push("LOOPBACK");
    }
    if flags & iff::POINTOPOINT != 0 {
        parts.push("POINTOPOINT");
    }
    if flags & iff::RUNNING != 0 {
        parts.push("RUNNING");
    }
    if flags & iff::NOARP != 0 {
        parts.push("NOARP");
    }
    if flags & iff::PROMISC != 0 {
        parts.push("PROMISC");
    }
    if flags & iff::ALLMULTI != 0 {
        parts.push("ALLMULTI");
    }
    if flags & iff::MULTICAST != 0 {
        parts.push("MULTICAST");
    }
    if flags & iff::DYNAMIC != 0 {
        parts.push("DYNAMIC");
    }
    parts.join(",")
}

// ============================================================================
// Data structures
// ============================================================================

/// Complete information about a single network interface.
struct InterfaceInfo {
    name: String,
    flags: u32,
    mtu: u32,
    mac: String,
    ip_addr: String,
    netmask: String,
    broadcast: String,
    tx_queuelen: u32,
    rx_bytes: u64,
    rx_packets: u64,
    rx_errors: u64,
    rx_dropped: u64,
    rx_overruns: u64,
    rx_frame: u64,
    tx_bytes: u64,
    tx_packets: u64,
    tx_errors: u64,
    tx_dropped: u64,
    tx_overruns: u64,
    tx_carrier: u64,
    tx_collisions: u64,
}

// ============================================================================
// File reading helpers
// ============================================================================

/// Read a file to a trimmed string, returning `None` on any error.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Parse a sysfs numeric value, returning a default on failure.
fn read_sysfs_u32(path: &str, default: u32) -> u32 {
    read_file(path)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Parse a sysfs u64 counter.
fn read_sysfs_u64(path: &str) -> u64 {
    read_file(path)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Parse a hex flags value from sysfs (e.g. "0x1003" -> 0x1003).
fn read_sysfs_hex_u32(path: &str, default: u32) -> u32 {
    read_file(path)
        .and_then(|s| {
            let s = s.trim_start_matches("0x").trim_start_matches("0X");
            u32::from_str_radix(s, 16).ok()
        })
        .unwrap_or(default)
}

// ============================================================================
// Interface discovery and data collection
// ============================================================================

/// Look up IP/netmask/broadcast for an interface from `/proc/net/if_inet`.
///
/// The file format (SlateOS-specific) is one line per addressed interface:
///   `<iface>  <ip>  <netmask>  <broadcast>`
fn get_iface_ip(iface: &str) -> (String, String, String) {
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

/// Read information about all interfaces from `/sys/class/net/`, falling back
/// to `/proc/net/dev` if sysfs is unavailable.
fn read_interfaces() -> Vec<InterfaceInfo> {
    let mut interfaces = Vec::new();

    // Primary source: /sys/class/net/.
    if let Ok(entries) = fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                let base = format!("/sys/class/net/{name}");

                let flags = read_sysfs_hex_u32(&format!("{base}/flags"), 0);
                let mtu = read_sysfs_u32(&format!("{base}/mtu"), 1500);
                let mac = read_file(&format!("{base}/address"))
                    .unwrap_or_else(|| "00:00:00:00:00:00".to_string());
                let tx_queuelen = read_sysfs_u32(&format!("{base}/tx_queue_len"), 0);

                let stat = format!("{base}/statistics");
                let rx_bytes = read_sysfs_u64(&format!("{stat}/rx_bytes"));
                let rx_packets = read_sysfs_u64(&format!("{stat}/rx_packets"));
                let rx_errors = read_sysfs_u64(&format!("{stat}/rx_errors"));
                let rx_dropped = read_sysfs_u64(&format!("{stat}/rx_dropped"));
                let rx_overruns = read_sysfs_u64(&format!("{stat}/rx_fifo_errors"));
                let rx_frame = read_sysfs_u64(&format!("{stat}/rx_frame_errors"));
                let tx_bytes = read_sysfs_u64(&format!("{stat}/tx_bytes"));
                let tx_packets = read_sysfs_u64(&format!("{stat}/tx_packets"));
                let tx_errors = read_sysfs_u64(&format!("{stat}/tx_errors"));
                let tx_dropped = read_sysfs_u64(&format!("{stat}/tx_dropped"));
                let tx_overruns = read_sysfs_u64(&format!("{stat}/tx_fifo_errors"));
                let tx_carrier = read_sysfs_u64(&format!("{stat}/tx_carrier_errors"));
                let tx_collisions = read_sysfs_u64(&format!("{stat}/collisions"));

                let (ip_addr, netmask, broadcast) = get_iface_ip(name);

                interfaces.push(InterfaceInfo {
                    name: name.to_string(),
                    flags,
                    mtu,
                    mac,
                    ip_addr,
                    netmask,
                    broadcast,
                    tx_queuelen,
                    rx_bytes,
                    rx_packets,
                    rx_errors,
                    rx_dropped,
                    rx_overruns,
                    rx_frame,
                    tx_bytes,
                    tx_packets,
                    tx_errors,
                    tx_dropped,
                    tx_overruns,
                    tx_carrier,
                    tx_collisions,
                });
            }
        }
    }

    // Fallback: parse /proc/net/dev.
    if interfaces.is_empty()
        && let Some(content) = read_file("/proc/net/dev") {
            for line in content.lines().skip(2) {
                let line = line.trim();
                if let Some((name, stats)) = line.split_once(':') {
                    let name = name.trim();
                    let nums: Vec<u64> = stats
                        .split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();

                    let (ip_addr, netmask, broadcast) = get_iface_ip(name);

                    // /proc/net/dev columns:
                    // RX: bytes packets errs drop fifo frame compressed multicast
                    // TX: bytes packets errs drop fifo colls carrier compressed
                    interfaces.push(InterfaceInfo {
                        name: name.to_string(),
                        flags: if ip_addr.is_empty() { 0 } else { iff::UP | iff::RUNNING },
                        mtu: 1500,
                        mac: String::new(),
                        ip_addr,
                        netmask,
                        broadcast,
                        tx_queuelen: 0,
                        rx_bytes: nums.first().copied().unwrap_or(0),
                        rx_packets: nums.get(1).copied().unwrap_or(0),
                        rx_errors: nums.get(2).copied().unwrap_or(0),
                        rx_dropped: nums.get(3).copied().unwrap_or(0),
                        rx_overruns: nums.get(4).copied().unwrap_or(0),
                        rx_frame: nums.get(5).copied().unwrap_or(0),
                        tx_bytes: nums.get(8).copied().unwrap_or(0),
                        tx_packets: nums.get(9).copied().unwrap_or(0),
                        tx_errors: nums.get(10).copied().unwrap_or(0),
                        tx_dropped: nums.get(11).copied().unwrap_or(0),
                        tx_overruns: nums.get(12).copied().unwrap_or(0),
                        tx_carrier: nums.get(14).copied().unwrap_or(0),
                        tx_collisions: nums.get(13).copied().unwrap_or(0),
                    });
                }
            }
        }

    // Last resort: query the kernel directly. The kernel does not yet populate
    // /sys/class/net/ or /proc/net/dev, so without this the tool would report
    // no interfaces at all. SYS_NET_IF_INFO yields the default interface's live
    // configuration (TD18 read-path wiring).
    if interfaces.is_empty()
        && let Some(info) = query_net_if_info()
    {
        interfaces.push(interface_from_net_if_info(&info));
    }

    interfaces.sort_by(|a, b| {
        // Sort loopback first, then alphabetically.
        let a_lo = a.name == "lo";
        let b_lo = b.name == "lo";
        match (a_lo, b_lo) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });
    interfaces
}

// ============================================================================
// IP address helpers
// ============================================================================

/// Parse a dotted-quad IPv4 address into a network-order u32.
fn ip_to_u32(ip: &str) -> Option<u32> {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let a: u32 = parts[0].parse().ok()?;
    let b: u32 = parts[1].parse().ok()?;
    let c: u32 = parts[2].parse().ok()?;
    let d: u32 = parts[3].parse().ok()?;
    if a > 255 || b > 255 || c > 255 || d > 255 {
        return None;
    }
    Some((a << 24) | (b << 16) | (c << 8) | d)
}

/// Parse a dotted-quad IPv4 string into its 4 network-order bytes.
fn ip_to_bytes(ip: &str) -> Option<[u8; 4]> {
    ip_to_u32(ip).map(u32::to_be_bytes)
}

/// Validate that a string looks like a dotted-quad IPv4 address.
fn is_ipv4(s: &str) -> bool {
    ip_to_u32(s).is_some()
}

// ============================================================================
// Human-readable byte formatting
// ============================================================================

/// Format a byte count into a human-readable string like "554.5 KiB".
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

// ============================================================================
// Display: full interface info (classic ifconfig output)
// ============================================================================

/// Print a single interface in the classic ifconfig format:
///
/// ```text
/// eth0: flags=4163<UP,BROADCAST,RUNNING,MULTICAST>  mtu 1500
///         inet 10.0.2.15  netmask 255.255.255.0  broadcast 10.0.2.255
///         ether 52:54:00:12:34:56  txqueuelen 1000
///         RX packets 1234  bytes 567890 (554.5 KiB)
///         TX packets 567   bytes 123456 (120.5 KiB)
///         RX errors 0  dropped 0  overruns 0  frame 0
///         TX errors 0  dropped 0  overruns 0  carrier 0  collisions 0
/// ```
fn print_interface(iface: &InterfaceInfo) {
    // Line 1: name, flags, mtu.
    let flag_str = flags_to_string(iface.flags);
    println!(
        "{}: flags={}<{}>  mtu {}",
        iface.name, iface.flags, flag_str, iface.mtu
    );

    // Line 2: inet address / netmask / broadcast (if present).
    if !iface.ip_addr.is_empty() {
        let mut line = format!("        inet {}", iface.ip_addr);
        if !iface.netmask.is_empty() {
            line.push_str(&format!("  netmask {}", iface.netmask));
        }
        if !iface.broadcast.is_empty() {
            line.push_str(&format!("  broadcast {}", iface.broadcast));
        }
        println!("{line}");
    }

    // Line 3: ether / txqueuelen.
    if !iface.mac.is_empty() && iface.mac != "00:00:00:00:00:00" {
        println!(
            "        ether {}  txqueuelen {}",
            iface.mac, iface.tx_queuelen
        );
    } else if iface.flags & iff::LOOPBACK != 0 {
        println!("        loop  txqueuelen {}", iface.tx_queuelen);
    }

    // Line 4-5: RX/TX packet and byte counters.
    println!(
        "        RX packets {}  bytes {} ({})",
        iface.rx_packets,
        iface.rx_bytes,
        format_bytes(iface.rx_bytes)
    );
    println!(
        "        TX packets {}  bytes {} ({})",
        iface.tx_packets,
        iface.tx_bytes,
        format_bytes(iface.tx_bytes)
    );

    // Line 6-7: RX/TX error counters.
    println!(
        "        RX errors {}  dropped {}  overruns {}  frame {}",
        iface.rx_errors, iface.rx_dropped, iface.rx_overruns, iface.rx_frame
    );
    println!(
        "        TX errors {}  dropped {}  overruns {}  carrier {}  collisions {}",
        iface.tx_errors,
        iface.tx_dropped,
        iface.tx_overruns,
        iface.tx_carrier,
        iface.tx_collisions
    );

    // Blank line between interfaces.
    println!();
}

/// Print a short one-line-per-interface table (`ifconfig -s`).
fn print_short_table(interfaces: &[&InterfaceInfo]) {
    println!(
        "{:<10} {:<6} {:<12} {:<12} {:<10} {:<10} {:<12} {:<12} {:<10} {:<10}",
        "Iface", "MTU", "RX-OK", "RX-ERR", "RX-DRP", "RX-OVR",
        "TX-OK", "TX-ERR", "TX-DRP", "TX-OVR"
    );

    for iface in interfaces {
        println!(
            "{:<10} {:<6} {:<12} {:<12} {:<10} {:<10} {:<12} {:<12} {:<10} {:<10}",
            iface.name,
            iface.mtu,
            iface.rx_packets,
            iface.rx_errors,
            iface.rx_dropped,
            iface.rx_overruns,
            iface.tx_packets,
            iface.tx_errors,
            iface.tx_dropped,
            iface.tx_overruns
        );
    }
}

// ============================================================================
// Configuration commands
// ============================================================================

/// Report a failed `SYS_NET_IF_CONFIG` call and exit. `-1` (EPERM) is the most
/// common failure — the caller lacks the `CAP_NET_ADMIN`-class authority.
fn config_fail(op: &str, iface: &str, ret: i64) -> ! {
    if ret == -1 {
        eprintln!("ifconfig: failed to {op} {iface}: permission denied (need root)");
    } else {
        eprintln!("ifconfig: failed to {op} {iface}: error {ret}");
    }
    process::exit(1);
}

/// Bring an interface up.
fn cmd_up(iface: &str) {
    let rec = build_config_record(None, None, None, None, Some(true));
    let ret = net_if_config(&rec);
    if ret < 0 {
        config_fail("bring up", iface, ret);
    }
}

/// Bring an interface down.
fn cmd_down(iface: &str) {
    let rec = build_config_record(None, None, None, None, Some(false));
    let ret = net_if_config(&rec);
    if ret < 0 {
        config_fail("bring down", iface, ret);
    }
}

/// Set the IP address for an interface.
fn cmd_set_ip(iface: &str, ip: &str) {
    let Some(ip_bytes) = ip_to_bytes(ip) else {
        eprintln!("ifconfig: invalid IP address: {ip}");
        process::exit(1);
    };
    let rec = build_config_record(Some(ip_bytes), None, None, None, None);
    let ret = net_if_config(&rec);
    if ret < 0 {
        config_fail("set IP on", iface, ret);
    }
}

/// Set the netmask for an interface.
fn cmd_set_netmask(iface: &str, mask: &str) {
    let Some(mask_bytes) = ip_to_bytes(mask) else {
        eprintln!("ifconfig: invalid netmask: {mask}");
        process::exit(1);
    };
    let rec = build_config_record(None, Some(mask_bytes), None, None, None);
    let ret = net_if_config(&rec);
    if ret < 0 {
        config_fail("set netmask on", iface, ret);
    }
}

/// Set the broadcast address for an interface.
///
/// The kernel derives the broadcast address from the IP/mask pair rather than
/// storing it independently (`SYS_NET_IF_CONFIG` has no broadcast field), so an
/// explicit broadcast override is not representable. Validate the argument and
/// report that it is unsupported rather than silently ignoring it.
fn cmd_set_broadcast(iface: &str, bcast: &str) {
    if !is_ipv4(bcast) {
        eprintln!("ifconfig: invalid broadcast address: {bcast}");
        process::exit(1);
    }
    eprintln!(
        "ifconfig: setting an explicit broadcast on {iface} is not supported \
         (the broadcast address is derived from the IP and netmask)"
    );
    process::exit(1);
}

/// Set the MTU for an interface.
///
/// The kernel interface model does not carry a per-interface MTU
/// (`SYS_NET_IF_CONFIG` has no MTU field), so this is not representable.
/// Validate the value and report that it is unsupported rather than silently
/// pretending to succeed.
fn cmd_set_mtu(iface: &str, mtu_str: &str) {
    let mtu: u32 = match mtu_str.parse() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("ifconfig: invalid MTU value: {mtu_str}");
            process::exit(1);
        }
    };
    if mtu == 0 || mtu > 65535 {
        eprintln!("ifconfig: MTU must be between 1 and 65535");
        process::exit(1);
    }
    eprintln!("ifconfig: setting the MTU on {iface} is not supported by this kernel");
    process::exit(1);
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage() {
    println!("Usage: ifconfig [-a] [-s] [INTERFACE]");
    println!("       ifconfig INTERFACE [ADDRESS] [OPTIONS]");
    println!();
    println!("Display or configure network interfaces.");
    println!();
    println!("OPTIONS:");
    println!("  -a                    Show all interfaces, even if down");
    println!("  -s                    Short format table listing");
    println!("  up                    Activate the interface");
    println!("  down                  Deactivate the interface");
    println!("  netmask MASK          Set the subnet mask");
    println!("  broadcast ADDR        Set the broadcast address");
    println!("  mtu N                 Set the Maximum Transfer Unit");
    println!("  ADDRESS               Set the IP address (dotted quad)");
    println!();
    println!("EXAMPLES:");
    println!("  ifconfig                              Show active interfaces");
    println!("  ifconfig -a                           Show all interfaces");
    println!("  ifconfig eth0                         Show eth0 details");
    println!("  ifconfig eth0 192.168.1.100           Set IP address");
    println!("  ifconfig eth0 netmask 255.255.255.0   Set netmask");
    println!("  ifconfig eth0 up                      Bring up interface");
    println!("  ifconfig eth0 mtu 9000                Set jumbo frame MTU");
}

// ============================================================================
// Argument parsing and main
// ============================================================================

/// Parsed command-line state.
struct Args {
    /// Show all interfaces, even inactive ones.
    show_all: bool,
    /// Short table output format.
    short_format: bool,
    /// Interface name to display or configure (if any).
    iface: Option<String>,
    /// Configuration actions to apply (processed in order).
    actions: Vec<Action>,
}

/// A single configuration action to apply to an interface.
enum Action {
    Up,
    Down,
    SetIp(String),
    SetNetmask(String),
    SetBroadcast(String),
    SetMtu(String),
}

fn parse_args() -> Args {
    let raw: Vec<String> = env::args().skip(1).collect();
    let mut result = Args {
        show_all: false,
        short_format: false,
        iface: None,
        actions: Vec::new(),
    };

    let mut idx = 0;
    while idx < raw.len() {
        let arg = &raw[idx];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-a" => {
                result.show_all = true;
            }
            "-s" => {
                result.short_format = true;
            }
            "-as" | "-sa" => {
                result.show_all = true;
                result.short_format = true;
            }
            "up" => {
                result.actions.push(Action::Up);
            }
            "down" => {
                result.actions.push(Action::Down);
            }
            "netmask" => {
                idx += 1;
                if idx >= raw.len() {
                    eprintln!("ifconfig: 'netmask' requires an argument");
                    process::exit(1);
                }
                result.actions.push(Action::SetNetmask(raw[idx].clone()));
            }
            "broadcast" => {
                idx += 1;
                if idx >= raw.len() {
                    eprintln!("ifconfig: 'broadcast' requires an argument");
                    process::exit(1);
                }
                result.actions.push(Action::SetBroadcast(raw[idx].clone()));
            }
            "mtu" => {
                idx += 1;
                if idx >= raw.len() {
                    eprintln!("ifconfig: 'mtu' requires an argument");
                    process::exit(1);
                }
                result.actions.push(Action::SetMtu(raw[idx].clone()));
            }
            other => {
                // First non-flag argument is the interface name.
                // Subsequent bare arguments that look like IPs are treated as
                // "set IP address".
                if result.iface.is_none() {
                    result.iface = Some(other.to_string());
                } else if is_ipv4(other) {
                    result.actions.push(Action::SetIp(other.to_string()));
                } else {
                    eprintln!("ifconfig: unknown argument: {other}");
                    process::exit(1);
                }
            }
        }
        idx += 1;
    }

    result
}

fn main() {
    let args = parse_args();

    // If there are configuration actions, apply them.
    if !args.actions.is_empty() {
        let iface = match &args.iface {
            Some(name) => name.as_str(),
            None => {
                eprintln!("ifconfig: no interface specified for configuration");
                process::exit(1);
            }
        };

        for action in &args.actions {
            match action {
                Action::Up => cmd_up(iface),
                Action::Down => cmd_down(iface),
                Action::SetIp(ip) => cmd_set_ip(iface, ip),
                Action::SetNetmask(mask) => cmd_set_netmask(iface, mask),
                Action::SetBroadcast(bcast) => cmd_set_broadcast(iface, bcast),
                Action::SetMtu(mtu) => cmd_set_mtu(iface, mtu),
            }
        }

        // After applying config, display the interface to confirm.
        let interfaces = read_interfaces();
        if let Some(info) = interfaces.iter().find(|i| i.name == iface) {
            println!();
            print_interface(info);
        }
        return;
    }

    // Display mode.
    let interfaces = read_interfaces();

    if interfaces.is_empty() {
        println!("No network interfaces found.");
        return;
    }

    // Short table output.
    if args.short_format {
        let filtered: Vec<&InterfaceInfo> = if args.show_all {
            interfaces.iter().collect()
        } else {
            interfaces
                .iter()
                .filter(|i| i.flags & iff::UP != 0)
                .collect()
        };

        if filtered.is_empty() {
            println!("No active interfaces. Use -a to show all.");
            return;
        }

        print_short_table(&filtered);
        return;
    }

    // Full display for a specific interface.
    if let Some(ref name) = args.iface {
        match interfaces.iter().find(|i| i.name == *name) {
            Some(info) => print_interface(info),
            None => {
                eprintln!("ifconfig: interface '{name}' not found");
                process::exit(1);
            }
        }
        return;
    }

    // Full display for all (or only active) interfaces.
    for iface in &interfaces {
        if !args.show_all && iface.flags & iff::UP == 0 {
            continue;
        }
        print_interface(iface);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- flags_to_string ---

    #[test]
    fn test_flags_to_string_empty() {
        assert_eq!(flags_to_string(0), "");
    }

    #[test]
    fn test_flags_to_string_up_only() {
        assert_eq!(flags_to_string(iff::UP), "UP");
    }

    #[test]
    fn test_flags_to_string_common_ethernet() {
        // UP | BROADCAST | RUNNING | MULTICAST = 0x1043
        let flags = iff::UP | iff::BROADCAST | iff::RUNNING | iff::MULTICAST;
        assert_eq!(flags_to_string(flags), "UP,BROADCAST,RUNNING,MULTICAST");
    }

    #[test]
    fn test_flags_to_string_loopback() {
        let flags = iff::UP | iff::LOOPBACK | iff::RUNNING;
        assert_eq!(flags_to_string(flags), "UP,LOOPBACK,RUNNING");
    }

    #[test]
    fn test_flags_to_string_all() {
        let flags = iff::UP
            | iff::BROADCAST
            | iff::DEBUG
            | iff::LOOPBACK
            | iff::POINTOPOINT
            | iff::RUNNING
            | iff::NOARP
            | iff::PROMISC
            | iff::ALLMULTI
            | iff::MULTICAST
            | iff::DYNAMIC;
        let result = flags_to_string(flags);
        assert!(result.contains("UP"));
        assert!(result.contains("BROADCAST"));
        assert!(result.contains("DEBUG"));
        assert!(result.contains("LOOPBACK"));
        assert!(result.contains("POINTOPOINT"));
        assert!(result.contains("RUNNING"));
        assert!(result.contains("NOARP"));
        assert!(result.contains("PROMISC"));
        assert!(result.contains("ALLMULTI"));
        assert!(result.contains("MULTICAST"));
        assert!(result.contains("DYNAMIC"));
    }

    // --- ip_to_u32 ---

    #[test]
    fn test_ip_to_u32_valid() {
        assert_eq!(ip_to_u32("192.168.1.100"), Some(0xC0A80164));
        assert_eq!(ip_to_u32("10.0.0.1"), Some(0x0A000001));
        assert_eq!(ip_to_u32("255.255.255.0"), Some(0xFFFFFF00));
        assert_eq!(ip_to_u32("0.0.0.0"), Some(0));
        assert_eq!(ip_to_u32("127.0.0.1"), Some(0x7F000001));
        assert_eq!(ip_to_u32("255.255.255.255"), Some(0xFFFFFFFF));
    }

    #[test]
    fn test_ip_to_u32_invalid() {
        assert_eq!(ip_to_u32(""), None);
        assert_eq!(ip_to_u32("256.0.0.1"), None);
        assert_eq!(ip_to_u32("1.2.3"), None);
        assert_eq!(ip_to_u32("1.2.3.4.5"), None);
        assert_eq!(ip_to_u32("abc.def.ghi.jkl"), None);
        assert_eq!(ip_to_u32("192.168.1.-1"), None);
    }

    // --- is_ipv4 ---

    #[test]
    fn test_is_ipv4_valid() {
        assert!(is_ipv4("192.168.1.1"));
        assert!(is_ipv4("0.0.0.0"));
        assert!(is_ipv4("255.255.255.255"));
    }

    #[test]
    fn test_is_ipv4_invalid() {
        assert!(!is_ipv4("hello"));
        assert!(!is_ipv4("eth0"));
        assert!(!is_ipv4("up"));
        assert!(!is_ipv4("256.0.0.0"));
        assert!(!is_ipv4(""));
    }

    // --- format_bytes ---

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_kib() {
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(567890), "554.6 KiB");
    }

    #[test]
    fn test_format_bytes_mib() {
        assert_eq!(format_bytes(1_048_576), "1.0 MiB");
        assert_eq!(format_bytes(5_242_880), "5.0 MiB");
    }

    #[test]
    fn test_format_bytes_gib() {
        assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
        assert_eq!(format_bytes(10_737_418_240), "10.0 GiB");
    }

    // --- print_interface output format verification ---

    #[test]
    fn test_print_interface_loopback() {
        // Verify it doesn't panic and produces output.
        let iface = InterfaceInfo {
            name: "lo".to_string(),
            flags: iff::UP | iff::LOOPBACK | iff::RUNNING,
            mtu: 65536,
            mac: "00:00:00:00:00:00".to_string(),
            ip_addr: "127.0.0.1".to_string(),
            netmask: "255.0.0.0".to_string(),
            broadcast: String::new(),
            tx_queuelen: 1000,
            rx_bytes: 12345,
            rx_packets: 100,
            rx_errors: 0,
            rx_dropped: 0,
            rx_overruns: 0,
            rx_frame: 0,
            tx_bytes: 12345,
            tx_packets: 100,
            tx_errors: 0,
            tx_dropped: 0,
            tx_overruns: 0,
            tx_carrier: 0,
            tx_collisions: 0,
        };
        // Just verify it runs without panic.
        print_interface(&iface);
    }

    #[test]
    fn test_print_interface_ethernet() {
        let iface = InterfaceInfo {
            name: "eth0".to_string(),
            flags: iff::UP | iff::BROADCAST | iff::RUNNING | iff::MULTICAST,
            mtu: 1500,
            mac: "52:54:00:12:34:56".to_string(),
            ip_addr: "10.0.2.15".to_string(),
            netmask: "255.255.255.0".to_string(),
            broadcast: "10.0.2.255".to_string(),
            tx_queuelen: 1000,
            rx_bytes: 567890,
            rx_packets: 1234,
            rx_errors: 0,
            rx_dropped: 0,
            rx_overruns: 0,
            rx_frame: 0,
            tx_bytes: 123456,
            tx_packets: 567,
            tx_errors: 0,
            tx_dropped: 0,
            tx_overruns: 0,
            tx_carrier: 0,
            tx_collisions: 0,
        };
        print_interface(&iface);
    }

    // --- Short table printing ---

    #[test]
    fn test_print_short_table_no_panic() {
        let ifaces = [
            InterfaceInfo {
                name: "lo".to_string(),
                flags: iff::UP | iff::LOOPBACK | iff::RUNNING,
                mtu: 65536,
                mac: String::new(),
                ip_addr: "127.0.0.1".to_string(),
                netmask: "255.0.0.0".to_string(),
                broadcast: String::new(),
                tx_queuelen: 1000,
                rx_bytes: 0,
                rx_packets: 0,
                rx_errors: 0,
                rx_dropped: 0,
                rx_overruns: 0,
                rx_frame: 0,
                tx_bytes: 0,
                tx_packets: 0,
                tx_errors: 0,
                tx_dropped: 0,
                tx_overruns: 0,
                tx_carrier: 0,
                tx_collisions: 0,
            },
            InterfaceInfo {
                name: "eth0".to_string(),
                flags: iff::UP | iff::BROADCAST | iff::RUNNING | iff::MULTICAST,
                mtu: 1500,
                mac: "52:54:00:12:34:56".to_string(),
                ip_addr: "10.0.2.15".to_string(),
                netmask: "255.255.255.0".to_string(),
                broadcast: "10.0.2.255".to_string(),
                tx_queuelen: 1000,
                rx_bytes: 999999,
                rx_packets: 5000,
                rx_errors: 1,
                rx_dropped: 2,
                rx_overruns: 0,
                rx_frame: 0,
                tx_bytes: 500000,
                tx_packets: 3000,
                tx_errors: 0,
                tx_dropped: 0,
                tx_overruns: 0,
                tx_carrier: 0,
                tx_collisions: 0,
            },
        ];
        let refs: Vec<&InterfaceInfo> = ifaces.iter().collect();
        print_short_table(&refs);
    }

    // --- Sorting behavior ---

    #[test]
    fn test_interface_sort_order() {
        // Verifies our sorting: lo first, then alphabetical.
        let mut ifaces = [
            InterfaceInfo {
                name: "veth0".to_string(),
                flags: 0,
                mtu: 1500,
                mac: String::new(),
                ip_addr: String::new(),
                netmask: String::new(),
                broadcast: String::new(),
                tx_queuelen: 0,
                rx_bytes: 0,
                rx_packets: 0,
                rx_errors: 0,
                rx_dropped: 0,
                rx_overruns: 0,
                rx_frame: 0,
                tx_bytes: 0,
                tx_packets: 0,
                tx_errors: 0,
                tx_dropped: 0,
                tx_overruns: 0,
                tx_carrier: 0,
                tx_collisions: 0,
            },
            InterfaceInfo {
                name: "lo".to_string(),
                flags: 0,
                mtu: 65536,
                mac: String::new(),
                ip_addr: String::new(),
                netmask: String::new(),
                broadcast: String::new(),
                tx_queuelen: 0,
                rx_bytes: 0,
                rx_packets: 0,
                rx_errors: 0,
                rx_dropped: 0,
                rx_overruns: 0,
                rx_frame: 0,
                tx_bytes: 0,
                tx_packets: 0,
                tx_errors: 0,
                tx_dropped: 0,
                tx_overruns: 0,
                tx_carrier: 0,
                tx_collisions: 0,
            },
            InterfaceInfo {
                name: "eth0".to_string(),
                flags: 0,
                mtu: 1500,
                mac: String::new(),
                ip_addr: String::new(),
                netmask: String::new(),
                broadcast: String::new(),
                tx_queuelen: 0,
                rx_bytes: 0,
                rx_packets: 0,
                rx_errors: 0,
                rx_dropped: 0,
                rx_overruns: 0,
                rx_frame: 0,
                tx_bytes: 0,
                tx_packets: 0,
                tx_errors: 0,
                tx_dropped: 0,
                tx_overruns: 0,
                tx_carrier: 0,
                tx_collisions: 0,
            },
        ];

        ifaces.sort_by(|a, b| {
            let a_lo = a.name == "lo";
            let b_lo = b.name == "lo";
            match (a_lo, b_lo) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        assert_eq!(ifaces[0].name, "lo");
        assert_eq!(ifaces[1].name, "eth0");
        assert_eq!(ifaces[2].name, "veth0");
    }

    // --- Edge cases ---

    #[test]
    fn test_flags_numeric_value() {
        // Verify the common Ethernet combination matches the classic ifconfig value.
        let flags = iff::UP | iff::BROADCAST | iff::RUNNING | iff::MULTICAST;
        assert_eq!(flags, 0x1043);
    }

    #[test]
    fn test_format_bytes_boundary_kib() {
        // Exactly 1 KiB.
        assert_eq!(format_bytes(1024), "1.0 KiB");
    }

    #[test]
    fn test_format_bytes_boundary_mib() {
        assert_eq!(format_bytes(1_048_576), "1.0 MiB");
    }

    #[test]
    fn test_format_bytes_boundary_gib() {
        assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
    }

    #[test]
    fn test_ip_to_u32_loopback() {
        assert_eq!(ip_to_u32("127.0.0.1"), Some(0x7F000001));
    }

    #[test]
    fn test_ip_to_u32_class_c_mask() {
        assert_eq!(ip_to_u32("255.255.255.0"), Some(0xFFFFFF00));
    }

    #[test]
    fn test_ip_to_u32_class_a_mask() {
        assert_eq!(ip_to_u32("255.0.0.0"), Some(0xFF000000));
    }

    // --- SYS_NET_IF_INFO record decoding ---

    #[test]
    fn test_parse_net_if_info_full() {
        // 10.0.2.15 / 255.255.255.0, mac 52:54:00:12:34:56, up.
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
        assert_eq!(info.mac, [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert!(info.up);
    }

    #[test]
    fn test_parse_net_if_info_down_unconfigured() {
        let rec = [0u8; NET_IF_INFO_SIZE];
        let info = parse_net_if_info(&rec);
        assert!(is_zero4(info.ip));
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
        assert_eq!(fmt_mac([0xff, 0xab, 0x0c, 0xde, 0x01, 0x9f]), "ff:ab:0c:de:01:9f");
    }

    // --- SYS_NET_IF_CONFIG record building ---

    #[test]
    fn test_ip_to_bytes() {
        assert_eq!(ip_to_bytes("10.0.2.15"), Some([10, 0, 2, 15]));
        assert_eq!(ip_to_bytes("255.255.255.0"), Some([255, 255, 255, 0]));
        assert_eq!(ip_to_bytes("0.0.0.0"), Some([0, 0, 0, 0]));
        assert_eq!(ip_to_bytes("not.an.ip.addr"), None);
        assert_eq!(ip_to_bytes("10.0.0.256"), None);
    }

    #[test]
    fn test_build_config_record_ip_only() {
        let rec = build_config_record(Some([10, 0, 2, 42]), None, None, None, None);
        assert_eq!(&rec[0..4], &[10, 0, 2, 42]);
        // Unset fields stay zero.
        assert_eq!(&rec[4..16], &[0u8; 12]);
        assert_eq!(rec[16], 0); // up byte unused
        assert_eq!(rec[17], cfg_mask::IP);
    }

    #[test]
    fn test_build_config_record_mask_only() {
        let rec = build_config_record(None, Some([255, 255, 255, 0]), None, None, None);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
        assert_eq!(rec[17], cfg_mask::MASK);
    }

    #[test]
    fn test_build_config_record_up_and_down() {
        let up = build_config_record(None, None, None, None, Some(true));
        assert_eq!(up[16], 1);
        assert_eq!(up[17], cfg_mask::UP);

        let down = build_config_record(None, None, None, None, Some(false));
        assert_eq!(down[16], 0);
        assert_eq!(down[17], cfg_mask::UP);
    }

    #[test]
    fn test_build_config_record_all_fields() {
        let rec = build_config_record(
            Some([10, 0, 2, 15]),
            Some([255, 255, 255, 0]),
            Some([10, 0, 2, 2]),
            Some([9, 9, 9, 9]),
            Some(true),
        );
        assert_eq!(&rec[0..4], &[10, 0, 2, 15]);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
        assert_eq!(&rec[8..12], &[10, 0, 2, 2]);
        assert_eq!(&rec[12..16], &[9, 9, 9, 9]);
        assert_eq!(rec[16], 1);
        assert_eq!(
            rec[17],
            cfg_mask::IP | cfg_mask::MASK | cfg_mask::GATEWAY | cfg_mask::DNS | cfg_mask::UP
        );
    }

    #[test]
    fn test_build_config_record_empty() {
        // No fields requested -> mask 0 (kernel treats this as a no-op success).
        let rec = build_config_record(None, None, None, None, None);
        assert_eq!(rec, [0u8; 18]);
    }

    #[test]
    fn test_is_zero4() {
        assert!(is_zero4([0, 0, 0, 0]));
        assert!(!is_zero4([0, 0, 0, 1]));
        assert!(!is_zero4([10, 0, 0, 0]));
    }

    #[test]
    fn test_compute_broadcast() {
        // /24 network.
        assert_eq!(
            compute_broadcast([10, 0, 2, 15], [255, 255, 255, 0]),
            [10, 0, 2, 255]
        );
        // /16 network.
        assert_eq!(
            compute_broadcast([192, 168, 5, 7], [255, 255, 0, 0]),
            [192, 168, 255, 255]
        );
        // /8 network.
        assert_eq!(
            compute_broadcast([10, 1, 2, 3], [255, 0, 0, 0]),
            [10, 255, 255, 255]
        );
        // host route /32 -> broadcast is the host itself.
        assert_eq!(
            compute_broadcast([10, 0, 0, 1], [255, 255, 255, 255]),
            [10, 0, 0, 1]
        );
    }

    #[test]
    fn test_interface_from_net_if_info_configured_up() {
        let info = NetIfInfo {
            ip: [10, 0, 2, 15],
            mask: [255, 255, 255, 0],
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            up: true,
        };
        let iface = interface_from_net_if_info(&info);
        assert_eq!(iface.name, "eth0");
        assert_eq!(iface.ip_addr, "10.0.2.15");
        assert_eq!(iface.netmask, "255.255.255.0");
        assert_eq!(iface.broadcast, "10.0.2.255");
        assert_eq!(iface.mac, "52:54:00:12:34:56");
        assert_ne!(iface.flags & iff::UP, 0);
        assert_ne!(iface.flags & iff::RUNNING, 0);
    }

    #[test]
    fn test_interface_from_net_if_info_unconfigured_down() {
        let info = NetIfInfo {
            ip: [0, 0, 0, 0],
            mask: [0, 0, 0, 0],
            mac: [0, 0, 0, 0, 0, 0],
            up: false,
        };
        let iface = interface_from_net_if_info(&info);
        // No IP => the inet line is suppressed (empty strings).
        assert!(iface.ip_addr.is_empty());
        assert!(iface.netmask.is_empty());
        assert!(iface.broadcast.is_empty());
        // Down => UP flag is clear.
        assert_eq!(iface.flags & iff::UP, 0);
    }
}
