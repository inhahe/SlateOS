//! Slate OS Routing Table Management
//!
//! Display and manage the kernel IP routing table.
//! Similar to the classic `route` command on Linux/BSD.
//!
//! # Usage
//!
//! ```text
//! route                       Show routing table (resolve names)
//! route -n                    Show routing table (numeric)
//! route add default gw IP     Add default route
//! route add -net NET gw IP    Add network route
//! route add -host IP gw GW    Add host route
//! route del default           Delete default route
//! route del -net NET          Delete network route
//! route flush                 Remove all routes
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

/// Read-only interface-info query syscall (`kernel/src/syscall/number.rs`,
/// `SYS_NET_IF_INFO`). Returns a fixed 24-byte record describing the default
/// network interface. The kernel does not populate `/proc/net/route`,
/// `/sys/net/routes`, or `/proc/net/if_inet`, so the routing table is
/// synthesized from this syscall (TD18 read-path wiring).
const SYS_NET_IF_INFO: u64 = 842;

/// Size of the `SYS_NET_IF_INFO` record (must match the kernel's `INFO_SIZE`).
const NET_IF_INFO_SIZE: usize = 24;

/// Interface-configuration write syscall (`kernel/src/syscall/number.rs`,
/// `SYS_NET_IF_CONFIG`). Root-gated. Reads an 18-byte record from `arg0`
/// (length in `arg1`) whose byte 17 is a per-field mask selecting which of
/// IP/mask/gateway/DNS/up to apply (read-modify-write against the live
/// config). The kernel models a single default gateway on the primary
/// interface (no general routing table yet), so `route` can only represent the
/// default route by writing this gateway field. See [`build_config_record`].
const SYS_NET_IF_CONFIG: u64 = 856;

/// Routing-table write/read syscalls (`kernel/src/syscall/number.rs`). These
/// carry *non-default* routes; the default route (`0.0.0.0/0`) lives in the
/// interface gateway (`SYS_NET_IF_CONFIG`), not here (design-decisions §52).
///
/// - `SYS_NET_ROUTE_ADD` (root-gated): 16-byte record
///   `[dest(4), mask(4), gateway(4), metric(4, LE)]`.
/// - `SYS_NET_ROUTE_DEL` (root-gated): 8-byte record `[dest(4), mask(4)]`.
/// - `SYS_NET_ROUTE_LIST` (read-only): fills a buffer with 16-byte records and
///   returns the count.
const SYS_NET_ROUTE_ADD: u64 = 857;
const SYS_NET_ROUTE_DEL: u64 = 858;
const SYS_NET_ROUTE_LIST: u64 = 859;

/// Size of a `SYS_NET_ROUTE_ADD` / `SYS_NET_ROUTE_LIST` record.
const ROUTE_REC_SIZE: usize = 16;
/// Size of a `SYS_NET_ROUTE_DEL` record.
const ROUTE_DEL_SIZE: usize = 8;

/// Field-mask bits for the `SYS_NET_IF_CONFIG` record (byte 17). A set bit
/// means "apply this field"; unset means "leave the current value untouched".
mod cfg_mask {
    /// Apply the gateway (record bytes 8..12).
    pub const GATEWAY: u8 = 1 << 2;
}

#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Build the 18-byte `SYS_NET_IF_CONFIG` record that sets the default gateway.
/// Only the gateway field and its mask bit are populated; every other field is
/// left clear so the kernel preserves the current IP/mask/DNS/up state
/// (read-modify-write). Pure (no syscall) so it is unit-testable on the host.
///
/// Layout: `[0..4]` ip, `[4..8]` mask, `[8..12]` gateway, `[12..16]` dns,
/// `[16]` up flag, `[17]` field mask.
fn build_gateway_record(gateway: [u8; 4]) -> [u8; 18] {
    let mut rec = [0u8; 18];
    rec[8..12].copy_from_slice(&gateway);
    rec[17] = cfg_mask::GATEWAY;
    rec
}

/// Apply an interface configuration via `SYS_NET_IF_CONFIG`. Returns the
/// kernel's signed result (0 on success, negative errno on failure).
fn net_if_config(rec: &[u8; 18]) -> i64 {
    // SAFETY: `rec` is exactly 18 bytes, matching the kernel's REC_SIZE
    // contract; the kernel only reads (never writes) the record. `arg2` is
    // unused by the syscall (it reads only arg0=ptr and arg1=len).
    unsafe { syscall3(SYS_NET_IF_CONFIG, rec.as_ptr() as u64, rec.len() as u64, 0) }
}

/// Build the 16-byte `SYS_NET_ROUTE_ADD` record from host-order `u32`
/// addresses. Addresses are stored big-endian (network order); the metric is
/// little-endian to match the kernel's `u32::from_le_bytes` decode. Pure (no
/// syscall) so it is unit-testable on the host.
///
/// Layout: `[0..4]` dest, `[4..8]` mask, `[8..12]` gateway, `[12..16]` metric.
fn build_route_record(dest: u32, mask: u32, gateway: u32, metric: u32) -> [u8; ROUTE_REC_SIZE] {
    let mut rec = [0u8; ROUTE_REC_SIZE];
    rec[0..4].copy_from_slice(&dest.to_be_bytes());
    rec[4..8].copy_from_slice(&mask.to_be_bytes());
    rec[8..12].copy_from_slice(&gateway.to_be_bytes());
    rec[12..16].copy_from_slice(&metric.to_le_bytes());
    rec
}

/// Build the 8-byte `SYS_NET_ROUTE_DEL` record. Pure (no syscall).
///
/// Layout: `[0..4]` dest, `[4..8]` mask.
fn build_route_del_record(dest: u32, mask: u32) -> [u8; ROUTE_DEL_SIZE] {
    let mut rec = [0u8; ROUTE_DEL_SIZE];
    rec[0..4].copy_from_slice(&dest.to_be_bytes());
    rec[4..8].copy_from_slice(&mask.to_be_bytes());
    rec
}

/// Add a route via `SYS_NET_ROUTE_ADD`. Returns the kernel's signed result.
fn net_route_add(rec: &[u8; ROUTE_REC_SIZE]) -> i64 {
    // SAFETY: `rec` is exactly `ROUTE_REC_SIZE` bytes, matching the kernel's
    // contract; the kernel only reads (never writes) the record.
    unsafe { syscall3(SYS_NET_ROUTE_ADD, rec.as_ptr() as u64, rec.len() as u64, 0) }
}

/// Delete a route via `SYS_NET_ROUTE_DEL`. Returns the kernel's signed result.
fn net_route_del(rec: &[u8; ROUTE_DEL_SIZE]) -> i64 {
    // SAFETY: `rec` is exactly `ROUTE_DEL_SIZE` bytes, matching the kernel's
    // contract; the kernel only reads (never writes) the record.
    unsafe { syscall3(SYS_NET_ROUTE_DEL, rec.as_ptr() as u64, rec.len() as u64, 0) }
}

/// List routes via `SYS_NET_ROUTE_LIST`. Fills `buf` with 16-byte records and
/// returns the kernel's signed result (route count, or negative errno).
fn net_route_list(buf: &mut [u8]) -> i64 {
    // SAFETY: `buf` is a valid writable slice of `buf.len()` bytes; the kernel
    // writes at most `buf.len()` bytes (whole 16-byte records only).
    unsafe { syscall3(SYS_NET_ROUTE_LIST, buf.as_mut_ptr() as u64, buf.len() as u64, 0) }
}

/// Decode one 16-byte route record into host-order (dest, mask, gateway,
/// metric). Pure (no syscall) so it is unit-testable on the host.
fn parse_route_record(rec: &[u8; ROUTE_REC_SIZE]) -> (u32, u32, u32, u32) {
    let dest = u32::from_be_bytes([rec[0], rec[1], rec[2], rec[3]]);
    let mask = u32::from_be_bytes([rec[4], rec[5], rec[6], rec[7]]);
    let gateway = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]);
    let metric = u32::from_le_bytes([rec[12], rec[13], rec[14], rec[15]]);
    (dest, mask, gateway, metric)
}

// ============================================================================
// IP address helpers
// ============================================================================

fn parse_ipv4(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let a: u8 = parts[0].parse().ok()?;
    let b: u8 = parts[1].parse().ok()?;
    let c: u8 = parts[2].parse().ok()?;
    let d: u8 = parts[3].parse().ok()?;
    Some(u32::from_be_bytes([a, b, c, d]))
}

fn ip_to_string(ip: u32) -> String {
    let bytes = ip.to_be_bytes();
    format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
}

fn cidr_to_mask(prefix_len: u32) -> u32 {
    if prefix_len == 0 {
        0
    } else if prefix_len >= 32 {
        0xFFFF_FFFF
    } else {
        !((1u32 << (32 - prefix_len)) - 1)
    }
}

fn mask_to_prefix(mask: u32) -> u32 {
    mask.count_ones()
}

/// Parse network/prefix, e.g., "10.0.0.0/24" → (ip, mask)
fn parse_network(s: &str) -> Option<(u32, u32)> {
    if let Some(idx) = s.find('/') {
        let ip_str = &s[..idx];
        let prefix_str = &s[idx + 1..];
        let ip = parse_ipv4(ip_str)?;
        let prefix: u32 = prefix_str.parse().ok()?;
        if prefix > 32 {
            return None;
        }
        Some((ip, cidr_to_mask(prefix)))
    } else {
        // Plain IP — assume /32 host route unless it looks like a network.
        let ip = parse_ipv4(s)?;
        Some((ip, 0xFFFF_FFFF))
    }
}

// ============================================================================
// Route entry
// ============================================================================

#[derive(Clone)]
struct RouteEntry {
    destination: u32,
    gateway: u32,
    genmask: u32,
    flags: u16,
    metric: u32,
    refcnt: u32,
    use_count: u32,
    iface: String,
}

// Route flags.
const RTF_UP: u16 = 0x0001;
const RTF_GATEWAY: u16 = 0x0002;
const RTF_HOST: u16 = 0x0004;
const RTF_REJECT: u16 = 0x0008;
#[allow(dead_code)]
const RTF_DYNAMIC: u16 = 0x0010;
#[allow(dead_code)]
const RTF_MODIFIED: u16 = 0x0020;

fn flags_to_string(flags: u16) -> String {
    let mut s = String::new();
    if flags & RTF_UP != 0 {
        s.push('U');
    }
    if flags & RTF_GATEWAY != 0 {
        s.push('G');
    }
    if flags & RTF_HOST != 0 {
        s.push('H');
    }
    if flags & RTF_REJECT != 0 {
        s.push('!');
    }
    if flags & RTF_DYNAMIC != 0 {
        s.push('D');
    }
    if flags & RTF_MODIFIED != 0 {
        s.push('M');
    }
    if s.is_empty() {
        s.push('-');
    }
    s
}

// ============================================================================
// Kernel interface-info query (SYS_NET_IF_INFO, read-only)
// ============================================================================

/// Decoded subset of the kernel's 24-byte `SYS_NET_IF_INFO` record needed to
/// synthesize the routing table.
///
/// Layout (see `kernel/src/syscall/handlers.rs::sys_net_if_info`):
/// `[0..4]` IPv4 address, `[4..8]` subnet mask, `[8..12]` gateway.
/// Addresses are stored big-endian as host `u32` to match the rest of this
/// tool's representation.
struct NetIfInfo {
    ip: u32,
    mask: u32,
    gateway: u32,
}

/// Decode a raw `SYS_NET_IF_INFO` record. Pure (no syscall) so it is unit-test
/// friendly on the host.
fn parse_net_if_info(rec: &[u8; NET_IF_INFO_SIZE]) -> NetIfInfo {
    NetIfInfo {
        ip: u32::from_be_bytes([rec[0], rec[1], rec[2], rec[3]]),
        mask: u32::from_be_bytes([rec[4], rec[5], rec[6], rec[7]]),
        gateway: u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]),
    }
}

/// Synthesize routing-table entries from a decoded interface record: the
/// directly-connected network route (if an IP/mask is configured) and the
/// default route via the gateway (if one is configured).
fn synth_routes_from_net_if_info(info: &NetIfInfo) -> Vec<RouteEntry> {
    let mut routes = Vec::new();
    if info.ip != 0 && info.mask != 0 {
        routes.push(RouteEntry {
            destination: info.ip & info.mask,
            gateway: 0,
            genmask: info.mask,
            flags: RTF_UP,
            metric: 0,
            refcnt: 0,
            use_count: 0,
            iface: "eth0".to_string(),
        });
    }
    if info.gateway != 0 {
        routes.push(RouteEntry {
            destination: 0,
            gateway: info.gateway,
            genmask: 0,
            flags: RTF_UP | RTF_GATEWAY,
            metric: 0,
            refcnt: 0,
            use_count: 0,
            iface: "eth0".to_string(),
        });
    }
    routes
}

/// Query the kernel for the default interface's configuration via
/// `SYS_NET_IF_INFO`. Returns `None` if the syscall fails.
fn query_net_if_info() -> Option<NetIfInfo> {
    let mut buf = [0u8; NET_IF_INFO_SIZE];
    // SAFETY: `buf` is exactly NET_IF_INFO_SIZE bytes, satisfying the kernel's
    // minimum-length contract; the kernel writes at most that many bytes.
    let ret = unsafe { syscall3(SYS_NET_IF_INFO, buf.as_mut_ptr() as u64, buf.len() as u64, 0) };
    if ret < 0 {
        return None;
    }
    Some(parse_net_if_info(&buf))
}

/// Read the caller's netns route table via `SYS_NET_ROUTE_LIST`. Returns an
/// empty vec on any error. The kernel caps the table at 32 entries, so a
/// 32-record buffer always suffices.
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
        let mut flags = RTF_UP;
        if gateway != 0 {
            flags |= RTF_GATEWAY;
        }
        if mask == 0xFFFF_FFFF {
            flags |= RTF_HOST;
        }
        routes.push(RouteEntry {
            destination: dest,
            gateway,
            genmask: mask,
            flags,
            metric,
            refcnt: 0,
            use_count: 0,
            iface: "eth0".to_string(),
        });
    }
    routes
}

// ============================================================================
// Read routing table
// ============================================================================

fn read_routes() -> Vec<RouteEntry> {
    let mut routes = Vec::new();

    // Try /proc/net/route (Linux-compatible format).
    if let Ok(content) = fs::read_to_string("/proc/net/route") {
        for (i, line) in content.lines().enumerate() {
            // Skip header line.
            if i == 0 {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 8 {
                continue;
            }
            let iface = fields[0].to_string();
            let dest = u32::from_str_radix(fields[1], 16).unwrap_or(0);
            let gw = u32::from_str_radix(fields[2], 16).unwrap_or(0);
            let flags = u16::from_str_radix(fields[3], 16).unwrap_or(0);
            let refcnt = fields[4].parse::<u32>().unwrap_or(0);
            let use_count = fields[5].parse::<u32>().unwrap_or(0);
            let metric = fields[6].parse::<u32>().unwrap_or(0);
            let mask = u32::from_str_radix(fields[7], 16).unwrap_or(0);

            routes.push(RouteEntry {
                destination: dest,
                gateway: gw,
                genmask: mask,
                flags,
                metric,
                refcnt,
                use_count,
                iface,
            });
        }
    }

    // Try /sys/net/routes as fallback.
    if routes.is_empty()
        && let Ok(content) = fs::read_to_string("/sys/net/routes") {
            for line in content.lines() {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() < 5 {
                    continue;
                }
                // Format: dest/prefix gateway metric flags iface
                let dest_str = fields[0];
                let (dest, mask) = if let Some(slash) = dest_str.find('/') {
                    let ip = parse_ipv4(&dest_str[..slash]).unwrap_or(0);
                    let prefix: u32 = dest_str[slash + 1..].parse().unwrap_or(0);
                    (ip, cidr_to_mask(prefix))
                } else if dest_str == "default" {
                    (0, 0)
                } else {
                    (parse_ipv4(dest_str).unwrap_or(0), 0xFFFF_FFFF)
                };
                let gw = parse_ipv4(fields[1]).unwrap_or(0);
                let metric = fields[2].parse::<u32>().unwrap_or(0);
                let flags_val = fields[3].parse::<u16>().unwrap_or(RTF_UP);
                let iface = fields[4].to_string();

                routes.push(RouteEntry {
                    destination: dest,
                    gateway: gw,
                    genmask: mask,
                    flags: flags_val,
                    metric,
                    refcnt: 0,
                    use_count: 0,
                    iface,
                });
            }
        }

    // If we still have nothing, create a synthetic default.
    if routes.is_empty() {
        // Try to read the interface config for some route info.
        if let Ok(content) = fs::read_to_string("/proc/net/if_inet") {
            for line in content.lines() {
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() >= 4 {
                    let iface = fields[0].to_string();
                    let ip = parse_ipv4(fields[1]).unwrap_or(0);
                    let mask = parse_ipv4(fields[2]).unwrap_or(0);
                    let gw = parse_ipv4(fields[3]).unwrap_or(0);

                    // Add network route.
                    let net = ip & mask;
                    routes.push(RouteEntry {
                        destination: net,
                        gateway: 0,
                        genmask: mask,
                        flags: RTF_UP,
                        metric: 0,
                        refcnt: 0,
                        use_count: 0,
                        iface: iface.clone(),
                    });

                    // Add default route via gateway.
                    if gw != 0 {
                        routes.push(RouteEntry {
                            destination: 0,
                            gateway: gw,
                            genmask: 0,
                            flags: RTF_UP | RTF_GATEWAY,
                            metric: 0,
                            refcnt: 0,
                            use_count: 0,
                            iface,
                        });
                    }
                }
            }
        }
    }

    // Final live source: /proc/net/route(s) and /proc/net/if_inet are not
    // populated on Slate OS, so the routing table is assembled from the kernel
    // route table (SYS_NET_ROUTE_LIST, non-default routes) plus the interface
    // record (SYS_NET_IF_INFO, connected + default routes).
    if routes.is_empty() {
        routes.extend(read_routes_from_kernel());
        if let Some(info) = query_net_if_info() {
            routes.extend(synth_routes_from_net_if_info(&info));
        }
    }

    routes
}

// ============================================================================
// Display routing table
// ============================================================================

fn display_routes(numeric: bool, verbose: bool) {
    let routes = read_routes();

    println!("Kernel IP routing table");
    println!(
        "{:<16} {:<16} {:<16} {:<6} {:<6} {:<4} {:<6} Iface",
        "Destination", "Gateway", "Genmask", "Flags", "Metric", "Ref", "Use"
    );

    if routes.is_empty() {
        if verbose {
            println!("(no routes)");
        }
        return;
    }

    for r in &routes {
        let dest_str = if r.destination == 0 && r.genmask == 0 {
            if numeric {
                "0.0.0.0".to_string()
            } else {
                "default".to_string()
            }
        } else {
            ip_to_string(r.destination)
        };

        let gw_str = if r.gateway == 0 {
            if numeric {
                "0.0.0.0".to_string()
            } else {
                "*".to_string()
            }
        } else {
            ip_to_string(r.gateway)
        };

        let mask_str = ip_to_string(r.genmask);
        let flags_str = flags_to_string(r.flags);

        println!(
            "{:<16} {:<16} {:<16} {:<6} {:<6} {:<4} {:<6} {}",
            dest_str, gw_str, mask_str, flags_str, r.metric, r.refcnt, r.use_count, r.iface
        );
    }

    if verbose {
        println!();
        println!("{} route(s) total", routes.len());
    }
}

// ============================================================================
// Route manipulation
// ============================================================================

/// True if `(dest, mask)` denotes the default route (`0.0.0.0/0`).
fn is_default_route(dest: u32, mask: u32) -> bool {
    dest == 0 && mask == 0
}

/// Report a failed `SYS_NET_IF_CONFIG` call and exit. `-1` (EPERM) is the most
/// common failure — the caller lacks the `CAP_NET_ADMIN`-class authority.
fn config_fail(msg: &str, ret: i64) -> ! {
    if ret == -1 {
        eprintln!("{msg}: Operation not permitted (need root)");
    } else {
        eprintln!("{msg}: error {ret}");
    }
    process::exit(1);
}

fn add_route(dest: u32, mask: u32, gateway: u32, metric: u32, _is_host: bool) {
    // The default route (0.0.0.0/0) is owned by the interface gateway
    // (SYS_NET_IF_CONFIG); every other prefix is a route-table entry written
    // via SYS_NET_ROUTE_ADD (design-decisions §52).
    if is_default_route(dest, mask) {
        if gateway == 0 {
            eprintln!("route: 'add default' requires a gateway ('gw <address>')");
            process::exit(1);
        }
        let rec = build_gateway_record(gateway.to_be_bytes());
        let ret = net_if_config(&rec);
        if ret < 0 {
            config_fail(
                &format!("SIOCADDRT: failed to add default route via {}", ip_to_string(gateway)),
                ret,
            );
        }
        println!("Added default route via {}", ip_to_string(gateway));
        return;
    }

    // Non-default route. The kernel requires a non-zero gateway (next hop).
    if gateway == 0 {
        eprintln!(
            "route: adding {}/{} requires a gateway ('gw <address>')",
            ip_to_string(dest),
            mask_to_prefix(mask)
        );
        process::exit(1);
    }
    let rec = build_route_record(dest, mask, gateway, metric);
    let ret = net_route_add(&rec);
    if ret < 0 {
        config_fail(
            &format!(
                "SIOCADDRT: failed to add route {}/{} via {}",
                ip_to_string(dest),
                mask_to_prefix(mask),
                ip_to_string(gateway)
            ),
            ret,
        );
    }
    println!(
        "Added route {}/{} via {} metric {}",
        ip_to_string(dest),
        mask_to_prefix(mask),
        ip_to_string(gateway),
        metric
    );
}

fn del_route(dest: u32, mask: u32) {
    // The default route clears the interface gateway; other prefixes are
    // removed from the route table via SYS_NET_ROUTE_DEL.
    if is_default_route(dest, mask) {
        let rec = build_gateway_record([0, 0, 0, 0]);
        let ret = net_if_config(&rec);
        if ret < 0 {
            config_fail("SIOCDELRT: failed to delete default route", ret);
        }
        println!("Deleted default route");
        return;
    }

    let rec = build_route_del_record(dest, mask);
    let ret = net_route_del(&rec);
    if ret < 0 {
        config_fail(
            &format!(
                "SIOCDELRT: failed to delete route {}/{}",
                ip_to_string(dest),
                mask_to_prefix(mask)
            ),
            ret,
        );
    }
    println!(
        "Deleted route {}/{}",
        ip_to_string(dest),
        mask_to_prefix(mask)
    );
}

fn flush_routes() {
    // Remove every non-default route from the kernel table, then clear the
    // interface gateway (the default route). Connected routes are implicit in
    // the interface IP/mask and are not stored in the table, so they remain.
    let table_routes = read_routes_from_kernel();
    let mut removed = 0usize;
    let mut first_err: Option<i64> = None;
    for r in &table_routes {
        if is_default_route(r.destination, r.genmask) {
            continue; // Handled below via the interface gateway.
        }
        let rec = build_route_del_record(r.destination, r.genmask);
        let ret = net_route_del(&rec);
        if ret < 0 {
            // Track the first failure but keep going so a single bad entry
            // doesn't abort the whole flush.
            if first_err.is_none() {
                first_err = Some(ret);
            }
        } else {
            removed += 1;
        }
    }

    // Clear the default gateway.
    let rec = build_gateway_record([0, 0, 0, 0]);
    let ret = net_if_config(&rec);
    if ret < 0 && first_err.is_none() {
        first_err = Some(ret);
    }

    if let Some(err) = first_err {
        config_fail(
            &format!("route: flush incomplete (removed {removed} route(s))"),
            err,
        );
    }
    println!("Flushed {removed} route(s) and cleared the default gateway");
}

// ============================================================================
// CLI
// ============================================================================

fn print_usage() {
    println!("Slate OS Route Management v0.1.0");
    println!();
    println!("Display and manage the kernel IP routing table.");
    println!();
    println!("USAGE:");
    println!("  route [options]                        Show routing table");
    println!("  route add [-net|-host] TARGET [opts]   Add a route");
    println!("  route del [-net|-host] TARGET          Delete a route");
    println!("  route flush                            Remove all routes");
    println!();
    println!("OPTIONS:");
    println!("  -n              Numeric output (don't resolve names)");
    println!("  -v              Verbose output");
    println!("  --version       Show version");
    println!("  -h, --help      Show this help");
    println!();
    println!("ADD/DEL OPTIONS:");
    println!("  -net NETWORK    Target is a network (e.g., 10.0.0.0/24)");
    println!("  -host ADDRESS   Target is a host");
    println!("  gw GATEWAY      Route through this gateway");
    println!("  dev IFACE       Route through this interface");
    println!("  metric N        Set route metric");
    println!();
    println!("EXAMPLES:");
    println!("  route -n");
    println!("  route add default gw 192.168.1.1");
    println!("  route add -net 10.0.0.0/24 gw 192.168.1.1");
    println!("  route add -host 10.0.0.5 gw 192.168.1.1 metric 100");
    println!("  route del default");
    println!("  route del -net 10.0.0.0/24");
    println!("  route flush");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        display_routes(false, false);
        return;
    }

    let mut numeric = false;
    let mut verbose = false;
    let mut action: Option<&str> = None;
    let mut is_host = false;
    let mut target: Option<String> = None;
    let mut gateway: Option<String> = None;
    let mut metric: u32 = 0;
    let mut idx = 1;

    // Parse global flags first.
    while idx < args.len() {
        match args[idx].as_str() {
            "-n" => {
                numeric = true;
                idx += 1;
            }
            "-v" => {
                verbose = true;
                idx += 1;
            }
            "-h" | "--help" | "help" => {
                print_usage();
                return;
            }
            "--version" => {
                println!("route (Slate OS) 0.1.0");
                return;
            }
            "add" | "del" | "delete" | "flush" => {
                action = Some(if args[idx] == "delete" { "del" } else { &args[idx] });
                idx += 1;
                break;
            }
            _ => {
                // Unknown flag before action — might just be display.
                break;
            }
        }
    }

    match action {
        None => {
            display_routes(numeric, verbose);
        }
        Some("flush") => {
            flush_routes();
        }
        Some("add") => {
            // Parse: add [-net|-host] TARGET [gw GW] [dev DEV] [metric N]
            while idx < args.len() {
                match args[idx].as_str() {
                    "-net" => {
                        is_host = false;
                        idx += 1;
                    }
                    "-host" => {
                        is_host = true;
                        idx += 1;
                    }
                    "gw" | "gateway" => {
                        idx += 1;
                        if idx < args.len() {
                            gateway = Some(args[idx].clone());
                            idx += 1;
                        } else {
                            eprintln!("error: 'gw' requires an address");
                            process::exit(1);
                        }
                    }
                    "dev" => {
                        idx += 1;
                        // Interface name — we note it but our syscall doesn't use it yet.
                        if idx < args.len() {
                            if verbose {
                                println!("(interface: {})", args[idx]);
                            }
                            idx += 1;
                        }
                    }
                    "metric" => {
                        idx += 1;
                        if idx < args.len() {
                            metric = args[idx].parse().unwrap_or(0);
                            idx += 1;
                        }
                    }
                    "netmask" => {
                        idx += 1;
                        // Legacy netmask specification — handled via target parsing.
                        if idx < args.len() {
                            idx += 1;
                        }
                    }
                    _ => {
                        if target.is_none() {
                            target = Some(args[idx].clone());
                        }
                        idx += 1;
                    }
                }
            }

            let target_str = match &target {
                Some(t) => t.clone(),
                None => {
                    eprintln!("error: route add requires a target");
                    eprintln!("Usage: route add [-net|-host] TARGET [gw GATEWAY] [metric N]");
                    process::exit(1);
                }
            };

            let (dest, mask) = if target_str == "default" {
                (0u32, 0u32)
            } else if is_host {
                match parse_ipv4(&target_str) {
                    Some(ip) => (ip, 0xFFFF_FFFF),
                    None => {
                        eprintln!("error: invalid host address: {}", target_str);
                        process::exit(1);
                    }
                }
            } else {
                match parse_network(&target_str) {
                    Some(pair) => pair,
                    None => {
                        eprintln!("error: invalid network: {}", target_str);
                        process::exit(1);
                    }
                }
            };

            let gw = match &gateway {
                Some(g) => match parse_ipv4(g) {
                    Some(ip) => ip,
                    None => {
                        eprintln!("error: invalid gateway: {}", g);
                        process::exit(1);
                    }
                },
                None => {
                    if dest == 0 && mask == 0 {
                        eprintln!("error: default route requires 'gw ADDRESS'");
                        process::exit(1);
                    }
                    0
                }
            };

            add_route(dest, mask, gw, metric, is_host);
        }
        Some("del") => {
            // Parse: del [-net|-host] TARGET
            while idx < args.len() {
                match args[idx].as_str() {
                    "-net" => {
                        is_host = false;
                        idx += 1;
                    }
                    "-host" => {
                        is_host = true;
                        idx += 1;
                    }
                    _ => {
                        if target.is_none() {
                            target = Some(args[idx].clone());
                        }
                        idx += 1;
                    }
                }
            }

            let target_str = match &target {
                Some(t) => t.clone(),
                None => {
                    eprintln!("error: route del requires a target");
                    process::exit(1);
                }
            };

            let (dest, mask) = if target_str == "default" {
                (0u32, 0u32)
            } else if is_host {
                match parse_ipv4(&target_str) {
                    Some(ip) => (ip, 0xFFFF_FFFF),
                    None => {
                        eprintln!("error: invalid address: {}", target_str);
                        process::exit(1);
                    }
                }
            } else {
                match parse_network(&target_str) {
                    Some(pair) => pair,
                    None => {
                        eprintln!("error: invalid network: {}", target_str);
                        process::exit(1);
                    }
                }
            };

            del_route(dest, mask);
        }
        Some(other) => {
            eprintln!("error: unknown action: {}", other);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipv4() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some(0xC0A80101));
        assert_eq!(parse_ipv4("0.0.0.0"), Some(0));
        assert_eq!(parse_ipv4("255.255.255.255"), Some(0xFFFFFFFF));
        assert_eq!(parse_ipv4("10.0.2.15"), Some(0x0A00020F));
        assert_eq!(parse_ipv4("invalid"), None);
        assert_eq!(parse_ipv4("1.2.3"), None);
        assert_eq!(parse_ipv4("256.0.0.0"), None);
    }

    #[test]
    fn test_ip_to_string() {
        assert_eq!(ip_to_string(0xC0A80101), "192.168.1.1");
        assert_eq!(ip_to_string(0), "0.0.0.0");
        assert_eq!(ip_to_string(0xFFFFFFFF), "255.255.255.255");
    }

    #[test]
    fn test_is_default_route() {
        assert!(is_default_route(0, 0));
        assert!(!is_default_route(0x0A000000, 0xFFFFFF00));
        assert!(!is_default_route(0x0A00020F, 0xFFFFFFFF));
    }

    #[test]
    fn test_build_gateway_record() {
        // `route add default gw 10.0.2.2` -> only the GATEWAY field set.
        let rec = build_gateway_record([10, 0, 2, 2]);
        assert_eq!(&rec[0..8], &[0u8; 8]); // ip + mask untouched
        assert_eq!(&rec[8..12], &[10, 0, 2, 2]);
        assert_eq!(&rec[12..17], &[0u8; 5]); // dns + up untouched
        assert_eq!(rec[17], cfg_mask::GATEWAY);

        // Clearing the gateway (del/flush).
        let cleared = build_gateway_record([0, 0, 0, 0]);
        assert_eq!(&cleared[8..12], &[0, 0, 0, 0]);
        assert_eq!(cleared[17], cfg_mask::GATEWAY);
    }

    #[test]
    fn test_build_route_record() {
        // 203.0.113.0/24 via 10.0.2.250 metric 5.
        let dest = parse_ipv4("203.0.113.0").unwrap();
        let mask = cidr_to_mask(24);
        let gw = parse_ipv4("10.0.2.250").unwrap();
        let rec = build_route_record(dest, mask, gw, 5);
        assert_eq!(&rec[0..4], &[203, 0, 113, 0]);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
        assert_eq!(&rec[8..12], &[10, 0, 2, 250]);
        assert_eq!(&rec[12..16], &[5, 0, 0, 0]); // metric little-endian
    }

    #[test]
    fn test_build_route_del_record() {
        let dest = parse_ipv4("203.0.113.0").unwrap();
        let mask = cidr_to_mask(24);
        let rec = build_route_del_record(dest, mask);
        assert_eq!(&rec[0..4], &[203, 0, 113, 0]);
        assert_eq!(&rec[4..8], &[255, 255, 255, 0]);
    }

    #[test]
    fn test_parse_route_record_roundtrip() {
        let dest = parse_ipv4("172.16.0.0").unwrap();
        let mask = cidr_to_mask(12);
        let gw = parse_ipv4("10.0.2.2").unwrap();
        let rec = build_route_record(dest, mask, gw, 300);
        let (d, m, g, metric) = parse_route_record(&rec);
        assert_eq!(d, dest);
        assert_eq!(m, mask);
        assert_eq!(g, gw);
        assert_eq!(metric, 300);
    }

    #[test]
    fn test_cidr_to_mask() {
        assert_eq!(cidr_to_mask(0), 0);
        assert_eq!(cidr_to_mask(8), 0xFF000000);
        assert_eq!(cidr_to_mask(16), 0xFFFF0000);
        assert_eq!(cidr_to_mask(24), 0xFFFFFF00);
        assert_eq!(cidr_to_mask(32), 0xFFFFFFFF);
    }

    #[test]
    fn test_mask_to_prefix() {
        assert_eq!(mask_to_prefix(0), 0);
        assert_eq!(mask_to_prefix(0xFF000000), 8);
        assert_eq!(mask_to_prefix(0xFFFF0000), 16);
        assert_eq!(mask_to_prefix(0xFFFFFF00), 24);
        assert_eq!(mask_to_prefix(0xFFFFFFFF), 32);
    }

    #[test]
    fn test_parse_network() {
        assert_eq!(parse_network("10.0.0.0/24"), Some((0x0A000000, 0xFFFFFF00)));
        assert_eq!(parse_network("0.0.0.0/0"), Some((0, 0)));
        assert_eq!(
            parse_network("192.168.1.0/16"),
            Some((0xC0A80100, 0xFFFF0000))
        );
        assert_eq!(parse_network("10.0.0.1"), Some((0x0A000001, 0xFFFFFFFF)));
    }

    #[test]
    fn test_flags_to_string() {
        assert_eq!(flags_to_string(RTF_UP), "U");
        assert_eq!(flags_to_string(RTF_UP | RTF_GATEWAY), "UG");
        assert_eq!(flags_to_string(RTF_UP | RTF_HOST), "UH");
        assert_eq!(flags_to_string(RTF_UP | RTF_GATEWAY | RTF_HOST), "UGH");
        assert_eq!(flags_to_string(RTF_REJECT), "!");
        assert_eq!(flags_to_string(0), "-");
    }

    // --- SYS_NET_IF_INFO record decoding + route synthesis ---

    #[test]
    fn test_parse_net_if_info() {
        // 10.0.2.15 / 255.255.255.0, gateway 10.0.2.2.
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
        assert_eq!(info.ip, 0x0A00020F);
        assert_eq!(info.mask, 0xFFFFFF00);
        assert_eq!(info.gateway, 0x0A000202);
    }

    #[test]
    fn test_synth_routes_connected_and_default() {
        let info = NetIfInfo {
            ip: 0x0A00020F,     // 10.0.2.15
            mask: 0xFFFFFF00,   // /24
            gateway: 0x0A000202, // 10.0.2.2
        };
        let routes = synth_routes_from_net_if_info(&info);
        assert_eq!(routes.len(), 2);
        // Connected network route 10.0.2.0/24, no gateway.
        assert_eq!(routes[0].destination, 0x0A000200);
        assert_eq!(routes[0].genmask, 0xFFFFFF00);
        assert_eq!(routes[0].gateway, 0);
        assert_eq!(routes[0].flags, RTF_UP);
        // Default route via 10.0.2.2.
        assert_eq!(routes[1].destination, 0);
        assert_eq!(routes[1].genmask, 0);
        assert_eq!(routes[1].gateway, 0x0A000202);
        assert_eq!(routes[1].flags, RTF_UP | RTF_GATEWAY);
    }

    #[test]
    fn test_synth_routes_no_gateway() {
        let info = NetIfInfo {
            ip: 0x0A00020F,
            mask: 0xFFFFFF00,
            gateway: 0,
        };
        let routes = synth_routes_from_net_if_info(&info);
        // Only the connected route; no default route without a gateway.
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].gateway, 0);
    }

    #[test]
    fn test_synth_routes_unconfigured() {
        let info = NetIfInfo {
            ip: 0,
            mask: 0,
            gateway: 0,
        };
        assert!(synth_routes_from_net_if_info(&info).is_empty());
    }
}
