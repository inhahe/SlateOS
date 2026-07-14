//! Slate OS `nft` — nftables firewall configuration utility.
//!
//! Multi-personality binary providing:
//! - **nft** — nftables rule language (add/delete/list/flush tables, chains,
//!   rules, sets, maps, counters, NAT, masquerade)
//! - **iptables** — iptables compatibility wrapper
//! - **ip6tables** — ip6tables compatibility wrapper
//!
//! The active personality is determined by `argv[0]`.
//!
//! **State handling (important):** this tool is currently a *stateless
//! parser/pretty-printer*. Each invocation builds a fresh in-memory [`Ruleset`],
//! applies the one command, prints, and discards everything on exit — it does
//! **not** persist to a file, does **not** read `/proc/net/nftables`, and does
//! **not** submit to the kernel firewall. So mutating commands (`add`/`delete`/
//! `insert`/`create`/`flush`) validate and echo syntax but have no lasting
//! effect — and each now prints an explicit "NOT applied — use `fw`" notice so
//! the user isn't misled. The native `fw` tool is the working front-end for the
//! kernel firewall (it issues the `SYS_NET_FW_*` syscalls, 860–864).
//!
//! **This is by design (open-questions Q21, resolved 2026-07-14, option C — see
//! design-decisions §62).** The kernel `Rule` model (one src IP/prefix + one dst
//! port, input/output only, no NAT/sets/maps) is far narrower than nftables, so
//! any wiring would be heavily lossy and risks *silently under-applying* a user's
//! intended policy. The operator chose to keep `nft`/`iptables` as an explicit
//! parser/pretty-printer only and steer users to `fw`; full/minimal kernel
//! wiring is deferred until a concrete need to run Linux firewall scripts
//! unmodified appears. The earlier `SYS_NET_IOCTL=810` plumbing was dead code
//! that, had it been called, would have aliased `SYS_UDP_BIND` and leaked UDP
//! sockets — it has been removed. The `NFT_*` sub-command numbers below are
//! retained as documentation of the control ABI the kernel would eventually
//! expose if that wiring is ever built.

#![cfg_attr(not(test), no_main)]
#![deny(clippy::all)]
#![allow(
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::match_same_arms,
    clippy::struct_excessive_bools
)]

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};

// ============================================================================
// Control ABI sub-commands (documentation only)
// ============================================================================
//
// Slate OS has no nftables-control syscall yet, so none of these are issued.
// They are retained to document the command set the kernel will eventually
// accept once the firewall-control ABI is defined. See the module-level docs.

// nftables sub-commands for the (future) firewall-control syscall.
#[allow(dead_code)]
const NFT_TABLE_ADD: u64 = 200;
#[allow(dead_code)]
const NFT_TABLE_DEL: u64 = 201;
#[allow(dead_code)]
const NFT_TABLE_LIST: u64 = 202;
#[allow(dead_code)]
const NFT_TABLE_FLUSH: u64 = 203;
#[allow(dead_code)]
const NFT_CHAIN_ADD: u64 = 210;
#[allow(dead_code)]
const NFT_CHAIN_DEL: u64 = 211;
#[allow(dead_code)]
const NFT_CHAIN_LIST: u64 = 212;
#[allow(dead_code)]
const NFT_RULE_ADD: u64 = 220;
#[allow(dead_code)]
const NFT_RULE_DEL: u64 = 221;
#[allow(dead_code)]
const NFT_RULE_LIST: u64 = 222;
#[allow(dead_code)]
const NFT_RULE_FLUSH: u64 = 223;
#[allow(dead_code)]
const NFT_SET_ADD: u64 = 230;
#[allow(dead_code)]
const NFT_MAP_ADD: u64 = 240;
#[allow(dead_code)]
const NFT_COUNTER_ADD: u64 = 250;

// ============================================================================
// Address family
// ============================================================================

/// nftables address family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Family {
    Ip,
    Ip6,
    Inet,
    Arp,
    Bridge,
    Netdev,
}

impl Family {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "ip" => Some(Self::Ip),
            "ip6" => Some(Self::Ip6),
            "inet" => Some(Self::Inet),
            "arp" => Some(Self::Arp),
            "bridge" => Some(Self::Bridge),
            "netdev" => Some(Self::Netdev),
            _ => None,
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Ip => "ip",
            Self::Ip6 => "ip6",
            Self::Inet => "inet",
            Self::Arp => "arp",
            Self::Bridge => "bridge",
            Self::Netdev => "netdev",
        };
        f.write_str(s)
    }
}

// ============================================================================
// Chain types and hooks
// ============================================================================

/// Chain type in nftables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChainType {
    Filter,
    Nat,
    Route,
}

impl ChainType {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "filter" => Some(Self::Filter),
            "nat" => Some(Self::Nat),
            "route" => Some(Self::Route),
            _ => None,
        }
    }
}

impl fmt::Display for ChainType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Filter => "filter",
            Self::Nat => "nat",
            Self::Route => "route",
        };
        f.write_str(s)
    }
}

/// Chain hook points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hook {
    Prerouting,
    Input,
    Forward,
    Output,
    Postrouting,
    Ingress,
}

impl Hook {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "prerouting" => Some(Self::Prerouting),
            "input" => Some(Self::Input),
            "forward" => Some(Self::Forward),
            "output" => Some(Self::Output),
            "postrouting" => Some(Self::Postrouting),
            "ingress" => Some(Self::Ingress),
            _ => None,
        }
    }
}

impl fmt::Display for Hook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Prerouting => "prerouting",
            Self::Input => "input",
            Self::Forward => "forward",
            Self::Output => "output",
            Self::Postrouting => "postrouting",
            Self::Ingress => "ingress",
        };
        f.write_str(s)
    }
}

/// Chain policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Policy {
    Accept,
    Drop,
}

impl Policy {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "accept" => Some(Self::Accept),
            "drop" => Some(Self::Drop),
            _ => None,
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Accept => "accept",
            Self::Drop => "drop",
        };
        f.write_str(s)
    }
}

// ============================================================================
// Protocol
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Icmpv6,
}

impl Protocol {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "tcp" => Some(Self::Tcp),
            "udp" => Some(Self::Udp),
            "icmp" => Some(Self::Icmp),
            "icmpv6" | "ipv6-icmp" => Some(Self::Icmpv6),
            _ => None,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Icmp => "icmp",
            Self::Icmpv6 => "icmpv6",
        };
        f.write_str(s)
    }
}

// ============================================================================
// Rule verdict / target
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Accept,
    Drop,
    Reject,
    Log(Option<String>),
    Counter(Option<String>),
    Masquerade,
    Snat(String),
    Dnat(String),
    Jump(String),
    Goto(String),
    Return,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accept => f.write_str("accept"),
            Self::Drop => f.write_str("drop"),
            Self::Reject => f.write_str("reject"),
            Self::Log(None) => f.write_str("log"),
            Self::Log(Some(prefix)) => write!(f, "log prefix \"{}\"", prefix),
            Self::Counter(None) => f.write_str("counter"),
            Self::Counter(Some(name)) => write!(f, "counter name \"{}\"", name),
            Self::Masquerade => f.write_str("masquerade"),
            Self::Snat(addr) => write!(f, "snat to {}", addr),
            Self::Dnat(addr) => write!(f, "dnat to {}", addr),
            Self::Jump(chain) => write!(f, "jump {}", chain),
            Self::Goto(chain) => write!(f, "goto {}", chain),
            Self::Return => f.write_str("return"),
        }
    }
}

// ============================================================================
// Match expressions
// ============================================================================

/// A single match expression in a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MatchExpr {
    Protocol(Protocol),
    Saddr(String),
    Daddr(String),
    Sport(u16),
    Dport(u16),
    SportRange(u16, u16),
    DportRange(u16, u16),
    Iif(String),
    Oif(String),
    CtState(String),
    Meta(String, String),
    SetLookup(String),
}

impl fmt::Display for MatchExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(p) => write!(f, "{} protocol {}", family_for_proto(p), p),
            Self::Saddr(a) => write!(f, "ip saddr {}", a),
            Self::Daddr(a) => write!(f, "ip daddr {}", a),
            Self::Sport(p) => write!(f, "sport {}", p),
            Self::Dport(p) => write!(f, "dport {}", p),
            Self::SportRange(lo, hi) => write!(f, "sport {}-{}", lo, hi),
            Self::DportRange(lo, hi) => write!(f, "dport {}-{}", lo, hi),
            Self::Iif(i) => write!(f, "iif \"{}\"", i),
            Self::Oif(i) => write!(f, "oif \"{}\"", i),
            Self::CtState(s) => write!(f, "ct state {}", s),
            Self::Meta(k, v) => write!(f, "meta {} {}", k, v),
            Self::SetLookup(name) => write!(f, "@{}", name),
        }
    }
}

fn family_for_proto(p: &Protocol) -> &'static str {
    match p {
        Protocol::Icmpv6 => "ip6",
        _ => "ip",
    }
}

// ============================================================================
// Rule
// ============================================================================

/// A single nftables rule.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    handle: u64,
    matches: Vec<MatchExpr>,
    verdicts: Vec<Verdict>,
    comment: Option<String>,
}

impl Rule {
    fn new(handle: u64) -> Self {
        Self {
            handle,
            matches: Vec::new(),
            verdicts: Vec::new(),
            comment: None,
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .matches
            .iter()
            .map(|m| m.to_string())
            .chain(self.verdicts.iter().map(|v| v.to_string()))
            .collect();
        let line = parts.join(" ");
        if let Some(ref c) = self.comment {
            write!(f, "{} comment \"{}\" # handle {}", line, c, self.handle)
        } else {
            write!(f, "{} # handle {}", line, self.handle)
        }
    }
}

// ============================================================================
// Chain
// ============================================================================

/// A chain within a table.
#[derive(Debug, Clone)]
struct Chain {
    name: String,
    chain_type: Option<ChainType>,
    hook: Option<Hook>,
    priority: Option<i32>,
    policy: Option<Policy>,
    rules: Vec<Rule>,
}

impl Chain {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            chain_type: None,
            hook: None,
            priority: None,
            policy: None,
            rules: Vec::new(),
        }
    }

    fn new_base(
        name: &str,
        chain_type: ChainType,
        hook: Hook,
        priority: i32,
        policy: Policy,
    ) -> Self {
        Self {
            name: name.to_string(),
            chain_type: Some(chain_type),
            hook: Some(hook),
            priority: Some(priority),
            policy: Some(policy),
            rules: Vec::new(),
        }
    }
}

// ============================================================================
// Set and Map
// ============================================================================

/// Named set of elements.
#[derive(Debug, Clone)]
struct NftSet {
    name: String,
    set_type: String,
    elements: Vec<String>,
}

/// Named map.
#[derive(Debug, Clone)]
struct NftMap {
    name: String,
    key_type: String,
    value_type: String,
    elements: BTreeMap<String, String>,
}

// ============================================================================
// Counter
// ============================================================================

/// Named counter.
#[derive(Debug, Clone)]
struct Counter {
    name: String,
    packets: u64,
    bytes: u64,
}

// ============================================================================
// Table
// ============================================================================

/// An nftables table containing chains, sets, maps, and counters.
#[derive(Debug, Clone)]
struct Table {
    family: Family,
    name: String,
    chains: Vec<Chain>,
    sets: Vec<NftSet>,
    maps: Vec<NftMap>,
    counters: Vec<Counter>,
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
        }
    }
}

// ============================================================================
// Ruleset — the in-memory state
// ============================================================================

/// The complete nftables ruleset.
#[derive(Debug, Clone)]
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
        self.next_handle += 1;
        h
    }

    fn find_table(&self, family: Family, name: &str) -> Option<usize> {
        self.tables
            .iter()
            .position(|t| t.family == family && t.name == name)
    }

    fn find_chain(&self, table_idx: usize, chain_name: &str) -> Option<usize> {
        self.tables
            .get(table_idx)
            .and_then(|t| t.chains.iter().position(|c| c.name == chain_name))
    }

    fn add_table(&mut self, family: Family, name: &str) -> Result<(), String> {
        if self.find_table(family, name).is_some() {
            // nft silently succeeds if table already exists
            return Ok(());
        }
        self.tables.push(Table::new(family, name));
        Ok(())
    }

    fn delete_table(&mut self, family: Family, name: &str) -> Result<(), String> {
        if let Some(idx) = self.find_table(family, name) {
            self.tables.remove(idx);
            Ok(())
        } else {
            Err(format!(
                "Error: No such table '{}' in family {}",
                name, family
            ))
        }
    }

    fn flush_table(&mut self, family: Family, name: &str) -> Result<(), String> {
        if let Some(idx) = self.find_table(family, name) {
            if let Some(t) = self.tables.get_mut(idx) {
                for chain in &mut t.chains {
                    chain.rules.clear();
                }
            }
            Ok(())
        } else {
            Err(format!(
                "Error: No such table '{}' in family {}",
                name, family
            ))
        }
    }

    fn add_chain(&mut self, family: Family, table_name: &str, chain: Chain) -> Result<(), String> {
        let idx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        if let Some(t) = self.tables.get_mut(idx) {
            // Duplicate chain name silently ignored per nft semantics
            if t.chains.iter().any(|c| c.name == chain.name) {
                return Ok(());
            }
            t.chains.push(chain);
        }
        Ok(())
    }

    fn delete_chain(
        &mut self,
        family: Family,
        table_name: &str,
        chain_name: &str,
    ) -> Result<(), String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        if let Some(t) = self.tables.get_mut(tidx) {
            if let Some(cidx) = t.chains.iter().position(|c| c.name == chain_name) {
                t.chains.remove(cidx);
                Ok(())
            } else {
                Err(format!(
                    "Error: No such chain '{}' in table {}",
                    chain_name, table_name
                ))
            }
        } else {
            Err(format!("Error: No such table '{}'", table_name))
        }
    }

    fn flush_chain(
        &mut self,
        family: Family,
        table_name: &str,
        chain_name: &str,
    ) -> Result<(), String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        let cidx = self
            .find_chain(tidx, chain_name)
            .ok_or_else(|| format!("Error: No such chain '{}'", chain_name))?;
        if let Some(t) = self.tables.get_mut(tidx)
            && let Some(c) = t.chains.get_mut(cidx)
        {
            c.rules.clear();
        }
        Ok(())
    }

    fn add_rule(
        &mut self,
        family: Family,
        table_name: &str,
        chain_name: &str,
        rule: Rule,
    ) -> Result<u64, String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        let cidx = self
            .find_chain(tidx, chain_name)
            .ok_or_else(|| format!("Error: No such chain '{}'", chain_name))?;
        let handle = rule.handle;
        if let Some(t) = self.tables.get_mut(tidx)
            && let Some(c) = t.chains.get_mut(cidx)
        {
            c.rules.push(rule);
        }
        Ok(handle)
    }

    fn delete_rule(
        &mut self,
        family: Family,
        table_name: &str,
        chain_name: &str,
        handle: u64,
    ) -> Result<(), String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        let cidx = self
            .find_chain(tidx, chain_name)
            .ok_or_else(|| format!("Error: No such chain '{}'", chain_name))?;
        if let Some(t) = self.tables.get_mut(tidx)
            && let Some(c) = t.chains.get_mut(cidx)
        {
            let before = c.rules.len();
            c.rules.retain(|r| r.handle != handle);
            if c.rules.len() == before {
                return Err(format!(
                    "Error: No rule with handle {} in chain {}",
                    handle, chain_name
                ));
            }
        }
        Ok(())
    }

    fn insert_rule(
        &mut self,
        family: Family,
        table_name: &str,
        chain_name: &str,
        position: usize,
        rule: Rule,
    ) -> Result<u64, String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        let cidx = self
            .find_chain(tidx, chain_name)
            .ok_or_else(|| format!("Error: No such chain '{}'", chain_name))?;
        let handle = rule.handle;
        if let Some(t) = self.tables.get_mut(tidx)
            && let Some(c) = t.chains.get_mut(cidx)
        {
            let pos = if position > c.rules.len() {
                c.rules.len()
            } else {
                position
            };
            c.rules.insert(pos, rule);
        }
        Ok(handle)
    }

    fn add_set(&mut self, family: Family, table_name: &str, set: NftSet) -> Result<(), String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        if let Some(t) = self.tables.get_mut(tidx) {
            if t.sets.iter().any(|s| s.name == set.name) {
                return Err(format!("Error: Set '{}' already exists", set.name));
            }
            t.sets.push(set);
        }
        Ok(())
    }

    fn add_set_element(
        &mut self,
        family: Family,
        table_name: &str,
        set_name: &str,
        element: &str,
    ) -> Result<(), String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        if let Some(t) = self.tables.get_mut(tidx) {
            if let Some(s) = t.sets.iter_mut().find(|s| s.name == set_name) {
                if !s.elements.contains(&element.to_string()) {
                    s.elements.push(element.to_string());
                }
                Ok(())
            } else {
                Err(format!("Error: No such set '{}'", set_name))
            }
        } else {
            Err(format!("Error: No such table '{}'", table_name))
        }
    }

    fn add_map(&mut self, family: Family, table_name: &str, map: NftMap) -> Result<(), String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        if let Some(t) = self.tables.get_mut(tidx) {
            if t.maps.iter().any(|m| m.name == map.name) {
                return Err(format!("Error: Map '{}' already exists", map.name));
            }
            t.maps.push(map);
        }
        Ok(())
    }

    fn add_map_element(
        &mut self,
        family: Family,
        table_name: &str,
        map_name: &str,
        key: &str,
        value: &str,
    ) -> Result<(), String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        if let Some(t) = self.tables.get_mut(tidx) {
            if let Some(m) = t.maps.iter_mut().find(|m| m.name == map_name) {
                m.elements.insert(key.to_string(), value.to_string());
                Ok(())
            } else {
                Err(format!("Error: No such map '{}'", map_name))
            }
        } else {
            Err(format!("Error: No such table '{}'", table_name))
        }
    }

    fn add_counter(&mut self, family: Family, table_name: &str, name: &str) -> Result<(), String> {
        let tidx = self
            .find_table(family, table_name)
            .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
        if let Some(t) = self.tables.get_mut(tidx) {
            if t.counters.iter().any(|c| c.name == name) {
                return Err(format!("Error: Counter '{}' already exists", name));
            }
            t.counters.push(Counter {
                name: name.to_string(),
                packets: 0,
                bytes: 0,
            });
        }
        Ok(())
    }

    fn flush_ruleset(&mut self) {
        self.tables.clear();
    }
}

// ============================================================================
// Listing / output
// ============================================================================

fn format_ruleset(rs: &Ruleset, out: &mut dyn Write) -> io::Result<()> {
    for table in &rs.tables {
        format_table(table, out)?;
    }
    Ok(())
}

fn format_table(table: &Table, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "table {} {} {{", table.family, table.name)?;
    for counter in &table.counters {
        writeln!(out, "\tcounter {} {{", counter.name)?;
        writeln!(
            out,
            "\t\tpackets {} bytes {}",
            counter.packets, counter.bytes
        )?;
        writeln!(out, "\t}}")?;
    }
    for set in &table.sets {
        writeln!(out, "\tset {} {{", set.name)?;
        writeln!(out, "\t\ttype {}", set.set_type)?;
        if !set.elements.is_empty() {
            writeln!(out, "\t\telements = {{ {} }}", set.elements.join(", "))?;
        }
        writeln!(out, "\t}}")?;
    }
    for map in &table.maps {
        writeln!(out, "\tmap {} {{", map.name)?;
        writeln!(out, "\t\ttype {} : {}", map.key_type, map.value_type)?;
        if !map.elements.is_empty() {
            let entries: Vec<String> = map
                .elements
                .iter()
                .map(|(k, v)| format!("{} : {}", k, v))
                .collect();
            writeln!(out, "\t\telements = {{ {} }}", entries.join(", "))?;
        }
        writeln!(out, "\t}}")?;
    }
    for chain in &table.chains {
        format_chain(chain, out)?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

fn format_chain(chain: &Chain, out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "\tchain {} {{", chain.name)?;
    if let (Some(ct), Some(hook), Some(prio), Some(pol)) =
        (chain.chain_type, chain.hook, chain.priority, chain.policy)
    {
        writeln!(
            out,
            "\t\ttype {} hook {} priority {} ; policy {} ;",
            ct, hook, prio, pol
        )?;
    }
    for rule in &chain.rules {
        writeln!(out, "\t\t{}", rule)?;
    }
    writeln!(out, "\t}}")?;
    Ok(())
}

fn format_tables_list(rs: &Ruleset, out: &mut dyn Write) -> io::Result<()> {
    for table in &rs.tables {
        writeln!(out, "table {} {}", table.family, table.name)?;
    }
    Ok(())
}

// ============================================================================
// nft rule language parser
// ============================================================================

/// Parse a match expression from tokens, returning the number of tokens consumed.
fn parse_match(tokens: &[&str]) -> Option<(MatchExpr, usize)> {
    if tokens.is_empty() {
        return None;
    }

    match tokens.first().copied() {
        Some("ip" | "ip6") => {
            if tokens.len() >= 3 {
                match tokens.get(1).copied() {
                    Some("saddr") => {
                        return Some((
                            MatchExpr::Saddr(tokens.get(2).unwrap_or(&"").to_string()),
                            3,
                        ));
                    }
                    Some("daddr") => {
                        return Some((
                            MatchExpr::Daddr(tokens.get(2).unwrap_or(&"").to_string()),
                            3,
                        ));
                    }
                    Some("protocol") => {
                        if let Some(proto) = tokens.get(2).and_then(|s| Protocol::parse(s)) {
                            return Some((MatchExpr::Protocol(proto), 3));
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        Some("tcp" | "udp") => {
            if tokens.len() >= 2 {
                let proto_str = tokens.first().copied().unwrap_or("");
                match tokens.get(1).copied() {
                    Some("dport") if tokens.len() >= 3 => {
                        let port_str = tokens.get(2).unwrap_or(&"");
                        if let Some(dash) = port_str.find('-') {
                            let lo: u16 = port_str[..dash].parse().ok()?;
                            let hi: u16 = port_str[dash + 1..].parse().ok()?;
                            // We also want protocol match implied
                            let proto = Protocol::parse(proto_str)?;
                            return Some((MatchExpr::Protocol(proto), 1)).map(|(m, c)| {
                                // Return two matches combined as just the dport range
                                // since Protocol is consumed separately — actually let
                                // the caller handle protocol.
                                let _ = m;
                                (MatchExpr::DportRange(lo, hi), c + 2)
                            });
                        }
                        if let Ok(port) = port_str.parse::<u16>() {
                            return Some((MatchExpr::Dport(port), 3));
                        }
                    }
                    Some("sport") if tokens.len() >= 3 => {
                        let port_str = tokens.get(2).unwrap_or(&"");
                        if let Some(dash) = port_str.find('-') {
                            let lo: u16 = port_str[..dash].parse().ok()?;
                            let hi: u16 = port_str[dash + 1..].parse().ok()?;
                            return Some((MatchExpr::SportRange(lo, hi), 3));
                        }
                        if let Ok(port) = port_str.parse::<u16>() {
                            return Some((MatchExpr::Sport(port), 3));
                        }
                    }
                    _ => {
                        // Just protocol match: "tcp" or "udp" by itself
                        if let Some(proto) = Protocol::parse(proto_str) {
                            return Some((MatchExpr::Protocol(proto), 1));
                        }
                    }
                }
            } else {
                // Single token: protocol name
                let proto_str = tokens.first().copied().unwrap_or("");
                if let Some(proto) = Protocol::parse(proto_str) {
                    return Some((MatchExpr::Protocol(proto), 1));
                }
            }
            None
        }
        Some("iif") if tokens.len() >= 2 => {
            let name = tokens.get(1).unwrap_or(&"").trim_matches('"');
            Some((MatchExpr::Iif(name.to_string()), 2))
        }
        Some("oif") if tokens.len() >= 2 => {
            let name = tokens.get(1).unwrap_or(&"").trim_matches('"');
            Some((MatchExpr::Oif(name.to_string()), 2))
        }
        Some("ct") if tokens.len() >= 3 && tokens.get(1).copied() == Some("state") => Some((
            MatchExpr::CtState(tokens.get(2).unwrap_or(&"").to_string()),
            3,
        )),
        Some("meta") if tokens.len() >= 3 => Some((
            MatchExpr::Meta(
                tokens.get(1).unwrap_or(&"").to_string(),
                tokens.get(2).unwrap_or(&"").to_string(),
            ),
            3,
        )),
        Some(s) if s.starts_with('@') => Some((
            MatchExpr::SetLookup(s.trim_start_matches('@').to_string()),
            1,
        )),
        _ => None,
    }
}

/// Parse a verdict from tokens, returning the number consumed.
fn parse_verdict(tokens: &[&str]) -> Option<(Verdict, usize)> {
    if tokens.is_empty() {
        return None;
    }
    match tokens.first().copied() {
        Some("accept") => Some((Verdict::Accept, 1)),
        Some("drop") => Some((Verdict::Drop, 1)),
        Some("reject") => Some((Verdict::Reject, 1)),
        Some("return") => Some((Verdict::Return, 1)),
        Some("masquerade") => Some((Verdict::Masquerade, 1)),
        Some("log") => {
            if tokens.len() >= 3 && tokens.get(1).copied() == Some("prefix") {
                let prefix = tokens.get(2).unwrap_or(&"").trim_matches('"').to_string();
                Some((Verdict::Log(Some(prefix)), 3))
            } else {
                Some((Verdict::Log(None), 1))
            }
        }
        Some("counter") => {
            if tokens.len() >= 3 && tokens.get(1).copied() == Some("name") {
                let name = tokens.get(2).unwrap_or(&"").trim_matches('"').to_string();
                Some((Verdict::Counter(Some(name)), 3))
            } else {
                Some((Verdict::Counter(None), 1))
            }
        }
        Some("snat") if tokens.len() >= 3 && tokens.get(1).copied() == Some("to") => {
            Some((Verdict::Snat(tokens.get(2).unwrap_or(&"").to_string()), 3))
        }
        Some("dnat") if tokens.len() >= 3 && tokens.get(1).copied() == Some("to") => {
            Some((Verdict::Dnat(tokens.get(2).unwrap_or(&"").to_string()), 3))
        }
        Some("jump") if tokens.len() >= 2 => {
            Some((Verdict::Jump(tokens.get(1).unwrap_or(&"").to_string()), 2))
        }
        Some("goto") if tokens.len() >= 2 => {
            Some((Verdict::Goto(tokens.get(1).unwrap_or(&"").to_string()), 2))
        }
        _ => None,
    }
}

/// Parse a complete rule expression (matches + verdicts) from a token slice.
fn parse_rule_expr(tokens: &[&str], handle: u64) -> Result<Rule, String> {
    let mut rule = Rule::new(handle);
    let mut i = 0;
    let mut comment_mode = false;

    while i < tokens.len() {
        // Check for "comment" keyword
        if tokens.get(i).copied() == Some("comment") {
            comment_mode = true;
            i += 1;
            if i < tokens.len() {
                rule.comment = Some(tokens.get(i).unwrap_or(&"").trim_matches('"').to_string());
                i += 1;
            }
            continue;
        }
        if comment_mode {
            i += 1;
            continue;
        }

        // Try to parse as verdict first, then as match
        if let Some((verdict, consumed)) = parse_verdict(&tokens[i..]) {
            rule.verdicts.push(verdict);
            i += consumed;
        } else if let Some((m, consumed)) = parse_match(&tokens[i..]) {
            rule.matches.push(m);
            i += consumed;
        } else {
            // Skip unknown tokens (e.g. ";" separators)
            i += 1;
        }
    }

    if rule.matches.is_empty() && rule.verdicts.is_empty() {
        return Err("Error: empty rule expression".to_string());
    }
    Ok(rule)
}

// ============================================================================
// nft command dispatcher
// ============================================================================

/// Process a single nft command.
fn nft_command(rs: &mut Ruleset, args: &[&str], out: &mut dyn Write) -> Result<(), String> {
    if args.is_empty() {
        return Err("Error: no command specified".to_string());
    }

    match args.first().copied() {
        Some("add") => nft_add(rs, &args[1..], out),
        Some("delete") => nft_delete(rs, &args[1..]),
        Some("list") => nft_list(rs, &args[1..], out),
        Some("flush") => nft_flush(rs, &args[1..]),
        Some("insert") => nft_insert(rs, &args[1..]),
        Some("create") => nft_create(rs, &args[1..]),
        Some(other) => Err(format!("Error: unknown command '{}'", other)),
        None => Err("Error: no command specified".to_string()),
    }
}

fn nft_add(rs: &mut Ruleset, args: &[&str], _out: &mut dyn Write) -> Result<(), String> {
    if args.is_empty() {
        return Err("Error: 'add' requires an object type".to_string());
    }
    match args.first().copied() {
        Some("table") => {
            // add table [family] <name>
            let (family, name) = parse_family_name(&args[1..])?;
            rs.add_table(family, name)
        }
        Some("chain") => {
            // add chain [family] <table> <chain> [{ type ... hook ... priority ... ; policy ... ; }]
            nft_add_chain(rs, &args[1..])
        }
        Some("rule") => {
            // add rule [family] <table> <chain> <expr>...
            nft_add_rule(rs, &args[1..])
        }
        Some("set") => nft_add_set(rs, &args[1..]),
        Some("map") => nft_add_map(rs, &args[1..]),
        Some("element") => nft_add_element(rs, &args[1..]),
        Some("counter") => nft_add_counter(rs, &args[1..]),
        Some(other) => Err(format!("Error: unknown object type '{}'", other)),
        None => Err("Error: 'add' requires an object type".to_string()),
    }
}

fn nft_delete(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    if args.is_empty() {
        return Err("Error: 'delete' requires an object type".to_string());
    }
    match args.first().copied() {
        Some("table") => {
            let (family, name) = parse_family_name(&args[1..])?;
            rs.delete_table(family, name)
        }
        Some("chain") => {
            let (family, table, chain) = parse_family_table_chain(&args[1..])?;
            rs.delete_chain(family, table, chain)
        }
        Some("rule") => {
            // delete rule [family] <table> <chain> handle <num>
            nft_delete_rule(rs, &args[1..])
        }
        Some(other) => Err(format!("Error: cannot delete '{}'", other)),
        None => Err("Error: 'delete' requires an object type".to_string()),
    }
}

fn nft_list(rs: &Ruleset, args: &[&str], out: &mut dyn Write) -> Result<(), String> {
    if args.is_empty() {
        return Err("Error: 'list' requires an object type".to_string());
    }
    match args.first().copied() {
        Some("ruleset") => format_ruleset(rs, out).map_err(|e| e.to_string()),
        Some("tables") => format_tables_list(rs, out).map_err(|e| e.to_string()),
        Some("table") => {
            let (family, name) = parse_family_name(&args[1..])?;
            if let Some(idx) = rs.find_table(family, name) {
                if let Some(t) = rs.tables.get(idx) {
                    format_table(t, out).map_err(|e| e.to_string())
                } else {
                    Err(format!("Error: No such table '{}'", name))
                }
            } else {
                Err(format!(
                    "Error: No such table '{}' in family {}",
                    name, family
                ))
            }
        }
        Some("chain") => {
            let (family, table_name, chain_name) = parse_family_table_chain(&args[1..])?;
            let tidx = rs
                .find_table(family, table_name)
                .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
            let cidx = rs
                .find_chain(tidx, chain_name)
                .ok_or_else(|| format!("Error: No such chain '{}'", chain_name))?;
            if let Some(t) = rs.tables.get(tidx) {
                if let Some(c) = t.chains.get(cidx) {
                    writeln!(out, "table {} {} {{", t.family, t.name).map_err(|e| e.to_string())?;
                    format_chain(c, out).map_err(|e| e.to_string())?;
                    writeln!(out, "}}").map_err(|e| e.to_string())?;
                    Ok(())
                } else {
                    Err(format!("Error: No such chain '{}'", chain_name))
                }
            } else {
                Err(format!("Error: No such table '{}'", table_name))
            }
        }
        Some("sets") => {
            for table in &rs.tables {
                for set in &table.sets {
                    writeln!(out, "table {} {} {{", table.family, table.name)
                        .map_err(|e| e.to_string())?;
                    writeln!(out, "\tset {} {{", set.name).map_err(|e| e.to_string())?;
                    writeln!(out, "\t\ttype {}", set.set_type).map_err(|e| e.to_string())?;
                    if !set.elements.is_empty() {
                        writeln!(out, "\t\telements = {{ {} }}", set.elements.join(", "))
                            .map_err(|e| e.to_string())?;
                    }
                    writeln!(out, "\t}}").map_err(|e| e.to_string())?;
                    writeln!(out, "}}").map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
        Some("maps") => {
            for table in &rs.tables {
                for map in &table.maps {
                    writeln!(out, "table {} {} {{", table.family, table.name)
                        .map_err(|e| e.to_string())?;
                    writeln!(out, "\tmap {} {{", map.name).map_err(|e| e.to_string())?;
                    writeln!(out, "\t\ttype {} : {}", map.key_type, map.value_type)
                        .map_err(|e| e.to_string())?;
                    if !map.elements.is_empty() {
                        let entries: Vec<String> = map
                            .elements
                            .iter()
                            .map(|(k, v)| format!("{} : {}", k, v))
                            .collect();
                        writeln!(out, "\t\telements = {{ {} }}", entries.join(", "))
                            .map_err(|e| e.to_string())?;
                    }
                    writeln!(out, "\t}}").map_err(|e| e.to_string())?;
                    writeln!(out, "}}").map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
        Some("counters") => {
            for table in &rs.tables {
                for counter in &table.counters {
                    writeln!(out, "table {} {} {{", table.family, table.name)
                        .map_err(|e| e.to_string())?;
                    writeln!(out, "\tcounter {} {{", counter.name).map_err(|e| e.to_string())?;
                    writeln!(
                        out,
                        "\t\tpackets {} bytes {}",
                        counter.packets, counter.bytes
                    )
                    .map_err(|e| e.to_string())?;
                    writeln!(out, "\t}}").map_err(|e| e.to_string())?;
                    writeln!(out, "}}").map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
        Some(other) => Err(format!("Error: cannot list '{}'", other)),
        None => Err("Error: 'list' requires an object type".to_string()),
    }
}

fn nft_flush(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    if args.is_empty() {
        return Err("Error: 'flush' requires an object type".to_string());
    }
    match args.first().copied() {
        Some("ruleset") => {
            rs.flush_ruleset();
            Ok(())
        }
        Some("table") => {
            let (family, name) = parse_family_name(&args[1..])?;
            rs.flush_table(family, name)
        }
        Some("chain") => {
            let (family, table, chain) = parse_family_table_chain(&args[1..])?;
            rs.flush_chain(family, table, chain)
        }
        Some(other) => Err(format!("Error: cannot flush '{}'", other)),
        None => Err("Error: 'flush' requires an object type".to_string()),
    }
}

fn nft_insert(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    if args.is_empty() {
        return Err("Error: 'insert' requires 'rule'".to_string());
    }
    if args.first().copied() != Some("rule") {
        return Err("Error: 'insert' only supports 'rule'".to_string());
    }
    let rest = &args[1..];
    let (family, table_name, chain_name, expr_start) = parse_family_table_chain_rest(rest)?;
    let handle = rs.alloc_handle();
    let rule = parse_rule_expr(&rest[expr_start..], handle)?;
    rs.insert_rule(family, table_name, chain_name, 0, rule)?;
    Ok(())
}

fn nft_create(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    // "create" is like "add" but fails if object already exists.
    if args.is_empty() {
        return Err("Error: 'create' requires an object type".to_string());
    }
    match args.first().copied() {
        Some("table") => {
            let (family, name) = parse_family_name(&args[1..])?;
            if rs.find_table(family, name).is_some() {
                return Err(format!(
                    "Error: table '{}' already exists in family {}",
                    name, family
                ));
            }
            rs.add_table(family, name)
        }
        Some("chain") => {
            let (family, table_name, chain_name) = parse_family_table_chain(&args[1..])?;
            let tidx = rs
                .find_table(family, table_name)
                .ok_or_else(|| format!("Error: No such table '{}'", table_name))?;
            if rs.find_chain(tidx, chain_name).is_some() {
                return Err(format!(
                    "Error: chain '{}' already exists in table {}",
                    chain_name, table_name
                ));
            }
            rs.add_chain(family, table_name, Chain::new(chain_name))
        }
        Some(other) => Err(format!("Error: cannot create '{}'", other)),
        None => Err("Error: 'create' requires an object type".to_string()),
    }
}

// ============================================================================
// nft sub-command helpers
// ============================================================================

fn nft_add_chain(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    // Minimum: [family] <table> <chain>
    // Base chain: [family] <table> <chain> { type <type> hook <hook> priority <num> ; policy <pol> ; }
    let (family, rest) = split_family(args);
    if rest.len() < 2 {
        return Err("Error: add chain requires <table> <chain>".to_string());
    }
    let table_name = rest.first().copied().unwrap_or("");
    let chain_name = rest.get(1).copied().unwrap_or("");

    // Check for base chain specification with braces
    let remaining = &rest[2..];
    if remaining.is_empty() {
        // Regular (non-base) chain
        rs.add_chain(family, table_name, Chain::new(chain_name))
    } else {
        // Collect everything between { } and parse type/hook/priority/policy
        let joined: String = remaining
            .iter()
            .map(|s| s.trim_matches(|c| c == '{' || c == '}'))
            .filter(|s| !s.is_empty() && *s != ";")
            .collect::<Vec<_>>()
            .join(" ");
        let parts: Vec<&str> = joined.split_whitespace().collect();

        let mut chain_type = ChainType::Filter;
        let mut hook = Hook::Input;
        let mut priority: i32 = 0;
        let mut policy = Policy::Accept;

        let mut i = 0;
        while i < parts.len() {
            match parts.get(i).copied() {
                Some("type") => {
                    i += 1;
                    if let Some(ct) = parts.get(i).and_then(|s| ChainType::parse(s)) {
                        chain_type = ct;
                    }
                }
                Some("hook") => {
                    i += 1;
                    if let Some(h) = parts.get(i).and_then(|s| Hook::parse(s)) {
                        hook = h;
                    }
                }
                Some("priority") => {
                    i += 1;
                    if let Some(p) = parts.get(i).and_then(|s| s.parse::<i32>().ok()) {
                        priority = p;
                    }
                }
                Some("policy") => {
                    i += 1;
                    if let Some(p) = parts.get(i).and_then(|s| Policy::parse(s)) {
                        policy = p;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        rs.add_chain(
            family,
            table_name,
            Chain::new_base(chain_name, chain_type, hook, priority, policy),
        )
    }
}

fn nft_add_rule(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    let (family, table_name, chain_name, expr_start) = parse_family_table_chain_rest(args)?;
    let handle = rs.alloc_handle();
    let rule = parse_rule_expr(&args[expr_start..], handle)?;
    rs.add_rule(family, table_name, chain_name, rule)?;
    Ok(())
}

fn nft_add_set(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    // add set [family] <table> <set_name> { type <type> ; }
    let (family, rest) = split_family(args);
    if rest.len() < 2 {
        return Err("Error: add set requires <table> <set_name>".to_string());
    }
    let table_name = rest.first().copied().unwrap_or("");
    let set_name = rest.get(1).copied().unwrap_or("");

    // Parse type from remaining tokens
    let remaining = &rest[2..];
    let joined: String = remaining
        .iter()
        .map(|s| s.trim_matches(|c| c == '{' || c == '}'))
        .filter(|s| !s.is_empty() && *s != ";")
        .collect::<Vec<_>>()
        .join(" ");
    let parts: Vec<&str> = joined.split_whitespace().collect();

    let mut set_type = "ipv4_addr".to_string();
    let mut i = 0;
    while i < parts.len() {
        if parts.get(i).copied() == Some("type") {
            i += 1;
            if let Some(t) = parts.get(i) {
                set_type = t.to_string();
            }
        }
        i += 1;
    }

    rs.add_set(
        family,
        table_name,
        NftSet {
            name: set_name.to_string(),
            set_type,
            elements: Vec::new(),
        },
    )
}

fn nft_add_map(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    // add map [family] <table> <map_name> { type <key_type> : <value_type> ; }
    let (family, rest) = split_family(args);
    if rest.len() < 2 {
        return Err("Error: add map requires <table> <map_name>".to_string());
    }
    let table_name = rest.first().copied().unwrap_or("");
    let map_name = rest.get(1).copied().unwrap_or("");

    let remaining = &rest[2..];
    let joined: String = remaining
        .iter()
        .map(|s| s.trim_matches(|c| c == '{' || c == '}'))
        .filter(|s| !s.is_empty() && *s != ";")
        .collect::<Vec<_>>()
        .join(" ");
    let parts: Vec<&str> = joined.split_whitespace().collect();

    let mut key_type = "ipv4_addr".to_string();
    let mut value_type = "verdict".to_string();
    let mut i = 0;
    while i < parts.len() {
        if parts.get(i).copied() == Some("type") {
            i += 1;
            if let Some(kt) = parts.get(i) {
                key_type = kt.to_string();
            }
            i += 1;
            // Skip ":"
            if parts.get(i).copied() == Some(":") {
                i += 1;
            }
            if let Some(vt) = parts.get(i) {
                value_type = vt.to_string();
            }
        }
        i += 1;
    }

    rs.add_map(
        family,
        table_name,
        NftMap {
            name: map_name.to_string(),
            key_type,
            value_type,
            elements: BTreeMap::new(),
        },
    )
}

fn nft_add_element(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    // add element [family] <table> <set_or_map> { <elem>, ... }
    let (family, rest) = split_family(args);
    if rest.len() < 2 {
        return Err("Error: add element requires <table> <set/map>".to_string());
    }
    let table_name = rest.first().copied().unwrap_or("");
    let obj_name = rest.get(1).copied().unwrap_or("");

    let remaining = &rest[2..];
    let joined: String = remaining
        .iter()
        .map(|s| s.trim_matches(|c| c == '{' || c == '}'))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    // Check if this is a map (has ":") or set
    if joined.contains(':') {
        // Map elements: "key : value, key2 : value2"
        for entry in joined.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if let Some(colon_pos) = entry.find(':') {
                let key = entry[..colon_pos].trim();
                let value = entry[colon_pos + 1..].trim();
                if !key.is_empty() && !value.is_empty() {
                    rs.add_map_element(family, table_name, obj_name, key, value)?;
                }
            }
        }
    } else {
        // Set elements: "elem1, elem2, elem3"
        for elem in joined.split(',') {
            let elem = elem.trim();
            if !elem.is_empty() {
                rs.add_set_element(family, table_name, obj_name, elem)?;
            }
        }
    }
    Ok(())
}

fn nft_add_counter(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    let (family, rest) = split_family(args);
    if rest.len() < 2 {
        return Err("Error: add counter requires <table> <name>".to_string());
    }
    let table_name = rest.first().copied().unwrap_or("");
    let counter_name = rest.get(1).copied().unwrap_or("");
    rs.add_counter(family, table_name, counter_name)
}

fn nft_delete_rule(rs: &mut Ruleset, args: &[&str]) -> Result<(), String> {
    let (family, table_name, chain_name, expr_start) = parse_family_table_chain_rest(args)?;
    // Look for "handle <num>" in remaining tokens
    let remaining = &args[expr_start..];
    let mut handle: Option<u64> = None;
    let mut i = 0;
    while i < remaining.len() {
        if remaining.get(i).copied() == Some("handle") {
            i += 1;
            if let Some(h) = remaining.get(i).and_then(|s| s.parse::<u64>().ok()) {
                handle = Some(h);
            }
        }
        i += 1;
    }
    let h = handle.ok_or_else(|| "Error: delete rule requires 'handle <num>'".to_string())?;
    rs.delete_rule(family, table_name, chain_name, h)
}

// ============================================================================
// Argument parsing helpers
// ============================================================================

/// Parse optional family + name from args.  Default family is "ip".
fn parse_family_name<'a>(args: &[&'a str]) -> Result<(Family, &'a str), String> {
    if args.is_empty() {
        return Err("Error: expected <name>".to_string());
    }
    if args.len() >= 2
        && let Some(f) = Family::parse(args.first().copied().unwrap_or(""))
    {
        return Ok((f, args.get(1).copied().unwrap_or("")));
    }
    Ok((Family::Ip, args.first().copied().unwrap_or("")))
}

/// Split optional leading family token from args.
fn split_family<'a>(args: &'a [&'a str]) -> (Family, &'a [&'a str]) {
    if let Some(first) = args.first()
        && let Some(f) = Family::parse(first)
    {
        return (f, &args[1..]);
    }
    (Family::Ip, args)
}

/// Parse [family] <table> <chain> and return indices.
fn parse_family_table_chain<'a>(args: &'a [&'a str]) -> Result<(Family, &'a str, &'a str), String> {
    let (family, rest) = split_family(args);
    if rest.len() < 2 {
        return Err("Error: expected <table> <chain>".to_string());
    }
    Ok((
        family,
        rest.first().copied().unwrap_or(""),
        rest.get(1).copied().unwrap_or(""),
    ))
}

/// Parse [family] <table> <chain> and return the index where the rest starts.
fn parse_family_table_chain_rest<'a>(
    args: &[&'a str],
) -> Result<(Family, &'a str, &'a str, usize), String> {
    if args.is_empty() {
        return Err("Error: expected [family] <table> <chain> <expr>...".to_string());
    }
    if let Some(f) = Family::parse(args.first().copied().unwrap_or("")) {
        if args.len() < 3 {
            return Err("Error: expected <table> <chain>".to_string());
        }
        Ok((
            f,
            args.get(1).copied().unwrap_or(""),
            args.get(2).copied().unwrap_or(""),
            3,
        ))
    } else {
        if args.len() < 2 {
            return Err("Error: expected <table> <chain>".to_string());
        }
        Ok((
            Family::Ip,
            args.first().copied().unwrap_or(""),
            args.get(1).copied().unwrap_or(""),
            2,
        ))
    }
}

// ============================================================================
// iptables / ip6tables compatibility layer
// ============================================================================

/// Parsed iptables command.
#[derive(Debug)]
struct IptablesCmd {
    /// IPv6 mode (ip6tables personality).
    ipv6: bool,
    /// Table name: filter (default), nat, mangle.
    table: String,
    /// Action.
    action: IptAction,
    /// Chain name.
    chain: String,
    /// Protocol.
    proto: Option<Protocol>,
    /// Source address.
    source: Option<String>,
    /// Destination address.
    dest: Option<String>,
    /// Source port.
    sport: Option<u16>,
    /// Destination port.
    dport: Option<u16>,
    /// Jump target (ACCEPT, DROP, REJECT, LOG, MASQUERADE, SNAT, DNAT).
    target: Option<String>,
    /// Policy (for -P).
    policy: Option<String>,
    /// Numeric output.
    numeric: bool,
    /// Interface in.
    in_iface: Option<String>,
    /// Interface out.
    out_iface: Option<String>,
    /// SNAT --to-source addr.
    to_source: Option<String>,
    /// DNAT --to-destination addr.
    to_dest: Option<String>,
    /// Rule number for -I/-D by number.
    rule_num: Option<usize>,
    /// Log prefix.
    log_prefix: Option<String>,
    /// Connection state.
    ct_state: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IptAction {
    Append,
    Delete,
    Insert,
    List,
    Flush,
    Policy,
    NewChain,
    DeleteChain,
    Help,
}

impl IptablesCmd {
    fn new(ipv6: bool) -> Self {
        Self {
            ipv6,
            table: "filter".to_string(),
            action: IptAction::List,
            chain: String::new(),
            proto: None,
            source: None,
            dest: None,
            sport: None,
            dport: None,
            target: None,
            policy: None,
            numeric: false,
            in_iface: None,
            out_iface: None,
            to_source: None,
            to_dest: None,
            rule_num: None,
            log_prefix: None,
            ct_state: None,
        }
    }
}

fn parse_iptables_args(args: &[&str], ipv6: bool) -> Result<IptablesCmd, String> {
    let mut cmd = IptablesCmd::new(ipv6);
    let mut i = 0;

    while i < args.len() {
        let arg = args.get(i).copied().unwrap_or("");
        match arg {
            "-A" => {
                cmd.action = IptAction::Append;
                i += 1;
                cmd.chain = args.get(i).copied().unwrap_or("").to_string();
            }
            "-D" => {
                cmd.action = IptAction::Delete;
                i += 1;
                cmd.chain = args.get(i).copied().unwrap_or("").to_string();
                // Check if next arg is a number (delete by rule number)
                if let Some(next) = args.get(i + 1)
                    && let Ok(n) = next.parse::<usize>()
                {
                    cmd.rule_num = Some(n);
                    i += 1;
                }
            }
            "-I" => {
                cmd.action = IptAction::Insert;
                i += 1;
                cmd.chain = args.get(i).copied().unwrap_or("").to_string();
                // Optional rule number
                if let Some(next) = args.get(i + 1)
                    && let Ok(n) = next.parse::<usize>()
                {
                    cmd.rule_num = Some(n);
                    i += 1;
                }
            }
            "-L" | "--list" => {
                cmd.action = IptAction::List;
                if let Some(next) = args.get(i + 1)
                    && !next.starts_with('-')
                {
                    cmd.chain = next.to_string();
                    i += 1;
                }
            }
            "-F" | "--flush" => {
                cmd.action = IptAction::Flush;
                if let Some(next) = args.get(i + 1)
                    && !next.starts_with('-')
                {
                    cmd.chain = next.to_string();
                    i += 1;
                }
            }
            "-P" => {
                cmd.action = IptAction::Policy;
                i += 1;
                cmd.chain = args.get(i).copied().unwrap_or("").to_string();
                i += 1;
                cmd.policy = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "-N" => {
                cmd.action = IptAction::NewChain;
                i += 1;
                cmd.chain = args.get(i).copied().unwrap_or("").to_string();
            }
            "-X" => {
                cmd.action = IptAction::DeleteChain;
                i += 1;
                cmd.chain = args.get(i).copied().unwrap_or("").to_string();
            }
            "-t" | "--table" => {
                i += 1;
                cmd.table = args.get(i).copied().unwrap_or("filter").to_string();
            }
            "-p" | "--protocol" => {
                i += 1;
                cmd.proto = args.get(i).and_then(|s| Protocol::parse(s));
            }
            "-s" | "--source" => {
                i += 1;
                cmd.source = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "-d" | "--destination" => {
                i += 1;
                cmd.dest = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "--sport" | "--source-port" => {
                i += 1;
                cmd.sport = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            "--dport" | "--destination-port" => {
                i += 1;
                cmd.dport = args.get(i).and_then(|s| s.parse::<u16>().ok());
            }
            "-j" | "--jump" => {
                i += 1;
                cmd.target = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "-n" | "--numeric" => {
                cmd.numeric = true;
            }
            "-i" | "--in-interface" => {
                i += 1;
                cmd.in_iface = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "-o" | "--out-interface" => {
                i += 1;
                cmd.out_iface = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "--to-source" => {
                i += 1;
                cmd.to_source = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "--to-destination" => {
                i += 1;
                cmd.to_dest = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "--log-prefix" => {
                i += 1;
                cmd.log_prefix = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "-m" | "--match" => {
                i += 1;
                let module = args.get(i).copied().unwrap_or("");
                if module == "state" || module == "conntrack" {
                    // Look for --state / --ctstate next
                    if let Some(next) = args.get(i + 1)
                        && (*next == "--state" || *next == "--ctstate")
                    {
                        i += 2;
                        cmd.ct_state = Some(args.get(i).copied().unwrap_or("").to_string());
                    }
                }
            }
            "--state" | "--ctstate" => {
                i += 1;
                cmd.ct_state = Some(args.get(i).copied().unwrap_or("").to_string());
            }
            "-h" | "--help" => {
                cmd.action = IptAction::Help;
            }
            _ => {
                // Unknown flags are silently skipped for compatibility
            }
        }
        i += 1;
    }
    Ok(cmd)
}

/// Build nft matches and verdicts from an iptables command.
fn iptables_to_nft_rule(cmd: &IptablesCmd, handle: u64) -> Rule {
    let mut rule = Rule::new(handle);

    if let Some(ref proto) = cmd.proto {
        rule.matches.push(MatchExpr::Protocol(*proto));
    }
    if let Some(ref src) = cmd.source {
        rule.matches.push(MatchExpr::Saddr(src.clone()));
    }
    if let Some(ref dst) = cmd.dest {
        rule.matches.push(MatchExpr::Daddr(dst.clone()));
    }
    if let Some(sport) = cmd.sport {
        rule.matches.push(MatchExpr::Sport(sport));
    }
    if let Some(dport) = cmd.dport {
        rule.matches.push(MatchExpr::Dport(dport));
    }
    if let Some(ref iif) = cmd.in_iface {
        rule.matches.push(MatchExpr::Iif(iif.clone()));
    }
    if let Some(ref oif) = cmd.out_iface {
        rule.matches.push(MatchExpr::Oif(oif.clone()));
    }
    if let Some(ref state) = cmd.ct_state {
        rule.matches.push(MatchExpr::CtState(state.clone()));
    }

    if let Some(ref target) = cmd.target {
        let verdict = match target.as_str() {
            "ACCEPT" => Verdict::Accept,
            "DROP" => Verdict::Drop,
            "REJECT" => Verdict::Reject,
            "LOG" => Verdict::Log(cmd.log_prefix.clone()),
            "MASQUERADE" => Verdict::Masquerade,
            "SNAT" => Verdict::Snat(cmd.to_source.clone().unwrap_or_default()),
            "DNAT" => Verdict::Dnat(cmd.to_dest.clone().unwrap_or_default()),
            "RETURN" => Verdict::Return,
            other => Verdict::Jump(other.to_string()),
        };
        rule.verdicts.push(verdict);
    }

    rule
}

/// Map iptables table names to nft table + chain type / hook combinations.
fn iptables_table_family(cmd: &IptablesCmd) -> Family {
    if cmd.ipv6 { Family::Ip6 } else { Family::Ip }
}

/// Ensure the iptables-compatible table + default chains exist.
fn ensure_iptables_table(rs: &mut Ruleset, cmd: &IptablesCmd) {
    let family = iptables_table_family(cmd);
    let table_name = &cmd.table;

    // Create table if missing
    let _ = rs.add_table(family, table_name);

    // Create default chains based on table type
    let defaults: &[(&str, ChainType, Hook)] = match table_name.as_str() {
        "nat" => &[
            ("PREROUTING", ChainType::Nat, Hook::Prerouting),
            ("INPUT", ChainType::Nat, Hook::Input),
            ("OUTPUT", ChainType::Nat, Hook::Output),
            ("POSTROUTING", ChainType::Nat, Hook::Postrouting),
        ],
        "mangle" => &[
            ("PREROUTING", ChainType::Route, Hook::Prerouting),
            ("INPUT", ChainType::Route, Hook::Input),
            ("FORWARD", ChainType::Route, Hook::Forward),
            ("OUTPUT", ChainType::Route, Hook::Output),
            ("POSTROUTING", ChainType::Route, Hook::Postrouting),
        ],
        _ => &[
            ("INPUT", ChainType::Filter, Hook::Input),
            ("FORWARD", ChainType::Filter, Hook::Forward),
            ("OUTPUT", ChainType::Filter, Hook::Output),
        ],
    };

    for (name, ct, hook) in defaults {
        let _ = rs.add_chain(
            family,
            table_name,
            Chain::new_base(name, *ct, *hook, 0, Policy::Accept),
        );
    }
}

/// Execute a parsed iptables command.
fn exec_iptables(rs: &mut Ruleset, cmd: &IptablesCmd, out: &mut dyn Write) -> Result<(), String> {
    let prog = if cmd.ipv6 { "ip6tables" } else { "iptables" };
    match cmd.action {
        IptAction::Help => {
            let _ = writeln!(
                out,
                "Usage: {} [-t table] -A|-D|-I chain [-p proto] [-s src] [-d dst]",
                prog
            );
            let _ = writeln!(out, "       [--sport port] [--dport port] [-j target]");
            let _ = writeln!(out, "       [-L [chain]] [-F [chain]] [-P chain policy]");
            let _ = writeln!(out, "       [-n] [-N chain] [-X chain]");
            let _ = writeln!(out, "\nTables: filter (default), nat, mangle");
            let _ = writeln!(
                out,
                "Targets: ACCEPT, DROP, REJECT, LOG, MASQUERADE, SNAT, DNAT"
            );
            Ok(())
        }
        IptAction::List => {
            ensure_iptables_table(rs, cmd);
            let family = iptables_table_family(cmd);
            let tidx = rs
                .find_table(family, &cmd.table)
                .ok_or_else(|| format!("Error: table '{}' not found", cmd.table))?;
            if let Some(table) = rs.tables.get(tidx) {
                if cmd.chain.is_empty() {
                    // List all chains
                    for chain in &table.chains {
                        format_iptables_chain(chain, cmd.numeric, out)?;
                    }
                } else {
                    // List specific chain
                    if let Some(c) = table.chains.iter().find(|c| c.name == cmd.chain) {
                        format_iptables_chain(c, cmd.numeric, out)?;
                    } else {
                        return Err(format!("{}: No chain/target/match by that name.", prog));
                    }
                }
            }
            Ok(())
        }
        IptAction::Flush => {
            ensure_iptables_table(rs, cmd);
            let family = iptables_table_family(cmd);
            if cmd.chain.is_empty() {
                rs.flush_table(family, &cmd.table)
            } else {
                rs.flush_chain(family, &cmd.table, &cmd.chain)
            }
        }
        IptAction::Policy => {
            ensure_iptables_table(rs, cmd);
            let family = iptables_table_family(cmd);
            let policy_str = cmd.policy.as_deref().unwrap_or("ACCEPT");
            let pol = match policy_str {
                "ACCEPT" => Policy::Accept,
                "DROP" => Policy::Drop,
                _ => return Err(format!("Error: bad policy '{}'", policy_str)),
            };
            let tidx = rs
                .find_table(family, &cmd.table)
                .ok_or_else(|| format!("Error: table '{}' not found", cmd.table))?;
            let cidx = rs
                .find_chain(tidx, &cmd.chain)
                .ok_or_else(|| format!("{}: No chain/target/match by that name.", prog))?;
            if let Some(t) = rs.tables.get_mut(tidx)
                && let Some(c) = t.chains.get_mut(cidx)
            {
                c.policy = Some(pol);
            }
            Ok(())
        }
        IptAction::Append => {
            ensure_iptables_table(rs, cmd);
            let family = iptables_table_family(cmd);
            let handle = rs.alloc_handle();
            let rule = iptables_to_nft_rule(cmd, handle);
            rs.add_rule(family, &cmd.table, &cmd.chain, rule)?;
            Ok(())
        }
        IptAction::Insert => {
            ensure_iptables_table(rs, cmd);
            let family = iptables_table_family(cmd);
            let handle = rs.alloc_handle();
            let rule = iptables_to_nft_rule(cmd, handle);
            let pos = cmd.rule_num.unwrap_or(1).saturating_sub(1);
            rs.insert_rule(family, &cmd.table, &cmd.chain, pos, rule)?;
            Ok(())
        }
        IptAction::Delete => {
            ensure_iptables_table(rs, cmd);
            let family = iptables_table_family(cmd);
            if let Some(num) = cmd.rule_num {
                // Delete by rule number (1-based)
                let tidx = rs
                    .find_table(family, &cmd.table)
                    .ok_or_else(|| format!("Error: table '{}' not found", cmd.table))?;
                let cidx = rs
                    .find_chain(tidx, &cmd.chain)
                    .ok_or_else(|| format!("{}: No chain/target/match by that name.", prog))?;
                let idx = num
                    .checked_sub(1)
                    .ok_or_else(|| format!("{}: Invalid rule number.", prog))?;
                if let Some(t) = rs.tables.get_mut(tidx)
                    && let Some(c) = t.chains.get_mut(cidx)
                {
                    if idx >= c.rules.len() {
                        return Err(format!("{}: Index of deletion too big.", prog));
                    }
                    c.rules.remove(idx);
                }
                Ok(())
            } else {
                // Delete matching rule — find first rule that matches all specified criteria
                let target_rule = iptables_to_nft_rule(cmd, 0);
                let tidx = rs
                    .find_table(family, &cmd.table)
                    .ok_or_else(|| format!("Error: table '{}' not found", cmd.table))?;
                let cidx = rs
                    .find_chain(tidx, &cmd.chain)
                    .ok_or_else(|| format!("{}: No chain/target/match by that name.", prog))?;
                if let Some(t) = rs.tables.get_mut(tidx)
                    && let Some(c) = t.chains.get_mut(cidx)
                    && let Some(pos) = c.rules.iter().position(|r| {
                        r.matches == target_rule.matches && r.verdicts == target_rule.verdicts
                    })
                {
                    c.rules.remove(pos);
                    return Ok(());
                }
                Err(format!(
                    "{}: Bad rule (does a matching rule exist in that chain?).",
                    prog
                ))
            }
        }
        IptAction::NewChain => {
            ensure_iptables_table(rs, cmd);
            let family = iptables_table_family(cmd);
            rs.add_chain(family, &cmd.table, Chain::new(&cmd.chain))
        }
        IptAction::DeleteChain => {
            ensure_iptables_table(rs, cmd);
            let family = iptables_table_family(cmd);
            rs.delete_chain(family, &cmd.table, &cmd.chain)
        }
    }
}

fn format_iptables_chain(chain: &Chain, _numeric: bool, out: &mut dyn Write) -> Result<(), String> {
    let policy_str = chain
        .policy
        .map(|p| match p {
            Policy::Accept => "ACCEPT",
            Policy::Drop => "DROP",
        })
        .unwrap_or("-");

    if chain.chain_type.is_some() {
        let _ = writeln!(out, "Chain {} (policy {})", chain.name, policy_str);
    } else {
        let _ = writeln!(out, "Chain {} (0 references)", chain.name);
    }
    let _ = writeln!(
        out,
        "{:<8}{:<6}{:<8}{:<18}{:<18}{:<8}",
        "num", "proto", "target", "source", "destination", "extra"
    );

    for (idx, rule) in chain.rules.iter().enumerate() {
        let proto = rule
            .matches
            .iter()
            .find_map(|m| {
                if let MatchExpr::Protocol(p) = m {
                    Some(p.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "all".to_string());

        let target = rule
            .verdicts
            .first()
            .map(|v| match v {
                Verdict::Accept => "ACCEPT".to_string(),
                Verdict::Drop => "DROP".to_string(),
                Verdict::Reject => "REJECT".to_string(),
                Verdict::Log(_) => "LOG".to_string(),
                Verdict::Masquerade => "MASQUERADE".to_string(),
                Verdict::Snat(a) => format!("SNAT:{}", a),
                Verdict::Dnat(a) => format!("DNAT:{}", a),
                Verdict::Jump(c) => c.clone(),
                Verdict::Goto(c) => c.clone(),
                Verdict::Return => "RETURN".to_string(),
                Verdict::Counter(_) => "counter".to_string(),
            })
            .unwrap_or_else(|| "--".to_string());

        let source = rule
            .matches
            .iter()
            .find_map(|m| {
                if let MatchExpr::Saddr(a) = m {
                    Some(a.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "0.0.0.0/0".to_string());

        let dest = rule
            .matches
            .iter()
            .find_map(|m| {
                if let MatchExpr::Daddr(a) = m {
                    Some(a.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "0.0.0.0/0".to_string());

        let mut extra_parts: Vec<String> = Vec::new();
        for m in &rule.matches {
            match m {
                MatchExpr::Sport(p) => extra_parts.push(format!("spt:{}", p)),
                MatchExpr::Dport(p) => extra_parts.push(format!("dpt:{}", p)),
                MatchExpr::SportRange(lo, hi) => {
                    extra_parts.push(format!("spts:{}-{}", lo, hi));
                }
                MatchExpr::DportRange(lo, hi) => {
                    extra_parts.push(format!("dpts:{}-{}", lo, hi));
                }
                MatchExpr::Iif(i) => extra_parts.push(format!("in:{}", i)),
                MatchExpr::Oif(i) => extra_parts.push(format!("out:{}", i)),
                MatchExpr::CtState(s) => extra_parts.push(format!("state:{}", s)),
                _ => {}
            }
        }
        let extra = if extra_parts.is_empty() {
            String::new()
        } else {
            extra_parts.join(" ")
        };

        let _ = writeln!(
            out,
            "{:<8}{:<6}{:<8}{:<18}{:<18}{}",
            idx + 1,
            proto,
            target,
            source,
            dest,
            extra
        );
    }
    Ok(())
}

// ============================================================================
// Main entry point
// ============================================================================

fn run(args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "nft: no arguments");
        return 1;
    }

    // Determine personality from argv[0]
    let prog = args
        .first()
        .map(|a| {
            // Extract basename, stripping any path and extension
            let s = a.as_str();
            let base = s.rsplit('/').next().unwrap_or(s);
            let base = base.rsplit('\\').next().unwrap_or(base);
            base.trim_end_matches(".exe")
        })
        .unwrap_or("nft");

    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();

    match prog {
        "iptables" | "iptables-nft" => run_iptables(&rest, false, out),
        "ip6tables" | "ip6tables-nft" => run_iptables(&rest, true, out),
        _ => run_nft(&rest, out),
    }
}

fn run_nft(args: &[&str], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Usage: nft <command> [<args>]");
        let _ = writeln!(out, "\nCommands:");
        let _ = writeln!(out, "  add table|chain|rule|set|map|element|counter ...");
        let _ = writeln!(out, "  delete table|chain|rule ...");
        let _ = writeln!(
            out,
            "  list ruleset|tables|table|chain|sets|maps|counters ..."
        );
        let _ = writeln!(out, "  flush ruleset|table|chain ...");
        let _ = writeln!(out, "  insert rule ...");
        let _ = writeln!(out, "  create table|chain ...");
        return 0;
    }

    let mut rs = Ruleset::new();

    // A mutating verb changes only the throwaway in-memory ruleset; nothing is
    // persisted or applied to the kernel firewall. Warn so the user isn't misled
    // into thinking `nft add rule …` took effect. See design-decisions §62 (Q21).
    let mutating = matches!(
        args.first().copied(),
        Some("add" | "delete" | "flush" | "insert" | "create" | "replace")
    );

    match nft_command(&mut rs, args, out) {
        Ok(()) => {
            if mutating {
                print_not_applied_notice(out);
            }
            0
        }
        Err(e) => {
            let _ = writeln!(out, "{}", e);
            1
        }
    }
}

/// Notice printed after a syntactically-valid *mutating* command to make clear
/// that `nft`/`iptables` do not persist or apply rules to the kernel firewall.
/// The native `fw` tool is the working firewall front-end (§53, §62).
fn print_not_applied_notice(out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "note: rule parsed but NOT applied — nft/iptables do not configure the \
         Slate OS kernel firewall. Use `fw` to apply firewall rules."
    );
}

fn run_iptables(args: &[&str], ipv6: bool, out: &mut dyn Write) -> i32 {
    match parse_iptables_args(args, ipv6) {
        Ok(cmd) => {
            let mut rs = Ruleset::new();
            // Mutating actions touch only the throwaway ruleset — not persisted,
            // not applied to the kernel. Warn the user. See §62 (Q21).
            let mutating = matches!(
                cmd.action,
                IptAction::Append
                    | IptAction::Delete
                    | IptAction::Insert
                    | IptAction::Flush
                    | IptAction::Policy
                    | IptAction::NewChain
                    | IptAction::DeleteChain
            );
            match exec_iptables(&mut rs, &cmd, out) {
                Ok(()) => {
                    if mutating {
                        print_not_applied_notice(out);
                    }
                    0
                }
                Err(e) => {
                    let _ = writeln!(out, "{}", e);
                    1
                }
            }
        }
        Err(e) => {
            let _ = writeln!(out, "{}", e);
            1
        }
    }
}

// ============================================================================
// Entry point (Slate OS)
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let mut stdout = io::stdout().lock();
    run(&args, &mut stdout)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn out_buf() -> Vec<u8> {
        Vec::new()
    }

    // --- Family parsing ---

    #[test]
    fn test_family_parse_all() {
        assert_eq!(Family::parse("ip"), Some(Family::Ip));
        assert_eq!(Family::parse("ip6"), Some(Family::Ip6));
        assert_eq!(Family::parse("inet"), Some(Family::Inet));
        assert_eq!(Family::parse("arp"), Some(Family::Arp));
        assert_eq!(Family::parse("bridge"), Some(Family::Bridge));
        assert_eq!(Family::parse("netdev"), Some(Family::Netdev));
        assert_eq!(Family::parse("bogus"), None);
    }

    #[test]
    fn test_family_display() {
        assert_eq!(Family::Ip.to_string(), "ip");
        assert_eq!(Family::Ip6.to_string(), "ip6");
        assert_eq!(Family::Inet.to_string(), "inet");
    }

    // --- ChainType / Hook / Policy / Protocol parsing ---

    #[test]
    fn test_chain_type_parse() {
        assert_eq!(ChainType::parse("filter"), Some(ChainType::Filter));
        assert_eq!(ChainType::parse("nat"), Some(ChainType::Nat));
        assert_eq!(ChainType::parse("route"), Some(ChainType::Route));
        assert_eq!(ChainType::parse("bad"), None);
    }

    #[test]
    fn test_hook_parse() {
        assert_eq!(Hook::parse("input"), Some(Hook::Input));
        assert_eq!(Hook::parse("forward"), Some(Hook::Forward));
        assert_eq!(Hook::parse("output"), Some(Hook::Output));
        assert_eq!(Hook::parse("prerouting"), Some(Hook::Prerouting));
        assert_eq!(Hook::parse("postrouting"), Some(Hook::Postrouting));
        assert_eq!(Hook::parse("ingress"), Some(Hook::Ingress));
        assert_eq!(Hook::parse("none"), None);
    }

    #[test]
    fn test_policy_parse() {
        assert_eq!(Policy::parse("accept"), Some(Policy::Accept));
        assert_eq!(Policy::parse("drop"), Some(Policy::Drop));
        assert_eq!(Policy::parse("reject"), None);
    }

    #[test]
    fn test_protocol_parse() {
        assert_eq!(Protocol::parse("tcp"), Some(Protocol::Tcp));
        assert_eq!(Protocol::parse("udp"), Some(Protocol::Udp));
        assert_eq!(Protocol::parse("icmp"), Some(Protocol::Icmp));
        assert_eq!(Protocol::parse("icmpv6"), Some(Protocol::Icmpv6));
        assert_eq!(Protocol::parse("ipv6-icmp"), Some(Protocol::Icmpv6));
        assert_eq!(Protocol::parse("sctp"), None);
    }

    // --- Verdict display ---

    #[test]
    fn test_verdict_display() {
        assert_eq!(Verdict::Accept.to_string(), "accept");
        assert_eq!(Verdict::Drop.to_string(), "drop");
        assert_eq!(Verdict::Reject.to_string(), "reject");
        assert_eq!(Verdict::Masquerade.to_string(), "masquerade");
        assert_eq!(Verdict::Return.to_string(), "return");
        assert_eq!(
            Verdict::Log(Some("test".to_string())).to_string(),
            "log prefix \"test\""
        );
        assert_eq!(Verdict::Log(None).to_string(), "log");
        assert_eq!(
            Verdict::Snat("1.2.3.4".to_string()).to_string(),
            "snat to 1.2.3.4"
        );
        assert_eq!(
            Verdict::Dnat("5.6.7.8:80".to_string()).to_string(),
            "dnat to 5.6.7.8:80"
        );
        assert_eq!(
            Verdict::Jump("my_chain".to_string()).to_string(),
            "jump my_chain"
        );
        assert_eq!(Verdict::Goto("other".to_string()).to_string(), "goto other");
    }

    #[test]
    fn test_counter_verdict_display() {
        assert_eq!(Verdict::Counter(None).to_string(), "counter");
        assert_eq!(
            Verdict::Counter(Some("hits".to_string())).to_string(),
            "counter name \"hits\""
        );
    }

    // --- Ruleset table operations ---

    #[test]
    fn test_add_table() {
        let mut rs = Ruleset::new();
        assert!(rs.add_table(Family::Ip, "filter").is_ok());
        assert_eq!(rs.tables.len(), 1);
        assert_eq!(rs.tables[0].name, "filter");
    }

    #[test]
    fn test_add_table_duplicate_is_ok() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        // nft allows duplicate add silently
        assert!(rs.add_table(Family::Ip, "filter").is_ok());
        assert_eq!(rs.tables.len(), 1);
    }

    #[test]
    fn test_delete_table() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        assert!(rs.delete_table(Family::Ip, "filter").is_ok());
        assert!(rs.tables.is_empty());
    }

    #[test]
    fn test_delete_table_not_found() {
        let mut rs = Ruleset::new();
        assert!(rs.delete_table(Family::Ip, "missing").is_err());
    }

    #[test]
    fn test_flush_table() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_chain(Family::Ip, "filter", Chain::new("input"))
            .unwrap();
        let h = rs.alloc_handle();
        let mut rule = Rule::new(h);
        rule.verdicts.push(Verdict::Accept);
        rs.add_rule(Family::Ip, "filter", "input", rule).unwrap();
        assert_eq!(rs.tables[0].chains[0].rules.len(), 1);
        rs.flush_table(Family::Ip, "filter").unwrap();
        assert!(rs.tables[0].chains[0].rules.is_empty());
    }

    #[test]
    fn test_flush_table_not_found() {
        let mut rs = Ruleset::new();
        assert!(rs.flush_table(Family::Ip, "nope").is_err());
    }

    #[test]
    fn test_flush_ruleset() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_table(Family::Ip6, "filter").unwrap();
        rs.flush_ruleset();
        assert!(rs.tables.is_empty());
    }

    // --- Chain operations ---

    #[test]
    fn test_add_chain() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        assert!(
            rs.add_chain(Family::Ip, "filter", Chain::new("input"))
                .is_ok()
        );
        assert_eq!(rs.tables[0].chains.len(), 1);
    }

    #[test]
    fn test_add_base_chain() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        let chain = Chain::new_base("input", ChainType::Filter, Hook::Input, 0, Policy::Accept);
        rs.add_chain(Family::Ip, "filter", chain).unwrap();
        let c = &rs.tables[0].chains[0];
        assert_eq!(c.chain_type, Some(ChainType::Filter));
        assert_eq!(c.hook, Some(Hook::Input));
        assert_eq!(c.priority, Some(0));
        assert_eq!(c.policy, Some(Policy::Accept));
    }

    #[test]
    fn test_add_chain_no_table() {
        let mut rs = Ruleset::new();
        assert!(
            rs.add_chain(Family::Ip, "missing", Chain::new("x"))
                .is_err()
        );
    }

    #[test]
    fn test_delete_chain() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_chain(Family::Ip, "filter", Chain::new("input"))
            .unwrap();
        assert!(rs.delete_chain(Family::Ip, "filter", "input").is_ok());
        assert!(rs.tables[0].chains.is_empty());
    }

    #[test]
    fn test_delete_chain_not_found() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        assert!(rs.delete_chain(Family::Ip, "filter", "nope").is_err());
    }

    #[test]
    fn test_flush_chain() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_chain(Family::Ip, "filter", Chain::new("input"))
            .unwrap();
        let h = rs.alloc_handle();
        let mut rule = Rule::new(h);
        rule.verdicts.push(Verdict::Drop);
        rs.add_rule(Family::Ip, "filter", "input", rule).unwrap();
        rs.flush_chain(Family::Ip, "filter", "input").unwrap();
        assert!(rs.tables[0].chains[0].rules.is_empty());
    }

    // --- Rule operations ---

    #[test]
    fn test_add_rule() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_chain(Family::Ip, "filter", Chain::new("input"))
            .unwrap();
        let h = rs.alloc_handle();
        let mut rule = Rule::new(h);
        rule.matches.push(MatchExpr::Protocol(Protocol::Tcp));
        rule.matches.push(MatchExpr::Dport(80));
        rule.verdicts.push(Verdict::Accept);
        let handle = rs.add_rule(Family::Ip, "filter", "input", rule).unwrap();
        assert_eq!(handle, 1);
        assert_eq!(rs.tables[0].chains[0].rules.len(), 1);
    }

    #[test]
    fn test_delete_rule_by_handle() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_chain(Family::Ip, "filter", Chain::new("input"))
            .unwrap();
        let h = rs.alloc_handle();
        let mut rule = Rule::new(h);
        rule.verdicts.push(Verdict::Accept);
        rs.add_rule(Family::Ip, "filter", "input", rule).unwrap();
        assert!(rs.delete_rule(Family::Ip, "filter", "input", h).is_ok());
        assert!(rs.tables[0].chains[0].rules.is_empty());
    }

    #[test]
    fn test_delete_rule_not_found() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_chain(Family::Ip, "filter", Chain::new("input"))
            .unwrap();
        assert!(rs.delete_rule(Family::Ip, "filter", "input", 999).is_err());
    }

    #[test]
    fn test_insert_rule() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_chain(Family::Ip, "filter", Chain::new("input"))
            .unwrap();
        let h1 = rs.alloc_handle();
        let mut r1 = Rule::new(h1);
        r1.verdicts.push(Verdict::Accept);
        rs.add_rule(Family::Ip, "filter", "input", r1).unwrap();

        let h2 = rs.alloc_handle();
        let mut r2 = Rule::new(h2);
        r2.verdicts.push(Verdict::Drop);
        rs.insert_rule(Family::Ip, "filter", "input", 0, r2)
            .unwrap();

        assert_eq!(rs.tables[0].chains[0].rules[0].verdicts[0], Verdict::Drop);
        assert_eq!(rs.tables[0].chains[0].rules[1].verdicts[0], Verdict::Accept);
    }

    // --- Set operations ---

    #[test]
    fn test_add_set() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_set(
            Family::Ip,
            "filter",
            NftSet {
                name: "blocked_ips".to_string(),
                set_type: "ipv4_addr".to_string(),
                elements: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(rs.tables[0].sets.len(), 1);
    }

    #[test]
    fn test_add_set_duplicate() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_set(
            Family::Ip,
            "filter",
            NftSet {
                name: "s".to_string(),
                set_type: "ipv4_addr".to_string(),
                elements: Vec::new(),
            },
        )
        .unwrap();
        assert!(
            rs.add_set(
                Family::Ip,
                "filter",
                NftSet {
                    name: "s".to_string(),
                    set_type: "ipv4_addr".to_string(),
                    elements: Vec::new(),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn test_add_set_element() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_set(
            Family::Ip,
            "filter",
            NftSet {
                name: "blocked".to_string(),
                set_type: "ipv4_addr".to_string(),
                elements: Vec::new(),
            },
        )
        .unwrap();
        rs.add_set_element(Family::Ip, "filter", "blocked", "10.0.0.1")
            .unwrap();
        rs.add_set_element(Family::Ip, "filter", "blocked", "10.0.0.2")
            .unwrap();
        assert_eq!(rs.tables[0].sets[0].elements.len(), 2);
    }

    #[test]
    fn test_add_set_element_no_set() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        assert!(
            rs.add_set_element(Family::Ip, "filter", "missing", "1.2.3.4")
                .is_err()
        );
    }

    #[test]
    fn test_add_set_element_dedup() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_set(
            Family::Ip,
            "filter",
            NftSet {
                name: "s".to_string(),
                set_type: "ipv4_addr".to_string(),
                elements: Vec::new(),
            },
        )
        .unwrap();
        rs.add_set_element(Family::Ip, "filter", "s", "1.1.1.1")
            .unwrap();
        rs.add_set_element(Family::Ip, "filter", "s", "1.1.1.1")
            .unwrap();
        assert_eq!(rs.tables[0].sets[0].elements.len(), 1);
    }

    // --- Map operations ---

    #[test]
    fn test_add_map() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "nat").unwrap();
        rs.add_map(
            Family::Ip,
            "nat",
            NftMap {
                name: "portmap".to_string(),
                key_type: "inet_service".to_string(),
                value_type: "ipv4_addr".to_string(),
                elements: BTreeMap::new(),
            },
        )
        .unwrap();
        assert_eq!(rs.tables[0].maps.len(), 1);
    }

    #[test]
    fn test_add_map_element() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "nat").unwrap();
        rs.add_map(
            Family::Ip,
            "nat",
            NftMap {
                name: "m".to_string(),
                key_type: "inet_service".to_string(),
                value_type: "ipv4_addr".to_string(),
                elements: BTreeMap::new(),
            },
        )
        .unwrap();
        rs.add_map_element(Family::Ip, "nat", "m", "80", "10.0.0.1")
            .unwrap();
        assert_eq!(rs.tables[0].maps[0].elements.len(), 1);
        assert_eq!(rs.tables[0].maps[0].elements["80"], "10.0.0.1");
    }

    #[test]
    fn test_add_map_duplicate() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "nat").unwrap();
        rs.add_map(
            Family::Ip,
            "nat",
            NftMap {
                name: "m".to_string(),
                key_type: "k".to_string(),
                value_type: "v".to_string(),
                elements: BTreeMap::new(),
            },
        )
        .unwrap();
        assert!(
            rs.add_map(
                Family::Ip,
                "nat",
                NftMap {
                    name: "m".to_string(),
                    key_type: "k".to_string(),
                    value_type: "v".to_string(),
                    elements: BTreeMap::new(),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn test_add_map_element_no_map() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "t").unwrap();
        assert!(
            rs.add_map_element(Family::Ip, "t", "missing", "k", "v")
                .is_err()
        );
    }

    // --- Counter operations ---

    #[test]
    fn test_add_counter() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_counter(Family::Ip, "filter", "http_count").unwrap();
        assert_eq!(rs.tables[0].counters.len(), 1);
        assert_eq!(rs.tables[0].counters[0].packets, 0);
    }

    #[test]
    fn test_add_counter_duplicate() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_counter(Family::Ip, "filter", "c").unwrap();
        assert!(rs.add_counter(Family::Ip, "filter", "c").is_err());
    }

    // --- Rule expression parsing ---

    #[test]
    fn test_parse_rule_accept() {
        let tokens = vec!["accept"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.verdicts.len(), 1);
        assert_eq!(rule.verdicts[0], Verdict::Accept);
    }

    #[test]
    fn test_parse_rule_tcp_dport_accept() {
        let tokens = vec!["tcp", "dport", "80", "accept"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert!(!rule.matches.is_empty());
        assert_eq!(rule.verdicts[0], Verdict::Accept);
    }

    #[test]
    fn test_parse_rule_ip_saddr() {
        let tokens = vec!["ip", "saddr", "192.168.1.0/24", "drop"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(
            rule.matches[0],
            MatchExpr::Saddr("192.168.1.0/24".to_string())
        );
        assert_eq!(rule.verdicts[0], Verdict::Drop);
    }

    #[test]
    fn test_parse_rule_dnat() {
        let tokens = vec!["dnat", "to", "10.0.0.1:8080"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.verdicts[0], Verdict::Dnat("10.0.0.1:8080".to_string()));
    }

    #[test]
    fn test_parse_rule_snat() {
        let tokens = vec!["snat", "to", "203.0.113.1"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.verdicts[0], Verdict::Snat("203.0.113.1".to_string()));
    }

    #[test]
    fn test_parse_rule_masquerade() {
        let tokens = vec!["masquerade"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.verdicts[0], Verdict::Masquerade);
    }

    #[test]
    fn test_parse_rule_log_prefix() {
        let tokens = vec!["log", "prefix", "\"DROPPED:\""];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.verdicts[0], Verdict::Log(Some("DROPPED:".to_string())));
    }

    #[test]
    fn test_parse_rule_counter_name() {
        let tokens = vec!["counter", "name", "\"http_hits\"", "accept"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(
            rule.verdicts[0],
            Verdict::Counter(Some("http_hits".to_string()))
        );
        assert_eq!(rule.verdicts[1], Verdict::Accept);
    }

    #[test]
    fn test_parse_rule_ct_state() {
        let tokens = vec!["ct", "state", "established,related", "accept"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(
            rule.matches[0],
            MatchExpr::CtState("established,related".to_string())
        );
    }

    #[test]
    fn test_parse_rule_iif_oif() {
        let tokens = vec!["iif", "eth0", "oif", "eth1", "accept"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.matches[0], MatchExpr::Iif("eth0".to_string()));
        assert_eq!(rule.matches[1], MatchExpr::Oif("eth1".to_string()));
    }

    #[test]
    fn test_parse_rule_meta() {
        let tokens = vec!["meta", "mark", "0xff", "accept"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(
            rule.matches[0],
            MatchExpr::Meta("mark".to_string(), "0xff".to_string())
        );
    }

    #[test]
    fn test_parse_rule_set_lookup() {
        let tokens = vec!["@blocked_ips", "drop"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(
            rule.matches[0],
            MatchExpr::SetLookup("blocked_ips".to_string())
        );
    }

    #[test]
    fn test_parse_rule_jump() {
        let tokens = vec!["jump", "my_chain"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.verdicts[0], Verdict::Jump("my_chain".to_string()));
    }

    #[test]
    fn test_parse_rule_goto() {
        let tokens = vec!["goto", "other_chain"];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.verdicts[0], Verdict::Goto("other_chain".to_string()));
    }

    #[test]
    fn test_parse_rule_empty() {
        let tokens: Vec<&str> = Vec::new();
        assert!(parse_rule_expr(&tokens, 1).is_err());
    }

    #[test]
    fn test_parse_rule_with_comment() {
        let tokens = vec!["accept", "comment", "\"allow all\""];
        let rule = parse_rule_expr(&tokens, 1).unwrap();
        assert_eq!(rule.comment, Some("allow all".to_string()));
    }

    // --- nft command integration ---

    #[test]
    fn test_nft_add_and_list_table() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        assert_eq!(rs.tables.len(), 1);

        let mut out = out_buf();
        nft_command(&mut rs, &["list", "tables"], &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("table ip filter"));
    }

    #[test]
    fn test_nft_add_chain_command() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "inet", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &["add", "chain", "inet", "filter", "input"],
            &mut out,
        )
        .unwrap();
        assert_eq!(rs.tables[0].chains.len(), 1);
    }

    #[test]
    fn test_nft_add_rule_command() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &["add", "chain", "ip", "filter", "input"],
            &mut out,
        )
        .unwrap();
        nft_command(
            &mut rs,
            &[
                "add", "rule", "ip", "filter", "input", "tcp", "dport", "80", "accept",
            ],
            &mut out,
        )
        .unwrap();
        assert_eq!(rs.tables[0].chains[0].rules.len(), 1);
    }

    #[test]
    fn test_nft_delete_rule_command() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &["add", "chain", "ip", "filter", "input"],
            &mut out,
        )
        .unwrap();
        nft_command(
            &mut rs,
            &["add", "rule", "ip", "filter", "input", "accept"],
            &mut out,
        )
        .unwrap();
        // Delete rule by handle 1
        nft_command(
            &mut rs,
            &["delete", "rule", "ip", "filter", "input", "handle", "1"],
            &mut out,
        )
        .unwrap();
        assert!(rs.tables[0].chains[0].rules.is_empty());
    }

    #[test]
    fn test_nft_flush_ruleset() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(&mut rs, &["flush", "ruleset"], &mut out).unwrap();
        assert!(rs.tables.is_empty());
    }

    #[test]
    fn test_nft_list_ruleset_output() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &["add", "chain", "ip", "filter", "input"],
            &mut out,
        )
        .unwrap();
        nft_command(
            &mut rs,
            &["add", "rule", "ip", "filter", "input", "drop"],
            &mut out,
        )
        .unwrap();

        let mut listing = out_buf();
        nft_command(&mut rs, &["list", "ruleset"], &mut listing).unwrap();
        let text = String::from_utf8(listing).unwrap();
        assert!(text.contains("table ip filter"));
        assert!(text.contains("chain input"));
        assert!(text.contains("drop"));
    }

    #[test]
    fn test_nft_create_table_fails_duplicate() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        let result = nft_command(&mut rs, &["create", "table", "ip", "filter"], &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_nft_create_chain_fails_duplicate() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &["add", "chain", "ip", "filter", "input"],
            &mut out,
        )
        .unwrap();
        let result = nft_command(
            &mut rs,
            &["create", "chain", "ip", "filter", "input"],
            &mut out,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_nft_insert_rule_at_front() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &["add", "chain", "ip", "filter", "input"],
            &mut out,
        )
        .unwrap();
        nft_command(
            &mut rs,
            &["add", "rule", "ip", "filter", "input", "accept"],
            &mut out,
        )
        .unwrap();
        nft_command(
            &mut rs,
            &["insert", "rule", "ip", "filter", "input", "drop"],
            &mut out,
        )
        .unwrap();

        assert_eq!(rs.tables[0].chains[0].rules[0].verdicts[0], Verdict::Drop);
    }

    #[test]
    fn test_nft_unknown_command() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let result = nft_command(&mut rs, &["bogus"], &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_nft_add_no_object() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let result = nft_command(&mut rs, &["add"], &mut out);
        assert!(result.is_err());
    }

    // --- nft set/map/counter command integration ---

    #[test]
    fn test_nft_add_set_command() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &[
                "add",
                "set",
                "ip",
                "filter",
                "blocked",
                "{",
                "type",
                "ipv4_addr",
                ";",
                "}",
            ],
            &mut out,
        )
        .unwrap();
        assert_eq!(rs.tables[0].sets.len(), 1);
        assert_eq!(rs.tables[0].sets[0].name, "blocked");
    }

    #[test]
    fn test_nft_add_element_to_set() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &[
                "add",
                "set",
                "ip",
                "filter",
                "blocked",
                "{",
                "type",
                "ipv4_addr",
                ";",
                "}",
            ],
            &mut out,
        )
        .unwrap();
        nft_command(
            &mut rs,
            &[
                "add",
                "element",
                "ip",
                "filter",
                "blocked",
                "{",
                "10.0.0.1,",
                "10.0.0.2",
                "}",
            ],
            &mut out,
        )
        .unwrap();
        assert_eq!(rs.tables[0].sets[0].elements.len(), 2);
    }

    #[test]
    fn test_nft_add_counter_command() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        nft_command(&mut rs, &["add", "table", "ip", "filter"], &mut out).unwrap();
        nft_command(
            &mut rs,
            &["add", "counter", "ip", "filter", "http_count"],
            &mut out,
        )
        .unwrap();
        assert_eq!(rs.tables[0].counters.len(), 1);
    }

    // --- iptables compatibility ---

    #[test]
    fn test_iptables_parse_append() {
        let args = vec!["-A", "INPUT", "-p", "tcp", "--dport", "22", "-j", "ACCEPT"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::Append);
        assert_eq!(cmd.chain, "INPUT");
        assert_eq!(cmd.proto, Some(Protocol::Tcp));
        assert_eq!(cmd.dport, Some(22));
        assert_eq!(cmd.target, Some("ACCEPT".to_string()));
    }

    #[test]
    fn test_iptables_parse_delete_by_num() {
        let args = vec!["-D", "INPUT", "3"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::Delete);
        assert_eq!(cmd.chain, "INPUT");
        assert_eq!(cmd.rule_num, Some(3));
    }

    #[test]
    fn test_iptables_parse_insert() {
        let args = vec!["-I", "INPUT", "2", "-j", "DROP"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::Insert);
        assert_eq!(cmd.rule_num, Some(2));
    }

    #[test]
    fn test_iptables_parse_list() {
        let args = vec!["-L", "-n"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::List);
        assert!(cmd.numeric);
    }

    #[test]
    fn test_iptables_parse_flush() {
        let args = vec!["-F", "INPUT"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::Flush);
        assert_eq!(cmd.chain, "INPUT");
    }

    #[test]
    fn test_iptables_parse_policy() {
        let args = vec!["-P", "INPUT", "DROP"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::Policy);
        assert_eq!(cmd.chain, "INPUT");
        assert_eq!(cmd.policy, Some("DROP".to_string()));
    }

    #[test]
    fn test_iptables_parse_nat_table() {
        let args = vec!["-t", "nat", "-A", "POSTROUTING", "-j", "MASQUERADE"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.table, "nat");
        assert_eq!(cmd.chain, "POSTROUTING");
        assert_eq!(cmd.target, Some("MASQUERADE".to_string()));
    }

    #[test]
    fn test_iptables_parse_source_dest() {
        let args = vec![
            "-A",
            "FORWARD",
            "-s",
            "10.0.0.0/8",
            "-d",
            "192.168.0.0/16",
            "-j",
            "ACCEPT",
        ];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.source, Some("10.0.0.0/8".to_string()));
        assert_eq!(cmd.dest, Some("192.168.0.0/16".to_string()));
    }

    #[test]
    fn test_iptables_parse_interfaces() {
        let args = vec!["-A", "FORWARD", "-i", "eth0", "-o", "eth1", "-j", "ACCEPT"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.in_iface, Some("eth0".to_string()));
        assert_eq!(cmd.out_iface, Some("eth1".to_string()));
    }

    #[test]
    fn test_iptables_parse_snat() {
        let args = vec![
            "-t",
            "nat",
            "-A",
            "POSTROUTING",
            "-j",
            "SNAT",
            "--to-source",
            "1.2.3.4",
        ];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.target, Some("SNAT".to_string()));
        assert_eq!(cmd.to_source, Some("1.2.3.4".to_string()));
    }

    #[test]
    fn test_iptables_parse_dnat() {
        let args = vec![
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
            "10.0.0.1:8080",
        ];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.target, Some("DNAT".to_string()));
        assert_eq!(cmd.to_dest, Some("10.0.0.1:8080".to_string()));
    }

    #[test]
    fn test_iptables_parse_log_prefix() {
        let args = vec!["-A", "INPUT", "-j", "LOG", "--log-prefix", "BLOCKED:"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.target, Some("LOG".to_string()));
        assert_eq!(cmd.log_prefix, Some("BLOCKED:".to_string()));
    }

    #[test]
    fn test_iptables_parse_conntrack() {
        let args = vec![
            "-A",
            "INPUT",
            "-m",
            "state",
            "--state",
            "ESTABLISHED,RELATED",
            "-j",
            "ACCEPT",
        ];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.ct_state, Some("ESTABLISHED,RELATED".to_string()));
    }

    #[test]
    fn test_iptables_parse_help() {
        let args = vec!["--help"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::Help);
    }

    #[test]
    fn test_iptables_parse_new_chain() {
        let args = vec!["-N", "MYCHAIN"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::NewChain);
        assert_eq!(cmd.chain, "MYCHAIN");
    }

    #[test]
    fn test_iptables_parse_delete_chain() {
        let args = vec!["-X", "MYCHAIN"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        assert_eq!(cmd.action, IptAction::DeleteChain);
        assert_eq!(cmd.chain, "MYCHAIN");
    }

    #[test]
    fn test_ip6tables_family() {
        let args = vec!["-L"];
        let cmd = parse_iptables_args(&args, true).unwrap();
        assert!(cmd.ipv6);
        assert_eq!(iptables_table_family(&cmd), Family::Ip6);
    }

    // --- iptables execution ---

    #[test]
    fn test_iptables_exec_list() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let args = vec!["-L"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        exec_iptables(&mut rs, &cmd, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Chain INPUT"));
    }

    #[test]
    fn test_iptables_exec_append_and_list() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let args = vec!["-A", "INPUT", "-p", "tcp", "--dport", "80", "-j", "ACCEPT"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        exec_iptables(&mut rs, &cmd, &mut out).unwrap();

        let mut listing = out_buf();
        let list_args = vec!["-L", "INPUT", "-n"];
        let list_cmd = parse_iptables_args(&list_args, false).unwrap();
        exec_iptables(&mut rs, &list_cmd, &mut listing).unwrap();
        let text = String::from_utf8(listing).unwrap();
        assert!(text.contains("ACCEPT"));
        assert!(text.contains("tcp"));
    }

    #[test]
    fn test_iptables_exec_policy() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let args = vec!["-P", "INPUT", "DROP"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        exec_iptables(&mut rs, &cmd, &mut out).unwrap();
        let tidx = rs.find_table(Family::Ip, "filter").unwrap();
        let cidx = rs.find_chain(tidx, "INPUT").unwrap();
        assert_eq!(rs.tables[tidx].chains[cidx].policy, Some(Policy::Drop));
    }

    #[test]
    fn test_iptables_exec_flush() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let args = vec!["-A", "INPUT", "-j", "DROP"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        exec_iptables(&mut rs, &cmd, &mut out).unwrap();

        let flush_args = vec!["-F", "INPUT"];
        let flush_cmd = parse_iptables_args(&flush_args, false).unwrap();
        exec_iptables(&mut rs, &flush_cmd, &mut out).unwrap();

        let tidx = rs.find_table(Family::Ip, "filter").unwrap();
        let cidx = rs.find_chain(tidx, "INPUT").unwrap();
        assert!(rs.tables[tidx].chains[cidx].rules.is_empty());
    }

    #[test]
    fn test_iptables_exec_insert() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();

        // Append two rules
        let a1 = vec!["-A", "INPUT", "-j", "ACCEPT"];
        exec_iptables(&mut rs, &parse_iptables_args(&a1, false).unwrap(), &mut out).unwrap();
        let a2 = vec!["-A", "INPUT", "-j", "REJECT"];
        exec_iptables(&mut rs, &parse_iptables_args(&a2, false).unwrap(), &mut out).unwrap();

        // Insert DROP at position 1 (front)
        let ins = vec!["-I", "INPUT", "1", "-j", "DROP"];
        exec_iptables(
            &mut rs,
            &parse_iptables_args(&ins, false).unwrap(),
            &mut out,
        )
        .unwrap();

        let tidx = rs.find_table(Family::Ip, "filter").unwrap();
        let cidx = rs.find_chain(tidx, "INPUT").unwrap();
        assert_eq!(
            rs.tables[tidx].chains[cidx].rules[0].verdicts[0],
            Verdict::Drop
        );
    }

    #[test]
    fn test_iptables_exec_delete_by_num() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let a1 = vec!["-A", "INPUT", "-j", "ACCEPT"];
        exec_iptables(&mut rs, &parse_iptables_args(&a1, false).unwrap(), &mut out).unwrap();
        let a2 = vec!["-A", "INPUT", "-j", "DROP"];
        exec_iptables(&mut rs, &parse_iptables_args(&a2, false).unwrap(), &mut out).unwrap();

        let del = vec!["-D", "INPUT", "1"];
        exec_iptables(
            &mut rs,
            &parse_iptables_args(&del, false).unwrap(),
            &mut out,
        )
        .unwrap();

        let tidx = rs.find_table(Family::Ip, "filter").unwrap();
        let cidx = rs.find_chain(tidx, "INPUT").unwrap();
        assert_eq!(rs.tables[tidx].chains[cidx].rules.len(), 1);
        assert_eq!(
            rs.tables[tidx].chains[cidx].rules[0].verdicts[0],
            Verdict::Drop
        );
    }

    #[test]
    fn test_iptables_exec_help() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let args = vec!["--help"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        exec_iptables(&mut rs, &cmd, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Usage: iptables"));
    }

    #[test]
    fn test_ip6tables_exec_help() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let args = vec!["-h"];
        let cmd = parse_iptables_args(&args, true).unwrap();
        exec_iptables(&mut rs, &cmd, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Usage: ip6tables"));
    }

    // --- Personality dispatch ---

    #[test]
    fn test_personality_nft() {
        let args = vec![
            "nft".to_string(),
            "add".to_string(),
            "table".to_string(),
            "ip".to_string(),
            "filter".to_string(),
        ];
        let mut out = out_buf();
        let rc = run(&args, &mut out);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_personality_iptables() {
        let args = vec!["iptables".to_string(), "-L".to_string()];
        let mut out = out_buf();
        let rc = run(&args, &mut out);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_personality_ip6tables() {
        let args = vec!["ip6tables".to_string(), "--help".to_string()];
        let mut out = out_buf();
        let rc = run(&args, &mut out);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_personality_with_path() {
        let args = vec!["/usr/sbin/iptables".to_string(), "-L".to_string()];
        let mut out = out_buf();
        let rc = run(&args, &mut out);
        assert_eq!(rc, 0);
    }

    #[test]
    fn test_personality_empty_args() {
        let args: Vec<String> = Vec::new();
        let mut out = out_buf();
        let rc = run(&args, &mut out);
        assert_eq!(rc, 1);
    }

    #[test]
    fn test_nft_no_subcommand_shows_usage() {
        let args = vec!["nft".to_string()];
        let mut out = out_buf();
        let rc = run(&args, &mut out);
        assert_eq!(rc, 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Usage: nft"));
    }

    // --- Formatting output ---

    #[test]
    fn test_format_table_with_sets_and_maps() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_set(
            Family::Ip,
            "filter",
            NftSet {
                name: "blocked".to_string(),
                set_type: "ipv4_addr".to_string(),
                elements: vec!["1.1.1.1".to_string(), "2.2.2.2".to_string()],
            },
        )
        .unwrap();
        rs.add_map(
            Family::Ip,
            "filter",
            NftMap {
                name: "redir".to_string(),
                key_type: "inet_service".to_string(),
                value_type: "ipv4_addr".to_string(),
                elements: {
                    let mut m = BTreeMap::new();
                    m.insert("80".to_string(), "10.0.0.1".to_string());
                    m
                },
            },
        )
        .unwrap();

        let mut out = out_buf();
        format_ruleset(&rs, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("set blocked"));
        assert!(text.contains("1.1.1.1, 2.2.2.2"));
        assert!(text.contains("map redir"));
        assert!(text.contains("80 : 10.0.0.1"));
    }

    #[test]
    fn test_format_chain_with_base() {
        let chain = Chain::new_base("input", ChainType::Filter, Hook::Input, 0, Policy::Accept);
        let mut out = out_buf();
        format_chain(&chain, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("type filter hook input priority 0"));
        assert!(text.contains("policy accept"));
    }

    // --- Handle allocation ---

    #[test]
    fn test_handle_allocation() {
        let mut rs = Ruleset::new();
        let h1 = rs.alloc_handle();
        let h2 = rs.alloc_handle();
        let h3 = rs.alloc_handle();
        assert_eq!(h1, 1);
        assert_eq!(h2, 2);
        assert_eq!(h3, 3);
    }

    // --- Multiple families ---

    #[test]
    fn test_different_families_independent() {
        let mut rs = Ruleset::new();
        rs.add_table(Family::Ip, "filter").unwrap();
        rs.add_table(Family::Ip6, "filter").unwrap();
        assert_eq!(rs.tables.len(), 2);
        rs.delete_table(Family::Ip, "filter").unwrap();
        assert_eq!(rs.tables.len(), 1);
        assert_eq!(rs.tables[0].family, Family::Ip6);
    }

    // --- iptables nat table auto-creation ---

    #[test]
    fn test_iptables_nat_table_chains() {
        let mut rs = Ruleset::new();
        let mut out = out_buf();
        let args = vec!["-t", "nat", "-A", "POSTROUTING", "-j", "MASQUERADE"];
        let cmd = parse_iptables_args(&args, false).unwrap();
        exec_iptables(&mut rs, &cmd, &mut out).unwrap();

        let tidx = rs.find_table(Family::Ip, "nat").unwrap();
        let table = &rs.tables[tidx];
        // nat table should have PREROUTING, INPUT, OUTPUT, POSTROUTING chains
        assert!(table.chains.iter().any(|c| c.name == "PREROUTING"));
        assert!(table.chains.iter().any(|c| c.name == "INPUT"));
        assert!(table.chains.iter().any(|c| c.name == "OUTPUT"));
        assert!(table.chains.iter().any(|c| c.name == "POSTROUTING"));
    }

    // --- Q21 (§62): mutating commands print a "NOT applied" notice ---

    fn buf_to_string(b: &[u8]) -> String {
        String::from_utf8(b.to_vec()).unwrap()
    }

    #[test]
    fn test_nft_mutating_prints_not_applied_notice() {
        let mut out = out_buf();
        let rc = run_nft(&["add", "table", "ip", "filter"], &mut out);
        assert_eq!(rc, 0);
        let s = buf_to_string(&out);
        assert!(
            s.contains("NOT applied") && s.contains("`fw`"),
            "mutating nft command must warn it isn't applied; got: {s:?}"
        );
    }

    #[test]
    fn test_nft_list_no_notice() {
        // Read-only `list` must NOT print the not-applied notice.
        let mut out = out_buf();
        let rc = run_nft(&["list", "ruleset"], &mut out);
        assert_eq!(rc, 0);
        let s = buf_to_string(&out);
        assert!(
            !s.contains("NOT applied"),
            "read-only list must not warn about application; got: {s:?}"
        );
    }

    #[test]
    fn test_iptables_mutating_prints_not_applied_notice() {
        let mut out = out_buf();
        let rc = run_iptables(
            &["-A", "INPUT", "-p", "tcp", "--dport", "22", "-j", "ACCEPT"],
            false,
            &mut out,
        );
        assert_eq!(rc, 0);
        let s = buf_to_string(&out);
        assert!(
            s.contains("NOT applied") && s.contains("`fw`"),
            "mutating iptables command must warn it isn't applied; got: {s:?}"
        );
    }

    #[test]
    fn test_iptables_list_no_notice() {
        let mut out = out_buf();
        let rc = run_iptables(&["-L"], false, &mut out);
        assert_eq!(rc, 0);
        let s = buf_to_string(&out);
        assert!(
            !s.contains("NOT applied"),
            "read-only -L must not warn about application; got: {s:?}"
        );
    }
}
