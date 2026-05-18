//! OurOS Routing Table Management
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

const SYS_NET_IOCTL: u64 = 810;

// Route ioctl sub-commands.
const ROUTE_ADD: u64 = 10;
const ROUTE_DEL: u64 = 11;
const ROUTE_FLUSH: u64 = 12;

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

#[cfg(target_arch = "x86_64")]
unsafe fn syscall6(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            in("r9") a6,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn net_ioctl(cmd: u64, a1: u64, a2: u64) -> i64 {
    unsafe { syscall3(SYS_NET_IOCTL, cmd, a1, a2) }
}

#[allow(dead_code)]
fn net_ioctl6(cmd: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    unsafe { syscall6(SYS_NET_IOCTL, cmd, a1, a2, a3, a4, a5) }
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
    if routes.is_empty() {
        if let Ok(content) = fs::read_to_string("/sys/net/routes") {
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

    routes
}

// ============================================================================
// Display routing table
// ============================================================================

fn display_routes(numeric: bool, verbose: bool) {
    let routes = read_routes();

    println!("Kernel IP routing table");
    println!(
        "{:<16} {:<16} {:<16} {:<6} {:<6} {:<4} {:<6} {}",
        "Destination", "Gateway", "Genmask", "Flags", "Metric", "Ref", "Use", "Iface"
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

fn add_route(dest: u32, mask: u32, gateway: u32, metric: u32, is_host: bool) {
    let mut flags: u16 = RTF_UP;
    if gateway != 0 {
        flags |= RTF_GATEWAY;
    }
    if is_host {
        flags |= RTF_HOST;
    }

    // Pack route info for the syscall.
    // Encoding: dest in a1 (high 32 bits = dest, low 32 bits = mask)
    //           gateway in a2 (high 32 bits = gw, low 16 bits = flags, next 16 bits = metric low)
    let a1 = ((dest as u64) << 32) | (mask as u64);
    let a2 = ((gateway as u64) << 32) | ((flags as u64) << 16) | ((metric & 0xFFFF) as u64);

    let ret = net_ioctl(ROUTE_ADD, a1, a2);
    if ret < 0 {
        // Try writing to sysfs as fallback.
        let prefix = mask_to_prefix(mask);
        let dest_str = if dest == 0 && mask == 0 {
            "default".to_string()
        } else {
            format!("{}/{}", ip_to_string(dest), prefix)
        };
        let gw_str = ip_to_string(gateway);
        let entry = format!("{} {} {}", dest_str, gw_str, metric);

        if fs::write("/sys/net/routes/add", &entry).is_ok() {
            if dest == 0 && mask == 0 {
                println!("Added default route via {}", gw_str);
            } else {
                println!("Added route to {} via {}", dest_str, gw_str);
            }
        } else {
            eprintln!(
                "SIOCADDRT: {}",
                match ret {
                    -1 => "Operation not permitted",
                    -17 => "File exists (route already present)",
                    -22 => "Invalid argument",
                    -99 => "Cannot assign requested address",
                    _ => "Network is unreachable",
                }
            );
            process::exit(1);
        }
    } else {
        let prefix = mask_to_prefix(mask);
        if dest == 0 && mask == 0 {
            println!("Added default route via {}", ip_to_string(gateway));
        } else {
            println!(
                "Added route to {}/{} via {}",
                ip_to_string(dest),
                prefix,
                ip_to_string(gateway)
            );
        }
    }
}

fn del_route(dest: u32, mask: u32) {
    let a1 = ((dest as u64) << 32) | (mask as u64);
    let ret = net_ioctl(ROUTE_DEL, a1, 0);
    if ret < 0 {
        // Try sysfs fallback.
        let prefix = mask_to_prefix(mask);
        let dest_str = if dest == 0 && mask == 0 {
            "default".to_string()
        } else {
            format!("{}/{}", ip_to_string(dest), prefix)
        };

        if fs::write("/sys/net/routes/del", &dest_str).is_ok() {
            println!("Deleted route {}", dest_str);
        } else {
            eprintln!(
                "SIOCDELRT: {}",
                match ret {
                    -1 => "Operation not permitted",
                    -3 => "No such process (route not found)",
                    -22 => "Invalid argument",
                    _ => "Unknown error",
                }
            );
            process::exit(1);
        }
    } else if dest == 0 && mask == 0 {
        println!("Deleted default route");
    } else {
        println!(
            "Deleted route to {}/{}",
            ip_to_string(dest),
            mask_to_prefix(mask)
        );
    }
}

fn flush_routes() {
    let ret = net_ioctl(ROUTE_FLUSH, 0, 0);
    if ret < 0 {
        if fs::write("/sys/net/routes/flush", "1").is_ok() {
            println!("Flushed all routes");
        } else {
            eprintln!("Failed to flush routes (error {})", ret);
            process::exit(1);
        }
    } else {
        println!("Flushed all routes");
    }
}

// ============================================================================
// CLI
// ============================================================================

fn print_usage() {
    println!("OurOS Route Management v0.1.0");
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
                println!("route (OurOS) 0.1.0");
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
}
