//! Multi-personality nftables firewall rule management utility for OurOS.
//!
//! Personalities detected via `argv[0]` basename (stripping path and `.exe`):
//!   - `nft`      -- nftables rule management (default)
//!   - `nft-list` -- quick listing shortcut (equivalent to `nft list ruleset`)

#![deny(clippy::all)]

use std::fmt;
use std::io::{self, BufRead, Read};
use std::process;

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// nftables address families.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Family {
    Ip,
    Ip6,
    Inet,
    Arp,
    Bridge,
    Netdev,
}

impl Family {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "ip" => Ok(Self::Ip),
            "ip6" => Ok(Self::Ip6),
            "inet" => Ok(Self::Inet),
            "arp" => Ok(Self::Arp),
            "bridge" => Ok(Self::Bridge),
            "netdev" => Ok(Self::Netdev),
            _ => Err(format!("unknown family '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ip => "ip",
            Self::Ip6 => "ip6",
            Self::Inet => "inet",
            Self::Arp => "arp",
            Self::Bridge => "bridge",
            Self::Netdev => "netdev",
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Chain type, hook, policy
// ---------------------------------------------------------------------------

/// Chain types for base chains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChainType {
    Filter,
    Nat,
    Route,
}

impl ChainType {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "filter" => Ok(Self::Filter),
            "nat" => Ok(Self::Nat),
            "route" => Ok(Self::Route),
            _ => Err(format!("unknown chain type '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Filter => "filter",
            Self::Nat => "nat",
            Self::Route => "route",
        }
    }
}

impl fmt::Display for ChainType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Hook points for base chains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hook {
    Prerouting,
    Input,
    Forward,
    Output,
    Postrouting,
    Ingress,
}

impl Hook {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "prerouting" => Ok(Self::Prerouting),
            "input" => Ok(Self::Input),
            "forward" => Ok(Self::Forward),
            "output" => Ok(Self::Output),
            "postrouting" => Ok(Self::Postrouting),
            "ingress" => Ok(Self::Ingress),
            _ => Err(format!("unknown hook '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Prerouting => "prerouting",
            Self::Input => "input",
            Self::Forward => "forward",
            Self::Output => "output",
            Self::Postrouting => "postrouting",
            Self::Ingress => "ingress",
        }
    }
}

impl fmt::Display for Hook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Chain policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Policy {
    Accept,
    Drop,
}

impl Policy {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "accept" => Ok(Self::Accept),
            "drop" => Ok(Self::Drop),
            _ => Err(format!("unknown policy '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Drop => "drop",
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Comparison operators
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    BitwiseAnd,
}

impl CmpOp {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "==" => Some(Self::Eq),
            "!=" => Some(Self::Ne),
            "<" => Some(Self::Lt),
            ">" => Some(Self::Gt),
            "<=" => Some(Self::Le),
            ">=" => Some(Self::Ge),
            "&" => Some(Self::BitwiseAnd),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::BitwiseAnd => "&",
        }
    }
}

impl fmt::Display for CmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Connection tracking state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CtState {
    New,
    Established,
    Related,
    Invalid,
}

impl CtState {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "new" => Ok(Self::New),
            "established" => Ok(Self::Established),
            "related" => Ok(Self::Related),
            "invalid" => Ok(Self::Invalid),
            _ => Err(format!("unknown ct state '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Established => "established",
            Self::Related => "related",
            Self::Invalid => "invalid",
        }
    }
}

impl fmt::Display for CtState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Meta keys
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MetaKey {
    Mark,
    Length,
    Protocol,
    Iiftype,
}

impl MetaKey {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "mark" => Ok(Self::Mark),
            "length" => Ok(Self::Length),
            "protocol" => Ok(Self::Protocol),
            "iiftype" => Ok(Self::Iiftype),
            _ => Err(format!("unknown meta key '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Mark => "mark",
            Self::Length => "length",
            Self::Protocol => "protocol",
            Self::Iiftype => "iiftype",
        }
    }
}

impl fmt::Display for MetaKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Match expressions
// ---------------------------------------------------------------------------

/// A single match expression in a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MatchExpr {
    IpSaddr { op: CmpOp, value: String },
    IpDaddr { op: CmpOp, value: String },
    IpProtocol { op: CmpOp, value: String },
    TcpDport { op: CmpOp, value: String },
    TcpSport { op: CmpOp, value: String },
    UdpDport { op: CmpOp, value: String },
    UdpSport { op: CmpOp, value: String },
    CtState { states: Vec<CtState> },
    Iifname { op: CmpOp, value: String },
    Oifname { op: CmpOp, value: String },
    Meta { key: MetaKey, op: CmpOp, value: String },
    EtherSaddr { op: CmpOp, value: String },
    EtherDaddr { op: CmpOp, value: String },
    IcmpType { op: CmpOp, value: String },
    SetLookup { field: String, set_name: String },
    AnonSet { field: String, elements: Vec<String> },
    Interval { field: String, low: String, high: String },
}

impl fmt::Display for MatchExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IpSaddr { op, value } => write!(f, "ip saddr {op} {value}"),
            Self::IpDaddr { op, value } => write!(f, "ip daddr {op} {value}"),
            Self::IpProtocol { op, value } => write!(f, "ip protocol {op} {value}"),
            Self::TcpDport { op, value } => write!(f, "tcp dport {op} {value}"),
            Self::TcpSport { op, value } => write!(f, "tcp sport {op} {value}"),
            Self::UdpDport { op, value } => write!(f, "udp dport {op} {value}"),
            Self::UdpSport { op, value } => write!(f, "udp sport {op} {value}"),
            Self::CtState { states } => {
                write!(f, "ct state ")?;
                let strs: Vec<&str> = states.iter().map(|s| s.as_str()).collect();
                write!(f, "{}", strs.join(","))
            }
            Self::Iifname { op, value } => write!(f, "iifname {op} \"{value}\""),
            Self::Oifname { op, value } => write!(f, "oifname {op} \"{value}\""),
            Self::Meta { key, op, value } => write!(f, "meta {key} {op} {value}"),
            Self::EtherSaddr { op, value } => write!(f, "ether saddr {op} {value}"),
            Self::EtherDaddr { op, value } => write!(f, "ether daddr {op} {value}"),
            Self::IcmpType { op, value } => write!(f, "icmp type {op} {value}"),
            Self::SetLookup { field, set_name } => write!(f, "{field} @{set_name}"),
            Self::AnonSet { field, elements } => {
                write!(f, "{field} {{ {} }}", elements.join(", "))
            }
            Self::Interval { field, low, high } => {
                write!(f, "{field} {{ {low}-{high} }}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Verdicts / actions
// ---------------------------------------------------------------------------

/// Verdict or action in a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
    Accept,
    Drop,
    Reject,
    Queue,
    Continue,
    Return,
    Jump(String),
    Goto(String),
    Counter,
    Log { prefix: Option<String> },
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accept => write!(f, "accept"),
            Self::Drop => write!(f, "drop"),
            Self::Reject => write!(f, "reject"),
            Self::Queue => write!(f, "queue"),
            Self::Continue => write!(f, "continue"),
            Self::Return => write!(f, "return"),
            Self::Jump(chain) => write!(f, "jump {chain}"),
            Self::Goto(chain) => write!(f, "goto {chain}"),
            Self::Counter => write!(f, "counter"),
            Self::Log { prefix } => {
                if let Some(p) = prefix {
                    write!(f, "log prefix \"{p}\"")
                } else {
                    write!(f, "log")
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rule
// ---------------------------------------------------------------------------

/// A single rule in a chain.
#[derive(Clone, Debug)]
struct Rule {
    handle: u64,
    matches: Vec<MatchExpr>,
    verdicts: Vec<Verdict>,
    comment: Option<String>,
}

impl Rule {
    fn new(handle: u64, matches: Vec<MatchExpr>, verdicts: Vec<Verdict>) -> Self {
        Self {
            handle,
            matches,
            verdicts,
            comment: None,
        }
    }

    fn display_nft(&self, show_handle: bool) -> String {
        let mut parts: Vec<String> = Vec::new();
        for m in &self.matches {
            parts.push(m.to_string());
        }
        for v in &self.verdicts {
            parts.push(v.to_string());
        }
        if let Some(ref c) = self.comment {
            parts.push(format!("comment \"{c}\""));
        }
        let mut line = parts.join(" ");
        if show_handle {
            line.push_str(&format!(" # handle {}", self.handle));
        }
        line
    }
}

// ---------------------------------------------------------------------------
// Base chain configuration
// ---------------------------------------------------------------------------

/// Configuration for a base chain (has type, hook, priority, policy).
#[derive(Clone, Debug)]
struct BaseChainConfig {
    chain_type: ChainType,
    hook: Hook,
    priority: i32,
    policy: Policy,
    device: Option<String>,
}

// ---------------------------------------------------------------------------
// Chain
// ---------------------------------------------------------------------------

/// A chain within a table.
#[derive(Clone, Debug)]
struct Chain {
    name: String,
    base_config: Option<BaseChainConfig>,
    rules: Vec<Rule>,
}

impl Chain {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            base_config: None,
            rules: Vec::new(),
        }
    }

    fn new_base(name: &str, config: BaseChainConfig) -> Self {
        Self {
            name: name.to_string(),
            base_config: Some(config),
            rules: Vec::new(),
        }
    }

    fn is_base(&self) -> bool {
        self.base_config.is_some()
    }
}

// ---------------------------------------------------------------------------
// Set element types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SetDataType {
    Ipv4Addr,
    Ipv6Addr,
    EtherAddr,
    InetProto,
    InetService,
    Mark,
    Ifname,
}

impl SetDataType {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "ipv4_addr" => Ok(Self::Ipv4Addr),
            "ipv6_addr" => Ok(Self::Ipv6Addr),
            "ether_addr" => Ok(Self::EtherAddr),
            "inet_proto" => Ok(Self::InetProto),
            "inet_service" => Ok(Self::InetService),
            "mark" => Ok(Self::Mark),
            "ifname" => Ok(Self::Ifname),
            _ => Err(format!("unknown set type '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ipv4Addr => "ipv4_addr",
            Self::Ipv6Addr => "ipv6_addr",
            Self::EtherAddr => "ether_addr",
            Self::InetProto => "inet_proto",
            Self::InetService => "inet_service",
            Self::Mark => "mark",
            Self::Ifname => "ifname",
        }
    }
}

impl fmt::Display for SetDataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Flags that can be set on an nftables set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SetFlag {
    Constant,
    Interval,
    Timeout,
}

impl SetFlag {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "constant" => Ok(Self::Constant),
            "interval" => Ok(Self::Interval),
            "timeout" => Ok(Self::Timeout),
            _ => Err(format!("unknown set flag '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Interval => "interval",
            Self::Timeout => "timeout",
        }
    }
}

impl fmt::Display for SetFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Named set
// ---------------------------------------------------------------------------

/// A named set in a table.
#[derive(Clone, Debug)]
struct NamedSet {
    name: String,
    key_type: SetDataType,
    flags: Vec<SetFlag>,
    elements: Vec<String>,
    typeof_expr: Option<String>,
}

impl NamedSet {
    fn new(name: &str, key_type: SetDataType) -> Self {
        Self {
            name: name.to_string(),
            key_type,
            flags: Vec::new(),
            elements: Vec::new(),
            typeof_expr: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Named map
// ---------------------------------------------------------------------------

/// A named map in a table.
#[derive(Clone, Debug)]
struct NamedMap {
    name: String,
    key_type: SetDataType,
    value_type: SetDataType,
    elements: Vec<(String, String)>,
}

impl NamedMap {
    fn new(name: &str, key_type: SetDataType, value_type: SetDataType) -> Self {
        Self {
            name: name.to_string(),
            key_type,
            value_type,
            elements: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Counter object
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct CounterObj {
    name: String,
    packets: u64,
    bytes: u64,
}

impl CounterObj {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            packets: 0,
            bytes: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Quota object
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct QuotaObj {
    name: String,
    bytes_limit: u64,
    used: u64,
    inv: bool,
}

impl QuotaObj {
    fn new(name: &str, bytes_limit: u64, inv: bool) -> Self {
        Self {
            name: name.to_string(),
            bytes_limit,
            used: 0,
            inv,
        }
    }
}

// ---------------------------------------------------------------------------
// Limit object
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LimitUnit {
    Second,
    Minute,
    Hour,
    Day,
}

impl LimitUnit {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "second" | "sec" | "/second" | "/sec" => Ok(Self::Second),
            "minute" | "min" | "/minute" | "/min" => Ok(Self::Minute),
            "hour" | "/hour" => Ok(Self::Hour),
            "day" | "/day" => Ok(Self::Day),
            _ => Err(format!("unknown limit unit '{s}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Second => "second",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
        }
    }
}

impl fmt::Display for LimitUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug)]
struct LimitObj {
    name: String,
    rate: u64,
    unit: LimitUnit,
    burst: Option<u64>,
}

impl LimitObj {
    fn new(name: &str, rate: u64, unit: LimitUnit) -> Self {
        Self {
            name: name.to_string(),
            rate,
            unit,
            burst: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

/// A table containing chains, sets, maps, and stateful objects.
#[derive(Clone, Debug)]
struct Table {
    family: Family,
    name: String,
    chains: Vec<Chain>,
    sets: Vec<NamedSet>,
    maps: Vec<NamedMap>,
    counters: Vec<CounterObj>,
    quotas: Vec<QuotaObj>,
    limits: Vec<LimitObj>,
    flags_dormant: bool,
}

impl Table {
    fn new(family: Family, name: &str) -> Self {
        Self {
            family,
            name: name.to_string(),
            chains: Vec::new(),
            sets: Vec::new(),
            maps: Vec::new(),
            counters: Vec::new(),
            quotas: Vec::new(),
            limits: Vec::new(),
            flags_dormant: false,
        }
    }

    fn find_chain(&self, name: &str) -> Option<usize> {
        self.chains.iter().position(|c| c.name == name)
    }

    fn find_set(&self, name: &str) -> Option<usize> {
        self.sets.iter().position(|s| s.name == name)
    }

    fn find_map(&self, name: &str) -> Option<usize> {
        self.maps.iter().position(|m| m.name == name)
    }

    fn find_counter(&self, name: &str) -> Option<usize> {
        self.counters.iter().position(|c| c.name == name)
    }

    fn find_quota(&self, name: &str) -> Option<usize> {
        self.quotas.iter().position(|q| q.name == name)
    }

    fn find_limit(&self, name: &str) -> Option<usize> {
        self.limits.iter().position(|l| l.name == name)
    }
}

// ---------------------------------------------------------------------------
// Ruleset (top-level state)
// ---------------------------------------------------------------------------

/// The entire nftables ruleset.
struct Ruleset {
    tables: Vec<Table>,
    next_handle: u64,
}

impl Ruleset {
    fn new() -> Self {
        Self {
            tables: Vec::new(),
            next_handle: 1,
        }
    }

    fn alloc_handle(&mut self) -> u64 {
        let h = self.next_handle;
        self.next_handle = self.next_handle.wrapping_add(1);
        h
    }

    fn find_table(&self, family: Family, name: &str) -> Option<usize> {
        self.tables
            .iter()
            .position(|t| t.family == family && t.name == name)
    }

    fn get_table(&self, family: Family, name: &str) -> Result<&Table, String> {
        self.find_table(family, name)
            .map(|i| &self.tables[i])
            .ok_or_else(|| format!("table '{name}' does not exist in family {family}"))
    }

    fn get_table_mut(&mut self, family: Family, name: &str) -> Result<&mut Table, String> {
        self.find_table(family, name)
            .map(|i| &mut self.tables[i])
            .ok_or_else(|| format!("table '{name}' does not exist in family {family}"))
    }
}

// ---------------------------------------------------------------------------
// Global flags
// ---------------------------------------------------------------------------

/// Runtime flags from command-line options.
#[derive(Clone, Debug)]
struct Flags {
    json: bool,
    numeric: bool,
    show_handles: bool,
}

impl Flags {
    fn new() -> Self {
        Self {
            json: false,
            numeric: false,
            show_handles: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Token parsing helpers
// ---------------------------------------------------------------------------

/// A simple token iterator over words.
struct Tokens<'a> {
    words: &'a [String],
    pos: usize,
}

impl<'a> Tokens<'a> {
    fn new(words: &'a [String]) -> Self {
        Self { words, pos: 0 }
    }

    fn peek(&self) -> Option<&str> {
        self.words.get(self.pos).map(|s| s.as_str())
    }

    fn next_token(&mut self) -> Option<&str> {
        if self.pos < self.words.len() {
            let tok = self.words[self.pos].as_str();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn expect(&mut self, what: &str) -> Result<&str, String> {
        self.next_token()
            .ok_or_else(|| format!("expected {what}"))
    }

    fn remaining(&self) -> &[String] {
        &self.words[self.pos..]
    }
}

// ---------------------------------------------------------------------------
// Parse match expressions from tokens
// ---------------------------------------------------------------------------

/// Try to parse the next match expression. Returns None if the next token is
/// a verdict keyword or we are at end-of-input.
fn parse_match(tokens: &mut Tokens<'_>) -> Result<Option<MatchExpr>, String> {
    let first = match tokens.peek() {
        Some(t) => t,
        None => return Ok(None),
    };

    // Check if this is a verdict keyword instead of a match
    if is_verdict_keyword(first) {
        return Ok(None);
    }

    // Also stop at "comment"
    if first == "comment" {
        return Ok(None);
    }

    match first {
        "ip" => {
            tokens.next_token();
            let field = tokens.expect("ip field (saddr/daddr/protocol)")?;
            match field {
                "saddr" => {
                    let (op, val) = parse_cmp_value(tokens)?;
                    Ok(Some(MatchExpr::IpSaddr {
                        op,
                        value: val,
                    }))
                }
                "daddr" => {
                    let (op, val) = parse_cmp_value(tokens)?;
                    Ok(Some(MatchExpr::IpDaddr {
                        op,
                        value: val,
                    }))
                }
                "protocol" => {
                    let (op, val) = parse_cmp_value(tokens)?;
                    Ok(Some(MatchExpr::IpProtocol {
                        op,
                        value: val,
                    }))
                }
                _ => Err(format!("unknown ip field '{field}'")),
            }
        }
        "tcp" => {
            tokens.next_token();
            let field = tokens.expect("tcp field (dport/sport)")?;
            match field {
                "dport" => {
                    let result = parse_set_or_cmp(tokens, "tcp dport")?;
                    Ok(Some(result))
                }
                "sport" => {
                    let result = parse_set_or_cmp(tokens, "tcp sport")?;
                    Ok(Some(result))
                }
                _ => Err(format!("unknown tcp field '{field}'")),
            }
        }
        "udp" => {
            tokens.next_token();
            let field = tokens.expect("udp field (dport/sport)")?;
            match field {
                "dport" => {
                    let result = parse_set_or_cmp(tokens, "udp dport")?;
                    Ok(Some(result))
                }
                "sport" => {
                    let (op, val) = parse_cmp_value(tokens)?;
                    Ok(Some(MatchExpr::UdpSport {
                        op,
                        value: val,
                    }))
                }
                _ => Err(format!("unknown udp field '{field}'")),
            }
        }
        "ct" => {
            tokens.next_token();
            let field = tokens.expect("ct field")?;
            if field != "state" {
                return Err(format!("unknown ct field '{field}'"));
            }
            let val_str = tokens.expect("ct state value")?;
            let mut states = Vec::new();
            for s in val_str.split(',') {
                states.push(CtState::parse(s.trim())?);
            }
            Ok(Some(MatchExpr::CtState { states }))
        }
        "iifname" => {
            tokens.next_token();
            let (op, val) = parse_cmp_value(tokens)?;
            let val = val.trim_matches('"').to_string();
            Ok(Some(MatchExpr::Iifname { op, value: val }))
        }
        "oifname" => {
            tokens.next_token();
            let (op, val) = parse_cmp_value(tokens)?;
            let val = val.trim_matches('"').to_string();
            Ok(Some(MatchExpr::Oifname { op, value: val }))
        }
        "meta" => {
            tokens.next_token();
            let key_str = tokens.expect("meta key")?;
            let key = MetaKey::parse(key_str)?;
            let (op, val) = parse_cmp_value(tokens)?;
            Ok(Some(MatchExpr::Meta {
                key,
                op,
                value: val,
            }))
        }
        "ether" => {
            tokens.next_token();
            let field = tokens.expect("ether field (saddr/daddr)")?;
            match field {
                "saddr" => {
                    let (op, val) = parse_cmp_value(tokens)?;
                    Ok(Some(MatchExpr::EtherSaddr {
                        op,
                        value: val,
                    }))
                }
                "daddr" => {
                    let (op, val) = parse_cmp_value(tokens)?;
                    Ok(Some(MatchExpr::EtherDaddr {
                        op,
                        value: val,
                    }))
                }
                _ => Err(format!("unknown ether field '{field}'")),
            }
        }
        "icmp" => {
            tokens.next_token();
            let field = tokens.expect("icmp field")?;
            if field != "type" {
                return Err(format!("unknown icmp field '{field}'"));
            }
            let (op, val) = parse_cmp_value(tokens)?;
            Ok(Some(MatchExpr::IcmpType { op, value: val }))
        }
        _ => {
            // Not a recognized match expression; the caller should treat it
            // as a verdict.
            Ok(None)
        }
    }
}

/// Parse a comparison operator + value, defaulting to Eq if no operator present.
fn parse_cmp_value(tokens: &mut Tokens<'_>) -> Result<(CmpOp, String), String> {
    let next = tokens.expect("operator or value")?;
    if let Some(op) = CmpOp::parse(next) {
        let val = tokens.expect("value")?.to_string();
        Ok((op, val))
    } else if next.starts_with('@') {
        // Set reference like @myset - not a cmp, put it back conceptually
        // This is handled by parse_set_or_cmp, but if we get here it means
        // a simple field with set reference.
        Ok((CmpOp::Eq, next.to_string()))
    } else {
        Ok((CmpOp::Eq, next.to_string()))
    }
}

/// Parse either a set/anonymous-set or a comparison for a given field.
fn parse_set_or_cmp(tokens: &mut Tokens<'_>, field: &str) -> Result<MatchExpr, String> {
    let next = match tokens.peek() {
        Some(t) => t.to_string(),
        None => return Err(format!("expected value after {field}")),
    };

    if next.starts_with('@') {
        tokens.next_token();
        let set_name = next[1..].to_string();
        return Ok(MatchExpr::SetLookup {
            field: field.to_string(),
            set_name,
        });
    }

    if next == "{" {
        tokens.next_token();
        let mut elements = Vec::new();
        loop {
            let elem = match tokens.next_token() {
                Some(t) => t.to_string(),
                None => return Err("unterminated set".to_string()),
            };
            if elem == "}" {
                break;
            }
            let elem = elem.trim_end_matches(',').to_string();
            // Check for interval: "X-Y"
            if elem.contains('-') && !elem.starts_with('-') {
                let parts: Vec<&str> = elem.splitn(2, '-').collect();
                if parts.len() == 2 {
                    // Consume until "}"
                    loop {
                        match tokens.peek() {
                            Some("}") => {
                                tokens.next_token();
                                break;
                            }
                            Some(_) => {
                                tokens.next_token();
                            }
                            None => break,
                        }
                    }
                    return Ok(MatchExpr::Interval {
                        field: field.to_string(),
                        low: parts[0].to_string(),
                        high: parts[1].to_string(),
                    });
                }
            }
            if !elem.is_empty() {
                elements.push(elem);
            }
            // Skip comma tokens
            if let Some(",") = tokens.peek() {
                tokens.next_token();
            }
        }
        return Ok(MatchExpr::AnonSet {
            field: field.to_string(),
            elements,
        });
    }

    // Regular comparison
    let (op, val) = parse_cmp_value(tokens)?;
    match field {
        "tcp dport" => Ok(MatchExpr::TcpDport { op, value: val }),
        "tcp sport" => Ok(MatchExpr::TcpSport { op, value: val }),
        "udp dport" => Ok(MatchExpr::UdpDport { op, value: val }),
        _ => Err(format!("unexpected field '{field}' in set_or_cmp")),
    }
}

/// Check if a token is a verdict keyword.
fn is_verdict_keyword(s: &str) -> bool {
    matches!(
        s,
        "accept"
            | "drop"
            | "reject"
            | "queue"
            | "continue"
            | "return"
            | "jump"
            | "goto"
            | "counter"
            | "log"
    )
}

/// Parse verdicts from remaining tokens.
fn parse_verdicts(tokens: &mut Tokens<'_>) -> Result<Vec<Verdict>, String> {
    let mut verdicts = Vec::new();
    while let Some(tok) = tokens.next_token() {
        match tok {
            "accept" => verdicts.push(Verdict::Accept),
            "drop" => verdicts.push(Verdict::Drop),
            "reject" => verdicts.push(Verdict::Reject),
            "queue" => verdicts.push(Verdict::Queue),
            "continue" => verdicts.push(Verdict::Continue),
            "return" => verdicts.push(Verdict::Return),
            "jump" => {
                let chain = tokens.expect("chain name for jump")?.to_string();
                verdicts.push(Verdict::Jump(chain));
            }
            "goto" => {
                let chain = tokens.expect("chain name for goto")?.to_string();
                verdicts.push(Verdict::Goto(chain));
            }
            "counter" => verdicts.push(Verdict::Counter),
            "log" => {
                let prefix = if tokens.peek() == Some("prefix") {
                    tokens.next_token();
                    let p = tokens.expect("log prefix string")?.to_string();
                    Some(p.trim_matches('"').to_string())
                } else {
                    None
                };
                verdicts.push(Verdict::Log { prefix });
            }
            "comment" => {
                // Skip the comment value (it goes to rule.comment)
                tokens.next_token();
                break;
            }
            _ => {
                return Err(format!("unexpected token '{tok}' in verdict position"));
            }
        }
    }
    Ok(verdicts)
}

// ---------------------------------------------------------------------------
// Command execution
// ---------------------------------------------------------------------------

/// Execute a single nft command line.
fn exec_command(
    rs: &mut Ruleset,
    flags: &Flags,
    words: &[String],
) -> Result<String, String> {
    if words.is_empty() {
        return Ok(String::new());
    }

    let mut tokens = Tokens::new(words);
    let cmd = tokens.expect("command")?;

    match cmd {
        "add" => exec_add(rs, &mut tokens),
        "create" => exec_add(rs, &mut tokens),
        "delete" => exec_delete(rs, &mut tokens),
        "list" => exec_list(rs, flags, &mut tokens),
        "flush" => exec_flush(rs, &mut tokens),
        "rename" => exec_rename(rs, &mut tokens),
        "export" => exec_export(rs, flags),
        "monitor" => Ok("monitoring not available in standalone mode\n".to_string()),
        "insert" => exec_insert(rs, &mut tokens),
        _ => Err(format!("unknown command '{cmd}'")),
    }
}

// ---------------------------------------------------------------------------
// add command
// ---------------------------------------------------------------------------

fn exec_add(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let obj_type = tokens.expect("object type")?;
    match obj_type {
        "table" => add_table(rs, tokens),
        "chain" => add_chain(rs, tokens),
        "rule" => add_rule(rs, tokens, false),
        "set" => add_set(rs, tokens),
        "map" => add_map(rs, tokens),
        "element" => add_element(rs, tokens),
        "counter" => add_counter(rs, tokens),
        "quota" => add_quota(rs, tokens),
        "limit" => add_limit(rs, tokens),
        _ => Err(format!("cannot add object type '{obj_type}'")),
    }
}

fn add_table(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let family_or_name = tokens.expect("family or table name")?;
    let (family, name) = if let Ok(f) = Family::parse(family_or_name) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_name.to_string())
    };

    if rs.find_table(family, &name).is_some() {
        // Idempotent: adding an existing table is not an error in nftables
        return Ok(String::new());
    }

    let mut table = Table::new(family, &name);

    // Check for optional flags
    while let Some(tok) = tokens.peek() {
        if tok == "flags" {
            tokens.next_token();
            let flag_val = tokens.expect("flag value")?;
            if flag_val == "dormant" {
                table.flags_dormant = true;
            }
        } else {
            break;
        }
    }

    rs.tables.push(table);
    Ok(String::new())
}

fn add_chain(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let family_or_name = tokens.expect("family or table name")?;
    let (family, table_name) = if let Ok(f) = Family::parse(family_or_name) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_name.to_string())
    };

    let chain_name = tokens.expect("chain name")?.to_string();

    let table = rs.get_table_mut(family, &table_name)?;

    if table.find_chain(&chain_name).is_some() {
        return Err(format!("chain '{chain_name}' already exists in table '{table_name}'"));
    }

    // Check for base chain specifiers
    if tokens.peek() == Some("{") || tokens.peek() == Some("type") {
        if tokens.peek() == Some("{") {
            tokens.next_token(); // consume "{"
        }

        // Parse: type <type> hook <hook> priority <prio> ; policy <policy> ;
        let type_kw = tokens.expect("'type' keyword")?;
        if type_kw != "type" {
            return Err(format!("expected 'type', got '{type_kw}'"));
        }
        let chain_type = ChainType::parse(tokens.expect("chain type")?)?;

        let hook_kw = tokens.expect("'hook' keyword")?;
        if hook_kw != "hook" {
            return Err(format!("expected 'hook', got '{hook_kw}'"));
        }
        let hook = Hook::parse(tokens.expect("hook name")?)?;

        // Optional device for ingress
        let device = if tokens.peek() == Some("device") {
            tokens.next_token();
            Some(tokens.expect("device name")?.to_string())
        } else {
            None
        };

        let prio_kw = tokens.expect("'priority' keyword")?;
        if prio_kw != "priority" {
            return Err(format!("expected 'priority', got '{prio_kw}'"));
        }
        let prio_str = tokens.expect("priority value")?;
        let prio_str = prio_str.trim_end_matches(';');
        let priority: i32 = prio_str
            .parse()
            .map_err(|_| format!("invalid priority '{prio_str}'"))?;

        // Skip semicolons
        while tokens.peek() == Some(";") {
            tokens.next_token();
        }

        let mut policy = Policy::Accept;
        if tokens.peek() == Some("policy") {
            tokens.next_token();
            let pol_str = tokens.expect("policy value")?;
            let pol_str = pol_str.trim_end_matches(';');
            policy = Policy::parse(pol_str)?;
        }

        // Skip closing brace and semicolons
        while let Some(tok) = tokens.peek() {
            if tok == "}" || tok == ";" {
                tokens.next_token();
            } else {
                break;
            }
        }

        let config = BaseChainConfig {
            chain_type,
            hook,
            priority,
            policy,
            device,
        };
        table.chains.push(Chain::new_base(&chain_name, config));
    } else {
        table.chains.push(Chain::new(&chain_name));
    }

    Ok(String::new())
}

fn add_rule(
    rs: &mut Ruleset,
    tokens: &mut Tokens<'_>,
    insert: bool,
) -> Result<String, String> {
    let family_or_table = tokens.expect("family or table name")?;
    let (family, table_name) = if let Ok(f) = Family::parse(family_or_table) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_table.to_string())
    };

    let chain_name = tokens.expect("chain name")?.to_string();

    // Check for optional "position <handle>" or "handle <handle>"
    let mut position: Option<usize> = None;
    if tokens.peek() == Some("position") || tokens.peek() == Some("handle") {
        tokens.next_token();
        let pos_str = tokens.expect("position/handle number")?;
        position = Some(
            pos_str
                .parse::<usize>()
                .map_err(|_| format!("invalid position '{pos_str}'"))?,
        );
    }

    // Parse match expressions
    let mut matches = Vec::new();
    while let Some(m) = parse_match(tokens)? {
        matches.push(m);
    }

    // Parse verdicts
    let verdicts = parse_verdicts(tokens)?;

    let handle = rs.alloc_handle();
    let mut rule = Rule::new(handle, matches, verdicts);

    // Check for comment in remaining tokens
    let remaining = tokens.remaining();
    for (i, tok) in remaining.iter().enumerate() {
        if tok == "comment" {
            if let Some(c) = remaining.get(i.wrapping_add(1)) {
                rule.comment = Some(c.trim_matches('"').to_string());
            }
            break;
        }
    }

    let table = rs.get_table_mut(family, &table_name)?;
    let chain_idx = table
        .find_chain(&chain_name)
        .ok_or_else(|| format!("chain '{chain_name}' not found in table '{table_name}'"))?;

    if insert {
        if let Some(pos) = position {
            // Insert at position (by handle)
            let idx = table.chains[chain_idx]
                .rules
                .iter()
                .position(|r| r.handle == pos as u64)
                .ok_or_else(|| format!("handle {pos} not found"))?;
            table.chains[chain_idx].rules.insert(idx, rule);
        } else {
            table.chains[chain_idx].rules.insert(0, rule);
        }
    } else if let Some(pos) = position {
        let idx = table.chains[chain_idx]
            .rules
            .iter()
            .position(|r| r.handle == pos as u64)
            .ok_or_else(|| format!("handle {pos} not found"))?;
        let insert_at = if idx < table.chains[chain_idx].rules.len() {
            idx.wrapping_add(1)
        } else {
            idx
        };
        table.chains[chain_idx].rules.insert(insert_at, rule);
    } else {
        table.chains[chain_idx].rules.push(rule);
    }

    Ok(String::new())
}

fn add_set(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let family_or_table = tokens.expect("family or table name")?;
    let (family, table_name) = if let Ok(f) = Family::parse(family_or_table) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_table.to_string())
    };

    let set_name = tokens.expect("set name")?.to_string();

    // Parse set body: { type <type> ; [flags <f1,f2>] ; [elements = { ... }] }
    if tokens.peek() == Some("{") {
        tokens.next_token(); // consume "{"
    }

    let mut key_type = SetDataType::Ipv4Addr;
    let mut flags = Vec::new();
    let mut elements = Vec::new();
    let mut typeof_expr = None;

    while let Some(tok) = tokens.peek() {
        if tok == "}" {
            tokens.next_token();
            break;
        }
        let keyword = tokens.next_token().unwrap_or("");
        match keyword {
            "type" => {
                let type_str = tokens.expect("set data type")?;
                let type_str = type_str.trim_end_matches(';');
                key_type = SetDataType::parse(type_str)?;
            }
            "typeof" => {
                let expr = tokens.expect("typeof expression")?;
                let expr = expr.trim_end_matches(';');
                typeof_expr = Some(expr.to_string());
            }
            "flags" => {
                let flags_str = tokens.expect("set flags")?;
                let flags_str = flags_str.trim_end_matches(';');
                for f in flags_str.split(',') {
                    flags.push(SetFlag::parse(f.trim())?);
                }
            }
            "elements" => {
                // Skip "="
                if tokens.peek() == Some("=") {
                    tokens.next_token();
                }
                // Parse element list
                if tokens.peek() == Some("{") {
                    tokens.next_token();
                    loop {
                        match tokens.peek() {
                            Some("}") => {
                                tokens.next_token();
                                break;
                            }
                            Some(_) => {
                                let e = tokens.next_token().unwrap_or("").to_string();
                                let e = e.trim_end_matches(',').to_string();
                                if !e.is_empty() {
                                    elements.push(e);
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            ";" => {}
            _ => {}
        }
    }

    let table = rs.get_table_mut(family, &table_name)?;
    if table.find_set(&set_name).is_some() {
        return Err(format!("set '{set_name}' already exists"));
    }
    let mut set = NamedSet::new(&set_name, key_type);
    set.flags = flags;
    set.elements = elements;
    set.typeof_expr = typeof_expr;
    table.sets.push(set);

    Ok(String::new())
}

fn add_map(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let family_or_table = tokens.expect("family or table name")?;
    let (family, table_name) = if let Ok(f) = Family::parse(family_or_table) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_table.to_string())
    };

    let map_name = tokens.expect("map name")?.to_string();

    if tokens.peek() == Some("{") {
        tokens.next_token();
    }

    let mut key_type = SetDataType::Ipv4Addr;
    let mut value_type = SetDataType::Ipv4Addr;
    let mut elements = Vec::new();

    while let Some(tok) = tokens.peek() {
        if tok == "}" {
            tokens.next_token();
            break;
        }
        let keyword = tokens.next_token().unwrap_or("");
        match keyword {
            "type" => {
                let kt = tokens.expect("key type")?;
                let kt = kt.trim_end_matches(';');
                key_type = SetDataType::parse(kt)?;

                // Expect ":"
                if tokens.peek() == Some(":") {
                    tokens.next_token();
                }

                let vt = tokens.expect("value type")?;
                let vt = vt.trim_end_matches(';');
                value_type = SetDataType::parse(vt)?;
            }
            "elements" => {
                if tokens.peek() == Some("=") {
                    tokens.next_token();
                }
                if tokens.peek() == Some("{") {
                    tokens.next_token();
                    loop {
                        match tokens.peek() {
                            Some("}") => {
                                tokens.next_token();
                                break;
                            }
                            Some(_) => {
                                let k = tokens.next_token().unwrap_or("").to_string();
                                let k = k.trim_end_matches(',').to_string();
                                if tokens.peek() == Some(":") {
                                    tokens.next_token();
                                    let v = tokens.next_token().unwrap_or("").to_string();
                                    let v = v.trim_end_matches(',').to_string();
                                    if !k.is_empty() && !v.is_empty() {
                                        elements.push((k, v));
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            ";" => {}
            _ => {}
        }
    }

    let table = rs.get_table_mut(family, &table_name)?;
    if table.find_map(&map_name).is_some() {
        return Err(format!("map '{map_name}' already exists"));
    }
    let mut map = NamedMap::new(&map_name, key_type, value_type);
    map.elements = elements;
    table.maps.push(map);

    Ok(String::new())
}

fn add_element(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let family_or_table = tokens.expect("family or table name")?;
    let (family, table_name) = if let Ok(f) = Family::parse(family_or_table) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_table.to_string())
    };

    let set_name = tokens.expect("set name")?.to_string();

    // Parse { elem1, elem2 }
    if tokens.peek() == Some("{") {
        tokens.next_token();
    }
    let mut new_elements = Vec::new();
    loop {
        match tokens.peek() {
            Some("}") => {
                tokens.next_token();
                break;
            }
            Some(_) => {
                let e = tokens.next_token().unwrap_or("").to_string();
                let e = e.trim_end_matches(',').to_string();
                if !e.is_empty() {
                    new_elements.push(e);
                }
            }
            None => break,
        }
    }

    let table = rs.get_table_mut(family, &table_name)?;
    let set_idx = table
        .find_set(&set_name)
        .ok_or_else(|| format!("set '{set_name}' not found in table '{table_name}'"))?;
    table.sets[set_idx].elements.extend(new_elements);

    Ok(String::new())
}

fn add_counter(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let family_or_table = tokens.expect("family or table name")?;
    let (family, table_name) = if let Ok(f) = Family::parse(family_or_table) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_table.to_string())
    };

    let counter_name = tokens.expect("counter name")?.to_string();
    let table = rs.get_table_mut(family, &table_name)?;
    if table.find_counter(&counter_name).is_some() {
        return Err(format!("counter '{counter_name}' already exists"));
    }
    table.counters.push(CounterObj::new(&counter_name));
    Ok(String::new())
}

fn add_quota(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let family_or_table = tokens.expect("family or table name")?;
    let (family, table_name) = if let Ok(f) = Family::parse(family_or_table) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_table.to_string())
    };

    let quota_name = tokens.expect("quota name")?.to_string();

    let mut limit: u64 = 0;
    let mut inv = false;

    while let Some(tok) = tokens.peek() {
        if tok == "over" {
            tokens.next_token();
            inv = true;
        } else if tok.parse::<u64>().is_ok() {
            limit = tokens.next_token().unwrap_or("0").parse().unwrap_or(0);
            // Optional unit (bytes, kbytes, mbytes, gbytes)
            if let Some(unit_tok) = tokens.peek() {
                let mult: u64 = match unit_tok {
                    "bytes" => 1,
                    "kbytes" => 1024,
                    "mbytes" => 1024 * 1024,
                    "gbytes" => 1024 * 1024 * 1024,
                    _ => 0,
                };
                if mult > 0 {
                    tokens.next_token();
                    limit = limit.saturating_mul(mult);
                }
            }
        } else {
            break;
        }
    }

    let table = rs.get_table_mut(family, &table_name)?;
    if table.find_quota(&quota_name).is_some() {
        return Err(format!("quota '{quota_name}' already exists"));
    }
    table.quotas.push(QuotaObj::new(&quota_name, limit, inv));
    Ok(String::new())
}

fn add_limit(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let family_or_table = tokens.expect("family or table name")?;
    let (family, table_name) = if let Ok(f) = Family::parse(family_or_table) {
        let n = tokens.expect("table name")?.to_string();
        (f, n)
    } else {
        (Family::Ip, family_or_table.to_string())
    };

    let limit_name = tokens.expect("limit name")?.to_string();

    // Parse: rate <N>/<unit> [burst <N>]
    let mut rate: u64 = 0;
    let mut unit = LimitUnit::Second;
    let mut burst: Option<u64> = None;

    if tokens.peek() == Some("rate") {
        tokens.next_token();
        let rate_str = tokens.expect("rate value")?;
        rate = rate_str.parse().unwrap_or(0);

        let unit_str = tokens.expect("rate unit")?;
        unit = LimitUnit::parse(unit_str)?;
    }

    if tokens.peek() == Some("burst") {
        tokens.next_token();
        let burst_str = tokens.expect("burst value")?;
        burst = Some(burst_str.parse().unwrap_or(0));
        // optional "packets" keyword
        if tokens.peek() == Some("packets") {
            tokens.next_token();
        }
    }

    let table = rs.get_table_mut(family, &table_name)?;
    if table.find_limit(&limit_name).is_some() {
        return Err(format!("limit '{limit_name}' already exists"));
    }
    let mut lim = LimitObj::new(&limit_name, rate, unit);
    lim.burst = burst;
    table.limits.push(lim);
    Ok(String::new())
}

// ---------------------------------------------------------------------------
// insert command
// ---------------------------------------------------------------------------

fn exec_insert(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let obj_type = tokens.expect("object type")?;
    if obj_type != "rule" {
        return Err(format!("can only insert rules, not '{obj_type}'"));
    }
    add_rule(rs, tokens, true)
}

// ---------------------------------------------------------------------------
// delete command
// ---------------------------------------------------------------------------

fn exec_delete(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let obj_type = tokens.expect("object type")?;
    match obj_type {
        "table" => delete_table(rs, tokens),
        "chain" => delete_chain(rs, tokens),
        "rule" => delete_rule(rs, tokens),
        "set" => delete_set(rs, tokens),
        "map" => delete_map(rs, tokens),
        "element" => delete_element(rs, tokens),
        "counter" => delete_counter(rs, tokens),
        "quota" => delete_quota(rs, tokens),
        "limit" => delete_limit(rs, tokens),
        _ => Err(format!("cannot delete object type '{obj_type}'")),
    }
}

fn parse_family_and_name(tokens: &mut Tokens<'_>) -> Result<(Family, String), String> {
    let first = tokens.expect("family or name")?;
    if let Ok(f) = Family::parse(first) {
        let n = tokens.expect("name")?.to_string();
        Ok((f, n))
    } else {
        Ok((Family::Ip, first.to_string()))
    }
}

fn delete_table(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, name) = parse_family_and_name(tokens)?;
    let idx = rs
        .find_table(family, &name)
        .ok_or_else(|| format!("table '{name}' not found in family {family}"))?;
    rs.tables.remove(idx);
    Ok(String::new())
}

fn delete_chain(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let chain_name = tokens.expect("chain name")?.to_string();
    let table = rs.get_table_mut(family, &table_name)?;
    let idx = table
        .find_chain(&chain_name)
        .ok_or_else(|| format!("chain '{chain_name}' not found"))?;
    if !table.chains[idx].rules.is_empty() {
        return Err(format!(
            "chain '{chain_name}' is not empty; flush it first"
        ));
    }
    table.chains.remove(idx);
    Ok(String::new())
}

fn delete_rule(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let chain_name = tokens.expect("chain name")?.to_string();

    // Accept either "handle <N>" or just a handle number
    let handle_tok = tokens.expect("handle keyword or number")?;
    let handle: u64 = if handle_tok == "handle" {
        let h_str = tokens.expect("handle number")?;
        h_str
            .parse()
            .map_err(|_| format!("invalid handle '{h_str}'"))?
    } else {
        handle_tok
            .parse()
            .map_err(|_| format!("invalid handle '{handle_tok}'"))?
    };

    let table = rs.get_table_mut(family, &table_name)?;
    let chain_idx = table
        .find_chain(&chain_name)
        .ok_or_else(|| format!("chain '{chain_name}' not found"))?;
    let rule_idx = table.chains[chain_idx]
        .rules
        .iter()
        .position(|r| r.handle == handle)
        .ok_or_else(|| format!("rule handle {handle} not found"))?;
    table.chains[chain_idx].rules.remove(rule_idx);
    Ok(String::new())
}

fn delete_set(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let set_name = tokens.expect("set name")?.to_string();
    let table = rs.get_table_mut(family, &table_name)?;
    let idx = table
        .find_set(&set_name)
        .ok_or_else(|| format!("set '{set_name}' not found"))?;
    table.sets.remove(idx);
    Ok(String::new())
}

fn delete_map(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let map_name = tokens.expect("map name")?.to_string();
    let table = rs.get_table_mut(family, &table_name)?;
    let idx = table
        .find_map(&map_name)
        .ok_or_else(|| format!("map '{map_name}' not found"))?;
    table.maps.remove(idx);
    Ok(String::new())
}

fn delete_element(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let set_name = tokens.expect("set name")?.to_string();

    // Parse { elem1, elem2 }
    if tokens.peek() == Some("{") {
        tokens.next_token();
    }
    let mut to_remove = Vec::new();
    loop {
        match tokens.peek() {
            Some("}") => {
                tokens.next_token();
                break;
            }
            Some(_) => {
                let e = tokens.next_token().unwrap_or("").to_string();
                let e = e.trim_end_matches(',').to_string();
                if !e.is_empty() {
                    to_remove.push(e);
                }
            }
            None => break,
        }
    }

    let table = rs.get_table_mut(family, &table_name)?;
    let set_idx = table
        .find_set(&set_name)
        .ok_or_else(|| format!("set '{set_name}' not found"))?;
    table.sets[set_idx]
        .elements
        .retain(|e| !to_remove.contains(e));
    Ok(String::new())
}

fn delete_counter(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let counter_name = tokens.expect("counter name")?.to_string();
    let table = rs.get_table_mut(family, &table_name)?;
    let idx = table
        .find_counter(&counter_name)
        .ok_or_else(|| format!("counter '{counter_name}' not found"))?;
    table.counters.remove(idx);
    Ok(String::new())
}

fn delete_quota(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let quota_name = tokens.expect("quota name")?.to_string();
    let table = rs.get_table_mut(family, &table_name)?;
    let idx = table
        .find_quota(&quota_name)
        .ok_or_else(|| format!("quota '{quota_name}' not found"))?;
    table.quotas.remove(idx);
    Ok(String::new())
}

fn delete_limit(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let limit_name = tokens.expect("limit name")?.to_string();
    let table = rs.get_table_mut(family, &table_name)?;
    let idx = table
        .find_limit(&limit_name)
        .ok_or_else(|| format!("limit '{limit_name}' not found"))?;
    table.limits.remove(idx);
    Ok(String::new())
}

// ---------------------------------------------------------------------------
// list command
// ---------------------------------------------------------------------------

fn exec_list(
    rs: &Ruleset,
    flags: &Flags,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let obj_type = tokens.expect("object type to list")?;
    match obj_type {
        "ruleset" => list_ruleset(rs, flags),
        "tables" => list_tables(rs, flags),
        "table" => list_table(rs, flags, tokens),
        "chain" => list_chain(rs, flags, tokens),
        "chains" => list_all_chains(rs, flags),
        "sets" => list_all_sets(rs, flags, tokens),
        "set" => list_set(rs, flags, tokens),
        "maps" => list_all_maps(rs, flags, tokens),
        "map" => list_map(rs, flags, tokens),
        "counters" => list_all_counters(rs, tokens),
        "quotas" => list_all_quotas(rs, tokens),
        "limits" => list_all_limits(rs, tokens),
        _ => Err(format!("cannot list '{obj_type}'")),
    }
}

fn list_ruleset(rs: &Ruleset, flags: &Flags) -> Result<String, String> {
    if flags.json {
        return list_ruleset_json(rs);
    }
    let mut out = String::new();
    for table in &rs.tables {
        format_table_nft(&mut out, table, flags);
    }
    Ok(out)
}

fn list_ruleset_json(rs: &Ruleset) -> Result<String, String> {
    let mut out = String::from("{ \"nftables\": [\n");
    out.push_str("  { \"metainfo\": { \"json_schema_version\": 1 } }");
    for table in &rs.tables {
        out.push_str(",\n");
        out.push_str(&format!(
            "  {{ \"table\": {{ \"family\": \"{}\", \"name\": \"{}\" }} }}",
            table.family, table.name
        ));
        for chain in &table.chains {
            out.push_str(",\n");
            out.push_str(&format!(
                "  {{ \"chain\": {{ \"family\": \"{}\", \"table\": \"{}\", \"name\": \"{}\"",
                table.family, table.name, chain.name
            ));
            if let Some(ref cfg) = chain.base_config {
                out.push_str(&format!(
                    ", \"type\": \"{}\", \"hook\": \"{}\", \"prio\": {}, \"policy\": \"{}\"",
                    cfg.chain_type, cfg.hook, cfg.priority, cfg.policy
                ));
            }
            out.push_str(" } }");
            for rule in &chain.rules {
                out.push_str(",\n");
                out.push_str(&format!(
                    "  {{ \"rule\": {{ \"family\": \"{}\", \"table\": \"{}\", \"chain\": \"{}\", \"handle\": {}, \"expr\": \"{}\" }} }}",
                    table.family, table.name, chain.name, rule.handle,
                    rule.display_nft(false).replace('"', "\\\"")
                ));
            }
        }
        for set in &table.sets {
            out.push_str(",\n");
            out.push_str(&format!(
                "  {{ \"set\": {{ \"family\": \"{}\", \"table\": \"{}\", \"name\": \"{}\", \"type\": \"{}\" }} }}",
                table.family, table.name, set.name, set.key_type
            ));
        }
        for map in &table.maps {
            out.push_str(",\n");
            out.push_str(&format!(
                "  {{ \"map\": {{ \"family\": \"{}\", \"table\": \"{}\", \"name\": \"{}\", \"type\": \"{}\", \"map\": \"{}\" }} }}",
                table.family, table.name, map.name, map.key_type, map.value_type
            ));
        }
    }
    out.push_str("\n] }\n");
    Ok(out)
}

fn list_tables(rs: &Ruleset, flags: &Flags) -> Result<String, String> {
    if flags.json {
        let mut out = String::from("{ \"nftables\": [\n");
        out.push_str("  { \"metainfo\": { \"json_schema_version\": 1 } }");
        for table in &rs.tables {
            out.push_str(",\n");
            out.push_str(&format!(
                "  {{ \"table\": {{ \"family\": \"{}\", \"name\": \"{}\" }} }}",
                table.family, table.name
            ));
        }
        out.push_str("\n] }\n");
        return Ok(out);
    }
    let mut out = String::new();
    for table in &rs.tables {
        out.push_str(&format!("table {} {}\n", table.family, table.name));
    }
    Ok(out)
}

fn list_table(
    rs: &Ruleset,
    flags: &Flags,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let (family, name) = parse_family_and_name(tokens)?;
    let table = rs.get_table(family, &name)?;
    let mut out = String::new();
    format_table_nft(&mut out, table, flags);
    Ok(out)
}

fn list_chain(
    rs: &Ruleset,
    flags: &Flags,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let chain_name = tokens.expect("chain name")?.to_string();
    let table = rs.get_table(family, &table_name)?;
    let chain_idx = table
        .find_chain(&chain_name)
        .ok_or_else(|| format!("chain '{chain_name}' not found"))?;
    let chain = &table.chains[chain_idx];
    let mut out = String::new();
    format_chain_nft(&mut out, table, chain, flags, 1);
    Ok(out)
}

fn list_all_chains(rs: &Ruleset, flags: &Flags) -> Result<String, String> {
    let mut out = String::new();
    for table in &rs.tables {
        for chain in &table.chains {
            if chain.is_base() {
                out.push_str(&format!(
                    "table {} {} chain {} {{\n",
                    table.family, table.name, chain.name
                ));
                let cfg = chain.base_config.as_ref().unwrap();
                out.push_str(&format!(
                    "\ttype {} hook {} priority {}; policy {};\n",
                    cfg.chain_type, cfg.hook, cfg.priority, cfg.policy
                ));
            } else {
                out.push_str(&format!(
                    "table {} {} chain {} {{\n",
                    table.family, table.name, chain.name
                ));
            }
            for rule in &chain.rules {
                out.push_str(&format!(
                    "\t{}\n",
                    rule.display_nft(flags.show_handles)
                ));
            }
            out.push_str("}\n");
        }
    }
    Ok(out)
}

fn list_all_sets(
    rs: &Ruleset,
    flags: &Flags,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    // Optional family filter
    let family_filter = if let Some(tok) = tokens.peek() {
        Family::parse(tok).ok().map(|f| {
            tokens.next_token();
            f
        })
    } else {
        None
    };

    let _ = flags;
    let mut out = String::new();
    for table in &rs.tables {
        if let Some(ff) = family_filter {
            if table.family != ff {
                continue;
            }
        }
        for set in &table.sets {
            format_set_nft(&mut out, table, set);
        }
    }
    Ok(out)
}

fn list_set(
    rs: &Ruleset,
    _flags: &Flags,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let set_name = tokens.expect("set name")?.to_string();
    let table = rs.get_table(family, &table_name)?;
    let set_idx = table
        .find_set(&set_name)
        .ok_or_else(|| format!("set '{set_name}' not found"))?;
    let mut out = String::new();
    format_set_nft(&mut out, table, &table.sets[set_idx]);
    Ok(out)
}

fn list_all_maps(
    rs: &Ruleset,
    _flags: &Flags,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let family_filter = if let Some(tok) = tokens.peek() {
        Family::parse(tok).ok().map(|f| {
            tokens.next_token();
            f
        })
    } else {
        None
    };

    let mut out = String::new();
    for table in &rs.tables {
        if let Some(ff) = family_filter {
            if table.family != ff {
                continue;
            }
        }
        for map in &table.maps {
            format_map_nft(&mut out, table, map);
        }
    }
    Ok(out)
}

fn list_map(
    rs: &Ruleset,
    _flags: &Flags,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let (family, table_name) = parse_family_and_name(tokens)?;
    let map_name = tokens.expect("map name")?.to_string();
    let table = rs.get_table(family, &table_name)?;
    let map_idx = table
        .find_map(&map_name)
        .ok_or_else(|| format!("map '{map_name}' not found"))?;
    let mut out = String::new();
    format_map_nft(&mut out, table, &table.maps[map_idx]);
    Ok(out)
}

fn list_all_counters(
    rs: &Ruleset,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let family_filter = if let Some(tok) = tokens.peek() {
        Family::parse(tok).ok().map(|f| {
            tokens.next_token();
            f
        })
    } else {
        None
    };

    let mut out = String::new();
    for table in &rs.tables {
        if let Some(ff) = family_filter {
            if table.family != ff {
                continue;
            }
        }
        for counter in &table.counters {
            out.push_str(&format!(
                "table {} {} {{\n\tcounter {} {{\n\t\tpackets {} bytes {}\n\t}}\n}}\n",
                table.family, table.name, counter.name, counter.packets, counter.bytes
            ));
        }
    }
    Ok(out)
}

fn list_all_quotas(
    rs: &Ruleset,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let family_filter = if let Some(tok) = tokens.peek() {
        Family::parse(tok).ok().map(|f| {
            tokens.next_token();
            f
        })
    } else {
        None
    };

    let mut out = String::new();
    for table in &rs.tables {
        if let Some(ff) = family_filter {
            if table.family != ff {
                continue;
            }
        }
        for quota in &table.quotas {
            let inv_str = if quota.inv { "over " } else { "" };
            out.push_str(&format!(
                "table {} {} {{\n\tquota {} {{\n\t\t{inv_str}{} bytes used {} bytes\n\t}}\n}}\n",
                table.family, table.name, quota.name, quota.bytes_limit, quota.used
            ));
        }
    }
    Ok(out)
}

fn list_all_limits(
    rs: &Ruleset,
    tokens: &mut Tokens<'_>,
) -> Result<String, String> {
    let family_filter = if let Some(tok) = tokens.peek() {
        Family::parse(tok).ok().map(|f| {
            tokens.next_token();
            f
        })
    } else {
        None
    };

    let mut out = String::new();
    for table in &rs.tables {
        if let Some(ff) = family_filter {
            if table.family != ff {
                continue;
            }
        }
        for limit in &table.limits {
            out.push_str(&format!(
                "table {} {} {{\n\tlimit {} {{\n\t\trate {}/{}\n",
                table.family, table.name, limit.name, limit.rate, limit.unit
            ));
            if let Some(b) = limit.burst {
                out.push_str(&format!("\t\tburst {} packets\n", b));
            }
            out.push_str("\t}\n}\n");
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// flush command
// ---------------------------------------------------------------------------

fn exec_flush(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let obj_type = tokens.expect("object type to flush")?;
    match obj_type {
        "ruleset" => {
            rs.tables.clear();
            Ok(String::new())
        }
        "table" => {
            let (family, name) = parse_family_and_name(tokens)?;
            let table = rs.get_table_mut(family, &name)?;
            for chain in &mut table.chains {
                chain.rules.clear();
            }
            table.sets.iter_mut().for_each(|s| s.elements.clear());
            Ok(String::new())
        }
        "chain" => {
            let (family, table_name) = parse_family_and_name(tokens)?;
            let chain_name = tokens.expect("chain name")?.to_string();
            let table = rs.get_table_mut(family, &table_name)?;
            let idx = table
                .find_chain(&chain_name)
                .ok_or_else(|| format!("chain '{chain_name}' not found"))?;
            table.chains[idx].rules.clear();
            Ok(String::new())
        }
        "set" => {
            let (family, table_name) = parse_family_and_name(tokens)?;
            let set_name = tokens.expect("set name")?.to_string();
            let table = rs.get_table_mut(family, &table_name)?;
            let idx = table
                .find_set(&set_name)
                .ok_or_else(|| format!("set '{set_name}' not found"))?;
            table.sets[idx].elements.clear();
            Ok(String::new())
        }
        "map" => {
            let (family, table_name) = parse_family_and_name(tokens)?;
            let map_name = tokens.expect("map name")?.to_string();
            let table = rs.get_table_mut(family, &table_name)?;
            let idx = table
                .find_map(&map_name)
                .ok_or_else(|| format!("map '{map_name}' not found"))?;
            table.maps[idx].elements.clear();
            Ok(String::new())
        }
        _ => Err(format!("cannot flush '{obj_type}'")),
    }
}

// ---------------------------------------------------------------------------
// rename command
// ---------------------------------------------------------------------------

fn exec_rename(rs: &mut Ruleset, tokens: &mut Tokens<'_>) -> Result<String, String> {
    let obj_type = tokens.expect("object type to rename")?;
    if obj_type != "chain" {
        return Err(format!("can only rename chains, not '{obj_type}'"));
    }
    let (family, table_name) = parse_family_and_name(tokens)?;
    let old_name = tokens.expect("old chain name")?.to_string();
    let new_name = tokens.expect("new chain name")?.to_string();

    let table = rs.get_table_mut(family, &table_name)?;
    let idx = table
        .find_chain(&old_name)
        .ok_or_else(|| format!("chain '{old_name}' not found"))?;

    if table.find_chain(&new_name).is_some() {
        return Err(format!("chain '{new_name}' already exists"));
    }

    table.chains[idx].name = new_name;
    Ok(String::new())
}

// ---------------------------------------------------------------------------
// export command
// ---------------------------------------------------------------------------

fn exec_export(rs: &Ruleset, flags: &Flags) -> Result<String, String> {
    if flags.json {
        list_ruleset_json(rs)
    } else {
        list_ruleset(rs, flags)
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn format_table_nft(out: &mut String, table: &Table, flags: &Flags) {
    out.push_str(&format!("table {} {} {{\n", table.family, table.name));
    if table.flags_dormant {
        out.push_str("\tflags dormant\n");
    }

    for set in &table.sets {
        format_set_nft_inner(out, set, 1);
    }

    for map in &table.maps {
        format_map_nft_inner(out, map, 1);
    }

    for counter in &table.counters {
        out.push_str(&format!(
            "\tcounter {} {{\n\t\tpackets {} bytes {}\n\t}}\n",
            counter.name, counter.packets, counter.bytes
        ));
    }

    for quota in &table.quotas {
        let inv_str = if quota.inv { "over " } else { "" };
        out.push_str(&format!(
            "\tquota {} {{\n\t\t{inv_str}{} bytes used {} bytes\n\t}}\n",
            quota.name, quota.bytes_limit, quota.used
        ));
    }

    for limit in &table.limits {
        out.push_str(&format!("\tlimit {} {{\n\t\trate {}/{}\n", limit.name, limit.rate, limit.unit));
        if let Some(b) = limit.burst {
            out.push_str(&format!("\t\tburst {} packets\n", b));
        }
        out.push_str("\t}\n");
    }

    for chain in &table.chains {
        format_chain_nft(out, table, chain, flags, 1);
    }

    out.push_str("}\n");
}

fn format_chain_nft(
    out: &mut String,
    _table: &Table,
    chain: &Chain,
    flags: &Flags,
    indent: usize,
) {
    let prefix: String = "\t".repeat(indent);
    out.push_str(&format!("{prefix}chain {} {{\n", chain.name));
    if let Some(ref cfg) = chain.base_config {
        out.push_str(&format!(
            "{prefix}\ttype {} hook {} priority {}; policy {};\n",
            cfg.chain_type, cfg.hook, cfg.priority, cfg.policy
        ));
        if let Some(ref dev) = cfg.device {
            out.push_str(&format!("{prefix}\tdevice \"{dev}\"\n"));
        }
    }
    for rule in &chain.rules {
        out.push_str(&format!(
            "{prefix}\t{}\n",
            rule.display_nft(flags.show_handles)
        ));
    }
    out.push_str(&format!("{prefix}}}\n"));
}

fn format_set_nft(out: &mut String, table: &Table, set: &NamedSet) {
    out.push_str(&format!("table {} {} {{\n", table.family, table.name));
    format_set_nft_inner(out, set, 1);
    out.push_str("}\n");
}

fn format_set_nft_inner(out: &mut String, set: &NamedSet, indent: usize) {
    let prefix: String = "\t".repeat(indent);
    out.push_str(&format!("{prefix}set {} {{\n", set.name));
    if let Some(ref te) = set.typeof_expr {
        out.push_str(&format!("{prefix}\ttypeof {te}\n"));
    } else {
        out.push_str(&format!("{prefix}\ttype {}\n", set.key_type));
    }
    if !set.flags.is_empty() {
        let flag_strs: Vec<&str> = set.flags.iter().map(|f| f.as_str()).collect();
        out.push_str(&format!("{prefix}\tflags {}\n", flag_strs.join(",")));
    }
    if !set.elements.is_empty() {
        out.push_str(&format!(
            "{prefix}\telements = {{ {} }}\n",
            set.elements.join(", ")
        ));
    }
    out.push_str(&format!("{prefix}}}\n"));
}

fn format_map_nft(out: &mut String, table: &Table, map: &NamedMap) {
    out.push_str(&format!("table {} {} {{\n", table.family, table.name));
    format_map_nft_inner(out, map, 1);
    out.push_str("}\n");
}

fn format_map_nft_inner(out: &mut String, map: &NamedMap, indent: usize) {
    let prefix: String = "\t".repeat(indent);
    out.push_str(&format!("{prefix}map {} {{\n", map.name));
    out.push_str(&format!(
        "{prefix}\ttype {} : {}\n",
        map.key_type, map.value_type
    ));
    if !map.elements.is_empty() {
        let elems: Vec<String> = map
            .elements
            .iter()
            .map(|(k, v)| format!("{k} : {v}"))
            .collect();
        out.push_str(&format!(
            "{prefix}\telements = {{ {} }}\n",
            elems.join(", ")
        ));
    }
    out.push_str(&format!("{prefix}}}\n"));
}

// ---------------------------------------------------------------------------
// Tokenizer for command lines
// ---------------------------------------------------------------------------

/// Split a line into tokens, respecting double-quoted strings and semicolons
/// as separators.
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i] as char;
        if in_quote {
            if ch == '"' {
                in_quote = false;
                // Keep the quotes for certain tokens
                current.push(ch);
            } else {
                current.push(ch);
            }
        } else if ch == '#' {
            // Rest of line is a comment
            break;
        } else if ch == '"' {
            in_quote = true;
            current.push(ch);
        } else if ch == ';' {
            // Semicolons act as token separators
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            tokens.push(";".to_string());
        } else if ch.is_ascii_whitespace() {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
        i += 1;
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Split tokens by semicolons into separate command token-lists.
fn split_commands(tokens: &[String]) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut current = Vec::new();
    for tok in tokens {
        if tok == ";" {
            if !current.is_empty() {
                commands.push(current.clone());
                current.clear();
            }
        } else {
            current.push(tok.clone());
        }
    }
    if !current.is_empty() {
        commands.push(current);
    }
    commands
}

// ---------------------------------------------------------------------------
// Batch file processing
// ---------------------------------------------------------------------------

fn run_batch_file(
    rs: &mut Ruleset,
    flags: &Flags,
    path: &str,
) -> Result<String, String> {
    let content = if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("failed to read stdin: {e}"))?;
        buf
    } else {
        std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read '{path}': {e}"))?
    };

    run_batch_string(rs, flags, &content)
}

fn run_batch_string(
    rs: &mut Ruleset,
    flags: &Flags,
    content: &str,
) -> Result<String, String> {
    let mut output = String::new();
    let mut line_num = 0u64;

    for line in content.lines() {
        line_num = line_num.wrapping_add(1);
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let tokens = tokenize(line);
        let commands = split_commands(&tokens);
        for cmd_tokens in &commands {
            if cmd_tokens.is_empty() {
                continue;
            }
            match exec_command(rs, flags, cmd_tokens) {
                Ok(s) => output.push_str(&s),
                Err(e) => {
                    return Err(format!("line {line_num}: {e}"));
                }
            }
        }
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Usage / help
// ---------------------------------------------------------------------------

fn print_usage(prog: &str) -> String {
    format!(
        "\
Usage: {prog} [options] <command>

Options:
  -f <file>    Read commands from file (- for stdin)
  -j           JSON output format
  -n           Numeric output (don't resolve names)
  -a           Show rule handles
  -h, --help   Show this help

Commands:
  add table [<family>] <name>
  add chain [<family>] <table> <chain> [{{ type <type> hook <hook> priority <prio> ; policy <pol> ; }}]
  add rule [<family>] <table> <chain> <matches...> <verdicts...>
  add set [<family>] <table> <name> {{ type <type> ; [flags <f,...>] ; }}
  add map [<family>] <table> <name> {{ type <ktype> : <vtype> ; }}
  add element [<family>] <table> <set> {{ <elem1>, <elem2> }}
  add counter [<family>] <table> <name>
  add quota [<family>] <table> <name> [over] <limit> <unit>
  add limit [<family>] <table> <name> rate <N> <unit> [burst <N> packets]

  delete table|chain|rule|set|map|element|counter|quota|limit ...

  list ruleset
  list tables
  list table [<family>] <name>
  list chain [<family>] <table> <chain>
  list chains
  list sets [<family>]
  list set [<family>] <table> <name>
  list maps [<family>]
  list map [<family>] <table> <name>
  list counters [<family>]
  list quotas [<family>]
  list limits [<family>]

  flush ruleset|table|chain|set|map ...
  rename chain [<family>] <table> <old> <new>
  insert rule [<family>] <table> <chain> [position <handle>] <matches...> <verdicts...>
  export [-j]
  monitor

Families: ip, ip6, inet, arp, bridge, netdev

Match expressions:
  ip saddr|daddr [op] <addr>    tcp|udp dport|sport [op] <port>
  ip protocol [op] <proto>      ct state <state1,state2,...>
  iifname|oifname [op] <name>   meta mark|length|protocol|iiftype [op] <val>
  ether saddr|daddr [op] <mac>  icmp type [op] <type>
  <field> @<setname>            <field> {{ elem1, elem2 }}

Operators: == != < > <= >= &

Verdicts: accept drop reject queue continue return jump <chain> goto <chain> counter log [prefix <str>]
"
    )
}

// ---------------------------------------------------------------------------
// Personality: nft-list
// ---------------------------------------------------------------------------

fn run_nft_list(rs: &Ruleset, flags: &Flags) -> Result<String, String> {
    list_ruleset(rs, flags)
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn run(args: Vec<String>) -> Result<String, String> {
    // Personality detection
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("nft");
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

    let mut rs = Ruleset::new();
    let mut flags = Flags::new();

    match prog_name.as_str() {
        "nft-list" => {
            // Parse flags from remaining args
            let cmd_args = if args.len() > 1 { &args[1..] } else { &[] };
            for a in cmd_args {
                match a.as_str() {
                    "-j" => flags.json = true,
                    "-n" => flags.numeric = true,
                    "-a" => flags.show_handles = true,
                    _ => {}
                }
            }
            return run_nft_list(&rs, &flags);
        }
        _ => {} // default "nft" personality
    }

    // Parse flags and collect command words
    let mut cmd_words: Vec<String> = Vec::new();
    let mut batch_file: Option<String> = None;
    let mut i = 1;
    let mut interactive = false;

    while i < args.len() {
        match args[i].as_str() {
            "-j" => flags.json = true,
            "-n" => flags.numeric = true,
            "-a" => flags.show_handles = true,
            "-h" | "--help" => {
                return Ok(print_usage(&prog_name));
            }
            "-f" => {
                i += 1;
                if i >= args.len() {
                    return Err("-f requires a filename argument".to_string());
                }
                batch_file = Some(args[i].clone());
            }
            "-i" | "--interactive" => {
                interactive = true;
            }
            _ => {
                cmd_words.push(args[i].clone());
            }
        }
        i += 1;
    }

    // Batch file mode
    if let Some(ref path) = batch_file {
        return run_batch_file(&mut rs, &flags, path);
    }

    // Interactive mode
    if interactive {
        return run_interactive(&mut rs, &flags);
    }

    // Single command mode
    if cmd_words.is_empty() {
        return Err("no command specified (use -h for help)".to_string());
    }

    let tokens = tokenize(&cmd_words.join(" "));
    let commands = split_commands(&tokens);
    let mut output = String::new();
    for cmd_tokens in &commands {
        if cmd_tokens.is_empty() {
            continue;
        }
        output.push_str(&exec_command(&mut rs, &flags, cmd_tokens)?);
    }
    Ok(output)
}

fn run_interactive(rs: &mut Ruleset, flags: &Flags) -> Result<String, String> {
    let stdin = io::stdin();
    let mut output = String::new();
    for line_result in stdin.lock().lines() {
        let line = line_result.map_err(|e| format!("read error: {e}"))?;
        let line = line.trim().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "quit" || line == "exit" {
            break;
        }
        let tokens = tokenize(&line);
        let commands = split_commands(&tokens);
        for cmd_tokens in &commands {
            if cmd_tokens.is_empty() {
                continue;
            }
            match exec_command(rs, flags, cmd_tokens) {
                Ok(s) => {
                    if !s.is_empty() {
                        output.push_str(&s);
                        print!("{s}");
                    }
                }
                Err(e) => {
                    let msg = format!("Error: {e}\n");
                    output.push_str(&msg);
                    eprint!("{msg}");
                }
            }
        }
    }
    Ok(output)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match run(args) {
        Ok(output) => {
            if !output.is_empty() {
                print!("{output}");
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: run a command string against a fresh or provided ruleset
    fn run_cmd(rs: &mut Ruleset, cmd: &str) -> Result<String, String> {
        let flags = Flags::new();
        let tokens = tokenize(cmd);
        let commands = split_commands(&tokens);
        let mut output = String::new();
        for cmd_tokens in &commands {
            output.push_str(&exec_command(rs, &flags, cmd_tokens)?);
        }
        Ok(output)
    }

    fn run_cmd_flags(
        rs: &mut Ruleset,
        flags: &Flags,
        cmd: &str,
    ) -> Result<String, String> {
        let tokens = tokenize(cmd);
        let commands = split_commands(&tokens);
        let mut output = String::new();
        for cmd_tokens in &commands {
            output.push_str(&exec_command(rs, flags, cmd_tokens)?);
        }
        Ok(output)
    }

    // -----------------------------------------------------------------------
    // Family parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_family_parse_ip() {
        assert_eq!(Family::parse("ip").unwrap(), Family::Ip);
    }

    #[test]
    fn test_family_parse_ip6() {
        assert_eq!(Family::parse("ip6").unwrap(), Family::Ip6);
    }

    #[test]
    fn test_family_parse_inet() {
        assert_eq!(Family::parse("inet").unwrap(), Family::Inet);
    }

    #[test]
    fn test_family_parse_arp() {
        assert_eq!(Family::parse("arp").unwrap(), Family::Arp);
    }

    #[test]
    fn test_family_parse_bridge() {
        assert_eq!(Family::parse("bridge").unwrap(), Family::Bridge);
    }

    #[test]
    fn test_family_parse_netdev() {
        assert_eq!(Family::parse("netdev").unwrap(), Family::Netdev);
    }

    #[test]
    fn test_family_parse_unknown() {
        assert!(Family::parse("foo").is_err());
    }

    #[test]
    fn test_family_display() {
        assert_eq!(Family::Ip.to_string(), "ip");
        assert_eq!(Family::Ip6.to_string(), "ip6");
        assert_eq!(Family::Inet.to_string(), "inet");
        assert_eq!(Family::Bridge.to_string(), "bridge");
    }

    // -----------------------------------------------------------------------
    // Chain type parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_chain_type_parse_filter() {
        assert_eq!(ChainType::parse("filter").unwrap(), ChainType::Filter);
    }

    #[test]
    fn test_chain_type_parse_nat() {
        assert_eq!(ChainType::parse("nat").unwrap(), ChainType::Nat);
    }

    #[test]
    fn test_chain_type_parse_route() {
        assert_eq!(ChainType::parse("route").unwrap(), ChainType::Route);
    }

    #[test]
    fn test_chain_type_parse_unknown() {
        assert!(ChainType::parse("mangle").is_err());
    }

    #[test]
    fn test_chain_type_display() {
        assert_eq!(ChainType::Filter.to_string(), "filter");
        assert_eq!(ChainType::Nat.to_string(), "nat");
        assert_eq!(ChainType::Route.to_string(), "route");
    }

    // -----------------------------------------------------------------------
    // Hook parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_hook_parse_all() {
        assert_eq!(Hook::parse("prerouting").unwrap(), Hook::Prerouting);
        assert_eq!(Hook::parse("input").unwrap(), Hook::Input);
        assert_eq!(Hook::parse("forward").unwrap(), Hook::Forward);
        assert_eq!(Hook::parse("output").unwrap(), Hook::Output);
        assert_eq!(Hook::parse("postrouting").unwrap(), Hook::Postrouting);
        assert_eq!(Hook::parse("ingress").unwrap(), Hook::Ingress);
    }

    #[test]
    fn test_hook_parse_unknown() {
        assert!(Hook::parse("egress").is_err());
    }

    #[test]
    fn test_hook_display() {
        assert_eq!(Hook::Input.to_string(), "input");
        assert_eq!(Hook::Output.to_string(), "output");
    }

    // -----------------------------------------------------------------------
    // Policy parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_policy_parse() {
        assert_eq!(Policy::parse("accept").unwrap(), Policy::Accept);
        assert_eq!(Policy::parse("drop").unwrap(), Policy::Drop);
    }

    #[test]
    fn test_policy_parse_unknown() {
        assert!(Policy::parse("reject").is_err());
    }

    #[test]
    fn test_policy_display() {
        assert_eq!(Policy::Accept.to_string(), "accept");
        assert_eq!(Policy::Drop.to_string(), "drop");
    }

    // -----------------------------------------------------------------------
    // CmpOp
    // -----------------------------------------------------------------------

    #[test]
    fn test_cmp_op_parse_all() {
        assert_eq!(CmpOp::parse("=="), Some(CmpOp::Eq));
        assert_eq!(CmpOp::parse("!="), Some(CmpOp::Ne));
        assert_eq!(CmpOp::parse("<"), Some(CmpOp::Lt));
        assert_eq!(CmpOp::parse(">"), Some(CmpOp::Gt));
        assert_eq!(CmpOp::parse("<="), Some(CmpOp::Le));
        assert_eq!(CmpOp::parse(">="), Some(CmpOp::Ge));
        assert_eq!(CmpOp::parse("&"), Some(CmpOp::BitwiseAnd));
    }

    #[test]
    fn test_cmp_op_parse_none() {
        assert_eq!(CmpOp::parse("~"), None);
    }

    #[test]
    fn test_cmp_op_display() {
        assert_eq!(CmpOp::Eq.to_string(), "==");
        assert_eq!(CmpOp::Ne.to_string(), "!=");
        assert_eq!(CmpOp::BitwiseAnd.to_string(), "&");
    }

    // -----------------------------------------------------------------------
    // CtState
    // -----------------------------------------------------------------------

    #[test]
    fn test_ct_state_parse_all() {
        assert_eq!(CtState::parse("new").unwrap(), CtState::New);
        assert_eq!(CtState::parse("established").unwrap(), CtState::Established);
        assert_eq!(CtState::parse("related").unwrap(), CtState::Related);
        assert_eq!(CtState::parse("invalid").unwrap(), CtState::Invalid);
    }

    #[test]
    fn test_ct_state_parse_unknown() {
        assert!(CtState::parse("untracked").is_err());
    }

    #[test]
    fn test_ct_state_display() {
        assert_eq!(CtState::New.to_string(), "new");
        assert_eq!(CtState::Established.to_string(), "established");
    }

    // -----------------------------------------------------------------------
    // MetaKey
    // -----------------------------------------------------------------------

    #[test]
    fn test_meta_key_parse() {
        assert_eq!(MetaKey::parse("mark").unwrap(), MetaKey::Mark);
        assert_eq!(MetaKey::parse("length").unwrap(), MetaKey::Length);
        assert_eq!(MetaKey::parse("protocol").unwrap(), MetaKey::Protocol);
        assert_eq!(MetaKey::parse("iiftype").unwrap(), MetaKey::Iiftype);
    }

    #[test]
    fn test_meta_key_parse_unknown() {
        assert!(MetaKey::parse("nfmark").is_err());
    }

    // -----------------------------------------------------------------------
    // SetDataType
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_data_type_parse_all() {
        assert_eq!(SetDataType::parse("ipv4_addr").unwrap(), SetDataType::Ipv4Addr);
        assert_eq!(SetDataType::parse("ipv6_addr").unwrap(), SetDataType::Ipv6Addr);
        assert_eq!(SetDataType::parse("ether_addr").unwrap(), SetDataType::EtherAddr);
        assert_eq!(SetDataType::parse("inet_proto").unwrap(), SetDataType::InetProto);
        assert_eq!(SetDataType::parse("inet_service").unwrap(), SetDataType::InetService);
        assert_eq!(SetDataType::parse("mark").unwrap(), SetDataType::Mark);
        assert_eq!(SetDataType::parse("ifname").unwrap(), SetDataType::Ifname);
    }

    #[test]
    fn test_set_data_type_display() {
        assert_eq!(SetDataType::Ipv4Addr.to_string(), "ipv4_addr");
        assert_eq!(SetDataType::Mark.to_string(), "mark");
    }

    // -----------------------------------------------------------------------
    // SetFlag
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_flag_parse() {
        assert_eq!(SetFlag::parse("constant").unwrap(), SetFlag::Constant);
        assert_eq!(SetFlag::parse("interval").unwrap(), SetFlag::Interval);
        assert_eq!(SetFlag::parse("timeout").unwrap(), SetFlag::Timeout);
    }

    #[test]
    fn test_set_flag_parse_unknown() {
        assert!(SetFlag::parse("dynamic").is_err());
    }

    // -----------------------------------------------------------------------
    // LimitUnit
    // -----------------------------------------------------------------------

    #[test]
    fn test_limit_unit_parse() {
        assert_eq!(LimitUnit::parse("second").unwrap(), LimitUnit::Second);
        assert_eq!(LimitUnit::parse("sec").unwrap(), LimitUnit::Second);
        assert_eq!(LimitUnit::parse("minute").unwrap(), LimitUnit::Minute);
        assert_eq!(LimitUnit::parse("min").unwrap(), LimitUnit::Minute);
        assert_eq!(LimitUnit::parse("hour").unwrap(), LimitUnit::Hour);
        assert_eq!(LimitUnit::parse("day").unwrap(), LimitUnit::Day);
    }

    #[test]
    fn test_limit_unit_display() {
        assert_eq!(LimitUnit::Second.to_string(), "second");
        assert_eq!(LimitUnit::Day.to_string(), "day");
    }

    // -----------------------------------------------------------------------
    // Verdict display
    // -----------------------------------------------------------------------

    #[test]
    fn test_verdict_display_simple() {
        assert_eq!(Verdict::Accept.to_string(), "accept");
        assert_eq!(Verdict::Drop.to_string(), "drop");
        assert_eq!(Verdict::Reject.to_string(), "reject");
        assert_eq!(Verdict::Queue.to_string(), "queue");
        assert_eq!(Verdict::Continue.to_string(), "continue");
        assert_eq!(Verdict::Return.to_string(), "return");
        assert_eq!(Verdict::Counter.to_string(), "counter");
    }

    #[test]
    fn test_verdict_display_jump() {
        assert_eq!(Verdict::Jump("mychain".to_string()).to_string(), "jump mychain");
    }

    #[test]
    fn test_verdict_display_goto() {
        assert_eq!(Verdict::Goto("other".to_string()).to_string(), "goto other");
    }

    #[test]
    fn test_verdict_display_log_no_prefix() {
        assert_eq!(Verdict::Log { prefix: None }.to_string(), "log");
    }

    #[test]
    fn test_verdict_display_log_with_prefix() {
        assert_eq!(
            Verdict::Log {
                prefix: Some("DROP:".to_string())
            }
            .to_string(),
            "log prefix \"DROP:\""
        );
    }

    // -----------------------------------------------------------------------
    // Tokenizer
    // -----------------------------------------------------------------------

    #[test]
    fn test_tokenize_simple() {
        let tokens = tokenize("add table inet filter");
        assert_eq!(tokens, vec!["add", "table", "inet", "filter"]);
    }

    #[test]
    fn test_tokenize_quoted() {
        let tokens = tokenize("iifname \"eth0\"");
        assert_eq!(tokens, vec!["iifname", "\"eth0\""]);
    }

    #[test]
    fn test_tokenize_semicolons() {
        let tokens = tokenize("add table ip t; add chain ip t c");
        assert_eq!(
            tokens,
            vec!["add", "table", "ip", "t", ";", "add", "chain", "ip", "t", "c"]
        );
    }

    #[test]
    fn test_tokenize_comment() {
        let tokens = tokenize("add table ip t # this is a comment");
        assert_eq!(tokens, vec!["add", "table", "ip", "t"]);
    }

    #[test]
    fn test_tokenize_empty() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_tokenize_whitespace_only() {
        let tokens = tokenize("   \t  ");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_split_commands_single() {
        let tokens = tokenize("add table ip filter");
        let cmds = split_commands(&tokens);
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_split_commands_multiple() {
        let tokens = tokenize("add table ip t ; add chain ip t c");
        let cmds = split_commands(&tokens);
        assert_eq!(cmds.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Table operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_table_ip() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table ip filter").unwrap();
        assert_eq!(rs.tables.len(), 1);
        assert_eq!(rs.tables[0].family, Family::Ip);
        assert_eq!(rs.tables[0].name, "filter");
    }

    #[test]
    fn test_add_table_inet() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet mytable").unwrap();
        assert_eq!(rs.tables[0].family, Family::Inet);
        assert_eq!(rs.tables[0].name, "mytable");
    }

    #[test]
    fn test_add_table_default_family() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table filter").unwrap();
        assert_eq!(rs.tables[0].family, Family::Ip);
    }

    #[test]
    fn test_add_table_idempotent() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        assert_eq!(rs.tables.len(), 1);
    }

    #[test]
    fn test_add_table_dormant() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter flags dormant").unwrap();
        assert!(rs.tables[0].flags_dormant);
    }

    #[test]
    fn test_delete_table() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table ip filter").unwrap();
        run_cmd(&mut rs, "delete table ip filter").unwrap();
        assert!(rs.tables.is_empty());
    }

    #[test]
    fn test_delete_table_not_found() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "delete table ip noexist");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_tables() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table ip filter").unwrap();
        run_cmd(&mut rs, "add table ip6 filter6").unwrap();
        let out = run_cmd(&mut rs, "list tables").unwrap();
        assert!(out.contains("table ip filter"));
        assert!(out.contains("table ip6 filter6"));
    }

    #[test]
    fn test_list_tables_empty() {
        let mut rs = Ruleset::new();
        let out = run_cmd(&mut rs, "list tables").unwrap();
        assert!(out.is_empty());
    }

    // -----------------------------------------------------------------------
    // Chain operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_regular_chain() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        assert_eq!(rs.tables[0].chains.len(), 1);
        assert_eq!(rs.tables[0].chains[0].name, "input");
        assert!(!rs.tables[0].chains[0].is_base());
    }

    #[test]
    fn test_add_base_chain() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add chain inet filter input type filter hook input priority 0 ; policy accept ;",
        )
        .unwrap();
        assert!(rs.tables[0].chains[0].is_base());
        let cfg = rs.tables[0].chains[0].base_config.as_ref().unwrap();
        assert_eq!(cfg.chain_type, ChainType::Filter);
        assert_eq!(cfg.hook, Hook::Input);
        assert_eq!(cfg.priority, 0);
        assert_eq!(cfg.policy, Policy::Accept);
    }

    #[test]
    fn test_add_chain_negative_priority() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table ip nat").unwrap();
        run_cmd(
            &mut rs,
            "add chain ip nat prerouting type nat hook prerouting priority -100 ; policy accept ;",
        )
        .unwrap();
        let cfg = rs.tables[0].chains[0].base_config.as_ref().unwrap();
        assert_eq!(cfg.priority, -100);
    }

    #[test]
    fn test_add_chain_duplicate_error() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        let result = run_cmd(&mut rs, "add chain inet filter input");
        assert!(result.is_err());
    }

    #[test]
    fn test_add_chain_table_not_found() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "add chain inet noexist input");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_chain_empty() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter mychain").unwrap();
        run_cmd(&mut rs, "delete chain inet filter mychain").unwrap();
        assert!(rs.tables[0].chains.is_empty());
    }

    #[test]
    fn test_delete_chain_not_empty_error() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter mychain").unwrap();
        run_cmd(&mut rs, "add rule inet filter mychain accept").unwrap();
        let result = run_cmd(&mut rs, "delete chain inet filter mychain");
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_chain() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter old").unwrap();
        run_cmd(&mut rs, "rename chain inet filter old new").unwrap();
        assert_eq!(rs.tables[0].chains[0].name, "new");
    }

    #[test]
    fn test_rename_chain_not_found() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        let result = run_cmd(&mut rs, "rename chain inet filter noexist newname");
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_chain_target_exists() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter a").unwrap();
        run_cmd(&mut rs, "add chain inet filter b").unwrap();
        let result = run_cmd(&mut rs, "rename chain inet filter a b");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Rule operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_rule_simple_accept() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        assert_eq!(rs.tables[0].chains[0].rules.len(), 1);
        assert_eq!(rs.tables[0].chains[0].rules[0].verdicts.len(), 1);
    }

    #[test]
    fn test_add_rule_drop() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input drop").unwrap();
        assert_eq!(rs.tables[0].chains[0].rules[0].verdicts[0], Verdict::Drop);
    }

    #[test]
    fn test_add_rule_ip_saddr_match() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input ip saddr 192.168.1.0/24 accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.matches.len(), 1);
    }

    #[test]
    fn test_add_rule_ip_daddr_match() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter output").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter output ip daddr 10.0.0.1 drop",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.matches.len(), 1);
        assert_eq!(rule.verdicts[0], Verdict::Drop);
    }

    #[test]
    fn test_add_rule_tcp_dport() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input tcp dport 80 accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.matches.len(), 1);
    }

    #[test]
    fn test_add_rule_udp_dport() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input udp dport 53 accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.matches.len(), 1);
    }

    #[test]
    fn test_add_rule_ct_state() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input ct state established,related accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::CtState { states } => {
                assert_eq!(states.len(), 2);
                assert_eq!(states[0], CtState::Established);
                assert_eq!(states[1], CtState::Related);
            }
            _ => panic!("expected CtState match"),
        }
    }

    #[test]
    fn test_add_rule_iifname() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input iifname \"eth0\" accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::Iifname { value, .. } => assert_eq!(value, "eth0"),
            _ => panic!("expected Iifname match"),
        }
    }

    #[test]
    fn test_add_rule_oifname() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter output").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter output oifname \"lo\" accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::Oifname { value, .. } => assert_eq!(value, "lo"),
            _ => panic!("expected Oifname match"),
        }
    }

    #[test]
    fn test_add_rule_meta_mark() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input meta mark 0x42 accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::Meta { key, value, .. } => {
                assert_eq!(*key, MetaKey::Mark);
                assert_eq!(value, "0x42");
            }
            _ => panic!("expected Meta match"),
        }
    }

    #[test]
    fn test_add_rule_ether_saddr() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table bridge filter").unwrap();
        run_cmd(&mut rs, "add chain bridge filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule bridge filter input ether saddr aa:bb:cc:dd:ee:ff drop",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::EtherSaddr { value, .. } => assert_eq!(value, "aa:bb:cc:dd:ee:ff"),
            _ => panic!("expected EtherSaddr match"),
        }
    }

    #[test]
    fn test_add_rule_icmp_type() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input icmp type echo-request accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::IcmpType { value, .. } => assert_eq!(value, "echo-request"),
            _ => panic!("expected IcmpType match"),
        }
    }

    #[test]
    fn test_add_rule_counter_verdict() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input counter accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.verdicts.len(), 2);
        assert_eq!(rule.verdicts[0], Verdict::Counter);
        assert_eq!(rule.verdicts[1], Verdict::Accept);
    }

    #[test]
    fn test_add_rule_log_verdict() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input log drop",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.verdicts.len(), 2);
        assert_eq!(rule.verdicts[0], Verdict::Log { prefix: None });
        assert_eq!(rule.verdicts[1], Verdict::Drop);
    }

    #[test]
    fn test_add_rule_jump() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add chain inet filter mychain").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input jump mychain",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.verdicts[0], Verdict::Jump("mychain".to_string()));
    }

    #[test]
    fn test_add_rule_goto() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add chain inet filter other").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input goto other",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.verdicts[0], Verdict::Goto("other".to_string()));
    }

    #[test]
    fn test_add_rule_with_operator() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input tcp dport != 22 drop",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::TcpDport { op, value } => {
                assert_eq!(*op, CmpOp::Ne);
                assert_eq!(value, "22");
            }
            _ => panic!("expected TcpDport match"),
        }
    }

    #[test]
    fn test_add_rule_ip_protocol() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input ip protocol tcp accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::IpProtocol { value, .. } => assert_eq!(value, "tcp"),
            _ => panic!("expected IpProtocol match"),
        }
    }

    #[test]
    fn test_add_multiple_rules() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        run_cmd(&mut rs, "add rule inet filter input drop").unwrap();
        assert_eq!(rs.tables[0].chains[0].rules.len(), 2);
    }

    #[test]
    fn test_add_rule_handles_increment() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        run_cmd(&mut rs, "add rule inet filter input drop").unwrap();
        assert_ne!(
            rs.tables[0].chains[0].rules[0].handle,
            rs.tables[0].chains[0].rules[1].handle
        );
    }

    #[test]
    fn test_delete_rule_by_handle() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        let handle = rs.tables[0].chains[0].rules[0].handle;
        run_cmd(
            &mut rs,
            &format!("delete rule inet filter input handle {handle}"),
        )
        .unwrap();
        assert!(rs.tables[0].chains[0].rules.is_empty());
    }

    #[test]
    fn test_delete_rule_handle_not_found() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        let result = run_cmd(&mut rs, "delete rule inet filter input handle 999");
        assert!(result.is_err());
    }

    #[test]
    fn test_insert_rule_beginning() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        run_cmd(&mut rs, "insert rule inet filter input drop").unwrap();
        assert_eq!(rs.tables[0].chains[0].rules[0].verdicts[0], Verdict::Drop);
        assert_eq!(
            rs.tables[0].chains[0].rules[1].verdicts[0],
            Verdict::Accept
        );
    }

    // -----------------------------------------------------------------------
    // Set operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_set() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; }",
        )
        .unwrap();
        assert_eq!(rs.tables[0].sets.len(), 1);
        assert_eq!(rs.tables[0].sets[0].name, "myset");
        assert_eq!(rs.tables[0].sets[0].key_type, SetDataType::Ipv4Addr);
    }

    #[test]
    fn test_add_set_with_flags() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter blocked { type ipv4_addr ; flags interval ; }",
        )
        .unwrap();
        assert_eq!(rs.tables[0].sets[0].flags, vec![SetFlag::Interval]);
    }

    #[test]
    fn test_add_set_with_elements() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; elements = { 1.2.3.4, 5.6.7.8 } }",
        )
        .unwrap();
        assert_eq!(rs.tables[0].sets[0].elements.len(), 2);
    }

    #[test]
    fn test_add_set_duplicate_error() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; }",
        )
        .unwrap();
        let result = run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; }",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_add_element_to_set() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; }",
        )
        .unwrap();
        run_cmd(
            &mut rs,
            "add element inet filter myset { 10.0.0.1, 10.0.0.2 }",
        )
        .unwrap();
        assert_eq!(rs.tables[0].sets[0].elements.len(), 2);
    }

    #[test]
    fn test_delete_element_from_set() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; }",
        )
        .unwrap();
        run_cmd(
            &mut rs,
            "add element inet filter myset { 10.0.0.1, 10.0.0.2 }",
        )
        .unwrap();
        run_cmd(
            &mut rs,
            "delete element inet filter myset { 10.0.0.1 }",
        )
        .unwrap();
        assert_eq!(rs.tables[0].sets[0].elements.len(), 1);
        assert_eq!(rs.tables[0].sets[0].elements[0], "10.0.0.2");
    }

    #[test]
    fn test_delete_set() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; }",
        )
        .unwrap();
        run_cmd(&mut rs, "delete set inet filter myset").unwrap();
        assert!(rs.tables[0].sets.is_empty());
    }

    #[test]
    fn test_flush_set() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; }",
        )
        .unwrap();
        run_cmd(
            &mut rs,
            "add element inet filter myset { 1.2.3.4 }",
        )
        .unwrap();
        run_cmd(&mut rs, "flush set inet filter myset").unwrap();
        assert!(rs.tables[0].sets[0].elements.is_empty());
    }

    #[test]
    fn test_list_set() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter blocked { type ipv4_addr ; }",
        )
        .unwrap();
        run_cmd(
            &mut rs,
            "add element inet filter blocked { 1.2.3.4 }",
        )
        .unwrap();
        let out = run_cmd(&mut rs, "list set inet filter blocked").unwrap();
        assert!(out.contains("blocked"));
        assert!(out.contains("ipv4_addr"));
        assert!(out.contains("1.2.3.4"));
    }

    // -----------------------------------------------------------------------
    // Map operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_map() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add map inet filter mymap { type ipv4_addr : inet_service ; }",
        )
        .unwrap();
        assert_eq!(rs.tables[0].maps.len(), 1);
        assert_eq!(rs.tables[0].maps[0].name, "mymap");
        assert_eq!(rs.tables[0].maps[0].key_type, SetDataType::Ipv4Addr);
        assert_eq!(rs.tables[0].maps[0].value_type, SetDataType::InetService);
    }

    #[test]
    fn test_add_map_duplicate_error() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add map inet filter m { type ipv4_addr : mark ; }",
        )
        .unwrap();
        let result = run_cmd(
            &mut rs,
            "add map inet filter m { type ipv4_addr : mark ; }",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_map() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add map inet filter m { type ipv4_addr : mark ; }",
        )
        .unwrap();
        run_cmd(&mut rs, "delete map inet filter m").unwrap();
        assert!(rs.tables[0].maps.is_empty());
    }

    #[test]
    fn test_flush_map() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add map inet filter m { type ipv4_addr : mark ; }",
        )
        .unwrap();
        run_cmd(&mut rs, "flush map inet filter m").unwrap();
        assert!(rs.tables[0].maps[0].elements.is_empty());
    }

    // -----------------------------------------------------------------------
    // Counter operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_counter() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add counter inet filter mycnt").unwrap();
        assert_eq!(rs.tables[0].counters.len(), 1);
        assert_eq!(rs.tables[0].counters[0].name, "mycnt");
        assert_eq!(rs.tables[0].counters[0].packets, 0);
    }

    #[test]
    fn test_add_counter_duplicate_error() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add counter inet filter cnt").unwrap();
        let result = run_cmd(&mut rs, "add counter inet filter cnt");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_counter() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add counter inet filter cnt").unwrap();
        run_cmd(&mut rs, "delete counter inet filter cnt").unwrap();
        assert!(rs.tables[0].counters.is_empty());
    }

    #[test]
    fn test_list_counters() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add counter inet filter cnt1").unwrap();
        let out = run_cmd(&mut rs, "list counters").unwrap();
        assert!(out.contains("cnt1"));
        assert!(out.contains("packets 0 bytes 0"));
    }

    // -----------------------------------------------------------------------
    // Quota operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_quota() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add quota inet filter q1 1024 bytes").unwrap();
        assert_eq!(rs.tables[0].quotas.len(), 1);
        assert_eq!(rs.tables[0].quotas[0].bytes_limit, 1024);
        assert!(!rs.tables[0].quotas[0].inv);
    }

    #[test]
    fn test_add_quota_over() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add quota inet filter q1 over 1 mbytes").unwrap();
        assert!(rs.tables[0].quotas[0].inv);
        assert_eq!(rs.tables[0].quotas[0].bytes_limit, 1024 * 1024);
    }

    #[test]
    fn test_delete_quota() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add quota inet filter q1 100 bytes").unwrap();
        run_cmd(&mut rs, "delete quota inet filter q1").unwrap();
        assert!(rs.tables[0].quotas.is_empty());
    }

    #[test]
    fn test_list_quotas() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add quota inet filter q1 1024 bytes").unwrap();
        let out = run_cmd(&mut rs, "list quotas").unwrap();
        assert!(out.contains("q1"));
        assert!(out.contains("1024 bytes"));
    }

    // -----------------------------------------------------------------------
    // Limit operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_limit() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add limit inet filter lim1 rate 100 second").unwrap();
        assert_eq!(rs.tables[0].limits.len(), 1);
        assert_eq!(rs.tables[0].limits[0].rate, 100);
        assert_eq!(rs.tables[0].limits[0].unit, LimitUnit::Second);
    }

    #[test]
    fn test_add_limit_with_burst() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add limit inet filter lim1 rate 50 minute burst 20 packets",
        )
        .unwrap();
        assert_eq!(rs.tables[0].limits[0].rate, 50);
        assert_eq!(rs.tables[0].limits[0].unit, LimitUnit::Minute);
        assert_eq!(rs.tables[0].limits[0].burst, Some(20));
    }

    #[test]
    fn test_delete_limit() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add limit inet filter lim1 rate 10 second").unwrap();
        run_cmd(&mut rs, "delete limit inet filter lim1").unwrap();
        assert!(rs.tables[0].limits.is_empty());
    }

    #[test]
    fn test_list_limits() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add limit inet filter lim1 rate 100 hour").unwrap();
        let out = run_cmd(&mut rs, "list limits").unwrap();
        assert!(out.contains("lim1"));
        assert!(out.contains("100/hour"));
    }

    // -----------------------------------------------------------------------
    // Flush operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_flush_ruleset() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add table ip nat").unwrap();
        run_cmd(&mut rs, "flush ruleset").unwrap();
        assert!(rs.tables.is_empty());
    }

    #[test]
    fn test_flush_table() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        run_cmd(&mut rs, "flush table inet filter").unwrap();
        assert!(rs.tables[0].chains[0].rules.is_empty());
    }

    #[test]
    fn test_flush_chain() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        run_cmd(&mut rs, "add rule inet filter input drop").unwrap();
        run_cmd(&mut rs, "flush chain inet filter input").unwrap();
        assert!(rs.tables[0].chains[0].rules.is_empty());
    }

    // -----------------------------------------------------------------------
    // List/export operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_ruleset_empty() {
        let mut rs = Ruleset::new();
        let out = run_cmd(&mut rs, "list ruleset").unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn test_list_ruleset_with_table() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        let out = run_cmd(&mut rs, "list ruleset").unwrap();
        assert!(out.contains("table inet filter"));
    }

    #[test]
    fn test_list_ruleset_with_chain_and_rule() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add chain inet filter input type filter hook input priority 0 ; policy accept ;",
        )
        .unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        let out = run_cmd(&mut rs, "list ruleset").unwrap();
        assert!(out.contains("chain input"));
        assert!(out.contains("type filter hook input priority 0; policy accept;"));
        assert!(out.contains("accept"));
    }

    #[test]
    fn test_list_table() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        let out = run_cmd(&mut rs, "list table inet filter").unwrap();
        assert!(out.contains("table inet filter"));
        assert!(out.contains("chain input"));
    }

    #[test]
    fn test_list_table_not_found() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "list table inet noexist");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_chain() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        let out = run_cmd(&mut rs, "list chain inet filter input").unwrap();
        assert!(out.contains("chain input"));
        assert!(out.contains("accept"));
    }

    #[test]
    fn test_list_chains() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add chain inet filter output").unwrap();
        let out = run_cmd(&mut rs, "list chains").unwrap();
        assert!(out.contains("chain input"));
        assert!(out.contains("chain output"));
    }

    #[test]
    fn test_list_with_handles() {
        let mut rs = Ruleset::new();
        let mut flags = Flags::new();
        flags.show_handles = true;
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        let out = run_cmd_flags(&mut rs, &flags, "list ruleset").unwrap();
        assert!(out.contains("# handle"));
    }

    #[test]
    fn test_list_sets() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter myset { type ipv4_addr ; }",
        )
        .unwrap();
        let out = run_cmd(&mut rs, "list sets").unwrap();
        assert!(out.contains("myset"));
    }

    #[test]
    fn test_list_maps() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add map inet filter m { type ipv4_addr : mark ; }",
        )
        .unwrap();
        let out = run_cmd(&mut rs, "list maps").unwrap();
        assert!(out.contains("map m"));
    }

    // -----------------------------------------------------------------------
    // JSON output
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_tables_json() {
        let mut rs = Ruleset::new();
        let mut flags = Flags::new();
        flags.json = true;
        run_cmd(&mut rs, "add table inet filter").unwrap();
        let out = run_cmd_flags(&mut rs, &flags, "list tables").unwrap();
        assert!(out.contains("\"nftables\""));
        assert!(out.contains("\"family\": \"inet\""));
        assert!(out.contains("\"name\": \"filter\""));
    }

    #[test]
    fn test_list_ruleset_json() {
        let mut rs = Ruleset::new();
        let mut flags = Flags::new();
        flags.json = true;
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        let out = run_cmd_flags(&mut rs, &flags, "list ruleset").unwrap();
        assert!(out.contains("\"nftables\""));
        assert!(out.contains("\"chain\""));
    }

    #[test]
    fn test_export_json() {
        let mut rs = Ruleset::new();
        let mut flags = Flags::new();
        flags.json = true;
        run_cmd(&mut rs, "add table inet filter").unwrap();
        let out = run_cmd_flags(&mut rs, &flags, "export").unwrap();
        assert!(out.contains("\"nftables\""));
    }

    #[test]
    fn test_export_nft() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        let out = run_cmd(&mut rs, "export").unwrap();
        assert!(out.contains("table inet filter"));
    }

    // -----------------------------------------------------------------------
    // Batch / multiple commands
    // -----------------------------------------------------------------------

    #[test]
    fn test_semicolon_separated_commands() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter ; add chain inet filter input").unwrap();
        assert_eq!(rs.tables.len(), 1);
        assert_eq!(rs.tables[0].chains.len(), 1);
    }

    #[test]
    fn test_batch_string() {
        let mut rs = Ruleset::new();
        let flags = Flags::new();
        let batch = "add table inet filter\nadd chain inet filter input\nadd rule inet filter input accept\n";
        let output = run_batch_string(&mut rs, &flags, batch).unwrap();
        assert!(output.is_empty());
        assert_eq!(rs.tables[0].chains[0].rules.len(), 1);
    }

    #[test]
    fn test_batch_string_with_comments() {
        let mut rs = Ruleset::new();
        let flags = Flags::new();
        let batch = "# Create filter table\nadd table inet filter\n# Done\n";
        run_batch_string(&mut rs, &flags, batch).unwrap();
        assert_eq!(rs.tables.len(), 1);
    }

    #[test]
    fn test_batch_string_empty_lines() {
        let mut rs = Ruleset::new();
        let flags = Flags::new();
        let batch = "\n\nadd table inet filter\n\n\n";
        run_batch_string(&mut rs, &flags, batch).unwrap();
        assert_eq!(rs.tables.len(), 1);
    }

    #[test]
    fn test_batch_string_error_reports_line() {
        let mut rs = Ruleset::new();
        let flags = Flags::new();
        let batch = "add table inet filter\ndelete table inet noexist\n";
        let result = run_batch_string(&mut rs, &flags, batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("line 2"));
    }

    // -----------------------------------------------------------------------
    // Atomic rule replacement
    // -----------------------------------------------------------------------

    #[test]
    fn test_atomic_rule_replacement() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input accept").unwrap();
        run_cmd(&mut rs, "add rule inet filter input drop").unwrap();
        assert_eq!(rs.tables[0].chains[0].rules.len(), 2);
        // Flush + re-add
        run_cmd(&mut rs, "flush chain inet filter input").unwrap();
        assert!(rs.tables[0].chains[0].rules.is_empty());
        run_cmd(&mut rs, "add rule inet filter input reject").unwrap();
        assert_eq!(rs.tables[0].chains[0].rules.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Display / formatting
    // -----------------------------------------------------------------------

    #[test]
    fn test_match_expr_display_ip_saddr() {
        let m = MatchExpr::IpSaddr {
            op: CmpOp::Eq,
            value: "10.0.0.0/8".to_string(),
        };
        assert_eq!(m.to_string(), "ip saddr == 10.0.0.0/8");
    }

    #[test]
    fn test_match_expr_display_tcp_dport() {
        let m = MatchExpr::TcpDport {
            op: CmpOp::Eq,
            value: "443".to_string(),
        };
        assert_eq!(m.to_string(), "tcp dport == 443");
    }

    #[test]
    fn test_match_expr_display_ct_state() {
        let m = MatchExpr::CtState {
            states: vec![CtState::New, CtState::Established],
        };
        assert_eq!(m.to_string(), "ct state new,established");
    }

    #[test]
    fn test_match_expr_display_iifname() {
        let m = MatchExpr::Iifname {
            op: CmpOp::Eq,
            value: "eth0".to_string(),
        };
        assert_eq!(m.to_string(), "iifname == \"eth0\"");
    }

    #[test]
    fn test_match_expr_display_meta() {
        let m = MatchExpr::Meta {
            key: MetaKey::Mark,
            op: CmpOp::Ne,
            value: "0".to_string(),
        };
        assert_eq!(m.to_string(), "meta mark != 0");
    }

    #[test]
    fn test_match_expr_display_set_lookup() {
        let m = MatchExpr::SetLookup {
            field: "ip saddr".to_string(),
            set_name: "blocklist".to_string(),
        };
        assert_eq!(m.to_string(), "ip saddr @blocklist");
    }

    #[test]
    fn test_match_expr_display_anon_set() {
        let m = MatchExpr::AnonSet {
            field: "tcp dport".to_string(),
            elements: vec!["80".to_string(), "443".to_string()],
        };
        assert_eq!(m.to_string(), "tcp dport { 80, 443 }");
    }

    #[test]
    fn test_match_expr_display_interval() {
        let m = MatchExpr::Interval {
            field: "tcp dport".to_string(),
            low: "1024".to_string(),
            high: "65535".to_string(),
        };
        assert_eq!(m.to_string(), "tcp dport { 1024-65535 }");
    }

    #[test]
    fn test_rule_display_nft_no_handle() {
        let rule = Rule::new(
            1,
            vec![MatchExpr::TcpDport {
                op: CmpOp::Eq,
                value: "80".to_string(),
            }],
            vec![Verdict::Accept],
        );
        let display = rule.display_nft(false);
        assert_eq!(display, "tcp dport == 80 accept");
        assert!(!display.contains("handle"));
    }

    #[test]
    fn test_rule_display_nft_with_handle() {
        let rule = Rule::new(42, vec![], vec![Verdict::Drop]);
        let display = rule.display_nft(true);
        assert!(display.contains("# handle 42"));
    }

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_nft() {
        let args = vec!["nft".to_string(), "list".to_string(), "ruleset".to_string()];
        let result = run(args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_personality_nft_exe() {
        let args = vec![
            "C:\\Programs\\nft.exe".to_string(),
            "list".to_string(),
            "ruleset".to_string(),
        ];
        let result = run(args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_personality_nft_unix_path() {
        let args = vec![
            "/usr/sbin/nft".to_string(),
            "list".to_string(),
            "ruleset".to_string(),
        ];
        let result = run(args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_personality_nft_list() {
        let args = vec!["nft-list".to_string()];
        let result = run(args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_personality_nft_list_json() {
        let args = vec!["nft-list".to_string(), "-j".to_string()];
        let result = run(args);
        assert!(result.is_ok());
        let out = result.unwrap();
        assert!(out.contains("nftables"));
    }

    // -----------------------------------------------------------------------
    // Help / usage
    // -----------------------------------------------------------------------

    #[test]
    fn test_help_flag() {
        let args = vec!["nft".to_string(), "-h".to_string()];
        let result = run(args);
        assert!(result.is_ok());
        let out = result.unwrap();
        assert!(out.contains("Usage:"));
    }

    #[test]
    fn test_help_flag_long() {
        let args = vec!["nft".to_string(), "--help".to_string()];
        let result = run(args);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Usage:"));
    }

    // -----------------------------------------------------------------------
    // Error handling
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_command_error() {
        let args = vec!["nft".to_string()];
        let result = run(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_command_error() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "frobnicate table inet filter");
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_object_type_add() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "add frobnicator inet filter foo");
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_object_type_delete() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "delete frobnicator inet filter foo");
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_object_type_list() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "list frobnicators");
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_object_type_flush() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "flush frobnicator");
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_non_chain_error() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "rename table inet filter newname");
        assert!(result.is_err());
    }

    #[test]
    fn test_insert_non_rule_error() {
        let mut rs = Ruleset::new();
        let result = run_cmd(&mut rs, "insert chain inet filter mychain");
        assert!(result.is_err());
    }

    #[test]
    fn test_flag_f_missing_filename() {
        let args = vec!["nft".to_string(), "-f".to_string()];
        let result = run(args);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Full workflow tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_firewall_setup_workflow() {
        let mut rs = Ruleset::new();
        let flags = Flags::new();
        let batch = "\
add table inet filter
add chain inet filter input type filter hook input priority 0 ; policy drop ;
add chain inet filter forward type filter hook forward priority 0 ; policy drop ;
add chain inet filter output type filter hook output priority 0 ; policy accept ;
add rule inet filter input ct state established,related accept
add rule inet filter input iifname \"lo\" accept
add rule inet filter input tcp dport 22 accept
add rule inet filter input tcp dport 80 accept
add rule inet filter input tcp dport 443 accept
add rule inet filter input icmp type echo-request accept
add rule inet filter input counter drop
";
        run_batch_string(&mut rs, &flags, batch).unwrap();

        assert_eq!(rs.tables.len(), 1);
        assert_eq!(rs.tables[0].chains.len(), 3);

        let input_chain = &rs.tables[0].chains[0];
        assert_eq!(input_chain.name, "input");
        assert_eq!(input_chain.rules.len(), 6);

        // Check the policy
        let cfg = input_chain.base_config.as_ref().unwrap();
        assert_eq!(cfg.policy, Policy::Drop);

        // List the whole ruleset
        let out = run_cmd(&mut rs, "list ruleset").unwrap();
        assert!(out.contains("table inet filter"));
        assert!(out.contains("chain input"));
        assert!(out.contains("chain forward"));
        assert!(out.contains("chain output"));
        assert!(out.contains("ct state established,related"));
        assert!(out.contains("tcp dport == 22 accept"));
    }

    #[test]
    fn test_set_based_blocklist_workflow() {
        let mut rs = Ruleset::new();
        let flags = Flags::new();
        let batch = "\
add table inet filter
add chain inet filter input
add set inet filter blocklist { type ipv4_addr ; }
add element inet filter blocklist { 10.0.0.1, 10.0.0.2, 192.168.1.100 }
add rule inet filter input ip saddr @blocklist drop
";
        run_batch_string(&mut rs, &flags, batch).unwrap();

        assert_eq!(rs.tables[0].sets[0].elements.len(), 3);

        let out = run_cmd(&mut rs, "list set inet filter blocklist").unwrap();
        assert!(out.contains("10.0.0.1"));
        assert!(out.contains("192.168.1.100"));
    }

    #[test]
    fn test_multi_family_tables() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table ip filter").unwrap();
        run_cmd(&mut rs, "add table ip6 filter").unwrap();
        run_cmd(&mut rs, "add table inet combined").unwrap();
        run_cmd(&mut rs, "add table bridge br_filter").unwrap();

        assert_eq!(rs.tables.len(), 4);
        let out = run_cmd(&mut rs, "list tables").unwrap();
        assert!(out.contains("table ip filter"));
        assert!(out.contains("table ip6 filter"));
        assert!(out.contains("table inet combined"));
        assert!(out.contains("table bridge br_filter"));
    }

    #[test]
    fn test_ether_daddr_match() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table bridge filter").unwrap();
        run_cmd(&mut rs, "add chain bridge filter forward").unwrap();
        run_cmd(
            &mut rs,
            "add rule bridge filter forward ether daddr ff:ff:ff:ff:ff:ff drop",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::EtherDaddr { value, .. } => {
                assert_eq!(value, "ff:ff:ff:ff:ff:ff");
            }
            _ => panic!("expected EtherDaddr"),
        }
    }

    #[test]
    fn test_multiple_matches_and_verdicts() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input ip saddr 10.0.0.0/8 tcp dport 80 counter accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        assert_eq!(rule.matches.len(), 2);
        assert_eq!(rule.verdicts.len(), 2);
    }

    #[test]
    fn test_monitor_returns_info() {
        let mut rs = Ruleset::new();
        let out = run_cmd(&mut rs, "monitor").unwrap();
        assert!(out.contains("monitoring not available"));
    }

    #[test]
    fn test_set_lookup_in_rule() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add set inet filter allowed_ports { type inet_service ; }",
        )
        .unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input tcp dport @allowed_ports accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::SetLookup { set_name, .. } => {
                assert_eq!(set_name, "allowed_ports");
            }
            _ => panic!("expected SetLookup"),
        }
    }

    #[test]
    fn test_tcp_sport_match() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter output").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter output tcp sport 443 accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::TcpSport { value, .. } => assert_eq!(value, "443"),
            _ => panic!("expected TcpSport"),
        }
    }

    #[test]
    fn test_udp_sport_match() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input udp sport 1234 accept",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::UdpSport { value, .. } => assert_eq!(value, "1234"),
            _ => panic!("expected UdpSport"),
        }
    }

    #[test]
    fn test_meta_length() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(
            &mut rs,
            "add rule inet filter input meta length > 1500 drop",
        )
        .unwrap();
        let rule = &rs.tables[0].chains[0].rules[0];
        match &rule.matches[0] {
            MatchExpr::Meta { key, op, value } => {
                assert_eq!(*key, MetaKey::Length);
                assert_eq!(*op, CmpOp::Gt);
                assert_eq!(value, "1500");
            }
            _ => panic!("expected Meta match"),
        }
    }

    #[test]
    fn test_verdict_reject() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input reject").unwrap();
        assert_eq!(rs.tables[0].chains[0].rules[0].verdicts[0], Verdict::Reject);
    }

    #[test]
    fn test_verdict_queue() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input queue").unwrap();
        assert_eq!(rs.tables[0].chains[0].rules[0].verdicts[0], Verdict::Queue);
    }

    #[test]
    fn test_verdict_continue() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter input").unwrap();
        run_cmd(&mut rs, "add rule inet filter input continue").unwrap();
        assert_eq!(
            rs.tables[0].chains[0].rules[0].verdicts[0],
            Verdict::Continue
        );
    }

    #[test]
    fn test_verdict_return() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add chain inet filter mychain").unwrap();
        run_cmd(&mut rs, "add rule inet filter mychain return").unwrap();
        assert_eq!(
            rs.tables[0].chains[0].rules[0].verdicts[0],
            Verdict::Return
        );
    }

    #[test]
    fn test_handle_alloc_starts_at_one() {
        let mut rs = Ruleset::new();
        assert_eq!(rs.alloc_handle(), 1);
        assert_eq!(rs.alloc_handle(), 2);
        assert_eq!(rs.alloc_handle(), 3);
    }

    #[test]
    fn test_chain_is_base() {
        let chain = Chain::new("regular");
        assert!(!chain.is_base());

        let base_chain = Chain::new_base(
            "base",
            BaseChainConfig {
                chain_type: ChainType::Filter,
                hook: Hook::Input,
                priority: 0,
                policy: Policy::Accept,
                device: None,
            },
        );
        assert!(base_chain.is_base());
    }

    #[test]
    fn test_table_find_operations() {
        let mut table = Table::new(Family::Inet, "test");
        table.chains.push(Chain::new("c1"));
        table.sets.push(NamedSet::new("s1", SetDataType::Ipv4Addr));
        table.maps.push(NamedMap::new("m1", SetDataType::Ipv4Addr, SetDataType::Mark));
        table.counters.push(CounterObj::new("cnt1"));
        table.quotas.push(QuotaObj::new("q1", 1000, false));
        table.limits.push(LimitObj::new("l1", 100, LimitUnit::Second));

        assert_eq!(table.find_chain("c1"), Some(0));
        assert_eq!(table.find_chain("noexist"), None);
        assert_eq!(table.find_set("s1"), Some(0));
        assert_eq!(table.find_set("noexist"), None);
        assert_eq!(table.find_map("m1"), Some(0));
        assert_eq!(table.find_map("noexist"), None);
        assert_eq!(table.find_counter("cnt1"), Some(0));
        assert_eq!(table.find_counter("noexist"), None);
        assert_eq!(table.find_quota("q1"), Some(0));
        assert_eq!(table.find_quota("noexist"), None);
        assert_eq!(table.find_limit("l1"), Some(0));
        assert_eq!(table.find_limit("noexist"), None);
    }

    #[test]
    fn test_ruleset_find_table() {
        let mut rs = Ruleset::new();
        rs.tables.push(Table::new(Family::Inet, "filter"));
        rs.tables.push(Table::new(Family::Ip, "nat"));

        assert_eq!(rs.find_table(Family::Inet, "filter"), Some(0));
        assert_eq!(rs.find_table(Family::Ip, "nat"), Some(1));
        assert_eq!(rs.find_table(Family::Inet, "nat"), None);
        assert!(rs.get_table(Family::Inet, "filter").is_ok());
        assert!(rs.get_table(Family::Bridge, "nope").is_err());
    }

    #[test]
    fn test_empty_command_is_noop() {
        let mut rs = Ruleset::new();
        let flags = Flags::new();
        let result = exec_command(&mut rs, &flags, &[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_flags_new_defaults() {
        let flags = Flags::new();
        assert!(!flags.json);
        assert!(!flags.numeric);
        assert!(!flags.show_handles);
    }

    #[test]
    fn test_counter_obj_defaults() {
        let c = CounterObj::new("test");
        assert_eq!(c.name, "test");
        assert_eq!(c.packets, 0);
        assert_eq!(c.bytes, 0);
    }

    #[test]
    fn test_quota_obj_fields() {
        let q = QuotaObj::new("test", 5000, true);
        assert_eq!(q.name, "test");
        assert_eq!(q.bytes_limit, 5000);
        assert!(q.inv);
        assert_eq!(q.used, 0);
    }

    #[test]
    fn test_limit_obj_burst() {
        let mut l = LimitObj::new("test", 50, LimitUnit::Minute);
        assert_eq!(l.burst, None);
        l.burst = Some(10);
        assert_eq!(l.burst, Some(10));
    }

    #[test]
    fn test_named_set_defaults() {
        let s = NamedSet::new("test", SetDataType::Ipv6Addr);
        assert_eq!(s.name, "test");
        assert_eq!(s.key_type, SetDataType::Ipv6Addr);
        assert!(s.flags.is_empty());
        assert!(s.elements.is_empty());
        assert!(s.typeof_expr.is_none());
    }

    #[test]
    fn test_named_map_defaults() {
        let m = NamedMap::new("test", SetDataType::Ipv4Addr, SetDataType::InetService);
        assert_eq!(m.name, "test");
        assert_eq!(m.key_type, SetDataType::Ipv4Addr);
        assert_eq!(m.value_type, SetDataType::InetService);
        assert!(m.elements.is_empty());
    }

    #[test]
    fn test_list_limits_with_burst() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(
            &mut rs,
            "add limit inet filter lim rate 10 second burst 5 packets",
        )
        .unwrap();
        let out = run_cmd(&mut rs, "list limits").unwrap();
        assert!(out.contains("burst 5 packets"));
    }

    #[test]
    fn test_list_quotas_over() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter").unwrap();
        run_cmd(&mut rs, "add quota inet filter q over 500 bytes").unwrap();
        let out = run_cmd(&mut rs, "list quotas").unwrap();
        assert!(out.contains("over"));
    }

    #[test]
    fn test_format_table_dormant() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "add table inet filter flags dormant").unwrap();
        let out = run_cmd(&mut rs, "list ruleset").unwrap();
        assert!(out.contains("flags dormant"));
    }

    #[test]
    fn test_create_is_alias_for_add() {
        let mut rs = Ruleset::new();
        run_cmd(&mut rs, "create table inet filter").unwrap();
        assert_eq!(rs.tables.len(), 1);
    }
}
