//! Slate OS `ifconfig` -- classic network interface configuration utility.
//!
//! Displays and configures network interfaces using the traditional `ifconfig`
//! command syntax. Reads live state from `/sys/class/net/` and `/proc/net/`,
//! writes configuration via `SYS_NET_IOCTL` syscalls.
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

/// Network IOCTL syscall number -- shared with the `ip` utility.
const SYS_NET_IOCTL: u64 = 810;

// IOCTL sub-commands for network interface configuration.
const NET_IF_UP: u64 = 1;
const NET_IF_DOWN: u64 = 2;
const NET_IF_SET_IP: u64 = 3;
const NET_IF_SET_MASK: u64 = 4;
#[allow(dead_code)] // Available when gateway-via-ifconfig is wired up.
const NET_IF_SET_GW: u64 = 5;
const NET_IF_SET_MTU: u64 = 6;
const NET_IF_SET_BCAST: u64 = 7;

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

/// Issue a network IOCTL syscall for the named interface.
fn net_ioctl(cmd: u64, iface: &str, arg: u64) -> i64 {
    let name = format!("{iface}\0");
    // SAFETY: We pass a valid NUL-terminated interface name and a numeric arg.
    unsafe { syscall4(SYS_NET_IOCTL, cmd, name.as_ptr() as u64, arg, 0) }
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

/// Bring an interface up.
fn cmd_up(iface: &str) {
    let ret = net_ioctl(NET_IF_UP, iface, 0);
    if ret < 0 {
        eprintln!("ifconfig: failed to bring up {iface}: error {ret}");
        process::exit(1);
    }
}

/// Bring an interface down.
fn cmd_down(iface: &str) {
    let ret = net_ioctl(NET_IF_DOWN, iface, 0);
    if ret < 0 {
        eprintln!("ifconfig: failed to bring down {iface}: error {ret}");
        process::exit(1);
    }
}

/// Set the IP address for an interface.
fn cmd_set_ip(iface: &str, ip: &str) {
    let ip_val = match ip_to_u32(ip) {
        Some(v) => v,
        None => {
            eprintln!("ifconfig: invalid IP address: {ip}");
            process::exit(1);
        }
    };
    let ret = net_ioctl(NET_IF_SET_IP, iface, u64::from(ip_val));
    if ret < 0 {
        eprintln!("ifconfig: failed to set IP on {iface}: error {ret}");
        process::exit(1);
    }
}

/// Set the netmask for an interface.
fn cmd_set_netmask(iface: &str, mask: &str) {
    let mask_val = match ip_to_u32(mask) {
        Some(v) => v,
        None => {
            eprintln!("ifconfig: invalid netmask: {mask}");
            process::exit(1);
        }
    };
    let ret = net_ioctl(NET_IF_SET_MASK, iface, u64::from(mask_val));
    if ret < 0 {
        eprintln!("ifconfig: failed to set netmask on {iface}: error {ret}");
        process::exit(1);
    }
}

/// Set the broadcast address for an interface.
fn cmd_set_broadcast(iface: &str, bcast: &str) {
    let bcast_val = match ip_to_u32(bcast) {
        Some(v) => v,
        None => {
            eprintln!("ifconfig: invalid broadcast address: {bcast}");
            process::exit(1);
        }
    };
    let ret = net_ioctl(NET_IF_SET_BCAST, iface, u64::from(bcast_val));
    if ret < 0 {
        eprintln!("ifconfig: failed to set broadcast on {iface}: error {ret}");
        process::exit(1);
    }
}

/// Set the MTU for an interface.
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
    let ret = net_ioctl(NET_IF_SET_MTU, iface, u64::from(mtu));
    if ret < 0 {
        eprintln!("ifconfig: failed to set MTU on {iface}: error {ret}");
        process::exit(1);
    }
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
}
