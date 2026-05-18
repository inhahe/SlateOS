//! OurOS IP Subnet Calculator (`ipcalc`)
//!
//! A comprehensive network calculator supporting IPv4 and IPv6 address
//! analysis, CIDR notation, subnet splitting, supernetting, and detailed
//! binary/class/scope reporting.
//!
//! # Usage
//!
//! ```text
//! ipcalc 192.168.1.0/24           IPv4 subnet info
//! ipcalc -m 255.255.255.0 10.0.0.1  IPv4 with explicit netmask
//! ipcalc -b 192.168.1.0/24        Show binary representations
//! ipcalc -s 50 10.0.0.0/16        Split into subnets holding 50 hosts
//! ipcalc fe80::1/64               IPv6 address info
//! ipcalc --network 192.168.1.5/24 Just print the network address
//! ipcalc --class 172.16.5.1       Print address class
//! ```

#![deny(clippy::all)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::needless_range_loop)]

use std::env;
use std::io::{self, Write};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::process;
use std::str::FromStr;

// ============================================================================
// ANSI color helpers
// ============================================================================

/// Terminal color codes for output formatting.
struct Colors {
    use_color: bool,
}

impl Colors {
    const RESET: &str = "\x1b[0m";
    const RED: &str = "\x1b[31m";
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";
    const CYAN: &str = "\x1b[36m";
    const BOLD: &str = "\x1b[1m";

    fn new(use_color: bool) -> Self {
        Self { use_color }
    }

    fn bold(&self, s: &str) -> String {
        if self.use_color {
            format!("{}{}{}", Self::BOLD, s, Self::RESET)
        } else {
            s.to_string()
        }
    }

    fn green(&self, s: &str) -> String {
        if self.use_color {
            format!("{}{}{}", Self::GREEN, s, Self::RESET)
        } else {
            s.to_string()
        }
    }

    fn cyan(&self, s: &str) -> String {
        if self.use_color {
            format!("{}{}{}", Self::CYAN, s, Self::RESET)
        } else {
            s.to_string()
        }
    }

    fn yellow(&self, s: &str) -> String {
        if self.use_color {
            format!("{}{}{}", Self::YELLOW, s, Self::RESET)
        } else {
            s.to_string()
        }
    }

    fn red(&self, s: &str) -> String {
        if self.use_color {
            format!("{}{}{}", Self::RED, s, Self::RESET)
        } else {
            s.to_string()
        }
    }
}

// ============================================================================
// IPv4 address info
// ============================================================================

/// Parsed IPv4 network information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ipv4Info {
    addr: u32,
    prefix: u8,
}

impl Ipv4Info {
    fn new(addr: u32, prefix: u8) -> Self {
        Self { addr, prefix }
    }

    /// Compute the netmask as a u32 from the prefix length.
    fn netmask(&self) -> u32 {
        if self.prefix == 0 {
            0
        } else {
            !0u32 << (32u32.saturating_sub(u32::from(self.prefix)))
        }
    }

    /// Wildcard (inverse) mask.
    fn wildcard(&self) -> u32 {
        !self.netmask()
    }

    /// Network address: addr AND netmask.
    fn network(&self) -> u32 {
        self.addr & self.netmask()
    }

    /// Broadcast address: network OR wildcard.
    fn broadcast(&self) -> u32 {
        self.network() | self.wildcard()
    }

    /// First usable host address in the subnet.
    /// For /31 and /32 the network *is* the first host.
    fn host_min(&self) -> u32 {
        if self.prefix >= 31 {
            self.network()
        } else {
            self.network().saturating_add(1)
        }
    }

    /// Last usable host address.
    /// For /32 it equals the address; for /31 it equals the broadcast.
    fn host_max(&self) -> u32 {
        if self.prefix == 32 {
            self.addr
        } else if self.prefix == 31 {
            self.broadcast()
        } else {
            self.broadcast().saturating_sub(1)
        }
    }

    /// Number of usable hosts.
    fn hosts_count(&self) -> u64 {
        match self.prefix {
            32 => 1,
            31 => 2,
            _ => {
                let total: u64 = 1u64 << (32 - u32::from(self.prefix));
                total.saturating_sub(2) // subtract network + broadcast
            }
        }
    }

    /// Total number of addresses in the subnet (including network/broadcast).
    fn total_addresses(&self) -> u64 {
        1u64 << (32 - u32::from(self.prefix))
    }

    /// Classful class of the address.
    fn class(&self) -> &'static str {
        let first = (self.addr >> 24) as u8;
        match first {
            0..=127 => "A",
            128..=191 => "B",
            192..=223 => "C",
            224..=239 => "D (multicast)",
            240..=255 => "E (reserved)",
        }
    }

    /// Whether the address is in an RFC 1918 private range.
    fn is_private(&self) -> bool {
        let first = (self.addr >> 24) as u8;
        let second = (self.addr >> 16) as u8;

        // 10.0.0.0/8
        if first == 10 {
            return true;
        }
        // 172.16.0.0/12
        if first == 172 && (second >= 16 && second <= 31) {
            return true;
        }
        // 192.168.0.0/16
        if first == 192 && second == 168 {
            return true;
        }
        false
    }

    /// Whether the address is the loopback range.
    fn is_loopback(&self) -> bool {
        (self.addr >> 24) as u8 == 127
    }

    /// Whether the address is a multicast address.
    fn is_multicast(&self) -> bool {
        let first = (self.addr >> 24) as u8;
        first >= 224 && first <= 239
    }

    /// Whether the address is in the link-local range (169.254.0.0/16).
    fn is_link_local(&self) -> bool {
        (self.addr >> 16) == 0xA9FE // 169.254
    }

    /// Address type description.
    fn addr_type(&self) -> &'static str {
        if self.is_loopback() {
            "Loopback"
        } else if self.is_link_local() {
            "Link-Local"
        } else if self.is_multicast() {
            "Multicast"
        } else if self.is_private() {
            "Private (RFC 1918)"
        } else {
            "Public"
        }
    }
}

// ============================================================================
// IPv6 address info
// ============================================================================

/// Parsed IPv6 network information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ipv6Info {
    addr: u128,
    prefix: u8,
}

impl Ipv6Info {
    fn new(addr: u128, prefix: u8) -> Self {
        Self { addr, prefix }
    }

    /// Network prefix mask.
    fn netmask(&self) -> u128 {
        if self.prefix == 0 {
            0
        } else if self.prefix >= 128 {
            !0u128
        } else {
            !0u128 << (128 - u32::from(self.prefix))
        }
    }

    /// Network address.
    fn network(&self) -> u128 {
        self.addr & self.netmask()
    }

    /// Last address in the prefix.
    fn last_addr(&self) -> u128 {
        if self.prefix >= 128 {
            self.addr
        } else {
            self.network() | (!self.netmask())
        }
    }

    /// Total addresses as u128.
    fn total_addresses(&self) -> u128 {
        if self.prefix >= 128 {
            1
        } else {
            1u128 << (128 - u32::from(self.prefix))
        }
    }

    /// Scope description.
    fn scope(&self) -> &'static str {
        if self.addr == 1 {
            "Loopback"
        } else if self.addr == 0 {
            "Unspecified"
        } else if (self.addr >> 118) == 0x3FA {
            // fe80::/10 → top 10 bits = 1111111010
            "Link-Local"
        } else if (self.addr >> 118) == 0x3FB {
            // fec0::/10 → top 10 bits = 1111111011 (deprecated)
            "Site-Local (deprecated)"
        } else if (self.addr >> 120) == 0xFF {
            "Multicast"
        } else if (self.addr >> 125) == 1 {
            // 2000::/3
            "Global Unicast"
        } else if (self.addr >> 121) == 0x7F {
            // fe00::/9 (includes fe80/fec0 but those already matched)
            "Link-Local"
        } else if (self.addr >> 120) == 0xFC || (self.addr >> 120) == 0xFD {
            "Unique Local (ULA)"
        } else {
            "Reserved"
        }
    }

    /// Format as full expanded IPv6 address.
    fn format_full(&self) -> String {
        let groups = ipv6_to_groups(self.addr);
        groups
            .iter()
            .map(|g| format!("{:04x}", g))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Format as compressed IPv6 address.
    fn format_compressed(&self) -> String {
        format_ipv6_compressed(self.addr)
    }
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Convert a u32 to dotted-quad string.
fn ipv4_to_string(ip: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        (ip >> 24) & 0xFF,
        (ip >> 16) & 0xFF,
        (ip >> 8) & 0xFF,
        ip & 0xFF
    )
}

/// Convert a u32 to dotted binary representation.
fn ipv4_to_binary(ip: u32) -> String {
    format!(
        "{:08b}.{:08b}.{:08b}.{:08b}",
        (ip >> 24) & 0xFF,
        (ip >> 16) & 0xFF,
        (ip >> 8) & 0xFF,
        ip & 0xFF
    )
}

/// Split a u128 into eight 16-bit groups for IPv6 display.
fn ipv6_to_groups(addr: u128) -> [u16; 8] {
    let mut groups = [0u16; 8];
    for i in 0..8 {
        groups[i] = ((addr >> (112 - i * 16)) & 0xFFFF) as u16;
    }
    groups
}

/// Compress an IPv6 address, collapsing the longest run of zero groups.
fn format_ipv6_compressed(addr: u128) -> String {
    let groups = ipv6_to_groups(addr);

    // Find the longest run of consecutive zero groups.
    let mut best_start = None;
    let mut best_len = 0usize;
    let mut cur_start = None;
    let mut cur_len = 0usize;

    for i in 0..8 {
        if groups[i] == 0 {
            if cur_start.is_none() {
                cur_start = Some(i);
                cur_len = 1;
            } else {
                cur_len += 1;
            }
        } else {
            if let Some(start) = cur_start
                && cur_len > best_len
            {
                best_start = Some(start);
                best_len = cur_len;
            }
            cur_start = None;
            cur_len = 0;
        }
    }
    // Final run check.
    if let Some(start) = cur_start
        && cur_len > best_len
    {
        best_start = Some(start);
        best_len = cur_len;
    }

    // Only compress runs of 2+ zero groups (RFC 5952 recommends this).
    if best_len < 2 {
        best_start = None;
    }

    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    let mut used_double_colon = false;
    while i < 8 {
        if let Some(start) = best_start
            && i == start && !used_double_colon
        {
            if i == 0 {
                parts.push(String::new());
            }
            parts.push(String::new());
            i += best_len;
            used_double_colon = true;
            if i == 8 {
                parts.push(String::new());
            }
            continue;
        }
        parts.push(format!("{:x}", groups[i]));
        i += 1;
    }

    parts.join(":")
}

/// Format an IPv6 address in full colon-hex with prefix bits highlighted.
fn ipv6_to_binary_groups(addr: u128, _prefix: u8) -> String {
    let groups = ipv6_to_groups(addr);
    groups
        .iter()
        .map(|g| format!("{:016b}", g))
        .collect::<Vec<_>>()
        .join(":")
}

// ============================================================================
// Parsing
// ============================================================================

/// Parse a dotted-quad IPv4 address string into a u32.
fn parse_ipv4(s: &str) -> Option<u32> {
    let std_addr = Ipv4Addr::from_str(s).ok()?;
    Some(u32::from(std_addr))
}

/// Parse a dotted-quad netmask into a prefix length. Returns None if the
/// mask is not contiguous.
fn netmask_to_prefix(mask: u32) -> Option<u8> {
    // A valid netmask is a contiguous sequence of 1-bits followed by 0-bits.
    if mask == 0 {
        return Some(0);
    }
    let leading = mask.leading_ones();
    let trailing = mask.trailing_zeros();
    if leading + trailing == 32 {
        Some(leading as u8)
    } else {
        None
    }
}

/// Parse an IPv6 address string into a u128.
fn parse_ipv6(s: &str) -> Option<u128> {
    let std_addr = Ipv6Addr::from_str(s).ok()?;
    Some(u128::from(std_addr))
}

/// Parse an input that might be IPv4/CIDR, IPv4 alone, or IPv6/prefix.
/// Returns either an Ipv4Info or Ipv6Info.
enum ParsedAddr {
    V4(Ipv4Info),
    V6(Ipv6Info),
}

fn parse_address(input: &str, explicit_mask: Option<&str>) -> Result<ParsedAddr, String> {
    // Check for IPv6 first (contains multiple colons or starts with ::).
    if input.contains("::") || input.matches(':').count() > 1 {
        let (addr_str, prefix) = if let Some(idx) = input.rfind('/') {
            let (a, p) = input.split_at(idx);
            let pfx: u8 = p[1..]
                .parse()
                .map_err(|_| format!("invalid prefix length: {}", &p[1..]))?;
            if pfx > 128 {
                return Err(format!("IPv6 prefix must be 0-128, got {}", pfx));
            }
            (a, pfx)
        } else {
            (input, 128)
        };

        let addr = parse_ipv6(addr_str)
            .ok_or_else(|| format!("invalid IPv6 address: {}", addr_str))?;
        return Ok(ParsedAddr::V6(Ipv6Info::new(addr, prefix)));
    }

    // IPv4 handling.
    let (addr_str, prefix) = if let Some(idx) = input.find('/') {
        let (a, p) = input.split_at(idx);
        let pfx: u8 = p[1..]
            .parse()
            .map_err(|_| format!("invalid prefix length: {}", &p[1..]))?;
        if pfx > 32 {
            return Err(format!("IPv4 prefix must be 0-32, got {}", pfx));
        }
        (a, Some(pfx))
    } else {
        (input, None)
    };

    let addr =
        parse_ipv4(addr_str).ok_or_else(|| format!("invalid IPv4 address: {}", addr_str))?;

    let final_prefix = if let Some(mask_str) = explicit_mask {
        let mask = parse_ipv4(mask_str)
            .ok_or_else(|| format!("invalid netmask: {}", mask_str))?;
        netmask_to_prefix(mask)
            .ok_or_else(|| format!("netmask is not contiguous: {}", mask_str))?
    } else if let Some(p) = prefix {
        p
    } else {
        // Default: classful prefix.
        let first = (addr >> 24) as u8;
        match first {
            0..=127 => 8,
            128..=191 => 16,
            192..=223 => 24,
            _ => 32,
        }
    };

    Ok(ParsedAddr::V4(Ipv4Info::new(addr, final_prefix)))
}

// ============================================================================
// Subnet splitting
// ============================================================================

/// Split a network into subnets, each holding at least `min_hosts` hosts.
/// Returns a vector of (network_address, prefix_length) pairs.
fn split_ipv4(info: &Ipv4Info, min_hosts: u64) -> Result<Vec<Ipv4Info>, String> {
    // Find the smallest prefix that holds min_hosts.
    // hosts = 2^(32-prefix) - 2  (for prefix < 31)
    let needed_bits = if min_hosts <= 2 {
        // /30 gives 2 hosts
        2u8
    } else {
        // We need 2^n - 2 >= min_hosts, so 2^n >= min_hosts + 2
        let mut n = 0u8;
        while (1u64 << n) < min_hosts.saturating_add(2) {
            n += 1;
            if n > 32 {
                return Err("requested host count exceeds IPv4 capacity".to_string());
            }
        }
        n
    };

    let sub_prefix = 32u8.saturating_sub(needed_bits);
    if sub_prefix < info.prefix {
        return Err(format!(
            "cannot fit {} hosts in /{} network (need at least /{})",
            min_hosts, info.prefix, sub_prefix
        ));
    }

    let net_start = info.network();
    let sub_size: u32 = 1u32
        .checked_shl(u32::from(32 - sub_prefix))
        .unwrap_or(0);
    let total_subs: u64 = 1u64 << (u32::from(sub_prefix) - u32::from(info.prefix));

    let mut result = Vec::new();
    let mut current = net_start;
    for _ in 0..total_subs {
        result.push(Ipv4Info::new(current, sub_prefix));
        current = current.wrapping_add(sub_size);
    }

    Ok(result)
}

/// Find the smallest supernet (CIDR block) that contains all given addresses.
fn supernet_ipv4(addrs: &[u32]) -> Option<Ipv4Info> {
    if addrs.is_empty() {
        return None;
    }

    let min_addr = addrs.iter().copied().min().unwrap_or(0);
    let max_addr = addrs.iter().copied().max().unwrap_or(0);

    // XOR reveals differing bits.
    let diff = min_addr ^ max_addr;
    let common_bits = if diff == 0 { 32 } else { diff.leading_zeros() };

    Some(Ipv4Info::new(
        min_addr & (!0u32 << (32 - common_bits)),
        common_bits as u8,
    ))
}

// ============================================================================
// Display routines
// ============================================================================

/// Print full IPv4 subnet information.
fn display_ipv4(info: &Ipv4Info, colors: &Colors, show_binary: bool) {
    let out = io::stdout();
    let mut w = out.lock();

    let label_w = 14;

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("Address:"),
        colors.cyan(&ipv4_to_string(info.addr)),
        width = label_w
    );
    if show_binary {
        let _ = writeln!(
            w,
            "{:<width$}{}",
            "",
            colors.yellow(&ipv4_to_binary(info.addr)),
            width = label_w
        );
    }

    let _ = writeln!(
        w,
        "{:<width$}{} = {}",
        colors.bold("Netmask:"),
        colors.cyan(&ipv4_to_string(info.netmask())),
        info.prefix,
        width = label_w
    );
    if show_binary {
        let _ = writeln!(
            w,
            "{:<width$}{}",
            "",
            colors.yellow(&ipv4_to_binary(info.netmask())),
            width = label_w
        );
    }

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("Wildcard:"),
        colors.cyan(&ipv4_to_string(info.wildcard())),
        width = label_w
    );
    if show_binary {
        let _ = writeln!(
            w,
            "{:<width$}{}",
            "",
            colors.yellow(&ipv4_to_binary(info.wildcard())),
            width = label_w
        );
    }

    let _ = writeln!(w, "{}", colors.bold("=>"));

    let _ = writeln!(
        w,
        "{:<width$}{}/{}",
        colors.bold("Network:"),
        colors.green(&ipv4_to_string(info.network())),
        info.prefix,
        width = label_w
    );
    if show_binary {
        let _ = writeln!(
            w,
            "{:<width$}{}",
            "",
            colors.yellow(&ipv4_to_binary(info.network())),
            width = label_w
        );
    }

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("HostMin:"),
        colors.green(&ipv4_to_string(info.host_min())),
        width = label_w
    );
    if show_binary {
        let _ = writeln!(
            w,
            "{:<width$}{}",
            "",
            colors.yellow(&ipv4_to_binary(info.host_min())),
            width = label_w
        );
    }

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("HostMax:"),
        colors.green(&ipv4_to_string(info.host_max())),
        width = label_w
    );
    if show_binary {
        let _ = writeln!(
            w,
            "{:<width$}{}",
            "",
            colors.yellow(&ipv4_to_binary(info.host_max())),
            width = label_w
        );
    }

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("Broadcast:"),
        colors.green(&ipv4_to_string(info.broadcast())),
        width = label_w
    );
    if show_binary {
        let _ = writeln!(
            w,
            "{:<width$}{}",
            "",
            colors.yellow(&ipv4_to_binary(info.broadcast())),
            width = label_w
        );
    }

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("Hosts/Net:"),
        info.hosts_count(),
        width = label_w
    );

    let type_label = info.addr_type();
    let type_colored = if info.is_multicast() || info.class().contains("reserved") {
        colors.red(type_label)
    } else {
        colors.yellow(type_label)
    };
    let _ = writeln!(
        w,
        "{:<width$}Class {}, {}",
        colors.bold("Class:"),
        info.class(),
        type_colored,
        width = label_w
    );
}

/// Print full IPv6 address information.
fn display_ipv6(info: &Ipv6Info, colors: &Colors, show_binary: bool) {
    let out = io::stdout();
    let mut w = out.lock();

    let label_w = 18;

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("Full Address:"),
        colors.cyan(&info.format_full()),
        width = label_w
    );

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("Compressed:"),
        colors.cyan(&info.format_compressed()),
        width = label_w
    );

    if show_binary {
        let _ = writeln!(
            w,
            "{:<width$}{}",
            colors.bold("Binary:"),
            colors.yellow(&ipv6_to_binary_groups(info.addr, info.prefix)),
            width = label_w
        );
    }

    let _ = writeln!(
        w,
        "{:<width$}/{}",
        colors.bold("Prefix Length:"),
        info.prefix,
        width = label_w
    );

    let _ = writeln!(w, "{}", colors.bold("=>"));

    let net = info.network();
    let _ = writeln!(
        w,
        "{:<width$}{}/{}",
        colors.bold("Network:"),
        colors.green(&format_ipv6_compressed(net)),
        info.prefix,
        width = label_w
    );

    let last = info.last_addr();
    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("Last Address:"),
        colors.green(&format_ipv6_compressed(last)),
        width = label_w
    );

    let _ = writeln!(
        w,
        "{:<width$}2^{} = {}",
        colors.bold("Addresses:"),
        128u32.saturating_sub(u32::from(info.prefix)),
        if info.prefix <= 64 {
            format!("~2^{}", 128 - u32::from(info.prefix))
        } else {
            format!("{}", info.total_addresses())
        },
        width = label_w
    );

    let _ = writeln!(
        w,
        "{:<width$}{}",
        colors.bold("Scope:"),
        colors.yellow(info.scope()),
        width = label_w
    );
}

/// Display subnet split results.
fn display_split(subnets: &[Ipv4Info], colors: &Colors) {
    let out = io::stdout();
    let mut w = out.lock();

    let _ = writeln!(
        w,
        "\n{}  ({} subnets)\n",
        colors.bold("Subnet Split"),
        subnets.len()
    );

    for (i, sub) in subnets.iter().enumerate() {
        let _ = writeln!(
            w,
            "  {:>3}. {:<18} {:<18} - {:<18} ({} hosts)",
            i + 1,
            format!("{}/{}", ipv4_to_string(sub.network()), sub.prefix),
            ipv4_to_string(sub.host_min()),
            ipv4_to_string(sub.host_max()),
            sub.hosts_count()
        );
    }
}

/// Display all possible subnet sizes for the address's classful network.
fn display_subnet_sizes() {
    let out = io::stdout();
    let mut w = out.lock();

    let _ = writeln!(w, "\nIPv4 Subnet Reference Table:\n");
    let _ = writeln!(w, "  {:>6}  {:>16}  {:>12}  {:>12}", "Prefix", "Netmask", "Addresses", "Usable Hosts");
    let _ = writeln!(w, "  {}  {}  {}  {}", "-".repeat(6), "-".repeat(16), "-".repeat(12), "-".repeat(12));

    for prefix in 0..=32u8 {
        let mask = if prefix == 0 {
            0u32
        } else {
            !0u32 << (32u32.saturating_sub(u32::from(prefix)))
        };
        let total: u64 = 1u64 << (32 - u32::from(prefix));
        let usable: u64 = if prefix >= 31 {
            total
        } else {
            total.saturating_sub(2)
        };
        let _ = writeln!(
            w,
            "  /{:<5} {:>16}  {:>12}  {:>12}",
            prefix,
            ipv4_to_string(mask),
            total,
            usable
        );
    }
}

// ============================================================================
// CLI options
// ============================================================================

struct Options {
    show_binary: bool,
    no_color: bool,
    split_hosts: Option<u64>,
    explicit_mask: Option<String>,
    // Single-value output modes (mutually exclusive with full display).
    output_minaddr: bool,
    output_maxaddr: bool,
    output_addresses: bool,
    output_network: bool,
    output_broadcast: bool,
    output_class: bool,
    show_subnets_table: bool,
    addresses: Vec<String>,
}

fn print_usage() {
    let out = io::stdout();
    let mut w = out.lock();
    let _ = writeln!(w, "Usage: ipcalc [OPTIONS] <ADDRESS>[/PREFIX] ...");
    let _ = writeln!(w);
    let _ = writeln!(w, "Options:");
    let _ = writeln!(w, "  -b, --binary          Show binary representations");
    let _ = writeln!(w, "  -n, --nocolor         Suppress colored output");
    let _ = writeln!(w, "  -m, --netmask MASK    Specify netmask (e.g., 255.255.255.0)");
    let _ = writeln!(w, "  -s, --split HOSTS     Split network into subnets of HOSTS size");
    let _ = writeln!(w, "      --minaddr         Print minimum host address only");
    let _ = writeln!(w, "      --maxaddr         Print maximum host address only");
    let _ = writeln!(w, "      --addresses       Print total address count only");
    let _ = writeln!(w, "      --network         Print network address only");
    let _ = writeln!(w, "      --broadcast       Print broadcast address only");
    let _ = writeln!(w, "      --class           Print address class only");
    let _ = writeln!(w, "      --subnets         Show all subnet sizes reference table");
    let _ = writeln!(w, "  -h, --help            Show this help");
    let _ = writeln!(w);
    let _ = writeln!(w, "Examples:");
    let _ = writeln!(w, "  ipcalc 192.168.1.0/24");
    let _ = writeln!(w, "  ipcalc -m 255.255.255.0 10.0.0.1");
    let _ = writeln!(w, "  ipcalc -b -s 50 10.0.0.0/16");
    let _ = writeln!(w, "  ipcalc fe80::1/64");
    let _ = writeln!(w, "  ipcalc 10.1.0.0 10.2.0.0   (supernet of multiple)");
}

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return Err("no arguments provided".to_string());
    }

    let mut opts = Options {
        show_binary: false,
        no_color: false,
        split_hosts: None,
        explicit_mask: None,
        output_minaddr: false,
        output_maxaddr: false,
        output_addresses: false,
        output_network: false,
        output_broadcast: false,
        output_class: false,
        show_subnets_table: false,
        addresses: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-b" | "--binary" => opts.show_binary = true,
            "-n" | "--nocolor" => opts.no_color = true,
            "--minaddr" => opts.output_minaddr = true,
            "--maxaddr" => opts.output_maxaddr = true,
            "--addresses" => opts.output_addresses = true,
            "--network" => opts.output_network = true,
            "--broadcast" => opts.output_broadcast = true,
            "--class" => opts.output_class = true,
            "--subnets" => opts.show_subnets_table = true,
            "-m" | "--netmask" => {
                i += 1;
                if i >= args.len() {
                    return Err("-m/--netmask requires a value".to_string());
                }
                opts.explicit_mask = Some(args[i].clone());
            }
            "-s" | "--split" => {
                i += 1;
                if i >= args.len() {
                    return Err("-s/--split requires a value".to_string());
                }
                let val: u64 = args[i]
                    .parse()
                    .map_err(|_| format!("invalid host count: {}", args[i]))?;
                opts.split_hosts = Some(val);
            }
            other => {
                // Handle --netmask=VALUE and --split=VALUE forms.
                if let Some(rest) = other.strip_prefix("--netmask=") {
                    opts.explicit_mask = Some(rest.to_string());
                } else if let Some(rest) = other.strip_prefix("--split=") {
                    let val: u64 = rest
                        .parse()
                        .map_err(|_| format!("invalid host count: {}", rest))?;
                    opts.split_hosts = Some(val);
                } else if other.starts_with('-') {
                    return Err(format!("unknown option: {}", other));
                } else {
                    opts.addresses.push(other.to_string());
                }
            }
        }
        i += 1;
    }

    Ok(opts)
}

// ============================================================================
// Main
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args()?;
    let colors = Colors::new(!opts.no_color);

    if opts.show_subnets_table {
        display_subnet_sizes();
        if opts.addresses.is_empty() {
            return Ok(());
        }
    }

    if opts.addresses.is_empty() {
        return Err("no address specified".to_string());
    }

    // Multiple addresses without CIDR → supernetting mode.
    if opts.addresses.len() > 1 && opts.split_hosts.is_none() {
        let mut v4addrs: Vec<u32> = Vec::new();
        for addr_str in &opts.addresses {
            match parse_address(addr_str, opts.explicit_mask.as_deref())? {
                ParsedAddr::V4(info) => v4addrs.push(info.addr),
                ParsedAddr::V6(_) => {
                    return Err("supernetting is only supported for IPv4".to_string());
                }
            }
        }

        if let Some(super_net) = supernet_ipv4(&v4addrs) {
            let out = io::stdout();
            let mut w = out.lock();
            let _ = writeln!(
                w,
                "{} {}/{}",
                colors.bold("Supernet:"),
                colors.green(&ipv4_to_string(super_net.network())),
                super_net.prefix
            );
            let _ = writeln!(w);
            display_ipv4(&super_net, &colors, opts.show_binary);
        }
        return Ok(());
    }

    // Single address mode.
    let addr_str = &opts.addresses[0];
    let parsed = parse_address(addr_str, opts.explicit_mask.as_deref())?;

    match parsed {
        ParsedAddr::V4(info) => {
            // Single-value output modes.
            if opts.output_minaddr {
                println!("{}", ipv4_to_string(info.host_min()));
                return Ok(());
            }
            if opts.output_maxaddr {
                println!("{}", ipv4_to_string(info.host_max()));
                return Ok(());
            }
            if opts.output_addresses {
                println!("{}", info.total_addresses());
                return Ok(());
            }
            if opts.output_network {
                println!("{}", ipv4_to_string(info.network()));
                return Ok(());
            }
            if opts.output_broadcast {
                println!("{}", ipv4_to_string(info.broadcast()));
                return Ok(());
            }
            if opts.output_class {
                println!("{}", info.class());
                return Ok(());
            }

            display_ipv4(&info, &colors, opts.show_binary);

            if let Some(min_hosts) = opts.split_hosts {
                let subnets = split_ipv4(&info, min_hosts)?;
                display_split(&subnets, &colors);
            }
        }
        ParsedAddr::V6(info) => {
            if opts.output_minaddr {
                println!("{}", format_ipv6_compressed(info.network()));
                return Ok(());
            }
            if opts.output_maxaddr {
                println!("{}", format_ipv6_compressed(info.last_addr()));
                return Ok(());
            }
            if opts.output_addresses {
                println!("{}", info.total_addresses());
                return Ok(());
            }
            if opts.output_network {
                println!("{}", format_ipv6_compressed(info.network()));
                return Ok(());
            }

            display_ipv6(&info, &colors, opts.show_binary);
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        let _ = writeln!(io::stderr(), "ipcalc: {}", e);
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- IPv4 parsing ---

    #[test]
    fn test_parse_ipv4_basic() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some(0xC0A80101));
    }

    #[test]
    fn test_parse_ipv4_zero() {
        assert_eq!(parse_ipv4("0.0.0.0"), Some(0));
    }

    #[test]
    fn test_parse_ipv4_broadcast() {
        assert_eq!(parse_ipv4("255.255.255.255"), Some(0xFFFFFFFF));
    }

    #[test]
    fn test_parse_ipv4_invalid() {
        assert_eq!(parse_ipv4("256.1.1.1"), None);
        assert_eq!(parse_ipv4("abc"), None);
        assert_eq!(parse_ipv4(""), None);
    }

    #[test]
    fn test_parse_ipv4_loopback() {
        assert_eq!(parse_ipv4("127.0.0.1"), Some(0x7F000001));
    }

    // --- Netmask conversion ---

    #[test]
    fn test_netmask_to_prefix_24() {
        assert_eq!(netmask_to_prefix(0xFFFFFF00), Some(24));
    }

    #[test]
    fn test_netmask_to_prefix_16() {
        assert_eq!(netmask_to_prefix(0xFFFF0000), Some(16));
    }

    #[test]
    fn test_netmask_to_prefix_8() {
        assert_eq!(netmask_to_prefix(0xFF000000), Some(8));
    }

    #[test]
    fn test_netmask_to_prefix_32() {
        assert_eq!(netmask_to_prefix(0xFFFFFFFF), Some(32));
    }

    #[test]
    fn test_netmask_to_prefix_0() {
        assert_eq!(netmask_to_prefix(0), Some(0));
    }

    #[test]
    fn test_netmask_to_prefix_invalid() {
        // Non-contiguous mask
        assert_eq!(netmask_to_prefix(0xFF00FF00), None);
    }

    #[test]
    fn test_netmask_to_prefix_25() {
        assert_eq!(netmask_to_prefix(0xFFFFFF80), Some(25));
    }

    // --- Ipv4Info calculations ---

    #[test]
    fn test_ipv4_netmask_slash24() {
        let info = Ipv4Info::new(0xC0A80105, 24); // 192.168.1.5/24
        assert_eq!(info.netmask(), 0xFFFFFF00);
    }

    #[test]
    fn test_ipv4_wildcard_slash24() {
        let info = Ipv4Info::new(0xC0A80105, 24);
        assert_eq!(info.wildcard(), 0x000000FF);
    }

    #[test]
    fn test_ipv4_network_slash24() {
        let info = Ipv4Info::new(0xC0A80105, 24); // 192.168.1.5/24
        assert_eq!(info.network(), 0xC0A80100); // 192.168.1.0
    }

    #[test]
    fn test_ipv4_broadcast_slash24() {
        let info = Ipv4Info::new(0xC0A80105, 24);
        assert_eq!(info.broadcast(), 0xC0A801FF); // 192.168.1.255
    }

    #[test]
    fn test_ipv4_host_min_slash24() {
        let info = Ipv4Info::new(0xC0A80100, 24);
        assert_eq!(info.host_min(), 0xC0A80101); // 192.168.1.1
    }

    #[test]
    fn test_ipv4_host_max_slash24() {
        let info = Ipv4Info::new(0xC0A80100, 24);
        assert_eq!(info.host_max(), 0xC0A801FE); // 192.168.1.254
    }

    #[test]
    fn test_ipv4_hosts_count_slash24() {
        let info = Ipv4Info::new(0xC0A80100, 24);
        assert_eq!(info.hosts_count(), 254);
    }

    #[test]
    fn test_ipv4_hosts_count_slash32() {
        let info = Ipv4Info::new(0xC0A80101, 32);
        assert_eq!(info.hosts_count(), 1);
    }

    #[test]
    fn test_ipv4_hosts_count_slash31() {
        let info = Ipv4Info::new(0xC0A80100, 31);
        assert_eq!(info.hosts_count(), 2);
    }

    #[test]
    fn test_ipv4_hosts_count_slash16() {
        let info = Ipv4Info::new(0x0A000000, 16);
        assert_eq!(info.hosts_count(), 65534);
    }

    #[test]
    fn test_ipv4_total_addresses_slash24() {
        let info = Ipv4Info::new(0xC0A80100, 24);
        assert_eq!(info.total_addresses(), 256);
    }

    #[test]
    fn test_ipv4_slash0() {
        let info = Ipv4Info::new(0, 0);
        assert_eq!(info.netmask(), 0);
        assert_eq!(info.wildcard(), 0xFFFFFFFF);
        assert_eq!(info.network(), 0);
        assert_eq!(info.broadcast(), 0xFFFFFFFF);
    }

    // --- Class detection ---

    #[test]
    fn test_class_a() {
        let info = Ipv4Info::new(0x0A000001, 8); // 10.0.0.1
        assert_eq!(info.class(), "A");
    }

    #[test]
    fn test_class_b() {
        let info = Ipv4Info::new(0xAC100001, 16); // 172.16.0.1
        assert_eq!(info.class(), "B");
    }

    #[test]
    fn test_class_c() {
        let info = Ipv4Info::new(0xC0A80101, 24); // 192.168.1.1
        assert_eq!(info.class(), "C");
    }

    #[test]
    fn test_class_d_multicast() {
        let info = Ipv4Info::new(0xE0000001, 32); // 224.0.0.1
        assert_eq!(info.class(), "D (multicast)");
    }

    #[test]
    fn test_class_e_reserved() {
        let info = Ipv4Info::new(0xF0000001, 32); // 240.0.0.1
        assert_eq!(info.class(), "E (reserved)");
    }

    // --- Private/public detection ---

    #[test]
    fn test_private_10() {
        let info = Ipv4Info::new(0x0A0A0A0A, 8); // 10.10.10.10
        assert!(info.is_private());
    }

    #[test]
    fn test_private_172_16() {
        let info = Ipv4Info::new(0xAC100001, 16); // 172.16.0.1
        assert!(info.is_private());
    }

    #[test]
    fn test_private_172_31() {
        let info = Ipv4Info::new(0xAC1F0001, 16); // 172.31.0.1
        assert!(info.is_private());
    }

    #[test]
    fn test_not_private_172_32() {
        let info = Ipv4Info::new(0xAC200001, 16); // 172.32.0.1
        assert!(!info.is_private());
    }

    #[test]
    fn test_private_192_168() {
        let info = Ipv4Info::new(0xC0A80001, 16); // 192.168.0.1
        assert!(info.is_private());
    }

    #[test]
    fn test_public_8_8_8_8() {
        let info = Ipv4Info::new(0x08080808, 32); // 8.8.8.8
        assert!(!info.is_private());
        assert_eq!(info.addr_type(), "Public");
    }

    // --- Special address types ---

    #[test]
    fn test_loopback() {
        let info = Ipv4Info::new(0x7F000001, 8); // 127.0.0.1
        assert!(info.is_loopback());
        assert_eq!(info.addr_type(), "Loopback");
    }

    #[test]
    fn test_multicast() {
        let info = Ipv4Info::new(0xE0000001, 32); // 224.0.0.1
        assert!(info.is_multicast());
        assert_eq!(info.addr_type(), "Multicast");
    }

    #[test]
    fn test_link_local() {
        let info = Ipv4Info::new(0xA9FE0101, 16); // 169.254.1.1
        assert!(info.is_link_local());
        assert_eq!(info.addr_type(), "Link-Local");
    }

    // --- Subnet splitting ---

    #[test]
    fn test_split_slash24_into_50_host_subnets() {
        let info = Ipv4Info::new(0xC0A80100, 24); // 192.168.1.0/24
        let subs = split_ipv4(&info, 50).unwrap();
        // Need /26 for 62 hosts each. 256/64 = 4 subnets.
        assert_eq!(subs.len(), 4);
        assert_eq!(subs[0].prefix, 26);
        assert_eq!(subs[0].network(), 0xC0A80100);
        assert_eq!(subs[1].network(), 0xC0A80140);
        assert_eq!(subs[2].network(), 0xC0A80180);
        assert_eq!(subs[3].network(), 0xC0A801C0);
    }

    #[test]
    fn test_split_too_large() {
        let info = Ipv4Info::new(0xC0A80100, 24);
        let result = split_ipv4(&info, 300);
        assert!(result.is_err());
    }

    #[test]
    fn test_split_exact_fit() {
        let info = Ipv4Info::new(0xC0A80100, 24);
        let subs = split_ipv4(&info, 126).unwrap();
        // /25 gives 126 hosts. 2 subnets.
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].prefix, 25);
    }

    // --- Supernetting ---

    #[test]
    fn test_supernet_two_adjacent() {
        let addrs = vec![0xC0A80100, 0xC0A80200]; // 192.168.1.0, 192.168.2.0
        let sup = supernet_ipv4(&addrs).unwrap();
        assert_eq!(sup.prefix, 22); // common bits
        assert_eq!(sup.network(), 0xC0A80000); // 192.168.0.0
    }

    #[test]
    fn test_supernet_same_address() {
        let addrs = vec![0xC0A80101, 0xC0A80101];
        let sup = supernet_ipv4(&addrs).unwrap();
        assert_eq!(sup.prefix, 32);
    }

    #[test]
    fn test_supernet_empty() {
        let addrs: Vec<u32> = vec![];
        assert!(supernet_ipv4(&addrs).is_none());
    }

    // --- IPv4 string formatting ---

    #[test]
    fn test_ipv4_to_string() {
        assert_eq!(ipv4_to_string(0xC0A80101), "192.168.1.1");
        assert_eq!(ipv4_to_string(0), "0.0.0.0");
        assert_eq!(ipv4_to_string(0xFFFFFFFF), "255.255.255.255");
    }

    #[test]
    fn test_ipv4_to_binary() {
        assert_eq!(
            ipv4_to_binary(0xC0A80101),
            "11000000.10101000.00000001.00000001"
        );
    }

    // --- IPv6 parsing ---

    #[test]
    fn test_parse_ipv6_loopback() {
        assert_eq!(parse_ipv6("::1"), Some(1));
    }

    #[test]
    fn test_parse_ipv6_unspecified() {
        assert_eq!(parse_ipv6("::"), Some(0));
    }

    #[test]
    fn test_parse_ipv6_full() {
        let addr = parse_ipv6("2001:0db8:85a3:0000:0000:8a2e:0370:7334");
        assert!(addr.is_some());
        let val = addr.unwrap();
        // Verify the first group.
        assert_eq!((val >> 112) as u16, 0x2001);
    }

    #[test]
    fn test_parse_ipv6_link_local() {
        let addr = parse_ipv6("fe80::1");
        assert!(addr.is_some());
    }

    #[test]
    fn test_parse_ipv6_invalid() {
        assert!(parse_ipv6("gggg::1").is_none());
    }

    // --- IPv6 info ---

    #[test]
    fn test_ipv6_network() {
        // fe80::1234:5678/64 → network = fe80::
        let addr = parse_ipv6("fe80::1234:5678").unwrap();
        let info = Ipv6Info::new(addr, 64);
        let net = info.network();
        // The bottom 64 bits should be zero.
        assert_eq!(net & 0xFFFFFFFFFFFFFFFF, 0);
        // Top 64 bits should be fe80::
        assert_eq!((net >> 112) as u16, 0xFE80);
    }

    #[test]
    fn test_ipv6_scope_loopback() {
        let info = Ipv6Info::new(1, 128);
        assert_eq!(info.scope(), "Loopback");
    }

    #[test]
    fn test_ipv6_scope_link_local() {
        let addr = parse_ipv6("fe80::1").unwrap();
        let info = Ipv6Info::new(addr, 10);
        assert_eq!(info.scope(), "Link-Local");
    }

    #[test]
    fn test_ipv6_scope_global() {
        let addr = parse_ipv6("2001:db8::1").unwrap();
        let info = Ipv6Info::new(addr, 32);
        assert_eq!(info.scope(), "Global Unicast");
    }

    #[test]
    fn test_ipv6_scope_multicast() {
        let addr = parse_ipv6("ff02::1").unwrap();
        let info = Ipv6Info::new(addr, 128);
        assert_eq!(info.scope(), "Multicast");
    }

    #[test]
    fn test_ipv6_scope_ula() {
        let addr = parse_ipv6("fd00::1").unwrap();
        let info = Ipv6Info::new(addr, 48);
        assert_eq!(info.scope(), "Unique Local (ULA)");
    }

    // --- IPv6 formatting ---

    #[test]
    fn test_ipv6_compressed_loopback() {
        assert_eq!(format_ipv6_compressed(1), "::1");
    }

    #[test]
    fn test_ipv6_compressed_unspecified() {
        assert_eq!(format_ipv6_compressed(0), "::");
    }

    #[test]
    fn test_ipv6_full_format() {
        let info = Ipv6Info::new(1, 128);
        assert_eq!(info.format_full(), "0000:0000:0000:0000:0000:0000:0000:0001");
    }

    // --- Address parsing integration ---

    #[test]
    fn test_parse_address_v4_cidr() {
        let result = parse_address("192.168.1.0/24", None);
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedAddr::V4(info) => {
                assert_eq!(info.prefix, 24);
                assert_eq!(info.addr, 0xC0A80100);
            }
            ParsedAddr::V6(_) => panic!("expected IPv4"),
        }
    }

    #[test]
    fn test_parse_address_v4_with_mask() {
        let result = parse_address("10.0.0.1", Some("255.0.0.0"));
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedAddr::V4(info) => {
                assert_eq!(info.prefix, 8);
            }
            ParsedAddr::V6(_) => panic!("expected IPv4"),
        }
    }

    #[test]
    fn test_parse_address_v6() {
        let result = parse_address("fe80::1/64", None);
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedAddr::V6(info) => {
                assert_eq!(info.prefix, 64);
            }
            ParsedAddr::V4(_) => panic!("expected IPv6"),
        }
    }

    #[test]
    fn test_parse_address_invalid_prefix() {
        assert!(parse_address("10.0.0.1/33", None).is_err());
        assert!(parse_address("::1/129", None).is_err());
    }

    #[test]
    fn test_parse_address_classful_default() {
        // No CIDR, no mask → classful default.
        match parse_address("10.0.0.1", None).unwrap() {
            ParsedAddr::V4(info) => assert_eq!(info.prefix, 8), // Class A
            _ => panic!("expected v4"),
        }
        match parse_address("172.16.0.1", None).unwrap() {
            ParsedAddr::V4(info) => assert_eq!(info.prefix, 16), // Class B
            _ => panic!("expected v4"),
        }
        match parse_address("192.168.1.1", None).unwrap() {
            ParsedAddr::V4(info) => assert_eq!(info.prefix, 24), // Class C
            _ => panic!("expected v4"),
        }
    }

    // --- IPv6 groups ---

    #[test]
    fn test_ipv6_to_groups() {
        let addr = parse_ipv6("2001:db8::1").unwrap();
        let groups = ipv6_to_groups(addr);
        assert_eq!(groups[0], 0x2001);
        assert_eq!(groups[1], 0x0db8);
        assert_eq!(groups[7], 0x0001);
    }
}
