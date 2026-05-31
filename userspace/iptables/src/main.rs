//! Multi-personality iptables packet filtering and NAT utility for OurOS.
//!
//! Personalities detected via `argv[0]` basename:
//!   - `iptables`         -- IPv4 packet filter (default)
//!   - `ip6tables`        -- IPv6 packet filter
//!   - `iptables-save`    -- dump rules in save format
//!   - `iptables-restore` -- restore rules from save format
//!   - `ip6tables-save`   -- dump IPv6 rules
//!   - `ip6tables-restore`-- restore IPv6 rules

#![deny(clippy::all)]

use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead};
use std::process;

// ---------------------------------------------------------------------------
// IP address types
// ---------------------------------------------------------------------------

/// An IPv4 address with CIDR prefix length.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ipv4Cidr {
    addr: [u8; 4],
    prefix_len: u8,
}

impl Ipv4Cidr {
    fn parse(s: &str) -> Result<Self, String> {
        let (addr_str, prefix_len) = if let Some(pos) = s.find('/') {
            let pl: u8 = s[pos + 1..]
                .parse()
                .map_err(|_| format!("invalid prefix length in '{s}'"))?;
            if pl > 32 {
                return Err(format!("prefix length {pl} > 32"));
            }
            (&s[..pos], pl)
        } else {
            (s, 32)
        };
        let addr = parse_ipv4_addr(addr_str)?;
        Ok(Self { addr, prefix_len })
    }

    /// True if `ip` falls within this CIDR block.  Not used by the rule
    /// *management* path (this tool edits rule tables, it doesn't evaluate
    /// packets), but kept and tested as the membership primitive a future
    /// `--check`/packet-match path will need.
    #[allow(dead_code)]
    fn contains(&self, ip: &[u8; 4]) -> bool {
        if self.prefix_len == 0 {
            return true;
        }
        let mask = if self.prefix_len >= 32 {
            u32::MAX
        } else {
            u32::MAX << (32 - self.prefix_len)
        };
        let self_u32 = u32::from_be_bytes(self.addr);
        let ip_u32 = u32::from_be_bytes(*ip);
        (self_u32 & mask) == (ip_u32 & mask)
    }
}

impl fmt::Display for Ipv4Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}/{}",
            self.addr[0], self.addr[1], self.addr[2], self.addr[3], self.prefix_len
        )
    }
}

fn parse_ipv4_addr(s: &str) -> Result<[u8; 4], String> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return Err(format!("invalid IPv4 address '{s}'"));
    }
    let mut addr = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        addr[i] = part
            .parse()
            .map_err(|_| format!("invalid octet '{part}' in '{s}'"))?;
    }
    Ok(addr)
}

/// An IPv6 address with CIDR prefix length.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ipv6Cidr {
    addr: [u8; 16],
    prefix_len: u8,
}

impl Ipv6Cidr {
    fn parse(s: &str) -> Result<Self, String> {
        let (addr_str, prefix_len) = if let Some(pos) = s.rfind('/') {
            let pl: u8 = s[pos + 1..]
                .parse()
                .map_err(|_| format!("invalid prefix length in '{s}'"))?;
            if pl > 128 {
                return Err(format!("prefix length {pl} > 128"));
            }
            (&s[..pos], pl)
        } else {
            (s, 128)
        };
        let addr = parse_ipv6_addr(addr_str)?;
        Ok(Self { addr, prefix_len })
    }

    /// True if `ip` falls within this CIDR block.  See `Ipv4Cidr::contains`
    /// for why this is `#[allow(dead_code)]`.
    #[allow(dead_code)]
    fn contains(&self, ip: &[u8; 16]) -> bool {
        if self.prefix_len == 0 {
            return true;
        }
        let full_bytes = (self.prefix_len / 8) as usize;
        let remaining_bits = self.prefix_len % 8;
        for (a, b) in self.addr.iter().zip(ip.iter()).take(full_bytes) {
            if a != b {
                return false;
            }
        }
        if remaining_bits > 0 && full_bytes < 16 {
            let mask = 0xFF_u8 << (8 - remaining_bits);
            if (self.addr[full_bytes] & mask) != (ip[full_bytes] & mask) {
                return false;
            }
        }
        true
    }
}

impl fmt::Display for Ipv6Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let segments: Vec<String> = (0..8)
            .map(|i| {
                let val = u16::from_be_bytes([self.addr[i * 2], self.addr[i * 2 + 1]]);
                format!("{val:x}")
            })
            .collect();
        write!(f, "{}/{}", segments.join(":"), self.prefix_len)
    }
}

fn parse_ipv6_addr(s: &str) -> Result<[u8; 16], String> {
    let mut addr = [0u8; 16];

    if s == "::" {
        return Ok(addr);
    }

    let parts: Vec<&str> = s.split("::").collect();
    if parts.len() > 2 {
        return Err(format!("invalid IPv6 address '{s}': multiple ::"));
    }

    let left_groups: Vec<&str> = if parts[0].is_empty() {
        vec![]
    } else {
        parts[0].split(':').collect()
    };
    let right_groups: Vec<&str> = if parts.len() == 2 {
        if parts[1].is_empty() {
            vec![]
        } else {
            parts[1].split(':').collect()
        }
    } else {
        vec![]
    };

    let total = left_groups.len() + right_groups.len();
    if parts.len() == 1 && total != 8 {
        return Err(format!(
            "invalid IPv6 address '{s}': expected 8 groups, got {total}"
        ));
    }
    if total > 8 {
        return Err(format!("invalid IPv6 address '{s}': too many groups"));
    }

    let zero_fill = 8 - total;

    for (i, grp) in left_groups.iter().enumerate() {
        let val =
            u16::from_str_radix(grp, 16).map_err(|_| format!("invalid IPv6 group '{grp}'"))?;
        let bytes = val.to_be_bytes();
        addr[i * 2] = bytes[0];
        addr[i * 2 + 1] = bytes[1];
    }

    let right_start = left_groups.len() + zero_fill;
    for (i, grp) in right_groups.iter().enumerate() {
        let val =
            u16::from_str_radix(grp, 16).map_err(|_| format!("invalid IPv6 group '{grp}'"))?;
        let bytes = val.to_be_bytes();
        let idx = right_start + i;
        addr[idx * 2] = bytes[0];
        addr[idx * 2 + 1] = bytes[1];
    }

    Ok(addr)
}

/// A unified address CIDR that can be either IPv4 or IPv6.
#[derive(Clone, Debug, PartialEq, Eq)]
enum AddrCidr {
    V4(Ipv4Cidr),
    V6(Ipv6Cidr),
}

impl AddrCidr {
    fn parse_v4(s: &str) -> Result<Self, String> {
        Ok(AddrCidr::V4(Ipv4Cidr::parse(s)?))
    }

    fn parse_v6(s: &str) -> Result<Self, String> {
        Ok(AddrCidr::V6(Ipv6Cidr::parse(s)?))
    }
}

impl fmt::Display for AddrCidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AddrCidr::V4(c) => write!(f, "{c}"),
            AddrCidr::V6(c) => write!(f, "{c}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Icmpv6,
    All,
}

impl Protocol {
    fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "tcp" => Ok(Self::Tcp),
            "udp" => Ok(Self::Udp),
            "icmp" => Ok(Self::Icmp),
            "icmpv6" | "ipv6-icmp" => Ok(Self::Icmpv6),
            "all" => Ok(Self::All),
            _ => Err(format!("unknown protocol '{s}'")),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Icmp => "icmp",
            Self::Icmpv6 => "icmpv6",
            Self::All => "all",
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Port specification
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum PortSpec {
    Single(u16),
    Range(u16, u16),
}

impl PortSpec {
    fn parse(s: &str) -> Result<Self, String> {
        if let Some(pos) = s.find(':') {
            let lo: u16 = s[..pos]
                .parse()
                .map_err(|_| format!("invalid port '{}'", &s[..pos]))?;
            let hi: u16 = s[pos + 1..]
                .parse()
                .map_err(|_| format!("invalid port '{}'", &s[pos + 1..]))?;
            if lo > hi {
                return Err(format!("port range {lo}:{hi} is invalid (lo > hi)"));
            }
            Ok(Self::Range(lo, hi))
        } else {
            let port: u16 = s.parse().map_err(|_| format!("invalid port '{s}'"))?;
            Ok(Self::Single(port))
        }
    }

    /// True if `port` matches this spec.  See `Ipv4Cidr::contains` for why
    /// this is `#[allow(dead_code)]`.
    #[allow(dead_code)]
    fn contains(&self, port: u16) -> bool {
        match self {
            Self::Single(p) => *p == port,
            Self::Range(lo, hi) => port >= *lo && port <= *hi,
        }
    }
}

impl fmt::Display for PortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single(p) => write!(f, "{p}"),
            Self::Range(lo, hi) => write!(f, "{lo}:{hi}"),
        }
    }
}

/// Multiport: a list of port specs.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MultiPort(Vec<PortSpec>);

impl MultiPort {
    fn parse(s: &str) -> Result<Self, String> {
        let mut specs = Vec::new();
        for part in s.split(',') {
            specs.push(PortSpec::parse(part.trim())?);
        }
        if specs.is_empty() {
            return Err("empty multiport specification".to_string());
        }
        Ok(Self(specs))
    }

    /// True if any contained spec matches `port`.  See `Ipv4Cidr::contains`
    /// for why this is `#[allow(dead_code)]`.
    #[allow(dead_code)]
    fn contains(&self, port: u16) -> bool {
        self.0.iter().any(|ps| ps.contains(port))
    }
}

impl fmt::Display for MultiPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let strs: Vec<String> = self.0.iter().map(|p| p.to_string()).collect();
        f.write_str(&strs.join(","))
    }
}

// ---------------------------------------------------------------------------
// Connection tracking state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ConnState {
    New,
    Established,
    Related,
    Invalid,
    Untracked,
}

impl ConnState {
    fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_uppercase().as_str() {
            "NEW" => Ok(Self::New),
            "ESTABLISHED" => Ok(Self::Established),
            "RELATED" => Ok(Self::Related),
            "INVALID" => Ok(Self::Invalid),
            "UNTRACKED" => Ok(Self::Untracked),
            _ => Err(format!("unknown state '{s}'")),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::New => "NEW",
            Self::Established => "ESTABLISHED",
            Self::Related => "RELATED",
            Self::Invalid => "INVALID",
            Self::Untracked => "UNTRACKED",
        }
    }
}

fn parse_state_list(s: &str) -> Result<Vec<ConnState>, String> {
    let mut states = Vec::new();
    for part in s.split(',') {
        states.push(ConnState::parse(part.trim())?);
    }
    if states.is_empty() {
        return Err("empty state list".to_string());
    }
    Ok(states)
}

fn format_state_list(states: &[ConnState]) -> String {
    states
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

// ---------------------------------------------------------------------------
// Limit specification
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct LimitSpec {
    rate: u32,
    unit: LimitUnit,
    burst: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LimitUnit {
    Second,
    Minute,
    Hour,
    Day,
}

impl LimitUnit {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Second => "sec",
            Self::Minute => "min",
            Self::Hour => "hour",
            Self::Day => "day",
        }
    }
}

impl LimitSpec {
    fn parse_rate(s: &str) -> Result<(u32, LimitUnit), String> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 2 {
            return Err(format!("invalid limit rate '{s}' (expected N/unit)"));
        }
        let rate: u32 = parts[0]
            .parse()
            .map_err(|_| format!("invalid rate number '{}'", parts[0]))?;
        let unit = match parts[1].to_ascii_lowercase().as_str() {
            "s" | "sec" | "second" => LimitUnit::Second,
            "m" | "min" | "minute" => LimitUnit::Minute,
            "h" | "hour" => LimitUnit::Hour,
            "d" | "day" => LimitUnit::Day,
            _ => {
                return Err(format!(
                    "unknown limit unit '{}' (use sec/min/hour/day)",
                    parts[1]
                ));
            }
        };
        Ok((rate, unit))
    }
}

impl fmt::Display for LimitSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{} burst {}",
            self.rate,
            self.unit.as_str(),
            self.burst
        )
    }
}

// ---------------------------------------------------------------------------
// Match extensions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum MatchExt {
    /// -m state --state STATE_LIST
    State(Vec<ConnState>),
    /// -m conntrack --ctstate STATE_LIST
    ConnTrack(Vec<ConnState>),
    /// -m multiport --dports PORTS
    MultiDport(MultiPort),
    /// -m multiport --sports PORTS
    MultiSport(MultiPort),
    /// -m limit --limit RATE --limit-burst BURST
    Limit(LimitSpec),
    /// -m comment --comment TEXT
    Comment(String),
}

impl fmt::Display for MatchExt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(states) => {
                write!(f, "-m state --state {}", format_state_list(states))
            }
            Self::ConnTrack(states) => {
                write!(f, "-m conntrack --ctstate {}", format_state_list(states))
            }
            Self::MultiDport(mp) => {
                write!(f, "-m multiport --dports {mp}")
            }
            Self::MultiSport(mp) => {
                write!(f, "-m multiport --sports {mp}")
            }
            Self::Limit(spec) => {
                write!(
                    f,
                    "-m limit --limit {}/{} --limit-burst {}",
                    spec.rate,
                    spec.unit.as_str(),
                    spec.burst
                )
            }
            Self::Comment(text) => {
                write!(f, "-m comment --comment \"{text}\"")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Target / action
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Accept,
    Drop,
    Reject {
        reject_with: Option<String>,
    },
    Log {
        prefix: Option<String>,
        level: Option<String>,
    },
    Snat {
        to_source: String,
    },
    Dnat {
        to_destination: String,
    },
    Masquerade,
    Redirect {
        to_ports: Option<PortSpec>,
    },
    Return,
    /// Jump to user-defined chain.
    UserChain(String),
}

impl Target {
    fn name(&self) -> &str {
        match self {
            Self::Accept => "ACCEPT",
            Self::Drop => "DROP",
            Self::Reject { .. } => "REJECT",
            Self::Log { .. } => "LOG",
            Self::Snat { .. } => "SNAT",
            Self::Dnat { .. } => "DNAT",
            Self::Masquerade => "MASQUERADE",
            Self::Redirect { .. } => "REDIRECT",
            Self::Return => "RETURN",
            Self::UserChain(name) => name,
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accept => write!(f, "ACCEPT"),
            Self::Drop => write!(f, "DROP"),
            Self::Reject {
                reject_with: Some(r),
            } => {
                write!(f, "REJECT --reject-with {r}")
            }
            Self::Reject { reject_with: None } => write!(f, "REJECT"),
            Self::Log { prefix, level } => {
                write!(f, "LOG")?;
                if let Some(p) = prefix {
                    write!(f, " --log-prefix \"{p}\"")?;
                }
                if let Some(l) = level {
                    write!(f, " --log-level {l}")?;
                }
                Ok(())
            }
            Self::Snat { to_source } => {
                write!(f, "SNAT --to-source {to_source}")
            }
            Self::Dnat { to_destination } => {
                write!(f, "DNAT --to-destination {to_destination}")
            }
            Self::Masquerade => write!(f, "MASQUERADE"),
            Self::Redirect { to_ports: Some(p) } => {
                write!(f, "REDIRECT --to-ports {p}")
            }
            Self::Redirect { to_ports: None } => write!(f, "REDIRECT"),
            Self::Return => write!(f, "RETURN"),
            Self::UserChain(name) => write!(f, "{name}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Rule
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Rule {
    protocol: Option<Protocol>,
    not_protocol: bool,
    source: Option<AddrCidr>,
    not_source: bool,
    destination: Option<AddrCidr>,
    not_destination: bool,
    in_interface: Option<String>,
    not_in_interface: bool,
    out_interface: Option<String>,
    not_out_interface: bool,
    sport: Option<PortSpec>,
    not_sport: bool,
    dport: Option<PortSpec>,
    not_dport: bool,
    match_extensions: Vec<MatchExt>,
    target: Option<Target>,
    /// Packet counter.
    packets: u64,
    /// Byte counter.
    bytes: u64,
}

impl Rule {
    fn new() -> Self {
        Self {
            protocol: None,
            not_protocol: false,
            source: None,
            not_source: false,
            destination: None,
            not_destination: false,
            in_interface: None,
            not_in_interface: false,
            out_interface: None,
            not_out_interface: false,
            sport: None,
            not_sport: false,
            dport: None,
            not_dport: false,
            match_extensions: Vec::new(),
            target: None,
            packets: 0,
            bytes: 0,
        }
    }

    /// Format rule as an iptables command-line fragment (used for save/list).
    fn to_args_string(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(ref proto) = self.protocol {
            if self.not_protocol {
                parts.push("!".to_string());
            }
            parts.push("-p".to_string());
            parts.push(proto.to_string());
        }

        if let Some(ref src) = self.source {
            if self.not_source {
                parts.push("!".to_string());
            }
            parts.push("-s".to_string());
            parts.push(src.to_string());
        }

        if let Some(ref dst) = self.destination {
            if self.not_destination {
                parts.push("!".to_string());
            }
            parts.push("-d".to_string());
            parts.push(dst.to_string());
        }

        if let Some(ref iface) = self.in_interface {
            if self.not_in_interface {
                parts.push("!".to_string());
            }
            parts.push("-i".to_string());
            parts.push(iface.clone());
        }

        if let Some(ref iface) = self.out_interface {
            if self.not_out_interface {
                parts.push("!".to_string());
            }
            parts.push("-o".to_string());
            parts.push(iface.clone());
        }

        if let Some(ref sp) = self.sport {
            if self.not_sport {
                parts.push("!".to_string());
            }
            parts.push("--sport".to_string());
            parts.push(sp.to_string());
        }

        if let Some(ref dp) = self.dport {
            if self.not_dport {
                parts.push("!".to_string());
            }
            parts.push("--dport".to_string());
            parts.push(dp.to_string());
        }

        for ext in &self.match_extensions {
            parts.push(ext.to_string());
        }

        if let Some(ref tgt) = self.target {
            parts.push("-j".to_string());
            parts.push(tgt.to_string());
        }

        parts.join(" ")
    }

    /// Check whether this rule's match criteria are the same as another's
    /// (ignoring counters and target).
    fn matches_spec(&self, other: &Rule) -> bool {
        self.protocol == other.protocol
            && self.not_protocol == other.not_protocol
            && self.source == other.source
            && self.not_source == other.not_source
            && self.destination == other.destination
            && self.not_destination == other.not_destination
            && self.in_interface == other.in_interface
            && self.not_in_interface == other.not_in_interface
            && self.out_interface == other.out_interface
            && self.not_out_interface == other.not_out_interface
            && self.sport == other.sport
            && self.not_sport == other.not_sport
            && self.dport == other.dport
            && self.not_dport == other.not_dport
            && self.match_extensions == other.match_extensions
            && self.target == other.target
    }
}

// ---------------------------------------------------------------------------
// Chain
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum ChainPolicy {
    Accept,
    Drop,
}

impl ChainPolicy {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Accept => "ACCEPT",
            Self::Drop => "DROP",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_uppercase().as_str() {
            "ACCEPT" => Ok(Self::Accept),
            "DROP" => Ok(Self::Drop),
            _ => Err(format!("invalid policy '{s}' (must be ACCEPT or DROP)")),
        }
    }
}

impl fmt::Display for ChainPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
struct Chain {
    name: String,
    /// Policy (only for built-in chains).
    policy: Option<ChainPolicy>,
    rules: Vec<Rule>,
    /// Chain-level packet counter.
    chain_packets: u64,
    /// Chain-level byte counter.
    chain_bytes: u64,
    /// Whether this is a built-in chain.
    builtin: bool,
}

impl Chain {
    fn new_builtin(name: &str, policy: ChainPolicy) -> Self {
        Self {
            name: name.to_string(),
            policy: Some(policy),
            rules: Vec::new(),
            chain_packets: 0,
            chain_bytes: 0,
            builtin: true,
        }
    }

    fn new_user(name: &str) -> Self {
        Self {
            name: name.to_string(),
            policy: None,
            rules: Vec::new(),
            chain_packets: 0,
            chain_bytes: 0,
            builtin: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TableName {
    Filter,
    Nat,
    Mangle,
    Raw,
}

impl TableName {
    fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "filter" => Ok(Self::Filter),
            "nat" => Ok(Self::Nat),
            "mangle" => Ok(Self::Mangle),
            "raw" => Ok(Self::Raw),
            _ => Err(format!("unknown table '{s}'")),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Filter => "filter",
            Self::Nat => "nat",
            Self::Mangle => "mangle",
            Self::Raw => "raw",
        }
    }

    fn builtin_chains(&self) -> Vec<&'static str> {
        match self {
            Self::Filter => vec!["INPUT", "FORWARD", "OUTPUT"],
            Self::Nat => vec!["PREROUTING", "INPUT", "OUTPUT", "POSTROUTING"],
            Self::Mangle => {
                vec!["PREROUTING", "INPUT", "FORWARD", "OUTPUT", "POSTROUTING"]
            }
            Self::Raw => vec!["PREROUTING", "OUTPUT"],
        }
    }
}

impl fmt::Display for TableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
struct Table {
    name: TableName,
    chains: Vec<Chain>,
}

impl Table {
    fn new(name: TableName) -> Self {
        let builtin_names = name.builtin_chains();
        let chains = builtin_names
            .iter()
            .map(|cn| Chain::new_builtin(cn, ChainPolicy::Accept))
            .collect();
        Self { name, chains }
    }

    fn find_chain(&self, name: &str) -> Option<usize> {
        self.chains.iter().position(|c| c.name == name)
    }

    fn get_chain(&self, name: &str) -> Result<&Chain, String> {
        self.find_chain(name)
            .map(|i| &self.chains[i])
            .ok_or_else(|| format!("chain '{name}' not found in table '{}'", self.name))
    }

    fn get_chain_mut(&mut self, name: &str) -> Result<&mut Chain, String> {
        let table_name = self.name.as_str().to_string();
        self.find_chain(name)
            .map(move |i| &mut self.chains[i])
            .ok_or_else(|| format!("chain '{name}' not found in table '{table_name}'"))
    }
}

// ---------------------------------------------------------------------------
// Firewall state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Firewall {
    tables: HashMap<TableName, Table>,
    ipv6: bool,
}

impl Firewall {
    fn new(ipv6: bool) -> Self {
        let mut tables = HashMap::new();
        for tn in &[
            TableName::Filter,
            TableName::Nat,
            TableName::Mangle,
            TableName::Raw,
        ] {
            tables.insert(tn.clone(), Table::new(tn.clone()));
        }
        Self { tables, ipv6 }
    }

    fn get_table(&self, name: &TableName) -> &Table {
        self.tables.get(name).expect("table must exist")
    }

    fn get_table_mut(&mut self, name: &TableName) -> &mut Table {
        self.tables.get_mut(name).expect("table must exist")
    }
}

// ---------------------------------------------------------------------------
// Personality
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Personality {
    Iptables,
    Ip6tables,
    IptablesSave,
    IptablesRestore,
    Ip6tablesSave,
    Ip6tablesRestore,
}

impl Personality {
    fn detect(prog_name: &str) -> Self {
        let lower = prog_name.to_ascii_lowercase();
        if lower.contains("ip6tables-restore") {
            Self::Ip6tablesRestore
        } else if lower.contains("ip6tables-save") {
            Self::Ip6tablesSave
        } else if lower.contains("iptables-restore") {
            Self::IptablesRestore
        } else if lower.contains("iptables-save") {
            Self::IptablesSave
        } else if lower.contains("ip6tables") {
            Self::Ip6tables
        } else {
            Self::Iptables
        }
    }

    fn is_ipv6(&self) -> bool {
        matches!(
            self,
            Self::Ip6tables | Self::Ip6tablesSave | Self::Ip6tablesRestore
        )
    }

    fn prog_name(&self) -> &'static str {
        match self {
            Self::Iptables => "iptables",
            Self::Ip6tables => "ip6tables",
            Self::IptablesSave => "iptables-save",
            Self::IptablesRestore => "iptables-restore",
            Self::Ip6tablesSave => "ip6tables-save",
            Self::Ip6tablesRestore => "ip6tables-restore",
        }
    }
}

// ---------------------------------------------------------------------------
// Argument parser
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Command {
    Append {
        table: TableName,
        chain: String,
        rule: Rule,
    },
    Insert {
        table: TableName,
        chain: String,
        pos: Option<usize>,
        rule: Rule,
    },
    Delete {
        table: TableName,
        chain: String,
        rule_or_num: DeleteTarget,
    },
    Replace {
        table: TableName,
        chain: String,
        pos: usize,
        rule: Rule,
    },
    List {
        table: TableName,
        chain: Option<String>,
        // Set by `-n`/`--numeric` but not yet acted on: output is always
        // numeric because we never do DNS/service-name resolution. Kept as
        // part of the parsed command surface for when symbolic rendering is
        // added. See todo.txt.
        #[allow(dead_code)]
        numeric: bool,
        verbose: bool,
        line_numbers: bool,
    },
    Flush {
        table: TableName,
        chain: Option<String>,
    },
    Zero {
        table: TableName,
        chain: Option<String>,
    },
    NewChain {
        table: TableName,
        chain: String,
    },
    DeleteChain {
        table: TableName,
        chain: Option<String>,
    },
    Policy {
        table: TableName,
        chain: String,
        policy: ChainPolicy,
    },
    RenameChain {
        table: TableName,
        old_name: String,
        new_name: String,
    },
    Check {
        table: TableName,
        chain: String,
        rule: Rule,
    },
}

impl Command {
    /// Mutable access to the command's target table. Every variant carries a
    /// `table`; this lets the `iptables-restore` parser override the table with
    /// the current `*table` block context (restore-format rule lines never
    /// carry their own `-t`, so the parsed command defaults to `filter`).
    fn table_mut(&mut self) -> &mut TableName {
        match self {
            Command::Append { table, .. }
            | Command::Insert { table, .. }
            | Command::Delete { table, .. }
            | Command::Replace { table, .. }
            | Command::List { table, .. }
            | Command::Flush { table, .. }
            | Command::Zero { table, .. }
            | Command::NewChain { table, .. }
            | Command::DeleteChain { table, .. }
            | Command::Policy { table, .. }
            | Command::RenameChain { table, .. }
            | Command::Check { table, .. } => table,
        }
    }
}

#[derive(Debug)]
enum DeleteTarget {
    ByNumber(usize),
    BySpec(Rule),
}

struct ArgParser {
    args: Vec<String>,
    pos: usize,
    ipv6: bool,
}

impl ArgParser {
    fn new(args: Vec<String>, ipv6: bool) -> Self {
        Self { args, pos: 0, ipv6 }
    }

    fn peek(&self) -> Option<&str> {
        self.args.get(self.pos).map(|s| s.as_str())
    }

    fn next_arg(&mut self) -> Option<String> {
        if self.pos < self.args.len() {
            let val = self.args[self.pos].clone();
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }

    fn expect_arg(&mut self, what: &str) -> Result<String, String> {
        self.next_arg()
            .ok_or_else(|| format!("expected {what} after option"))
    }

    fn parse_command(&mut self) -> Result<Command, String> {
        let mut table = TableName::Filter;
        let mut command: Option<String> = None;
        let mut chain_name: Option<String> = None;
        let mut rule = Rule::new();
        let mut numeric = false;
        let mut verbose = false;
        let mut line_numbers = false;
        let mut insert_pos: Option<usize> = None;
        let mut delete_num: Option<usize> = None;
        let mut policy_val: Option<String> = None;
        let mut rename_new: Option<String> = None;

        // Track negation state
        let mut negate_next = false;

        while self.pos < self.args.len() {
            let arg = self.args[self.pos].clone();
            self.pos += 1;

            match arg.as_str() {
                "-t" | "--table" => {
                    let tname = self.expect_arg("table name")?;
                    table = TableName::parse(&tname)?;
                }
                "-A" | "--append" => {
                    command = Some("append".to_string());
                    chain_name = Some(self.expect_arg("chain name")?);
                }
                "-I" | "--insert" => {
                    command = Some("insert".to_string());
                    chain_name = Some(self.expect_arg("chain name")?);
                    // Next arg might be a position number
                    if let Some(next) = self.peek()
                        && let Ok(n) = next.parse::<usize>()
                    {
                        insert_pos = Some(n);
                        self.pos += 1;
                    }
                }
                "-D" | "--delete" => {
                    command = Some("delete".to_string());
                    chain_name = Some(self.expect_arg("chain name")?);
                    // Next arg might be a rule number
                    if let Some(next) = self.peek()
                        && let Ok(n) = next.parse::<usize>()
                    {
                        // Only treat as number if there are no further
                        // match/target args that would indicate a spec.
                        delete_num = Some(n);
                        self.pos += 1;
                    }
                }
                "-R" | "--replace" => {
                    command = Some("replace".to_string());
                    chain_name = Some(self.expect_arg("chain name")?);
                    let pos_str = self.expect_arg("rule number")?;
                    insert_pos = Some(
                        pos_str
                            .parse()
                            .map_err(|_| format!("invalid rule number '{pos_str}'"))?,
                    );
                }
                "-L" | "--list" => {
                    command = Some("list".to_string());
                    if let Some(next) = self.peek()
                        && !next.starts_with('-')
                    {
                        chain_name = Some(next.to_string());
                        self.pos += 1;
                    }
                }
                "-F" | "--flush" => {
                    command = Some("flush".to_string());
                    if let Some(next) = self.peek()
                        && !next.starts_with('-')
                    {
                        chain_name = Some(next.to_string());
                        self.pos += 1;
                    }
                }
                "-Z" | "--zero" => {
                    command = Some("zero".to_string());
                    if let Some(next) = self.peek()
                        && !next.starts_with('-')
                    {
                        chain_name = Some(next.to_string());
                        self.pos += 1;
                    }
                }
                "-N" | "--new-chain" => {
                    command = Some("new-chain".to_string());
                    chain_name = Some(self.expect_arg("chain name")?);
                }
                "-X" | "--delete-chain" => {
                    command = Some("delete-chain".to_string());
                    if let Some(next) = self.peek()
                        && !next.starts_with('-')
                    {
                        chain_name = Some(next.to_string());
                        self.pos += 1;
                    }
                }
                "-P" | "--policy" => {
                    command = Some("policy".to_string());
                    chain_name = Some(self.expect_arg("chain name")?);
                    policy_val = Some(self.expect_arg("policy")?);
                }
                "-E" | "--rename-chain" => {
                    command = Some("rename-chain".to_string());
                    chain_name = Some(self.expect_arg("old chain name")?);
                    rename_new = Some(self.expect_arg("new chain name")?);
                }
                "-C" | "--check" => {
                    command = Some("check".to_string());
                    chain_name = Some(self.expect_arg("chain name")?);
                }
                "!" => {
                    negate_next = true;
                    continue;
                }
                "-p" | "--protocol" => {
                    rule.not_protocol = negate_next;
                    let proto_str = self.expect_arg("protocol")?;
                    rule.protocol = Some(Protocol::parse(&proto_str)?);
                }
                "-s" | "--source" | "--src" => {
                    rule.not_source = negate_next;
                    let addr_str = self.expect_arg("source address")?;
                    rule.source = Some(if self.ipv6 {
                        AddrCidr::parse_v6(&addr_str)?
                    } else {
                        AddrCidr::parse_v4(&addr_str)?
                    });
                }
                "-d" | "--destination" | "--dst" => {
                    rule.not_destination = negate_next;
                    let addr_str = self.expect_arg("destination address")?;
                    rule.destination = Some(if self.ipv6 {
                        AddrCidr::parse_v6(&addr_str)?
                    } else {
                        AddrCidr::parse_v4(&addr_str)?
                    });
                }
                "-i" | "--in-interface" => {
                    rule.not_in_interface = negate_next;
                    rule.in_interface = Some(self.expect_arg("input interface")?);
                }
                "-o" | "--out-interface" => {
                    rule.not_out_interface = negate_next;
                    rule.out_interface = Some(self.expect_arg("output interface")?);
                }
                "--sport" | "--source-port" => {
                    rule.not_sport = negate_next;
                    let port_str = self.expect_arg("source port")?;
                    rule.sport = Some(PortSpec::parse(&port_str)?);
                }
                "--dport" | "--destination-port" => {
                    rule.not_dport = negate_next;
                    let port_str = self.expect_arg("destination port")?;
                    rule.dport = Some(PortSpec::parse(&port_str)?);
                }
                "-j" | "--jump" => {
                    let target_str = self.expect_arg("target")?;
                    rule.target = Some(self.parse_target(&target_str)?);
                }
                "-m" | "--match" => {
                    let match_name = self.expect_arg("match module name")?;
                    let ext = self.parse_match_ext(&match_name)?;
                    rule.match_extensions.push(ext);
                }
                "-n" | "--numeric" => {
                    numeric = true;
                }
                "-v" | "--verbose" => {
                    verbose = true;
                }
                "--line-numbers" => {
                    line_numbers = true;
                }
                "-c" | "--set-counters" => {
                    // -c packets bytes (used in restore)
                    let pkts_str = self.expect_arg("packet count")?;
                    let bytes_str = self.expect_arg("byte count")?;
                    rule.packets = pkts_str.parse().unwrap_or(0);
                    rule.bytes = bytes_str.parse().unwrap_or(0);
                }
                "--help" | "-h" => {
                    return Err("help requested".to_string());
                }
                other => {
                    return Err(format!("unknown option '{other}'"));
                }
            }
            negate_next = false;
        }

        let cmd_str = command.ok_or("no command specified")?;
        match cmd_str.as_str() {
            "append" => Ok(Command::Append {
                table,
                chain: chain_name.ok_or("chain name required")?,
                rule,
            }),
            "insert" => Ok(Command::Insert {
                table,
                chain: chain_name.ok_or("chain name required")?,
                pos: insert_pos,
                rule,
            }),
            "delete" => {
                let chain = chain_name.ok_or("chain name required")?;
                // If we captured a delete number and the rule is essentially empty
                // (no match criteria or target set after the number), treat as by-number.
                let target = if let Some(num) = delete_num {
                    if rule.protocol.is_none()
                        && rule.source.is_none()
                        && rule.destination.is_none()
                        && rule.target.is_none()
                        && rule.match_extensions.is_empty()
                        && rule.sport.is_none()
                        && rule.dport.is_none()
                        && rule.in_interface.is_none()
                        && rule.out_interface.is_none()
                    {
                        DeleteTarget::ByNumber(num)
                    } else {
                        DeleteTarget::BySpec(rule)
                    }
                } else {
                    DeleteTarget::BySpec(rule)
                };
                Ok(Command::Delete {
                    table,
                    chain,
                    rule_or_num: target,
                })
            }
            "replace" => Ok(Command::Replace {
                table,
                chain: chain_name.ok_or("chain name required")?,
                pos: insert_pos.ok_or("rule number required for replace")?,
                rule,
            }),
            "list" => Ok(Command::List {
                table,
                chain: chain_name,
                numeric,
                verbose,
                line_numbers,
            }),
            "flush" => Ok(Command::Flush {
                table,
                chain: chain_name,
            }),
            "zero" => Ok(Command::Zero {
                table,
                chain: chain_name,
            }),
            "new-chain" => Ok(Command::NewChain {
                table,
                chain: chain_name.ok_or("chain name required")?,
            }),
            "delete-chain" => Ok(Command::DeleteChain {
                table,
                chain: chain_name,
            }),
            "policy" => Ok(Command::Policy {
                table,
                chain: chain_name.ok_or("chain name required")?,
                policy: ChainPolicy::parse(&policy_val.ok_or("policy required")?)?,
            }),
            "rename-chain" => Ok(Command::RenameChain {
                table,
                old_name: chain_name.ok_or("old chain name required")?,
                new_name: rename_new.ok_or("new chain name required")?,
            }),
            "check" => Ok(Command::Check {
                table,
                chain: chain_name.ok_or("chain name required")?,
                rule,
            }),
            other => Err(format!("unknown command '{other}'")),
        }
    }

    fn parse_target(&mut self, name: &str) -> Result<Target, String> {
        match name.to_ascii_uppercase().as_str() {
            "ACCEPT" => Ok(Target::Accept),
            "DROP" => Ok(Target::Drop),
            "REJECT" => {
                let mut reject_with = None;
                if let Some(next) = self.peek()
                    && next == "--reject-with"
                {
                    self.pos += 1;
                    reject_with = Some(self.expect_arg("reject type")?);
                }
                Ok(Target::Reject { reject_with })
            }
            "LOG" => {
                let mut prefix = None;
                let mut level = None;
                loop {
                    match self.peek() {
                        Some("--log-prefix") => {
                            self.pos += 1;
                            prefix = Some(self.expect_arg("log prefix")?);
                        }
                        Some("--log-level") => {
                            self.pos += 1;
                            level = Some(self.expect_arg("log level")?);
                        }
                        _ => break,
                    }
                }
                Ok(Target::Log { prefix, level })
            }
            "SNAT" => {
                if self.peek() == Some("--to-source") {
                    self.pos += 1;
                    let addr = self.expect_arg("SNAT address")?;
                    Ok(Target::Snat { to_source: addr })
                } else {
                    Err("SNAT requires --to-source".to_string())
                }
            }
            "DNAT" => {
                if self.peek() == Some("--to-destination") {
                    self.pos += 1;
                    let addr = self.expect_arg("DNAT address")?;
                    Ok(Target::Dnat {
                        to_destination: addr,
                    })
                } else {
                    Err("DNAT requires --to-destination".to_string())
                }
            }
            "MASQUERADE" => Ok(Target::Masquerade),
            "REDIRECT" => {
                let mut to_ports = None;
                if self.peek() == Some("--to-ports") {
                    self.pos += 1;
                    let port_str = self.expect_arg("redirect port")?;
                    to_ports = Some(PortSpec::parse(&port_str)?);
                }
                Ok(Target::Redirect { to_ports })
            }
            "RETURN" => Ok(Target::Return),
            _ => Ok(Target::UserChain(name.to_string())),
        }
    }

    fn parse_match_ext(&mut self, module: &str) -> Result<MatchExt, String> {
        match module.to_ascii_lowercase().as_str() {
            "state" => {
                if self.peek() == Some("--state") {
                    self.pos += 1;
                    let states_str = self.expect_arg("state list")?;
                    let states = parse_state_list(&states_str)?;
                    Ok(MatchExt::State(states))
                } else {
                    Err("-m state requires --state".to_string())
                }
            }
            "conntrack" => {
                if self.peek() == Some("--ctstate") {
                    self.pos += 1;
                    let states_str = self.expect_arg("ctstate list")?;
                    let states = parse_state_list(&states_str)?;
                    Ok(MatchExt::ConnTrack(states))
                } else {
                    Err("-m conntrack requires --ctstate".to_string())
                }
            }
            "multiport" => {
                let next = self
                    .peek()
                    .ok_or("-m multiport requires --dports or --sports")?;
                match next {
                    "--dports" | "--destination-ports" => {
                        self.pos += 1;
                        let ports_str = self.expect_arg("destination ports")?;
                        let mp = MultiPort::parse(&ports_str)?;
                        Ok(MatchExt::MultiDport(mp))
                    }
                    "--sports" | "--source-ports" => {
                        self.pos += 1;
                        let ports_str = self.expect_arg("source ports")?;
                        let mp = MultiPort::parse(&ports_str)?;
                        Ok(MatchExt::MultiSport(mp))
                    }
                    _ => Err("-m multiport requires --dports or --sports".to_string()),
                }
            }
            "limit" => {
                let mut rate = 3;
                let mut unit = LimitUnit::Hour;
                let mut burst = 5;
                loop {
                    match self.peek() {
                        Some("--limit") => {
                            self.pos += 1;
                            let rate_str = self.expect_arg("limit rate")?;
                            let (r, u) = LimitSpec::parse_rate(&rate_str)?;
                            rate = r;
                            unit = u;
                        }
                        Some("--limit-burst") => {
                            self.pos += 1;
                            let burst_str = self.expect_arg("burst count")?;
                            burst = burst_str
                                .parse()
                                .map_err(|_| format!("invalid burst '{burst_str}'"))?;
                        }
                        _ => break,
                    }
                }
                Ok(MatchExt::Limit(LimitSpec { rate, unit, burst }))
            }
            "comment" => {
                if self.peek() == Some("--comment") {
                    self.pos += 1;
                    let text = self.expect_arg("comment text")?;
                    Ok(MatchExt::Comment(text))
                } else {
                    Err("-m comment requires --comment".to_string())
                }
            }
            other => Err(format!("unknown match module '{other}'")),
        }
    }
}

// ---------------------------------------------------------------------------
// Command execution
// ---------------------------------------------------------------------------

fn execute_command(fw: &mut Firewall, cmd: Command) -> Result<String, String> {
    match cmd {
        Command::Append { table, chain, rule } => {
            let tbl = fw.get_table_mut(&table);
            let ch = tbl.get_chain_mut(&chain)?;
            ch.rules.push(rule);
            Ok(String::new())
        }
        Command::Insert {
            table,
            chain,
            pos,
            rule,
        } => {
            let tbl = fw.get_table_mut(&table);
            let ch = tbl.get_chain_mut(&chain)?;
            let idx = match pos {
                Some(p) if p > 0 => {
                    let i = p - 1;
                    if i > ch.rules.len() {
                        return Err(format!(
                            "rule number {p} too large (chain has {} rules)",
                            ch.rules.len()
                        ));
                    }
                    i
                }
                Some(0) => return Err("rule number must be >= 1".to_string()),
                None | Some(_) => 0,
            };
            ch.rules.insert(idx, rule);
            Ok(String::new())
        }
        Command::Delete {
            table,
            chain,
            rule_or_num,
        } => {
            let tbl = fw.get_table_mut(&table);
            let ch = tbl.get_chain_mut(&chain)?;
            match rule_or_num {
                DeleteTarget::ByNumber(num) => {
                    if num == 0 || num > ch.rules.len() {
                        return Err(format!(
                            "invalid rule number {num} (chain has {} rules)",
                            ch.rules.len()
                        ));
                    }
                    ch.rules.remove(num - 1);
                    Ok(String::new())
                }
                DeleteTarget::BySpec(spec) => {
                    if let Some(pos) = ch.rules.iter().position(|r| r.matches_spec(&spec)) {
                        ch.rules.remove(pos);
                        Ok(String::new())
                    } else {
                        Err("no matching rule found".to_string())
                    }
                }
            }
        }
        Command::Replace {
            table,
            chain,
            pos,
            rule,
        } => {
            let tbl = fw.get_table_mut(&table);
            let ch = tbl.get_chain_mut(&chain)?;
            if pos == 0 || pos > ch.rules.len() {
                return Err(format!(
                    "invalid rule number {pos} (chain has {} rules)",
                    ch.rules.len()
                ));
            }
            ch.rules[pos - 1] = rule;
            Ok(String::new())
        }
        Command::List {
            table,
            chain,
            // `-n`/`--numeric` is accepted but has no effect: this tool never
            // performs reverse-DNS or /etc/services lookups, so addresses and
            // ports are always rendered numerically (exactly what `-n` would
            // produce in upstream iptables). See todo.txt.
            numeric: _,
            verbose,
            line_numbers,
        } => {
            let tbl = fw.get_table(&table);
            let chains_to_list: Vec<&Chain> = if let Some(ref cname) = chain {
                vec![tbl.get_chain(cname)?]
            } else {
                tbl.chains.iter().collect()
            };
            let mut output = String::new();
            for (ci, ch) in chains_to_list.iter().enumerate() {
                if ci > 0 {
                    output.push('\n');
                }
                let policy_str = ch
                    .policy
                    .as_ref()
                    .map(|p| format!("policy {p}"))
                    .unwrap_or_else(|| "no policy".to_string());
                output.push_str(&format!("Chain {} ({policy_str})\n", ch.name));
                if verbose {
                    output.push_str(" pkts bytes target     prot opt in     out     source               destination\n");
                } else {
                    // The header row is identical for numeric and symbolic
                    // output; `numeric` only changes how addresses below render.
                    output.push_str("target     prot opt source               destination\n");
                }
                for (ri, rule) in ch.rules.iter().enumerate() {
                    let mut line = String::new();
                    if line_numbers {
                        line.push_str(&format!("{:<5}", ri + 1));
                    }
                    if verbose {
                        line.push_str(&format!("{:<6}{:<6}", rule.packets, rule.bytes));
                    }
                    let target_str = rule
                        .target
                        .as_ref()
                        .map(|t| t.name().to_string())
                        .unwrap_or_default();
                    let proto_str = rule
                        .protocol
                        .as_ref()
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "all".to_string());
                    let src_str = rule
                        .source
                        .as_ref()
                        .map(|s| {
                            let prefix = if rule.not_source { "!" } else { "" };
                            format!("{prefix}{s}")
                        })
                        .unwrap_or_else(|| "0.0.0.0/0".to_string());
                    let dst_str = rule
                        .destination
                        .as_ref()
                        .map(|d| {
                            let prefix = if rule.not_destination { "!" } else { "" };
                            format!("{prefix}{d}")
                        })
                        .unwrap_or_else(|| "0.0.0.0/0".to_string());
                    if verbose {
                        let in_str = rule.in_interface.as_deref().unwrap_or("*");
                        let out_str = rule.out_interface.as_deref().unwrap_or("*");
                        line.push_str(&format!(
                            "{:<11}{:<5}{:<4}{:<7}{:<8}{:<21}{:<21}",
                            target_str, proto_str, "--", in_str, out_str, src_str, dst_str
                        ));
                    } else {
                        line.push_str(&format!(
                            "{:<11}{:<5}{:<4}{:<21}{:<21}",
                            target_str, proto_str, "--", src_str, dst_str
                        ));
                    }
                    // Append extended match info
                    for ext in &rule.match_extensions {
                        line.push_str(&format!(" {ext}"));
                    }
                    // Append target options
                    if let Some(ref tgt) = rule.target {
                        match tgt {
                            Target::Reject {
                                reject_with: Some(r),
                            } => {
                                line.push_str(&format!(" reject-with {r}"));
                            }
                            Target::Log { prefix, level } => {
                                if let Some(p) = prefix {
                                    line.push_str(&format!(" LOG prefix \"{p}\""));
                                }
                                if let Some(l) = level {
                                    line.push_str(&format!(" level {l}"));
                                }
                            }
                            _ => {}
                        }
                    }
                    // Append port info
                    if let Some(ref sp) = rule.sport {
                        let neg = if rule.not_sport { "!" } else { "" };
                        line.push_str(&format!(" spt:{neg}{sp}"));
                    }
                    if let Some(ref dp) = rule.dport {
                        let neg = if rule.not_dport { "!" } else { "" };
                        line.push_str(&format!(" dpt:{neg}{dp}"));
                    }
                    output.push_str(line.trim_end());
                    output.push('\n');
                }
            }
            Ok(output)
        }
        Command::Flush { table, chain } => {
            let tbl = fw.get_table_mut(&table);
            if let Some(cname) = chain {
                let ch = tbl.get_chain_mut(&cname)?;
                ch.rules.clear();
            } else {
                for ch in &mut tbl.chains {
                    ch.rules.clear();
                }
            }
            Ok(String::new())
        }
        Command::Zero { table, chain } => {
            let tbl = fw.get_table_mut(&table);
            if let Some(cname) = chain {
                let ch = tbl.get_chain_mut(&cname)?;
                ch.chain_packets = 0;
                ch.chain_bytes = 0;
                for rule in &mut ch.rules {
                    rule.packets = 0;
                    rule.bytes = 0;
                }
            } else {
                for ch in &mut tbl.chains {
                    ch.chain_packets = 0;
                    ch.chain_bytes = 0;
                    for rule in &mut ch.rules {
                        rule.packets = 0;
                        rule.bytes = 0;
                    }
                }
            }
            Ok(String::new())
        }
        Command::NewChain { table, chain } => {
            let tbl = fw.get_table_mut(&table);
            if tbl.find_chain(&chain).is_some() {
                return Err(format!("chain '{chain}' already exists"));
            }
            tbl.chains.push(Chain::new_user(&chain));
            Ok(String::new())
        }
        Command::DeleteChain { table, chain } => {
            let tbl = fw.get_table_mut(&table);
            if let Some(cname) = chain {
                let idx = tbl
                    .find_chain(&cname)
                    .ok_or_else(|| format!("chain '{cname}' not found"))?;
                if tbl.chains[idx].builtin {
                    return Err(format!("cannot delete built-in chain '{cname}'"));
                }
                if !tbl.chains[idx].rules.is_empty() {
                    return Err(format!("cannot delete non-empty chain '{cname}'"));
                }
                // Check no other chain references this one
                for ch in &tbl.chains {
                    for rule in &ch.rules {
                        if let Some(Target::UserChain(ref name)) = rule.target
                            && name == &cname
                        {
                            return Err(format!(
                                "cannot delete chain '{cname}': referenced by chain '{}'",
                                ch.name
                            ));
                        }
                    }
                }
                tbl.chains.remove(idx);
            } else {
                // Delete all empty user-defined chains
                let mut to_remove = Vec::new();
                for (i, ch) in tbl.chains.iter().enumerate() {
                    if !ch.builtin && ch.rules.is_empty() {
                        to_remove.push(i);
                    }
                }
                // Remove in reverse to keep indices valid
                for i in to_remove.into_iter().rev() {
                    tbl.chains.remove(i);
                }
            }
            Ok(String::new())
        }
        Command::Policy {
            table,
            chain,
            policy,
        } => {
            let tbl = fw.get_table_mut(&table);
            let ch = tbl.get_chain_mut(&chain)?;
            if !ch.builtin {
                return Err(format!("cannot set policy on user-defined chain '{chain}'"));
            }
            ch.policy = Some(policy);
            Ok(String::new())
        }
        Command::RenameChain {
            table,
            old_name,
            new_name,
        } => {
            let tbl = fw.get_table_mut(&table);
            let idx = tbl
                .find_chain(&old_name)
                .ok_or_else(|| format!("chain '{old_name}' not found"))?;
            if tbl.chains[idx].builtin {
                return Err(format!("cannot rename built-in chain '{old_name}'"));
            }
            if tbl.find_chain(&new_name).is_some() {
                return Err(format!("chain '{new_name}' already exists"));
            }
            tbl.chains[idx].name = new_name.clone();
            // Also update any jump targets pointing to the old name
            let old_name_clone = old_name.clone();
            for ch in &mut tbl.chains {
                for rule in &mut ch.rules {
                    if let Some(Target::UserChain(ref mut name)) = rule.target
                        && *name == old_name_clone
                    {
                        *name = new_name.clone();
                    }
                }
            }
            Ok(String::new())
        }
        Command::Check { table, chain, rule } => {
            let tbl = fw.get_table(&table);
            let ch = tbl.get_chain(&chain)?;
            if ch.rules.iter().any(|r| r.matches_spec(&rule)) {
                Ok(String::new())
            } else {
                Err("no matching rule found".to_string())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// iptables-save
// ---------------------------------------------------------------------------

fn save_firewall(fw: &Firewall) -> String {
    let mut output = String::new();
    let table_order = [
        TableName::Raw,
        TableName::Mangle,
        TableName::Nat,
        TableName::Filter,
    ];
    for tn in &table_order {
        let tbl = fw.get_table(tn);
        // Only output tables that have non-default state.
        let has_rules = tbl.chains.iter().any(|c| !c.rules.is_empty());
        let has_custom_chains = tbl.chains.iter().any(|c| !c.builtin);
        let has_non_accept_policy = tbl
            .chains
            .iter()
            .any(|c| c.policy.as_ref().is_some_and(|p| *p != ChainPolicy::Accept));
        if !has_rules && !has_custom_chains && !has_non_accept_policy {
            // Still output the table header and COMMIT for completeness
        }
        output.push_str(&format!("*{}\n", tn.as_str()));
        for ch in &tbl.chains {
            let policy = ch
                .policy
                .as_ref()
                .map(|p| p.as_str().to_string())
                .unwrap_or_else(|| "-".to_string());
            output.push_str(&format!(
                ":{} {} [{}:{}]\n",
                ch.name, policy, ch.chain_packets, ch.chain_bytes
            ));
        }
        for ch in &tbl.chains {
            for rule in &ch.rules {
                let args = rule.to_args_string();
                if rule.packets != 0 || rule.bytes != 0 {
                    output.push_str(&format!(
                        "[{}:{}] -A {} {}\n",
                        rule.packets, rule.bytes, ch.name, args
                    ));
                } else {
                    output.push_str(&format!("-A {} {}\n", ch.name, args));
                }
            }
        }
        output.push_str("COMMIT\n");
    }
    output
}

// ---------------------------------------------------------------------------
// iptables-restore
// ---------------------------------------------------------------------------

fn restore_firewall(fw: &mut Firewall, input: &str) -> Result<(), String> {
    let mut current_table: Option<TableName> = None;

    for (line_num, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(table_str) = line.strip_prefix('*') {
            let tn = TableName::parse(table_str.trim())?;
            // Reset the table
            *fw.get_table_mut(&tn) = Table::new(tn.clone());
            current_table = Some(tn);
            continue;
        }
        if line == "COMMIT" {
            current_table = None;
            continue;
        }
        let tn = current_table
            .as_ref()
            .ok_or_else(|| format!("line {}: no table context", line_num + 1))?;

        if let Some(chain_def) = line.strip_prefix(':') {
            // :CHAIN POLICY [packets:bytes]
            let parts: Vec<&str> = chain_def.split_whitespace().collect();
            if parts.len() < 2 {
                return Err(format!(
                    "line {}: invalid chain definition '{line}'",
                    line_num + 1
                ));
            }
            let chain_name = parts[0];
            let policy_str = parts[1];
            let (pkts, bytes_val) = if parts.len() >= 3 {
                parse_counter_bracket(parts[2])?
            } else {
                (0, 0)
            };
            let tbl = fw.get_table_mut(tn);
            if let Some(idx) = tbl.find_chain(chain_name) {
                // Update existing chain
                if policy_str != "-" {
                    tbl.chains[idx].policy = Some(ChainPolicy::parse(policy_str)?);
                }
                tbl.chains[idx].chain_packets = pkts;
                tbl.chains[idx].chain_bytes = bytes_val;
            } else {
                // User-defined chain
                let mut ch = Chain::new_user(chain_name);
                ch.chain_packets = pkts;
                ch.chain_bytes = bytes_val;
                tbl.chains.push(ch);
            }
            continue;
        }

        // Rule line, possibly with counter prefix: [packets:bytes] -A CHAIN ...
        let (rule_pkts, rule_bytes, rule_line) = if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                let counter_str = &line[..=end];
                let (p, b) = parse_counter_bracket(counter_str)?;
                let rest = line[end + 1..].trim();
                (p, b, rest)
            } else {
                (0u64, 0u64, line)
            }
        } else {
            (0u64, 0u64, line)
        };

        // Parse as iptables arguments
        let words = shell_split(rule_line)?;
        if words.is_empty() {
            continue;
        }

        let mut parser = ArgParser::new(words, fw.ipv6);
        let mut cmd = parser
            .parse_command()
            .map_err(|e| format!("line {}: {e}", line_num + 1))?;

        // In iptables-restore format the table comes from the enclosing
        // `*table` block, not from a per-rule `-t`. Override whatever the
        // command parser defaulted to (filter) with the current block's table.
        *cmd.table_mut() = tn.clone();

        // Apply counter from bracket prefix
        if rule_pkts != 0 || rule_bytes != 0 {
            match &mut cmd {
                Command::Append { rule, .. } | Command::Insert { rule, .. } => {
                    rule.packets = rule_pkts;
                    rule.bytes = rule_bytes;
                }
                _ => {}
            }
        }

        execute_command(fw, cmd).map_err(|e| format!("line {}: {e}", line_num + 1))?;
    }

    Ok(())
}

fn parse_counter_bracket(s: &str) -> Result<(u64, u64), String> {
    let s = s.trim_start_matches('[').trim_end_matches(']');
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(format!("invalid counter format '{s}'"));
    }
    let pkts: u64 = parts[0]
        .parse()
        .map_err(|_| format!("invalid packet count '{}'", parts[0]))?;
    let bytes_val: u64 = parts[1]
        .parse()
        .map_err(|_| format!("invalid byte count '{}'", parts[1]))?;
    Ok((pkts, bytes_val))
}

/// Simple shell-like word splitting (handles double-quoted strings).
fn shell_split(s: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in s.chars() {
        if in_quotes {
            if ch == '"' {
                in_quotes = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == ' ' || ch == '\t' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if in_quotes {
        return Err("unterminated quote".to_string());
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn run() -> i32 {
    let args: Vec<String> = std::env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("iptables");
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

    let personality = Personality::detect(&prog_name);
    let ipv6 = personality.is_ipv6();
    let mut fw = Firewall::new(ipv6);

    match personality {
        Personality::IptablesSave | Personality::Ip6tablesSave => {
            // Optional -t argument to save only one table
            let table_filter = if args.len() > 2 && (args[1] == "-t" || args[1] == "--table") {
                Some(TableName::parse(&args[2]).unwrap_or_else(|e| {
                    eprintln!("{}: {e}", personality.prog_name());
                    process::exit(1);
                }))
            } else {
                None
            };
            let output = save_firewall(&fw);
            if let Some(ref tf) = table_filter {
                // Only print the requested table section
                let marker = format!("*{}", tf.as_str());
                let mut printing = false;
                for line in output.lines() {
                    if line == marker {
                        printing = true;
                    }
                    if printing {
                        println!("{line}");
                        if line == "COMMIT" {
                            break;
                        }
                    }
                }
            } else {
                print!("{output}");
            }
            0
        }
        Personality::IptablesRestore | Personality::Ip6tablesRestore => {
            let stdin = io::stdin();
            let mut input = String::new();
            for line in stdin.lock().lines() {
                match line {
                    Ok(l) => {
                        input.push_str(&l);
                        input.push('\n');
                    }
                    Err(e) => {
                        eprintln!("{}: read error: {e}", personality.prog_name());
                        return 1;
                    }
                }
            }
            match restore_firewall(&mut fw, &input) {
                Ok(()) => {
                    // In a real system, we'd install the rules into the kernel.
                    // For now, we verify the restore completed successfully.
                    0
                }
                Err(e) => {
                    eprintln!("{}: {e}", personality.prog_name());
                    1
                }
            }
        }
        Personality::Iptables | Personality::Ip6tables => {
            if args.len() < 2 {
                eprintln!("{}: no command specified", personality.prog_name());
                eprintln!(
                    "Try `{} -h' or `{} --help' for more information.",
                    personality.prog_name(),
                    personality.prog_name()
                );
                return 2;
            }
            let cmd_args: Vec<String> = args[1..].to_vec();
            let mut parser = ArgParser::new(cmd_args, ipv6);
            match parser.parse_command() {
                Ok(cmd) => match execute_command(&mut fw, cmd) {
                    Ok(output) => {
                        if !output.is_empty() {
                            print!("{output}");
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("{}: {e}", personality.prog_name());
                        1
                    }
                },
                Err(e) => {
                    if e == "help requested" {
                        print_help(&personality);
                        0
                    } else {
                        eprintln!("{}: {e}", personality.prog_name());
                        2
                    }
                }
            }
        }
    }
}

fn print_help(personality: &Personality) {
    let name = personality.prog_name();
    println!("{name} v1.0.0 - OurOS packet filtering utility");
    println!();
    println!("Usage: {name} [-t table] command chain [options]");
    println!();
    println!("Commands:");
    println!("  -A, --append chain rule-spec    Append rule to chain");
    println!("  -I, --insert chain [num] rule   Insert rule at position");
    println!("  -D, --delete chain rule|num     Delete rule by spec or number");
    println!("  -R, --replace chain num rule    Replace rule at position");
    println!("  -L, --list [chain]              List rules");
    println!("  -F, --flush [chain]             Flush chain or all chains");
    println!("  -Z, --zero [chain]              Zero packet/byte counters");
    println!("  -N, --new-chain chain           Create user-defined chain");
    println!("  -X, --delete-chain [chain]      Delete user-defined chain");
    println!("  -P, --policy chain target       Set chain policy");
    println!("  -E, --rename-chain old new      Rename chain");
    println!("  -C, --check chain rule-spec     Check if rule exists");
    println!();
    println!("Options:");
    println!("  -t, --table table    Table (filter/nat/mangle/raw)");
    println!("  -p, --protocol proto Protocol (tcp/udp/icmp/all)");
    println!("  -s, --source addr    Source address/CIDR");
    println!("  -d, --destination a  Destination address/CIDR");
    println!("  -i, --in-interface   Input interface");
    println!("  -o, --out-interface  Output interface");
    println!("  -j, --jump target    Rule target");
    println!("  -n, --numeric        Numeric output");
    println!("  -v, --verbose        Verbose output");
    println!("  --line-numbers       Show line numbers in list");
    println!("  --sport port         Source port");
    println!("  --dport port         Destination port");
    println!("  -m match --opts      Extended match module");
    println!("  ! [option]           Negate next match");
}

fn main() {
    let code = run();
    process::exit(code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // IPv4 CIDR parsing and matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipv4_cidr_parse_host() {
        let c = Ipv4Cidr::parse("192.168.1.1").unwrap();
        assert_eq!(c.addr, [192, 168, 1, 1]);
        assert_eq!(c.prefix_len, 32);
    }

    #[test]
    fn test_ipv4_cidr_parse_with_prefix() {
        let c = Ipv4Cidr::parse("10.0.0.0/8").unwrap();
        assert_eq!(c.addr, [10, 0, 0, 0]);
        assert_eq!(c.prefix_len, 8);
    }

    #[test]
    fn test_ipv4_cidr_parse_zero() {
        let c = Ipv4Cidr::parse("0.0.0.0/0").unwrap();
        assert_eq!(c.addr, [0, 0, 0, 0]);
        assert_eq!(c.prefix_len, 0);
    }

    #[test]
    fn test_ipv4_cidr_parse_16() {
        let c = Ipv4Cidr::parse("172.16.0.0/16").unwrap();
        assert_eq!(c.addr, [172, 16, 0, 0]);
        assert_eq!(c.prefix_len, 16);
    }

    #[test]
    fn test_ipv4_cidr_invalid_prefix() {
        assert!(Ipv4Cidr::parse("10.0.0.0/33").is_err());
    }

    #[test]
    fn test_ipv4_cidr_invalid_addr() {
        assert!(Ipv4Cidr::parse("256.0.0.0/8").is_err());
    }

    #[test]
    fn test_ipv4_cidr_invalid_format() {
        assert!(Ipv4Cidr::parse("10.0.0/8").is_err());
    }

    #[test]
    fn test_ipv4_cidr_contains_match() {
        let c = Ipv4Cidr::parse("192.168.1.0/24").unwrap();
        assert!(c.contains(&[192, 168, 1, 100]));
        assert!(c.contains(&[192, 168, 1, 0]));
        assert!(c.contains(&[192, 168, 1, 255]));
    }

    #[test]
    fn test_ipv4_cidr_contains_no_match() {
        let c = Ipv4Cidr::parse("192.168.1.0/24").unwrap();
        assert!(!c.contains(&[192, 168, 2, 1]));
        assert!(!c.contains(&[10, 0, 0, 1]));
    }

    #[test]
    fn test_ipv4_cidr_contains_zero_prefix() {
        let c = Ipv4Cidr::parse("0.0.0.0/0").unwrap();
        assert!(c.contains(&[1, 2, 3, 4]));
        assert!(c.contains(&[255, 255, 255, 255]));
    }

    #[test]
    fn test_ipv4_cidr_contains_host() {
        let c = Ipv4Cidr::parse("10.0.0.1/32").unwrap();
        assert!(c.contains(&[10, 0, 0, 1]));
        assert!(!c.contains(&[10, 0, 0, 2]));
    }

    #[test]
    fn test_ipv4_cidr_display() {
        let c = Ipv4Cidr::parse("10.0.0.0/8").unwrap();
        assert_eq!(c.to_string(), "10.0.0.0/8");
    }

    #[test]
    fn test_ipv4_cidr_contains_slash17() {
        let c = Ipv4Cidr::parse("10.128.0.0/17").unwrap();
        assert!(c.contains(&[10, 128, 0, 1]));
        assert!(c.contains(&[10, 128, 127, 255]));
        assert!(!c.contains(&[10, 128, 128, 0]));
    }

    // -----------------------------------------------------------------------
    // IPv6 CIDR parsing and matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipv6_cidr_parse_full() {
        let c = Ipv6Cidr::parse("2001:db8::/32").unwrap();
        assert_eq!(c.prefix_len, 32);
        assert_eq!(c.addr[0], 0x20);
        assert_eq!(c.addr[1], 0x01);
        assert_eq!(c.addr[2], 0x0d);
        assert_eq!(c.addr[3], 0xb8);
    }

    #[test]
    fn test_ipv6_cidr_parse_loopback() {
        let c = Ipv6Cidr::parse("::1/128").unwrap();
        assert_eq!(c.prefix_len, 128);
        assert_eq!(c.addr[15], 1);
        for i in 0..15 {
            assert_eq!(c.addr[i], 0);
        }
    }

    #[test]
    fn test_ipv6_cidr_parse_all_zeros() {
        let c = Ipv6Cidr::parse("::/0").unwrap();
        assert_eq!(c.prefix_len, 0);
        assert_eq!(c.addr, [0; 16]);
    }

    #[test]
    fn test_ipv6_cidr_invalid_prefix() {
        assert!(Ipv6Cidr::parse("::1/129").is_err());
    }

    #[test]
    fn test_ipv6_cidr_contains() {
        let c = Ipv6Cidr::parse("2001:db8::/32").unwrap();
        let mut ip = [0u8; 16];
        ip[0] = 0x20;
        ip[1] = 0x01;
        ip[2] = 0x0d;
        ip[3] = 0xb8;
        ip[4] = 0x00;
        ip[5] = 0x01;
        assert!(c.contains(&ip));
    }

    #[test]
    fn test_ipv6_cidr_not_contains() {
        let c = Ipv6Cidr::parse("2001:db8::/32").unwrap();
        let mut ip = [0u8; 16];
        ip[0] = 0x20;
        ip[1] = 0x02;
        assert!(!c.contains(&ip));
    }

    #[test]
    fn test_ipv6_cidr_zero_prefix_contains_all() {
        let c = Ipv6Cidr::parse("::/0").unwrap();
        assert!(c.contains(&[0xFF; 16]));
        assert!(c.contains(&[0; 16]));
    }

    #[test]
    fn test_ipv6_display() {
        let c = Ipv6Cidr::parse("::1/128").unwrap();
        let s = c.to_string();
        assert!(s.contains("/128"));
    }

    #[test]
    fn test_ipv6_full_address() {
        let c = Ipv6Cidr::parse("fe80:0:0:0:0:0:0:1/64").unwrap();
        assert_eq!(c.prefix_len, 64);
        assert_eq!(c.addr[0], 0xfe);
        assert_eq!(c.addr[1], 0x80);
        assert_eq!(c.addr[15], 1);
    }

    // -----------------------------------------------------------------------
    // Protocol parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_protocol_parse_tcp() {
        assert_eq!(Protocol::parse("tcp").unwrap(), Protocol::Tcp);
    }

    #[test]
    fn test_protocol_parse_udp() {
        assert_eq!(Protocol::parse("UDP").unwrap(), Protocol::Udp);
    }

    #[test]
    fn test_protocol_parse_icmp() {
        assert_eq!(Protocol::parse("icmp").unwrap(), Protocol::Icmp);
    }

    #[test]
    fn test_protocol_parse_all() {
        assert_eq!(Protocol::parse("all").unwrap(), Protocol::All);
    }

    #[test]
    fn test_protocol_parse_icmpv6() {
        assert_eq!(Protocol::parse("icmpv6").unwrap(), Protocol::Icmpv6);
    }

    #[test]
    fn test_protocol_parse_invalid() {
        assert!(Protocol::parse("sctp").is_err());
    }

    #[test]
    fn test_protocol_display() {
        assert_eq!(Protocol::Tcp.to_string(), "tcp");
        assert_eq!(Protocol::All.to_string(), "all");
    }

    // -----------------------------------------------------------------------
    // Port specification
    // -----------------------------------------------------------------------

    #[test]
    fn test_port_single() {
        let p = PortSpec::parse("80").unwrap();
        assert_eq!(p, PortSpec::Single(80));
        assert!(p.contains(80));
        assert!(!p.contains(81));
    }

    #[test]
    fn test_port_range() {
        let p = PortSpec::parse("1024:65535").unwrap();
        assert_eq!(p, PortSpec::Range(1024, 65535));
        assert!(p.contains(1024));
        assert!(p.contains(8080));
        assert!(p.contains(65535));
        assert!(!p.contains(80));
    }

    #[test]
    fn test_port_invalid() {
        assert!(PortSpec::parse("abc").is_err());
    }

    #[test]
    fn test_port_range_invalid_order() {
        assert!(PortSpec::parse("100:50").is_err());
    }

    #[test]
    fn test_port_display_single() {
        assert_eq!(PortSpec::Single(443).to_string(), "443");
    }

    #[test]
    fn test_port_display_range() {
        assert_eq!(PortSpec::Range(1000, 2000).to_string(), "1000:2000");
    }

    // -----------------------------------------------------------------------
    // MultiPort
    // -----------------------------------------------------------------------

    #[test]
    fn test_multiport_parse() {
        let mp = MultiPort::parse("80,443,8080").unwrap();
        assert_eq!(mp.0.len(), 3);
        assert!(mp.contains(80));
        assert!(mp.contains(443));
        assert!(mp.contains(8080));
        assert!(!mp.contains(81));
    }

    #[test]
    fn test_multiport_parse_with_ranges() {
        let mp = MultiPort::parse("80,1024:2048,443").unwrap();
        assert!(mp.contains(80));
        assert!(mp.contains(1500));
        assert!(mp.contains(443));
        assert!(!mp.contains(81));
    }

    #[test]
    fn test_multiport_display() {
        let mp = MultiPort::parse("80,443").unwrap();
        assert_eq!(mp.to_string(), "80,443");
    }

    // -----------------------------------------------------------------------
    // Connection state
    // -----------------------------------------------------------------------

    #[test]
    fn test_conn_state_parse() {
        assert_eq!(ConnState::parse("NEW").unwrap(), ConnState::New);
        assert_eq!(
            ConnState::parse("ESTABLISHED").unwrap(),
            ConnState::Established
        );
        assert_eq!(ConnState::parse("RELATED").unwrap(), ConnState::Related);
        assert_eq!(ConnState::parse("INVALID").unwrap(), ConnState::Invalid);
        assert_eq!(ConnState::parse("UNTRACKED").unwrap(), ConnState::Untracked);
    }

    #[test]
    fn test_conn_state_parse_case_insensitive() {
        assert_eq!(ConnState::parse("new").unwrap(), ConnState::New);
        assert_eq!(
            ConnState::parse("established").unwrap(),
            ConnState::Established
        );
    }

    #[test]
    fn test_conn_state_parse_invalid() {
        assert!(ConnState::parse("UNKNOWN").is_err());
    }

    #[test]
    fn test_parse_state_list() {
        let states = parse_state_list("NEW,ESTABLISHED,RELATED").unwrap();
        assert_eq!(states.len(), 3);
        assert_eq!(states[0], ConnState::New);
        assert_eq!(states[1], ConnState::Established);
        assert_eq!(states[2], ConnState::Related);
    }

    #[test]
    fn test_format_state_list() {
        let states = vec![ConnState::New, ConnState::Established];
        assert_eq!(format_state_list(&states), "NEW,ESTABLISHED");
    }

    // -----------------------------------------------------------------------
    // Limit specification
    // -----------------------------------------------------------------------

    #[test]
    fn test_limit_parse_rate_second() {
        let (r, u) = LimitSpec::parse_rate("5/sec").unwrap();
        assert_eq!(r, 5);
        assert_eq!(u, LimitUnit::Second);
    }

    #[test]
    fn test_limit_parse_rate_minute() {
        let (r, u) = LimitSpec::parse_rate("10/min").unwrap();
        assert_eq!(r, 10);
        assert_eq!(u, LimitUnit::Minute);
    }

    #[test]
    fn test_limit_parse_rate_hour() {
        let (r, u) = LimitSpec::parse_rate("100/hour").unwrap();
        assert_eq!(r, 100);
        assert_eq!(u, LimitUnit::Hour);
    }

    #[test]
    fn test_limit_parse_rate_day() {
        let (r, u) = LimitSpec::parse_rate("1000/day").unwrap();
        assert_eq!(r, 1000);
        assert_eq!(u, LimitUnit::Day);
    }

    #[test]
    fn test_limit_parse_rate_invalid() {
        assert!(LimitSpec::parse_rate("10").is_err());
        assert!(LimitSpec::parse_rate("10/xyz").is_err());
    }

    #[test]
    fn test_limit_display() {
        let spec = LimitSpec {
            rate: 5,
            unit: LimitUnit::Second,
            burst: 10,
        };
        assert_eq!(spec.to_string(), "5/sec burst 10");
    }

    // -----------------------------------------------------------------------
    // Table and chain structure
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_name_parse() {
        assert_eq!(TableName::parse("filter").unwrap(), TableName::Filter);
        assert_eq!(TableName::parse("nat").unwrap(), TableName::Nat);
        assert_eq!(TableName::parse("mangle").unwrap(), TableName::Mangle);
        assert_eq!(TableName::parse("raw").unwrap(), TableName::Raw);
    }

    #[test]
    fn test_table_name_parse_case_insensitive() {
        assert_eq!(TableName::parse("FILTER").unwrap(), TableName::Filter);
        assert_eq!(TableName::parse("NAT").unwrap(), TableName::Nat);
    }

    #[test]
    fn test_table_name_parse_invalid() {
        assert!(TableName::parse("security").is_err());
    }

    #[test]
    fn test_table_builtin_chains_filter() {
        assert_eq!(
            TableName::Filter.builtin_chains(),
            vec!["INPUT", "FORWARD", "OUTPUT"]
        );
    }

    #[test]
    fn test_table_builtin_chains_nat() {
        assert_eq!(
            TableName::Nat.builtin_chains(),
            vec!["PREROUTING", "INPUT", "OUTPUT", "POSTROUTING"]
        );
    }

    #[test]
    fn test_table_builtin_chains_mangle() {
        assert_eq!(
            TableName::Mangle.builtin_chains(),
            vec!["PREROUTING", "INPUT", "FORWARD", "OUTPUT", "POSTROUTING"]
        );
    }

    #[test]
    fn test_table_builtin_chains_raw() {
        assert_eq!(
            TableName::Raw.builtin_chains(),
            vec!["PREROUTING", "OUTPUT"]
        );
    }

    #[test]
    fn test_table_new() {
        let t = Table::new(TableName::Filter);
        assert_eq!(t.chains.len(), 3);
        assert!(t.find_chain("INPUT").is_some());
        assert!(t.find_chain("FORWARD").is_some());
        assert!(t.find_chain("OUTPUT").is_some());
    }

    #[test]
    fn test_table_find_chain_not_found() {
        let t = Table::new(TableName::Filter);
        assert!(t.find_chain("NONEXISTENT").is_none());
    }

    #[test]
    fn test_chain_builtin_has_policy() {
        let ch = Chain::new_builtin("INPUT", ChainPolicy::Accept);
        assert_eq!(ch.policy, Some(ChainPolicy::Accept));
        assert!(ch.builtin);
    }

    #[test]
    fn test_chain_user_no_policy() {
        let ch = Chain::new_user("MYCHAIN");
        assert_eq!(ch.policy, None);
        assert!(!ch.builtin);
    }

    #[test]
    fn test_chain_policy_parse() {
        assert_eq!(ChainPolicy::parse("ACCEPT").unwrap(), ChainPolicy::Accept);
        assert_eq!(ChainPolicy::parse("DROP").unwrap(), ChainPolicy::Drop);
        assert!(ChainPolicy::parse("REJECT").is_err());
    }

    // -----------------------------------------------------------------------
    // Firewall initialization
    // -----------------------------------------------------------------------

    #[test]
    fn test_firewall_new_ipv4() {
        let fw = Firewall::new(false);
        assert!(!fw.ipv6);
        assert_eq!(fw.tables.len(), 4);
    }

    #[test]
    fn test_firewall_new_ipv6() {
        let fw = Firewall::new(true);
        assert!(fw.ipv6);
        assert_eq!(fw.tables.len(), 4);
    }

    #[test]
    fn test_firewall_default_policies() {
        let fw = Firewall::new(false);
        let filter = fw.get_table(&TableName::Filter);
        for ch in &filter.chains {
            assert_eq!(ch.policy, Some(ChainPolicy::Accept));
        }
    }

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_iptables() {
        assert_eq!(Personality::detect("iptables"), Personality::Iptables);
    }

    #[test]
    fn test_personality_ip6tables() {
        assert_eq!(Personality::detect("ip6tables"), Personality::Ip6tables);
    }

    #[test]
    fn test_personality_iptables_save() {
        assert_eq!(
            Personality::detect("iptables-save"),
            Personality::IptablesSave
        );
    }

    #[test]
    fn test_personality_iptables_restore() {
        assert_eq!(
            Personality::detect("iptables-restore"),
            Personality::IptablesRestore
        );
    }

    #[test]
    fn test_personality_ip6tables_save() {
        assert_eq!(
            Personality::detect("ip6tables-save"),
            Personality::Ip6tablesSave
        );
    }

    #[test]
    fn test_personality_ip6tables_restore() {
        assert_eq!(
            Personality::detect("ip6tables-restore"),
            Personality::Ip6tablesRestore
        );
    }

    #[test]
    fn test_personality_unknown_defaults_iptables() {
        assert_eq!(Personality::detect("foo"), Personality::Iptables);
    }

    #[test]
    fn test_personality_is_ipv6() {
        assert!(!Personality::Iptables.is_ipv6());
        assert!(Personality::Ip6tables.is_ipv6());
        assert!(!Personality::IptablesSave.is_ipv6());
        assert!(Personality::Ip6tablesSave.is_ipv6());
    }

    #[test]
    fn test_personality_prog_name() {
        assert_eq!(Personality::Iptables.prog_name(), "iptables");
        assert_eq!(Personality::Ip6tables.prog_name(), "ip6tables");
    }

    // -----------------------------------------------------------------------
    // Append and list rules
    // -----------------------------------------------------------------------

    fn parse_and_exec(fw: &mut Firewall, args: &[&str]) -> Result<String, String> {
        let args_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut parser = ArgParser::new(args_vec, fw.ipv6);
        let cmd = parser.parse_command()?;
        execute_command(fw, cmd)
    }

    #[test]
    fn test_append_rule() {
        let mut fw = Firewall::new(false);
        let result = parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-p", "tcp", "--dport", "80", "-j", "ACCEPT"],
        );
        assert!(result.is_ok());
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 1);
        assert_eq!(input.rules[0].protocol, Some(Protocol::Tcp));
        assert_eq!(input.rules[0].dport, Some(PortSpec::Single(80)));
    }

    #[test]
    fn test_append_multiple_rules() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "udp", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 2);
        assert_eq!(input.rules[0].protocol, Some(Protocol::Tcp));
        assert_eq!(input.rules[1].protocol, Some(Protocol::Udp));
    }

    #[test]
    fn test_append_with_source() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-s", "192.168.1.0/24", "-j", "ACCEPT"],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert!(input.rules[0].source.is_some());
    }

    #[test]
    fn test_append_with_destination() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "OUTPUT", "-d", "10.0.0.0/8", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let output = filter.get_chain("OUTPUT").unwrap();
        assert!(output.rules[0].destination.is_some());
    }

    #[test]
    fn test_append_with_interface() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-i", "eth0", "-j", "ACCEPT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].in_interface, Some("eth0".to_string()));
    }

    #[test]
    fn test_append_to_nonexistent_chain() {
        let mut fw = Firewall::new(false);
        let result = parse_and_exec(&mut fw, &["-A", "NONEXISTENT", "-j", "ACCEPT"]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Insert rules
    // -----------------------------------------------------------------------

    #[test]
    fn test_insert_at_beginning() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-I", "INPUT", "-p", "udp", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 2);
        assert_eq!(input.rules[0].protocol, Some(Protocol::Udp));
        assert_eq!(input.rules[1].protocol, Some(Protocol::Tcp));
    }

    #[test]
    fn test_insert_at_position() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "udp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-I", "INPUT", "2", "-p", "icmp", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 3);
        assert_eq!(input.rules[1].protocol, Some(Protocol::Icmp));
    }

    #[test]
    fn test_insert_at_end() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-I", "INPUT", "2", "-p", "udp", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 2);
        assert_eq!(input.rules[1].protocol, Some(Protocol::Udp));
    }

    // -----------------------------------------------------------------------
    // Delete rules
    // -----------------------------------------------------------------------

    #[test]
    fn test_delete_by_number() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "udp", "-j", "DROP"]).unwrap();
        parse_and_exec(&mut fw, &["-D", "INPUT", "1"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 1);
        assert_eq!(input.rules[0].protocol, Some(Protocol::Udp));
    }

    #[test]
    fn test_delete_by_spec() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-D", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 0);
    }

    #[test]
    fn test_delete_nonexistent_number() {
        let mut fw = Firewall::new(false);
        let result = parse_and_exec(&mut fw, &["-D", "INPUT", "1"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_nonexistent_spec() {
        let mut fw = Firewall::new(false);
        let result = parse_and_exec(&mut fw, &["-D", "INPUT", "-p", "tcp", "-j", "ACCEPT"]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Replace rules
    // -----------------------------------------------------------------------

    #[test]
    fn test_replace_rule() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-R", "INPUT", "1", "-p", "udp", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].protocol, Some(Protocol::Udp));
    }

    #[test]
    fn test_replace_invalid_position() {
        let mut fw = Firewall::new(false);
        let result = parse_and_exec(&mut fw, &["-R", "INPUT", "1", "-p", "tcp", "-j", "ACCEPT"]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Flush
    // -----------------------------------------------------------------------

    #[test]
    fn test_flush_chain() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "udp", "-j", "DROP"]).unwrap();
        parse_and_exec(&mut fw, &["-F", "INPUT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 0);
    }

    #[test]
    fn test_flush_all() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "OUTPUT", "-p", "udp", "-j", "DROP"]).unwrap();
        parse_and_exec(&mut fw, &["-F"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        for ch in &filter.chains {
            assert_eq!(ch.rules.len(), 0);
        }
    }

    // -----------------------------------------------------------------------
    // Zero counters
    // -----------------------------------------------------------------------

    #[test]
    fn test_zero_chain_counters() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        {
            let tbl = fw.get_table_mut(&TableName::Filter);
            let ch = tbl.get_chain_mut("INPUT").unwrap();
            ch.rules[0].packets = 100;
            ch.rules[0].bytes = 5000;
            ch.chain_packets = 200;
            ch.chain_bytes = 10000;
        }
        parse_and_exec(&mut fw, &["-Z", "INPUT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.chain_packets, 0);
        assert_eq!(input.chain_bytes, 0);
        assert_eq!(input.rules[0].packets, 0);
        assert_eq!(input.rules[0].bytes, 0);
    }

    #[test]
    fn test_zero_all_counters() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "ACCEPT"]).unwrap();
        {
            let tbl = fw.get_table_mut(&TableName::Filter);
            let ch = tbl.get_chain_mut("INPUT").unwrap();
            ch.rules[0].packets = 50;
        }
        parse_and_exec(&mut fw, &["-Z"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].packets, 0);
    }

    // -----------------------------------------------------------------------
    // User-defined chains
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_chain() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        assert!(filter.find_chain("MYCHAIN").is_some());
        let ch = filter.get_chain("MYCHAIN").unwrap();
        assert!(!ch.builtin);
        assert_eq!(ch.policy, None);
    }

    #[test]
    fn test_new_chain_duplicate() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        let result = parse_and_exec(&mut fw, &["-N", "MYCHAIN"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_user_chain() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        parse_and_exec(&mut fw, &["-X", "MYCHAIN"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        assert!(filter.find_chain("MYCHAIN").is_none());
    }

    #[test]
    fn test_delete_builtin_chain_fails() {
        let mut fw = Firewall::new(false);
        let result = parse_and_exec(&mut fw, &["-X", "INPUT"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_nonempty_chain_fails() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "MYCHAIN", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        let result = parse_and_exec(&mut fw, &["-X", "MYCHAIN"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_referenced_chain_fails() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "MYCHAIN"]).unwrap();
        let result = parse_and_exec(&mut fw, &["-X", "MYCHAIN"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_all_empty_user_chains() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "CHAIN1"]).unwrap();
        parse_and_exec(&mut fw, &["-N", "CHAIN2"]).unwrap();
        parse_and_exec(&mut fw, &["-X"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        assert!(filter.find_chain("CHAIN1").is_none());
        assert!(filter.find_chain("CHAIN2").is_none());
        // Built-in chains remain
        assert!(filter.find_chain("INPUT").is_some());
    }

    // -----------------------------------------------------------------------
    // Policy
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_policy_drop() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-P", "INPUT", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.policy, Some(ChainPolicy::Drop));
    }

    #[test]
    fn test_set_policy_accept() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-P", "INPUT", "DROP"]).unwrap();
        parse_and_exec(&mut fw, &["-P", "INPUT", "ACCEPT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.policy, Some(ChainPolicy::Accept));
    }

    #[test]
    fn test_set_policy_user_chain_fails() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        let result = parse_and_exec(&mut fw, &["-P", "MYCHAIN", "DROP"]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Rename chain
    // -----------------------------------------------------------------------

    #[test]
    fn test_rename_chain() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "OLD"]).unwrap();
        parse_and_exec(&mut fw, &["-E", "OLD", "NEW"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        assert!(filter.find_chain("OLD").is_none());
        assert!(filter.find_chain("NEW").is_some());
    }

    #[test]
    fn test_rename_updates_jump_targets() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "OLD"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "OLD"]).unwrap();
        parse_and_exec(&mut fw, &["-E", "OLD", "NEW"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(
            input.rules[0].target,
            Some(Target::UserChain("NEW".to_string()))
        );
    }

    #[test]
    fn test_rename_builtin_fails() {
        let mut fw = Firewall::new(false);
        let result = parse_and_exec(&mut fw, &["-E", "INPUT", "NEWINPUT"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_to_existing_fails() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "CHAIN1"]).unwrap();
        parse_and_exec(&mut fw, &["-N", "CHAIN2"]).unwrap();
        let result = parse_and_exec(&mut fw, &["-E", "CHAIN1", "CHAIN2"]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Check rule
    // -----------------------------------------------------------------------

    #[test]
    fn test_check_existing_rule() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-p", "tcp", "--dport", "80", "-j", "ACCEPT"],
        )
        .unwrap();
        let result = parse_and_exec(
            &mut fw,
            &["-C", "INPUT", "-p", "tcp", "--dport", "80", "-j", "ACCEPT"],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_nonexistent_rule() {
        let mut fw = Firewall::new(false);
        let result = parse_and_exec(
            &mut fw,
            &["-C", "INPUT", "-p", "tcp", "--dport", "80", "-j", "ACCEPT"],
        );
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Table specification
    // -----------------------------------------------------------------------

    #[test]
    fn test_nat_table_append() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-t", "nat", "-A", "POSTROUTING", "-j", "MASQUERADE"],
        )
        .unwrap();
        let nat = fw.get_table(&TableName::Nat);
        let post = nat.get_chain("POSTROUTING").unwrap();
        assert_eq!(post.rules.len(), 1);
    }

    #[test]
    fn test_mangle_table() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-t", "mangle", "-A", "PREROUTING", "-j", "ACCEPT"],
        )
        .unwrap();
        let mangle = fw.get_table(&TableName::Mangle);
        let pre = mangle.get_chain("PREROUTING").unwrap();
        assert_eq!(pre.rules.len(), 1);
    }

    #[test]
    fn test_raw_table() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-t", "raw", "-A", "PREROUTING", "-j", "ACCEPT"]).unwrap();
        let raw = fw.get_table(&TableName::Raw);
        let pre = raw.get_chain("PREROUTING").unwrap();
        assert_eq!(pre.rules.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Targets
    // -----------------------------------------------------------------------

    #[test]
    fn test_target_accept() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "ACCEPT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].target, Some(Target::Accept));
    }

    #[test]
    fn test_target_drop() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].target, Some(Target::Drop));
    }

    #[test]
    fn test_target_reject() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "REJECT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(
            input.rules[0].target,
            Some(Target::Reject { reject_with: None })
        );
    }

    #[test]
    fn test_target_reject_with() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-j",
                "REJECT",
                "--reject-with",
                "icmp-port-unreachable",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(
            input.rules[0].target,
            Some(Target::Reject {
                reject_with: Some("icmp-port-unreachable".to_string())
            })
        );
    }

    #[test]
    fn test_target_log() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-j",
                "LOG",
                "--log-prefix",
                "DROPPED: ",
                "--log-level",
                "4",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(
            input.rules[0].target,
            Some(Target::Log {
                prefix: Some("DROPPED: ".to_string()),
                level: Some("4".to_string()),
            })
        );
    }

    #[test]
    fn test_target_snat() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-t",
                "nat",
                "-A",
                "POSTROUTING",
                "-j",
                "SNAT",
                "--to-source",
                "1.2.3.4",
            ],
        )
        .unwrap();
        let nat = fw.get_table(&TableName::Nat);
        let post = nat.get_chain("POSTROUTING").unwrap();
        assert_eq!(
            post.rules[0].target,
            Some(Target::Snat {
                to_source: "1.2.3.4".to_string()
            })
        );
    }

    #[test]
    fn test_target_dnat() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-t",
                "nat",
                "-A",
                "PREROUTING",
                "-j",
                "DNAT",
                "--to-destination",
                "192.168.1.1:8080",
            ],
        )
        .unwrap();
        let nat = fw.get_table(&TableName::Nat);
        let pre = nat.get_chain("PREROUTING").unwrap();
        assert_eq!(
            pre.rules[0].target,
            Some(Target::Dnat {
                to_destination: "192.168.1.1:8080".to_string()
            })
        );
    }

    #[test]
    fn test_target_masquerade() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-t", "nat", "-A", "POSTROUTING", "-j", "MASQUERADE"],
        )
        .unwrap();
        let nat = fw.get_table(&TableName::Nat);
        let post = nat.get_chain("POSTROUTING").unwrap();
        assert_eq!(post.rules[0].target, Some(Target::Masquerade));
    }

    #[test]
    fn test_target_redirect() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-t",
                "nat",
                "-A",
                "PREROUTING",
                "-j",
                "REDIRECT",
                "--to-ports",
                "8080",
            ],
        )
        .unwrap();
        let nat = fw.get_table(&TableName::Nat);
        let pre = nat.get_chain("PREROUTING").unwrap();
        assert_eq!(
            pre.rules[0].target,
            Some(Target::Redirect {
                to_ports: Some(PortSpec::Single(8080))
            })
        );
    }

    #[test]
    fn test_target_return() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "MYCHAIN", "-j", "RETURN"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let ch = filter.get_chain("MYCHAIN").unwrap();
        assert_eq!(ch.rules[0].target, Some(Target::Return));
    }

    #[test]
    fn test_target_user_chain() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "MYCHAIN"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(
            input.rules[0].target,
            Some(Target::UserChain("MYCHAIN".to_string()))
        );
    }

    #[test]
    fn test_target_display() {
        assert_eq!(Target::Accept.to_string(), "ACCEPT");
        assert_eq!(Target::Drop.to_string(), "DROP");
        assert_eq!(Target::Masquerade.to_string(), "MASQUERADE");
        assert_eq!(Target::Return.to_string(), "RETURN");
        assert_eq!(
            Target::Snat {
                to_source: "1.2.3.4".to_string()
            }
            .to_string(),
            "SNAT --to-source 1.2.3.4"
        );
    }

    #[test]
    fn test_target_name() {
        assert_eq!(Target::Accept.name(), "ACCEPT");
        assert_eq!(Target::Drop.name(), "DROP");
        assert_eq!(Target::UserChain("FOO".to_string()).name(), "FOO");
    }

    // -----------------------------------------------------------------------
    // Negation
    // -----------------------------------------------------------------------

    #[test]
    fn test_negation_source() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "!", "-s", "10.0.0.0/8", "-j", "ACCEPT"],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert!(input.rules[0].not_source);
    }

    #[test]
    fn test_negation_destination() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "!", "-d", "10.0.0.0/8", "-j", "DROP"],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert!(input.rules[0].not_destination);
    }

    #[test]
    fn test_negation_protocol() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "!", "-p", "tcp", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert!(input.rules[0].not_protocol);
    }

    #[test]
    fn test_negation_interface() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "!", "-i", "lo", "-j", "DROP"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert!(input.rules[0].not_in_interface);
    }

    #[test]
    fn test_negation_dport() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A", "INPUT", "-p", "tcp", "!", "--dport", "22", "-j", "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert!(input.rules[0].not_dport);
    }

    // -----------------------------------------------------------------------
    // Match extensions
    // -----------------------------------------------------------------------

    #[test]
    fn test_match_state() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-m",
                "state",
                "--state",
                "NEW,ESTABLISHED",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].match_extensions.len(), 1);
        if let MatchExt::State(ref states) = input.rules[0].match_extensions[0] {
            assert_eq!(states.len(), 2);
            assert_eq!(states[0], ConnState::New);
            assert_eq!(states[1], ConnState::Established);
        } else {
            panic!("expected State match extension");
        }
    }

    #[test]
    fn test_match_conntrack() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-m",
                "conntrack",
                "--ctstate",
                "RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        if let MatchExt::ConnTrack(ref states) = input.rules[0].match_extensions[0] {
            assert_eq!(states.len(), 2);
        } else {
            panic!("expected ConnTrack match extension");
        }
    }

    #[test]
    fn test_match_multiport_dports() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-p",
                "tcp",
                "-m",
                "multiport",
                "--dports",
                "80,443,8080",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        if let MatchExt::MultiDport(ref mp) = input.rules[0].match_extensions[0] {
            assert_eq!(mp.0.len(), 3);
            assert!(mp.contains(80));
            assert!(mp.contains(443));
        } else {
            panic!("expected MultiDport");
        }
    }

    #[test]
    fn test_match_multiport_sports() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-p",
                "tcp",
                "-m",
                "multiport",
                "--sports",
                "1024:65535",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        if let MatchExt::MultiSport(ref mp) = input.rules[0].match_extensions[0] {
            assert_eq!(mp.0.len(), 1);
        } else {
            panic!("expected MultiSport");
        }
    }

    #[test]
    fn test_match_limit() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-m",
                "limit",
                "--limit",
                "5/sec",
                "--limit-burst",
                "10",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        if let MatchExt::Limit(ref spec) = input.rules[0].match_extensions[0] {
            assert_eq!(spec.rate, 5);
            assert_eq!(spec.unit, LimitUnit::Second);
            assert_eq!(spec.burst, 10);
        } else {
            panic!("expected Limit");
        }
    }

    #[test]
    fn test_match_comment() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-m",
                "comment",
                "--comment",
                "allow web traffic",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        if let MatchExt::Comment(ref text) = input.rules[0].match_extensions[0] {
            assert_eq!(text, "allow web traffic");
        } else {
            panic!("expected Comment");
        }
    }

    #[test]
    fn test_match_ext_display() {
        let ext = MatchExt::State(vec![ConnState::New, ConnState::Established]);
        assert_eq!(ext.to_string(), "-m state --state NEW,ESTABLISHED");
    }

    #[test]
    fn test_match_conntrack_display() {
        let ext = MatchExt::ConnTrack(vec![ConnState::Related]);
        assert_eq!(ext.to_string(), "-m conntrack --ctstate RELATED");
    }

    // -----------------------------------------------------------------------
    // Port ranges and matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_sport_rule() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-p",
                "tcp",
                "--sport",
                "1024:65535",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].sport, Some(PortSpec::Range(1024, 65535)));
    }

    #[test]
    fn test_dport_rule() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-p", "tcp", "--dport", "443", "-j", "ACCEPT"],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].dport, Some(PortSpec::Single(443)));
    }

    // -----------------------------------------------------------------------
    // List output
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_empty_chain() {
        let mut fw = Firewall::new(false);
        let output = parse_and_exec(&mut fw, &["-L", "INPUT"]).unwrap();
        assert!(output.contains("Chain INPUT"));
        assert!(output.contains("policy ACCEPT"));
    }

    #[test]
    fn test_list_with_rules() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-p", "tcp", "--dport", "80", "-j", "ACCEPT"],
        )
        .unwrap();
        let output = parse_and_exec(&mut fw, &["-L", "INPUT"]).unwrap();
        assert!(output.contains("ACCEPT"));
        assert!(output.contains("tcp"));
    }

    #[test]
    fn test_list_verbose() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-p", "tcp", "-j", "ACCEPT"]).unwrap();
        let output = parse_and_exec(&mut fw, &["-L", "INPUT", "-v"]).unwrap();
        assert!(output.contains("pkts"));
        assert!(output.contains("bytes"));
    }

    #[test]
    fn test_list_with_line_numbers() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "ACCEPT"]).unwrap();
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "DROP"]).unwrap();
        let output = parse_and_exec(&mut fw, &["-L", "INPUT", "--line-numbers"]).unwrap();
        assert!(output.contains("1"));
        assert!(output.contains("2"));
    }

    #[test]
    fn test_list_numeric() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-s", "192.168.1.0/24", "-j", "ACCEPT"],
        )
        .unwrap();
        let output = parse_and_exec(&mut fw, &["-L", "INPUT", "-n"]).unwrap();
        assert!(output.contains("192.168.1.0/24"));
    }

    #[test]
    fn test_list_all_chains() {
        let mut fw = Firewall::new(false);
        let output = parse_and_exec(&mut fw, &["-L"]).unwrap();
        assert!(output.contains("Chain INPUT"));
        assert!(output.contains("Chain FORWARD"));
        assert!(output.contains("Chain OUTPUT"));
    }

    // -----------------------------------------------------------------------
    // Save/restore format
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_empty_firewall() {
        let fw = Firewall::new(false);
        let output = save_firewall(&fw);
        assert!(output.contains("*filter"));
        assert!(output.contains("*nat"));
        assert!(output.contains("*mangle"));
        assert!(output.contains("*raw"));
        assert!(output.contains("COMMIT"));
    }

    #[test]
    fn test_save_with_rules() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-p", "tcp", "--dport", "80", "-j", "ACCEPT"],
        )
        .unwrap();
        let output = save_firewall(&fw);
        assert!(output.contains("-A INPUT -p tcp --dport 80 -j ACCEPT"));
    }

    #[test]
    fn test_save_with_policy() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-P", "INPUT", "DROP"]).unwrap();
        let output = save_firewall(&fw);
        assert!(output.contains(":INPUT DROP [0:0]"));
    }

    #[test]
    fn test_save_with_counters() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "ACCEPT"]).unwrap();
        {
            let tbl = fw.get_table_mut(&TableName::Filter);
            let ch = tbl.get_chain_mut("INPUT").unwrap();
            ch.rules[0].packets = 100;
            ch.rules[0].bytes = 5000;
        }
        let output = save_firewall(&fw);
        assert!(output.contains("[100:5000]"));
    }

    #[test]
    fn test_save_user_chain() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        let output = save_firewall(&fw);
        assert!(output.contains(":MYCHAIN - [0:0]"));
    }

    #[test]
    fn test_restore_basic() {
        let mut fw = Firewall::new(false);
        let input = "*filter\n\
                     :INPUT ACCEPT [0:0]\n\
                     :FORWARD ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\
                     -A INPUT -p tcp --dport 80 -j ACCEPT\n\
                     COMMIT\n";
        restore_firewall(&mut fw, input).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input_chain = filter.get_chain("INPUT").unwrap();
        assert_eq!(input_chain.rules.len(), 1);
        assert_eq!(input_chain.rules[0].protocol, Some(Protocol::Tcp));
    }

    #[test]
    fn test_restore_with_counters() {
        let mut fw = Firewall::new(false);
        let input = "*filter\n\
                     :INPUT ACCEPT [0:0]\n\
                     :FORWARD ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\
                     [100:5000] -A INPUT -p tcp -j ACCEPT\n\
                     COMMIT\n";
        restore_firewall(&mut fw, input).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input_chain = filter.get_chain("INPUT").unwrap();
        assert_eq!(input_chain.rules[0].packets, 100);
        assert_eq!(input_chain.rules[0].bytes, 5000);
    }

    #[test]
    fn test_restore_with_policy() {
        let mut fw = Firewall::new(false);
        let input = "*filter\n\
                     :INPUT DROP [0:0]\n\
                     :FORWARD ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\
                     COMMIT\n";
        restore_firewall(&mut fw, input).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input_chain = filter.get_chain("INPUT").unwrap();
        assert_eq!(input_chain.policy, Some(ChainPolicy::Drop));
    }

    #[test]
    fn test_restore_user_chain() {
        let mut fw = Firewall::new(false);
        let input = "*filter\n\
                     :INPUT ACCEPT [0:0]\n\
                     :FORWARD ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\
                     :MYCHAIN - [0:0]\n\
                     -A MYCHAIN -p tcp -j ACCEPT\n\
                     COMMIT\n";
        restore_firewall(&mut fw, input).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let ch = filter.get_chain("MYCHAIN").unwrap();
        assert_eq!(ch.rules.len(), 1);
        assert!(!ch.builtin);
    }

    #[test]
    fn test_restore_comments_ignored() {
        let mut fw = Firewall::new(false);
        let input = "# Generated by iptables-save\n\
                     *filter\n\
                     :INPUT ACCEPT [0:0]\n\
                     :FORWARD ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\
                     # This is a comment\n\
                     COMMIT\n";
        assert!(restore_firewall(&mut fw, input).is_ok());
    }

    #[test]
    fn test_save_restore_roundtrip() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-p", "tcp", "--dport", "22", "-j", "ACCEPT"],
        )
        .unwrap();
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-p", "tcp", "--dport", "80", "-j", "ACCEPT"],
        )
        .unwrap();
        parse_and_exec(&mut fw, &["-P", "INPUT", "DROP"]).unwrap();
        parse_and_exec(
            &mut fw,
            &["-t", "nat", "-A", "POSTROUTING", "-j", "MASQUERADE"],
        )
        .unwrap();

        let saved = save_firewall(&fw);

        let mut fw2 = Firewall::new(false);
        restore_firewall(&mut fw2, &saved).unwrap();

        let filter = fw2.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules.len(), 2);
        assert_eq!(input.policy, Some(ChainPolicy::Drop));

        let nat = fw2.get_table(&TableName::Nat);
        let post = nat.get_chain("POSTROUTING").unwrap();
        assert_eq!(post.rules.len(), 1);
    }

    #[test]
    fn test_restore_multiple_tables() {
        let mut fw = Firewall::new(false);
        let input = "*filter\n\
                     :INPUT ACCEPT [0:0]\n\
                     :FORWARD ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\
                     -A INPUT -j ACCEPT\n\
                     COMMIT\n\
                     *nat\n\
                     :PREROUTING ACCEPT [0:0]\n\
                     :INPUT ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\
                     :POSTROUTING ACCEPT [0:0]\n\
                     -A POSTROUTING -j MASQUERADE\n\
                     COMMIT\n";
        restore_firewall(&mut fw, input).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        assert_eq!(filter.get_chain("INPUT").unwrap().rules.len(), 1);
        let nat = fw.get_table(&TableName::Nat);
        assert_eq!(nat.get_chain("POSTROUTING").unwrap().rules.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Counter tracking
    // -----------------------------------------------------------------------

    #[test]
    fn test_counter_init_zero() {
        let rule = Rule::new();
        assert_eq!(rule.packets, 0);
        assert_eq!(rule.bytes, 0);
    }

    #[test]
    fn test_set_counters_via_parse() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-c", "50", "1000", "-j", "ACCEPT"],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].packets, 50);
        assert_eq!(input.rules[0].bytes, 1000);
    }

    #[test]
    fn test_chain_counters() {
        let ch = Chain::new_builtin("INPUT", ChainPolicy::Accept);
        assert_eq!(ch.chain_packets, 0);
        assert_eq!(ch.chain_bytes, 0);
    }

    // -----------------------------------------------------------------------
    // Rule spec matching
    // -----------------------------------------------------------------------

    #[test]
    fn test_rule_matches_spec_identical() {
        let mut r1 = Rule::new();
        r1.protocol = Some(Protocol::Tcp);
        r1.dport = Some(PortSpec::Single(80));
        r1.target = Some(Target::Accept);

        let mut r2 = Rule::new();
        r2.protocol = Some(Protocol::Tcp);
        r2.dport = Some(PortSpec::Single(80));
        r2.target = Some(Target::Accept);

        assert!(r1.matches_spec(&r2));
    }

    #[test]
    fn test_rule_matches_spec_different() {
        let mut r1 = Rule::new();
        r1.protocol = Some(Protocol::Tcp);
        r1.target = Some(Target::Accept);

        let mut r2 = Rule::new();
        r2.protocol = Some(Protocol::Udp);
        r2.target = Some(Target::Accept);

        assert!(!r1.matches_spec(&r2));
    }

    #[test]
    fn test_rule_matches_spec_ignores_counters() {
        let mut r1 = Rule::new();
        r1.protocol = Some(Protocol::Tcp);
        r1.target = Some(Target::Accept);
        r1.packets = 100;
        r1.bytes = 5000;

        let mut r2 = Rule::new();
        r2.protocol = Some(Protocol::Tcp);
        r2.target = Some(Target::Accept);
        r2.packets = 0;
        r2.bytes = 0;

        assert!(r1.matches_spec(&r2));
    }

    // -----------------------------------------------------------------------
    // Rule to_args_string
    // -----------------------------------------------------------------------

    #[test]
    fn test_rule_to_args_basic() {
        let mut rule = Rule::new();
        rule.protocol = Some(Protocol::Tcp);
        rule.dport = Some(PortSpec::Single(80));
        rule.target = Some(Target::Accept);
        let s = rule.to_args_string();
        assert!(s.contains("-p tcp"));
        assert!(s.contains("--dport 80"));
        assert!(s.contains("-j ACCEPT"));
    }

    #[test]
    fn test_rule_to_args_with_negation() {
        let mut rule = Rule::new();
        rule.not_source = true;
        rule.source = Some(AddrCidr::V4(Ipv4Cidr::parse("10.0.0.0/8").unwrap()));
        rule.target = Some(Target::Drop);
        let s = rule.to_args_string();
        assert!(s.contains("! -s 10.0.0.0/8"));
    }

    #[test]
    fn test_rule_to_args_with_extensions() {
        let mut rule = Rule::new();
        rule.match_extensions
            .push(MatchExt::State(vec![ConnState::New]));
        rule.target = Some(Target::Accept);
        let s = rule.to_args_string();
        assert!(s.contains("-m state --state NEW"));
    }

    // -----------------------------------------------------------------------
    // Shell splitting
    // -----------------------------------------------------------------------

    #[test]
    fn test_shell_split_basic() {
        let words = shell_split("hello world").unwrap();
        assert_eq!(words, vec!["hello", "world"]);
    }

    #[test]
    fn test_shell_split_quoted() {
        let words = shell_split("-m comment --comment \"hello world\"").unwrap();
        assert_eq!(words.len(), 4);
        assert_eq!(words[3], "hello world");
    }

    #[test]
    fn test_shell_split_empty() {
        let words = shell_split("").unwrap();
        assert_eq!(words.len(), 0);
    }

    #[test]
    fn test_shell_split_unterminated_quote() {
        assert!(shell_split("\"unterminated").is_err());
    }

    #[test]
    fn test_shell_split_multiple_spaces() {
        let words = shell_split("a   b   c").unwrap();
        assert_eq!(words, vec!["a", "b", "c"]);
    }

    // -----------------------------------------------------------------------
    // Counter bracket parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_counter_bracket() {
        let (p, b) = parse_counter_bracket("[100:5000]").unwrap();
        assert_eq!(p, 100);
        assert_eq!(b, 5000);
    }

    #[test]
    fn test_parse_counter_bracket_zero() {
        let (p, b) = parse_counter_bracket("[0:0]").unwrap();
        assert_eq!(p, 0);
        assert_eq!(b, 0);
    }

    #[test]
    fn test_parse_counter_bracket_invalid() {
        assert!(parse_counter_bracket("[abc:def]").is_err());
    }

    // -----------------------------------------------------------------------
    // Complex scenarios
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_firewall_setup() {
        let mut fw = Firewall::new(false);
        // Set policies
        parse_and_exec(&mut fw, &["-P", "INPUT", "DROP"]).unwrap();
        parse_and_exec(&mut fw, &["-P", "FORWARD", "DROP"]).unwrap();
        // Allow loopback
        parse_and_exec(&mut fw, &["-A", "INPUT", "-i", "lo", "-j", "ACCEPT"]).unwrap();
        // Allow established
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-m",
                "state",
                "--state",
                "ESTABLISHED,RELATED",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        // Allow SSH
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-p", "tcp", "--dport", "22", "-j", "ACCEPT"],
        )
        .unwrap();
        // Allow HTTP/HTTPS
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-p",
                "tcp",
                "-m",
                "multiport",
                "--dports",
                "80,443",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        // NAT
        parse_and_exec(
            &mut fw,
            &[
                "-t",
                "nat",
                "-A",
                "POSTROUTING",
                "-o",
                "eth0",
                "-j",
                "MASQUERADE",
            ],
        )
        .unwrap();

        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.policy, Some(ChainPolicy::Drop));
        assert_eq!(input.rules.len(), 4);

        let nat = fw.get_table(&TableName::Nat);
        let post = nat.get_chain("POSTROUTING").unwrap();
        assert_eq!(post.rules.len(), 1);
    }

    #[test]
    fn test_user_chain_workflow() {
        let mut fw = Firewall::new(false);
        // Create chain
        parse_and_exec(&mut fw, &["-N", "LOGGING"]).unwrap();
        // Add rules to it
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "LOGGING",
                "-j",
                "LOG",
                "--log-prefix",
                "IPTables-Dropped: ",
            ],
        )
        .unwrap();
        parse_and_exec(&mut fw, &["-A", "LOGGING", "-j", "DROP"]).unwrap();
        // Jump from INPUT
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "LOGGING"]).unwrap();

        let filter = fw.get_table(&TableName::Filter);
        let logging = filter.get_chain("LOGGING").unwrap();
        assert_eq!(logging.rules.len(), 2);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(
            input.rules[0].target,
            Some(Target::UserChain("LOGGING".to_string()))
        );
    }

    #[test]
    fn test_dnat_port_forwarding() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-t",
                "nat",
                "-A",
                "PREROUTING",
                "-p",
                "tcp",
                "--dport",
                "80",
                "-j",
                "DNAT",
                "--to-destination",
                "192.168.1.100:8080",
            ],
        )
        .unwrap();
        let nat = fw.get_table(&TableName::Nat);
        let pre = nat.get_chain("PREROUTING").unwrap();
        assert_eq!(pre.rules.len(), 1);
        assert_eq!(
            pre.rules[0].target,
            Some(Target::Dnat {
                to_destination: "192.168.1.100:8080".to_string()
            })
        );
    }

    #[test]
    fn test_redirect_rule() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-t",
                "nat",
                "-A",
                "PREROUTING",
                "-p",
                "tcp",
                "--dport",
                "80",
                "-j",
                "REDIRECT",
                "--to-ports",
                "8080",
            ],
        )
        .unwrap();
        let nat = fw.get_table(&TableName::Nat);
        let pre = nat.get_chain("PREROUTING").unwrap();
        assert_eq!(
            pre.rules[0].target,
            Some(Target::Redirect {
                to_ports: Some(PortSpec::Single(8080))
            })
        );
    }

    #[test]
    fn test_out_interface_rule() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "OUTPUT", "-o", "eth0", "-j", "ACCEPT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let output = filter.get_chain("OUTPUT").unwrap();
        assert_eq!(output.rules[0].out_interface, Some("eth0".to_string()));
    }

    #[test]
    fn test_combined_sport_dport() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &[
                "-A",
                "INPUT",
                "-p",
                "tcp",
                "--sport",
                "1024:65535",
                "--dport",
                "22",
                "-j",
                "ACCEPT",
            ],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(input.rules[0].sport, Some(PortSpec::Range(1024, 65535)));
        assert_eq!(input.rules[0].dport, Some(PortSpec::Single(22)));
    }

    #[test]
    fn test_ipv6_source_address() {
        let mut fw = Firewall::new(true);
        parse_and_exec(
            &mut fw,
            &["-A", "INPUT", "-s", "2001:db8::/32", "-j", "ACCEPT"],
        )
        .unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert!(input.rules[0].source.is_some());
    }

    #[test]
    fn test_table_display() {
        assert_eq!(TableName::Filter.to_string(), "filter");
        assert_eq!(TableName::Nat.to_string(), "nat");
        assert_eq!(TableName::Mangle.to_string(), "mangle");
        assert_eq!(TableName::Raw.to_string(), "raw");
    }

    #[test]
    fn test_addr_cidr_display_v4() {
        let c = AddrCidr::parse_v4("10.0.0.0/8").unwrap();
        assert_eq!(c.to_string(), "10.0.0.0/8");
    }

    #[test]
    fn test_chain_policy_display() {
        assert_eq!(ChainPolicy::Accept.to_string(), "ACCEPT");
        assert_eq!(ChainPolicy::Drop.to_string(), "DROP");
    }

    #[test]
    fn test_redirect_without_ports() {
        let mut fw = Firewall::new(false);
        parse_and_exec(
            &mut fw,
            &["-t", "nat", "-A", "PREROUTING", "-j", "REDIRECT"],
        )
        .unwrap();
        let nat = fw.get_table(&TableName::Nat);
        let pre = nat.get_chain("PREROUTING").unwrap();
        assert_eq!(
            pre.rules[0].target,
            Some(Target::Redirect { to_ports: None })
        );
    }

    #[test]
    fn test_log_without_options() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "LOG"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        assert_eq!(
            input.rules[0].target,
            Some(Target::Log {
                prefix: None,
                level: None,
            })
        );
    }

    #[test]
    fn test_limit_defaults() {
        let mut fw = Firewall::new(false);
        // Default limit: 3/hour, burst 5
        parse_and_exec(&mut fw, &["-A", "INPUT", "-m", "limit", "-j", "ACCEPT"]).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input = filter.get_chain("INPUT").unwrap();
        if let MatchExt::Limit(ref spec) = input.rules[0].match_extensions[0] {
            assert_eq!(spec.rate, 3);
            assert_eq!(spec.unit, LimitUnit::Hour);
            assert_eq!(spec.burst, 5);
        } else {
            panic!("expected Limit");
        }
    }

    #[test]
    fn test_restore_chain_counters() {
        let mut fw = Firewall::new(false);
        let input = "*filter\n\
                     :INPUT ACCEPT [1000:50000]\n\
                     :FORWARD ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\
                     COMMIT\n";
        restore_firewall(&mut fw, input).unwrap();
        let filter = fw.get_table(&TableName::Filter);
        let input_chain = filter.get_chain("INPUT").unwrap();
        assert_eq!(input_chain.chain_packets, 1000);
        assert_eq!(input_chain.chain_bytes, 50000);
    }

    #[test]
    fn test_restore_empty_lines_ignored() {
        let mut fw = Firewall::new(false);
        let input = "\n\n*filter\n\n:INPUT ACCEPT [0:0]\n\
                     :FORWARD ACCEPT [0:0]\n\
                     :OUTPUT ACCEPT [0:0]\n\n\
                     COMMIT\n\n";
        assert!(restore_firewall(&mut fw, input).is_ok());
    }

    #[test]
    fn test_multiport_with_range_display() {
        let mp = MultiPort::parse("80,1024:2048,443").unwrap();
        assert_eq!(mp.to_string(), "80,1024:2048,443");
    }

    #[test]
    fn test_match_ext_limit_display() {
        let ext = MatchExt::Limit(LimitSpec {
            rate: 10,
            unit: LimitUnit::Minute,
            burst: 20,
        });
        assert_eq!(ext.to_string(), "-m limit --limit 10/min --limit-burst 20");
    }

    #[test]
    fn test_match_ext_comment_display() {
        let ext = MatchExt::Comment("test rule".to_string());
        assert_eq!(ext.to_string(), "-m comment --comment \"test rule\"");
    }

    #[test]
    fn test_match_ext_multiport_dport_display() {
        let ext = MatchExt::MultiDport(MultiPort::parse("80,443").unwrap());
        assert_eq!(ext.to_string(), "-m multiport --dports 80,443");
    }

    #[test]
    fn test_match_ext_multiport_sport_display() {
        let ext = MatchExt::MultiSport(MultiPort::parse("1024:65535").unwrap());
        assert_eq!(ext.to_string(), "-m multiport --sports 1024:65535");
    }

    #[test]
    fn test_snat_requires_to_source() {
        let args: Vec<String> = vec!["-t", "nat", "-A", "POSTROUTING", "-j", "SNAT"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut parser = ArgParser::new(args, false);
        assert!(parser.parse_command().is_err());
    }

    #[test]
    fn test_dnat_requires_to_destination() {
        let args: Vec<String> = vec!["-t", "nat", "-A", "PREROUTING", "-j", "DNAT"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut parser = ArgParser::new(args, false);
        assert!(parser.parse_command().is_err());
    }

    #[test]
    fn test_no_command_error() {
        let args: Vec<String> = vec!["-t", "filter"].into_iter().map(String::from).collect();
        let mut parser = ArgParser::new(args, false);
        assert!(parser.parse_command().is_err());
    }

    #[test]
    fn test_unknown_option_error() {
        let args: Vec<String> = vec!["-A", "INPUT", "--nonexistent", "foo"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut parser = ArgParser::new(args, false);
        assert!(parser.parse_command().is_err());
    }

    #[test]
    fn test_unknown_match_module_error() {
        let args: Vec<String> = vec!["-A", "INPUT", "-m", "nonexistent"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut parser = ArgParser::new(args, false);
        assert!(parser.parse_command().is_err());
    }

    #[test]
    fn test_reject_display_with_type() {
        let t = Target::Reject {
            reject_with: Some("icmp-port-unreachable".to_string()),
        };
        assert_eq!(t.to_string(), "REJECT --reject-with icmp-port-unreachable");
    }

    #[test]
    fn test_log_display_full() {
        let t = Target::Log {
            prefix: Some("DROPPED: ".to_string()),
            level: Some("4".to_string()),
        };
        let s = t.to_string();
        assert!(s.contains("LOG"));
        assert!(s.contains("--log-prefix \"DROPPED: \""));
        assert!(s.contains("--log-level 4"));
    }

    #[test]
    fn test_redirect_display_with_ports() {
        let t = Target::Redirect {
            to_ports: Some(PortSpec::Single(8080)),
        };
        assert_eq!(t.to_string(), "REDIRECT --to-ports 8080");
    }

    #[test]
    fn test_dnat_display() {
        let t = Target::Dnat {
            to_destination: "192.168.1.1:80".to_string(),
        };
        assert_eq!(t.to_string(), "DNAT --to-destination 192.168.1.1:80");
    }

    #[test]
    fn test_list_chain_with_drop_policy() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-P", "INPUT", "DROP"]).unwrap();
        let output = parse_and_exec(&mut fw, &["-L", "INPUT"]).unwrap();
        assert!(output.contains("policy DROP"));
    }

    #[test]
    fn test_list_user_chain_no_policy() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-N", "MYCHAIN"]).unwrap();
        let output = parse_and_exec(&mut fw, &["-L", "MYCHAIN"]).unwrap();
        assert!(output.contains("no policy"));
    }

    #[test]
    fn test_insert_position_zero_error() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "ACCEPT"]).unwrap();
        let result = parse_and_exec(&mut fw, &["-I", "INPUT", "0", "-j", "DROP"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_replace_position_zero_error() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "ACCEPT"]).unwrap();
        let result = parse_and_exec(&mut fw, &["-R", "INPUT", "0", "-j", "DROP"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_number_zero_error() {
        let mut fw = Firewall::new(false);
        parse_and_exec(&mut fw, &["-A", "INPUT", "-j", "ACCEPT"]).unwrap();
        let result = parse_and_exec(&mut fw, &["-D", "INPUT", "0"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ipv4_addr_parse_valid() {
        let addr = parse_ipv4_addr("127.0.0.1").unwrap();
        assert_eq!(addr, [127, 0, 0, 1]);
    }

    #[test]
    fn test_ipv4_addr_parse_zero() {
        let addr = parse_ipv4_addr("0.0.0.0").unwrap();
        assert_eq!(addr, [0, 0, 0, 0]);
    }

    #[test]
    fn test_ipv6_addr_parse_double_colon() {
        let addr = parse_ipv6_addr("::").unwrap();
        assert_eq!(addr, [0u8; 16]);
    }

    #[test]
    fn test_ipv6_cidr_contains_128_prefix() {
        let c = Ipv6Cidr::parse("::1/128").unwrap();
        let mut ip = [0u8; 16];
        ip[15] = 1;
        assert!(c.contains(&ip));
        ip[15] = 2;
        assert!(!c.contains(&ip));
    }

    #[test]
    fn test_ipv6_double_colon_middle() {
        // fe80::1 should be fe80:0:0:0:0:0:0:1
        let c = Ipv6Cidr::parse("fe80::1/128").unwrap();
        assert_eq!(c.addr[0], 0xfe);
        assert_eq!(c.addr[1], 0x80);
        assert_eq!(c.addr[15], 1);
        // Middle bytes should be zero
        for i in 2..15 {
            assert_eq!(c.addr[i], 0);
        }
    }

    #[test]
    fn test_limit_unit_as_str() {
        assert_eq!(LimitUnit::Second.as_str(), "sec");
        assert_eq!(LimitUnit::Minute.as_str(), "min");
        assert_eq!(LimitUnit::Hour.as_str(), "hour");
        assert_eq!(LimitUnit::Day.as_str(), "day");
    }

    #[test]
    fn test_conn_state_as_str() {
        assert_eq!(ConnState::New.as_str(), "NEW");
        assert_eq!(ConnState::Established.as_str(), "ESTABLISHED");
        assert_eq!(ConnState::Related.as_str(), "RELATED");
        assert_eq!(ConnState::Invalid.as_str(), "INVALID");
        assert_eq!(ConnState::Untracked.as_str(), "UNTRACKED");
    }

    #[test]
    fn test_protocol_ipv6_icmp() {
        assert_eq!(Protocol::parse("ipv6-icmp").unwrap(), Protocol::Icmpv6);
    }

    #[test]
    fn test_protocol_icmpv6_as_str() {
        assert_eq!(Protocol::Icmpv6.as_str(), "icmpv6");
    }

    #[test]
    fn test_addr_cidr_parse_v6() {
        let c = AddrCidr::parse_v6("::1/128").unwrap();
        match c {
            AddrCidr::V6(_) => {}
            _ => panic!("expected V6"),
        }
    }

    #[test]
    fn test_port_zero() {
        let p = PortSpec::parse("0").unwrap();
        assert_eq!(p, PortSpec::Single(0));
        assert!(p.contains(0));
    }
}
