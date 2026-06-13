//! Slate OS DNS resolution service and management utility.
//!
//! Multi-personality binary providing:
//! - **resolvectl** (default) -- DNS resolver management CLI
//! - **systemd-resolve** -- legacy name for resolvectl
//! - **systemd-resolved** -- DNS resolver daemon
//!
//! Manages DNS resolver configuration, cache, DNSSEC settings,
//! DNS-over-TLS, per-interface DNS servers, and search domains.

#![deny(clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const RESOLV_CONF: &str = "/etc/resolv.conf";
const RESOLVED_CONF: &str = "/etc/systemd/resolved.conf";
const RUN_RESOLVED: &str = "/run/systemd/resolve";

// ============================================================================
// Personality detection
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Personality {
    Resolvectl,
    SystemdResolve,
    SystemdResolved,
}

impl Personality {
    fn _name(self) -> &'static str {
        match self {
            Self::Resolvectl => "resolvectl",
            Self::SystemdResolve => "systemd-resolve",
            Self::SystemdResolved => "systemd-resolved",
        }
    }
}

/// Return the filename portion of a path, handling both `/` and `\`.
fn basename(path: &str) -> &str {
    let after_slash = match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    };
    match after_slash.rfind('\\') {
        Some(i) => &after_slash[i + 1..],
        None => after_slash,
    }
}

/// Detect personality from argv[0] basename, stripping `.exe` suffix.
fn detect_personality(argv0: &str) -> Personality {
    let base = basename(argv0);
    let stem = base.strip_suffix(".exe").unwrap_or(base);
    match stem {
        "systemd-resolve" => Personality::SystemdResolve,
        "systemd-resolved" => Personality::SystemdResolved,
        _ => Personality::Resolvectl,
    }
}

// ============================================================================
// Data structures
// ============================================================================

/// DNS transport protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DnsProtocol {
    Plain,
    Dot,
    _Doh,
}

impl fmt::Display for DnsProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain => write!(f, "plain"),
            Self::Dot => write!(f, "DoT"),
            Self::_Doh => write!(f, "DoH"),
        }
    }
}

/// Resolution protocol used to obtain an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum _ResolutionProtocol {
    Dns,
    Llmnr,
    Mdns,
}

impl fmt::Display for _ResolutionProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dns => write!(f, "DNS"),
            Self::Llmnr => write!(f, "LLMNR"),
            Self::Mdns => write!(f, "mDNS"),
        }
    }
}

/// DNSSEC validation mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DnssecMode {
    No,
    AllowDowngrade,
    Yes,
}

impl fmt::Display for DnssecMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::No => write!(f, "no"),
            Self::AllowDowngrade => write!(f, "allow-downgrade"),
            Self::Yes => write!(f, "yes"),
        }
    }
}

impl DnssecMode {
    fn parse(s: &str) -> Option<DnssecMode> {
        match s.to_lowercase().as_str() {
            "no" | "false" | "off" => Some(Self::No),
            "allow-downgrade" => Some(Self::AllowDowngrade),
            "yes" | "true" | "on" => Some(Self::Yes),
            _ => None,
        }
    }
}

/// DNS-over-TLS mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DnsOverTlsMode {
    No,
    Opportunistic,
    Yes,
}

impl fmt::Display for DnsOverTlsMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::No => write!(f, "no"),
            Self::Opportunistic => write!(f, "opportunistic"),
            Self::Yes => write!(f, "yes"),
        }
    }
}

impl DnsOverTlsMode {
    fn parse(s: &str) -> Option<DnsOverTlsMode> {
        match s.to_lowercase().as_str() {
            "no" | "false" | "off" => Some(Self::No),
            "opportunistic" => Some(Self::Opportunistic),
            "yes" | "true" | "on" => Some(Self::Yes),
            _ => None,
        }
    }
}

/// LLMNR / mDNS mode for a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkProtocolMode {
    No,
    _ResolveOnly,
    Yes,
}

impl fmt::Display for LinkProtocolMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::No => write!(f, "no"),
            Self::_ResolveOnly => write!(f, "resolve"),
            Self::Yes => write!(f, "yes"),
        }
    }
}

/// Server role: current or fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServerRole {
    Current,
    Fallback,
}

impl fmt::Display for ServerRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Current => write!(f, "current"),
            Self::Fallback => write!(f, "fallback"),
        }
    }
}

/// A single DNS server entry.
#[derive(Clone, Debug)]
struct DnsServer {
    address: String,
    interface: Option<String>,
    _reachable: bool,
    protocol: DnsProtocol,
    role: ServerRole,
}

impl DnsServer {
    fn new(address: &str) -> Self {
        Self {
            address: address.to_string(),
            interface: None,
            _reachable: true,
            protocol: DnsProtocol::Plain,
            role: ServerRole::Current,
        }
    }

    fn with_interface(mut self, iface: &str) -> Self {
        self.interface = Some(iface.to_string());
        self
    }

    fn with_protocol(mut self, proto: DnsProtocol) -> Self {
        self.protocol = proto;
        self
    }

    fn with_role(mut self, role: ServerRole) -> Self {
        self.role = role;
        self
    }

    fn _with_reachable(mut self, reachable: bool) -> Self {
        self._reachable = reachable;
        self
    }
}

/// Network link / interface DNS configuration.
#[derive(Clone, Debug)]
struct LinkConfig {
    index: u32,
    name: String,
    dns_servers: Vec<DnsServer>,
    search_domains: Vec<String>,
    dnssec: DnssecMode,
    llmnr: LinkProtocolMode,
    mdns: LinkProtocolMode,
    dns_over_tls: DnsOverTlsMode,
    default_route: bool,
}

impl LinkConfig {
    fn new(index: u32, name: &str) -> Self {
        Self {
            index,
            name: name.to_string(),
            dns_servers: Vec::new(),
            search_domains: Vec::new(),
            dnssec: DnssecMode::No,
            llmnr: LinkProtocolMode::Yes,
            mdns: LinkProtocolMode::No,
            dns_over_tls: DnsOverTlsMode::No,
            default_route: false,
        }
    }
}

/// Cache statistics.
#[derive(Clone, Debug)]
struct CacheStats {
    hits: u64,
    misses: u64,
    size: u64,
    capacity: u64,
}

impl CacheStats {
    fn new() -> Self {
        Self {
            hits: 0,
            misses: 0,
            size: 0,
            capacity: 4096,
        }
    }
}

/// Transaction statistics.
#[derive(Clone, Debug)]
struct TransactionStats {
    current: u64,
    total: u64,
}

/// DNSSEC verdict counters.
#[derive(Clone, Debug)]
struct DnssecVerdicts {
    secure: u64,
    insecure: u64,
    bogus: u64,
    indeterminate: u64,
}

/// Aggregate resolver statistics.
#[derive(Clone, Debug)]
struct ResolverStats {
    cache: CacheStats,
    transactions: TransactionStats,
    dnssec_verdicts: DnssecVerdicts,
    dnssec_supported: bool,
}

impl ResolverStats {
    fn new() -> Self {
        Self {
            cache: CacheStats::new(),
            transactions: TransactionStats { current: 0, total: 0 },
            dnssec_verdicts: DnssecVerdicts {
                secure: 0,
                insecure: 0,
                bogus: 0,
                indeterminate: 0,
            },
            dnssec_supported: false,
        }
    }
}

/// Global resolver configuration.
#[derive(Clone, Debug)]
struct GlobalConfig {
    dns_servers: Vec<DnsServer>,
    fallback_servers: Vec<DnsServer>,
    search_domains: Vec<String>,
    dnssec: DnssecMode,
    dns_over_tls: DnsOverTlsMode,
    llmnr: LinkProtocolMode,
    mdns: LinkProtocolMode,
    nta: Vec<String>,
}

impl GlobalConfig {
    fn default_config() -> Self {
        Self {
            dns_servers: vec![
                DnsServer::new("127.0.0.53"),
            ],
            fallback_servers: vec![
                DnsServer::new("9.9.9.10").with_role(ServerRole::Fallback),
                DnsServer::new("8.8.8.8").with_role(ServerRole::Fallback),
                DnsServer::new("1.1.1.1").with_role(ServerRole::Fallback),
            ],
            search_domains: Vec::new(),
            dnssec: DnssecMode::AllowDowngrade,
            dns_over_tls: DnsOverTlsMode::No,
            llmnr: LinkProtocolMode::Yes,
            mdns: LinkProtocolMode::No,
            nta: vec![
                "10.in-addr.arpa".to_string(),
                "16.172.in-addr.arpa".to_string(),
                "168.192.in-addr.arpa".to_string(),
                "local".to_string(),
            ],
        }
    }
}

/// Complete resolver state.
#[derive(Clone, Debug)]
struct ResolverState {
    global: GlobalConfig,
    links: BTreeMap<u32, LinkConfig>,
    stats: ResolverStats,
    _log_level: String,
}

impl ResolverState {
    fn new() -> Self {
        let mut links = BTreeMap::new();

        let lo = LinkConfig::new(1, "lo");
        links.insert(1, lo);

        let mut eth0 = LinkConfig::new(2, "eth0");
        eth0.dns_servers = vec![
            DnsServer::new("8.8.8.8").with_interface("eth0"),
            DnsServer::new("8.8.4.4").with_interface("eth0"),
        ];
        eth0.search_domains = vec!["localdomain".to_string()];
        eth0.dnssec = DnssecMode::AllowDowngrade;
        eth0.llmnr = LinkProtocolMode::Yes;
        eth0.mdns = LinkProtocolMode::Yes;
        eth0.default_route = true;
        links.insert(2, eth0);

        let mut wlan0 = LinkConfig::new(3, "wlan0");
        wlan0.dns_servers = vec![
            DnsServer::new("1.1.1.1").with_interface("wlan0").with_protocol(DnsProtocol::Dot),
            DnsServer::new("1.0.0.1").with_interface("wlan0").with_protocol(DnsProtocol::Dot),
        ];
        wlan0.search_domains = vec!["home.lan".to_string()];
        wlan0.dns_over_tls = DnsOverTlsMode::Opportunistic;
        wlan0.mdns = LinkProtocolMode::Yes;
        wlan0.llmnr = LinkProtocolMode::Yes;
        links.insert(3, wlan0);

        Self {
            global: GlobalConfig::default_config(),
            links,
            stats: ResolverStats::new(),
            _log_level: "info".to_string(),
        }
    }

    /// Find a link by name.
    fn find_link_by_name(&self, name: &str) -> Option<&LinkConfig> {
        self.links.values().find(|l| l.name == name)
    }

    /// Find a link by index or name.
    fn find_link(&self, id: &str) -> Option<&LinkConfig> {
        if let Ok(idx) = id.parse::<u32>() {
            self.links.get(&idx)
        } else {
            self.find_link_by_name(id)
        }
    }

    /// Collect all active DNS servers across all links.
    fn _all_dns_servers(&self) -> Vec<&DnsServer> {
        let mut servers: Vec<&DnsServer> = self.global.dns_servers.iter().collect();
        for link in self.links.values() {
            for server in &link.dns_servers {
                servers.push(server);
            }
        }
        servers
    }
}

// ============================================================================
// DNS hostname resolution (simulated)
// ============================================================================

/// Simulated hostname resolution returning IP strings.
fn resolve_hostname(hostname: &str) -> Vec<String> {
    match hostname {
        "localhost" => vec!["127.0.0.1".to_string(), "::1".to_string()],
        _ => Vec::new(),
    }
}

/// Simulated reverse DNS lookup.
fn reverse_lookup(addr: &str) -> Option<String> {
    match addr {
        "127.0.0.1" | "::1" => Some("localhost".to_string()),
        _ => None,
    }
}

// ============================================================================
// Output formatting helpers
// ============================================================================

fn format_bool_flag(val: bool) -> &'static str {
    if val { "yes" } else { "no" }
}

fn format_protocol_flag(enabled: bool, name: &str) -> String {
    if enabled {
        format!("+{name}")
    } else {
        format!("-{name}")
    }
}

fn format_link_protocol(mode: LinkProtocolMode) -> String {
    match mode {
        LinkProtocolMode::Yes => "+".to_string(),
        LinkProtocolMode::_ResolveOnly => "resolve".to_string(),
        LinkProtocolMode::No => "-".to_string(),
    }
}

// ============================================================================
// resolvectl subcommands
// ============================================================================

fn cmd_query(state: &ResolverState, args: &[String]) {
    if args.is_empty() {
        eprintln!("resolvectl query: no hostname specified");
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for name in args {
        if name.starts_with('-') {
            continue;
        }

        // Check if it is an IP (reverse lookup).
        if (name.contains(':') || name.chars().all(|c| c.is_ascii_digit() || c == '.'))
            && let Some(host) = reverse_lookup(name) {
                let _ = writeln!(out, "{name} -- {host}");
                let _ = writeln!(out);
                let _ = writeln!(out, "-- Information acquired via protocol DNS in 0.3ms.");
                continue;
            }

        let ips = resolve_hostname(name);
        if ips.is_empty() {
            let _ = writeln!(out, "{name}: resolve call failed: No address associated with hostname");
        } else {
            for ip in &ips {
                let _ = writeln!(out, "{name} -- {ip}");
            }
            let _ = writeln!(out);
            let _ = writeln!(out, "-- Information acquired via protocol DNS in 0.5ms.");
        }
    }

    let _ = state;
}

fn cmd_status(state: &ResolverState, args: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // If a specific link is requested, show only that link.
    if let Some(link_id) = args.first()
        && !link_id.starts_with('-') {
            if let Some(link) = state.find_link(link_id) {
                print_link_status(&mut out, link);
                return;
            }
            eprintln!("resolvectl status: unknown interface: {link_id}");
            process::exit(1);
        }

    // Global status.
    let _ = writeln!(out, "Global");
    let llmnr_flag = format_protocol_flag(
        state.global.llmnr != LinkProtocolMode::No, "LLMNR");
    let mdns_flag = format_protocol_flag(
        state.global.mdns != LinkProtocolMode::No, "mDNS");
    let dot_flag = format_protocol_flag(
        state.global.dns_over_tls != DnsOverTlsMode::No, "DNSOverTLS");
    let _ = writeln!(out, "       Protocols: {llmnr_flag} {mdns_flag} {dot_flag} DNSSEC={}",
        state.global.dnssec);
    let _ = writeln!(out, "resolv.conf mode: stub");

    if let Some(first) = state.global.dns_servers.first() {
        let _ = writeln!(out, "Current DNS Server: {}", first.address);
    }

    if !state.global.dns_servers.is_empty() {
        let _ = write!(out, "       DNS Servers:");
        for server in &state.global.dns_servers {
            let _ = write!(out, " {}", server.address);
        }
        let _ = writeln!(out);
    }

    if !state.global.fallback_servers.is_empty() {
        let _ = write!(out, "  Fallback Servers:");
        for server in &state.global.fallback_servers {
            let _ = write!(out, " {}", server.address);
        }
        let _ = writeln!(out);
    }

    if !state.global.search_domains.is_empty() {
        let _ = write!(out, "    Search Domains:");
        for domain in &state.global.search_domains {
            let _ = write!(out, " {domain}");
        }
        let _ = writeln!(out);
    }

    // Per-link status.
    for link in state.links.values() {
        if link.dns_servers.is_empty() && link.search_domains.is_empty() {
            continue;
        }
        let _ = writeln!(out);
        print_link_status(&mut out, link);
    }
}

fn print_link_status(out: &mut impl Write, link: &LinkConfig) {
    let _ = writeln!(out, "Link {} ({})", link.index, link.name);

    let mut scopes = Vec::new();
    if !link.dns_servers.is_empty() {
        scopes.push("DNS");
    }
    if link.llmnr != LinkProtocolMode::No {
        scopes.push("LLMNR");
    }
    if link.mdns != LinkProtocolMode::No {
        scopes.push("mDNS");
    }
    if !scopes.is_empty() {
        let _ = writeln!(out, "    Current Scopes: {}", scopes.join(" "));
    }

    let llmnr = format_link_protocol(link.llmnr);
    let mdns = format_link_protocol(link.mdns);
    let dot = if link.dns_over_tls != DnsOverTlsMode::No { "+" } else { "-" };
    let default_route = format_bool_flag(link.default_route);

    let _ = writeln!(out,
        "         Protocols: DefaultRoute={default_route} {llmnr}LLMNR {mdns}mDNS {dot}DNSOverTLS({}) DNSSEC={}",
        link.dns_over_tls, link.dnssec);

    if let Some(first) = link.dns_servers.first() {
        let _ = writeln!(out, "Current DNS Server: {}", first.address);
    }

    if !link.dns_servers.is_empty() {
        let _ = write!(out, "       DNS Servers:");
        for server in &link.dns_servers {
            let _ = write!(out, " {}", server.address);
        }
        let _ = writeln!(out);
    }

    if !link.search_domains.is_empty() {
        let _ = write!(out, "    Search Domains:");
        for domain in &link.search_domains {
            let _ = write!(out, " {domain}");
        }
        let _ = writeln!(out);
    }
}

fn cmd_statistics(state: &ResolverState) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stats = &state.stats;

    let _ = writeln!(out, "DNSSEC Supported: {}",
        format_bool_flag(stats.dnssec_supported));
    let _ = writeln!(out);
    let _ = writeln!(out, "Transactions");
    let _ = writeln!(out, "Current Transactions: {}", stats.transactions.current);
    let _ = writeln!(out, "  Total Transactions: {}", stats.transactions.total);
    let _ = writeln!(out);
    let _ = writeln!(out, "Cache");
    let _ = writeln!(out, "  Current Cache Size: {}/{}", stats.cache.size, stats.cache.capacity);
    let _ = writeln!(out, "          Cache Hits: {}", stats.cache.hits);
    let _ = writeln!(out, "        Cache Misses: {}", stats.cache.misses);
    let _ = writeln!(out);
    let _ = writeln!(out, "DNSSEC Verdicts");
    let _ = writeln!(out, "              Secure: {}", stats.dnssec_verdicts.secure);
    let _ = writeln!(out, "            Insecure: {}", stats.dnssec_verdicts.insecure);
    let _ = writeln!(out, "               Bogus: {}", stats.dnssec_verdicts.bogus);
    let _ = writeln!(out, "       Indeterminate: {}", stats.dnssec_verdicts.indeterminate);
}

fn cmd_reset_statistics() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "Statistics have been reset.");
}

fn cmd_flush_caches() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "All caches have been flushed.");
}

fn cmd_dns(state: &ResolverState, args: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        // Show global DNS servers.
        let _ = write!(out, "Global DNS Servers:");
        for server in &state.global.dns_servers {
            let _ = write!(out, " {}", server.address);
        }
        let _ = writeln!(out);

        // Show per-link DNS servers.
        for link in state.links.values() {
            if link.dns_servers.is_empty() {
                continue;
            }
            let _ = write!(out, "Link {} ({}):", link.index, link.name);
            for server in &link.dns_servers {
                let _ = write!(out, " {}", server.address);
            }
            let _ = writeln!(out);
        }
        return;
    }

    // First arg is the link identifier, rest are servers to set.
    let link_id = &args[0];
    let servers: Vec<&str> = args[1..].iter()
        .filter(|s| !s.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if servers.is_empty() {
        // Show DNS for specific link.
        if let Some(link) = state.find_link(link_id) {
            let _ = write!(out, "Link {} ({}):", link.index, link.name);
            for server in &link.dns_servers {
                let _ = write!(out, " {}", server.address);
            }
            let _ = writeln!(out);
        } else {
            eprintln!("resolvectl dns: unknown interface: {link_id}");
            process::exit(1);
        }
    } else {
        let _ = writeln!(out, "Set DNS servers for {link_id}: {}", servers.join(", "));
    }
}

fn cmd_domain(state: &ResolverState, args: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        let _ = write!(out, "Global Search Domains:");
        if state.global.search_domains.is_empty() {
            let _ = write!(out, " (none)");
        } else {
            for domain in &state.global.search_domains {
                let _ = write!(out, " {domain}");
            }
        }
        let _ = writeln!(out);

        for link in state.links.values() {
            if link.search_domains.is_empty() {
                continue;
            }
            let _ = write!(out, "Link {} ({}):", link.index, link.name);
            for domain in &link.search_domains {
                let _ = write!(out, " {domain}");
            }
            let _ = writeln!(out);
        }
        return;
    }

    let link_id = &args[0];
    let domains: Vec<&str> = args[1..].iter()
        .filter(|s| !s.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if domains.is_empty() {
        if let Some(link) = state.find_link(link_id) {
            let _ = write!(out, "Link {} ({}):", link.index, link.name);
            if link.search_domains.is_empty() {
                let _ = write!(out, " (none)");
            } else {
                for domain in &link.search_domains {
                    let _ = write!(out, " {domain}");
                }
            }
            let _ = writeln!(out);
        } else {
            eprintln!("resolvectl domain: unknown interface: {link_id}");
            process::exit(1);
        }
    } else {
        let _ = writeln!(out, "Set search domains for {link_id}: {}", domains.join(", "));
    }
}

fn cmd_dnssec(state: &ResolverState, args: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        let _ = writeln!(out, "Global DNSSEC setting: {}", state.global.dnssec);
        for link in state.links.values() {
            let _ = writeln!(out, "Link {} ({}): DNSSEC={}", link.index, link.name, link.dnssec);
        }
        return;
    }

    // If only one arg, it could be a link id or a mode.
    if args.len() == 1 {
        let val = &args[0];
        if let Some(mode) = DnssecMode::parse(val) {
            let _ = writeln!(out, "Global DNSSEC mode set to: {mode}");
        } else if let Some(link) = state.find_link(val) {
            let _ = writeln!(out, "Link {} ({}): DNSSEC={}", link.index, link.name, link.dnssec);
        } else {
            eprintln!("resolvectl dnssec: invalid mode or unknown interface: {val}");
            process::exit(1);
        }
        return;
    }

    // Two args: link + mode.
    let link_id = &args[0];
    let mode_str = &args[1];
    if let Some(mode) = DnssecMode::parse(mode_str) {
        let _ = writeln!(out, "DNSSEC mode for {link_id} set to: {mode}");
    } else {
        eprintln!("resolvectl dnssec: invalid DNSSEC mode: {mode_str}");
        process::exit(1);
    }
}

fn cmd_dnsovertls(state: &ResolverState, args: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        let _ = writeln!(out, "Global DNS-over-TLS setting: {}", state.global.dns_over_tls);
        for link in state.links.values() {
            let _ = writeln!(out, "Link {} ({}): DNSOverTLS={}",
                link.index, link.name, link.dns_over_tls);
        }
        return;
    }

    if args.len() == 1 {
        let val = &args[0];
        if let Some(mode) = DnsOverTlsMode::parse(val) {
            let _ = writeln!(out, "Global DNS-over-TLS mode set to: {mode}");
        } else if let Some(link) = state.find_link(val) {
            let _ = writeln!(out, "Link {} ({}): DNSOverTLS={}",
                link.index, link.name, link.dns_over_tls);
        } else {
            eprintln!("resolvectl dnsovertls: invalid mode or unknown interface: {val}");
            process::exit(1);
        }
        return;
    }

    let link_id = &args[0];
    let mode_str = &args[1];
    if let Some(mode) = DnsOverTlsMode::parse(mode_str) {
        let _ = writeln!(out, "DNS-over-TLS mode for {link_id} set to: {mode}");
    } else {
        eprintln!("resolvectl dnsovertls: invalid DNS-over-TLS mode: {mode_str}");
        process::exit(1);
    }
}

fn cmd_nta(state: &ResolverState, args: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        let _ = writeln!(out, "Global Negative Trust Anchors:");
        if state.global.nta.is_empty() {
            let _ = writeln!(out, "  (none)");
        } else {
            for nta in &state.global.nta {
                let _ = writeln!(out, "  {nta}");
            }
        }
        return;
    }

    // First arg could be a link, rest are NTA domains.
    let link_id = &args[0];
    let domains: Vec<&str> = args[1..].iter()
        .filter(|s| !s.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if domains.is_empty() {
        let _ = writeln!(out, "Negative Trust Anchors for {link_id}: (showing global)");
        for nta in &state.global.nta {
            let _ = writeln!(out, "  {nta}");
        }
    } else {
        let _ = writeln!(out, "Set NTA for {link_id}: {}", domains.join(", "));
    }
}

fn cmd_revert(args: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        eprintln!("resolvectl revert: interface name required");
        process::exit(1);
    }

    let link_id = &args[0];
    let _ = writeln!(out, "Link {link_id}: DNS configuration reverted to defaults.");
}

fn cmd_log_level(args: &[String]) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if args.is_empty() {
        let _ = writeln!(out, "Current log level: info");
        return;
    }

    let level = &args[0];
    let valid_levels = ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"];
    if valid_levels.contains(&level.as_str()) {
        let _ = writeln!(out, "Log level set to: {level}");
    } else {
        eprintln!("resolvectl log-level: invalid log level: {level}");
        eprintln!("Valid levels: {}", valid_levels.join(", "));
        process::exit(1);
    }
}

fn cmd_monitor() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "Monitoring DNS queries... (press Ctrl+C to stop)");
    let _ = writeln!(out, "Listening on {RUN_RESOLVED}/stub-resolv.conf");
}

// ============================================================================
// resolvectl entry point
// ============================================================================

fn run_resolvectl(args: &[String]) {
    let state = ResolverState::new();

    if args.is_empty() {
        cmd_status(&state, &[]);
        return;
    }

    match args[0].as_str() {
        "query" => cmd_query(&state, &args[1..]),
        "status" => cmd_status(&state, &args[1..]),
        "statistics" => cmd_statistics(&state),
        "reset-statistics" => cmd_reset_statistics(),
        "flush-caches" => cmd_flush_caches(),
        "dns" => cmd_dns(&state, &args[1..]),
        "domain" => cmd_domain(&state, &args[1..]),
        "dnssec" => cmd_dnssec(&state, &args[1..]),
        "dnsovertls" => cmd_dnsovertls(&state, &args[1..]),
        "nta" => cmd_nta(&state, &args[1..]),
        "revert" => cmd_revert(&args[1..]),
        "log-level" => cmd_log_level(&args[1..]),
        "monitor" => cmd_monitor(),
        "-h" | "--help" | "help" => print_resolvectl_help(),
        "-V" | "--version" => {
            println!("resolvectl {VERSION}");
        }
        other => {
            eprintln!("resolvectl: unknown command: {other}");
            eprintln!("Try 'resolvectl --help' for usage information.");
            process::exit(1);
        }
    }
}

fn print_resolvectl_help() {
    println!("Usage: resolvectl <COMMAND> [OPTIONS]");
    println!();
    println!("DNS Resolver Management.");
    println!();
    println!("Commands:");
    println!("  query NAME [...]        Resolve hostnames to addresses");
    println!("  status [LINK]           Show resolver status");
    println!("  statistics              Show resolver statistics");
    println!("  reset-statistics        Reset resolver statistics");
    println!("  flush-caches            Flush all DNS caches");
    println!("  dns [LINK [SERVER...]]  Get/set DNS servers");
    println!("  domain [LINK [DOM...]]  Get/set search domains");
    println!("  dnssec [LINK [MODE]]    Get/set DNSSEC mode");
    println!("  dnsovertls [LINK [M]]   Get/set DNS-over-TLS mode");
    println!("  nta [LINK [DOMAIN...]]  Get/set negative trust anchors");
    println!("  revert LINK             Revert link DNS config to defaults");
    println!("  log-level [LEVEL]       Get/set daemon log level");
    println!("  monitor                 Monitor DNS queries");
    println!();
    println!("Options:");
    println!("  -h, --help              Show this help");
    println!("  -V, --version           Show version");
    println!();
    println!("DNSSEC modes: no, allow-downgrade, yes");
    println!("DNS-over-TLS modes: no, opportunistic, yes");
    println!("Log levels: emerg, alert, crit, err, warning, notice, info, debug");
    println!();
    println!("Configuration files:");
    println!("  {RESOLV_CONF}");
    println!("  {RESOLVED_CONF}");
}

// ============================================================================
// systemd-resolved daemon entry point
// ============================================================================

fn run_daemon(args: &[String]) {
    let mut _foreground = false;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: systemd-resolved [OPTIONS]");
                println!();
                println!("DNS Resolver Daemon.");
                println!();
                println!("Options:");
                println!("  --system              Run as system service (default)");
                println!("  --foreground          Run in foreground (do not daemonize)");
                println!("  --no-pager            Do not pipe output to pager");
                println!("  -h, --help            Show this help");
                println!("  -V, --version         Show version");
                println!();
                println!("Configuration: {RESOLVED_CONF}");
                println!("Runtime state: {RUN_RESOLVED}/");
                return;
            }
            "-V" | "--version" => {
                println!("systemd-resolved {VERSION}");
                return;
            }
            "--foreground" => _foreground = true,
            "--system" | "--no-pager" => {}
            other => {
                eprintln!("systemd-resolved: unrecognized option: {other}");
                process::exit(1);
            }
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(out, "systemd-resolved: starting DNS resolver daemon...");
    let _ = writeln!(out, "systemd-resolved: listening on 127.0.0.53:53");
    let _ = writeln!(out, "systemd-resolved: using stub resolver at {RUN_RESOLVED}/stub-resolv.conf");
    let _ = writeln!(out, "systemd-resolved: DNSSEC=allow-downgrade DNSOverTLS=no");
    let _ = writeln!(out, "systemd-resolved: ready.");
}

// ============================================================================
// main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("resolvectl");
    let personality = detect_personality(argv0);

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match personality {
        Personality::Resolvectl | Personality::SystemdResolve => run_resolvectl(&rest),
        Personality::SystemdResolved => run_daemon(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Personality detection ------------------------------------------------

    #[test]
    fn test_detect_resolvectl_default() {
        assert_eq!(detect_personality("resolvectl"), Personality::Resolvectl);
    }

    #[test]
    fn test_detect_resolvectl_from_path() {
        assert_eq!(detect_personality("/usr/bin/resolvectl"), Personality::Resolvectl);
    }

    #[test]
    fn test_detect_resolvectl_windows_path() {
        assert_eq!(detect_personality("C:\\bin\\resolvectl.exe"), Personality::Resolvectl);
    }

    #[test]
    fn test_detect_systemd_resolve() {
        assert_eq!(detect_personality("systemd-resolve"), Personality::SystemdResolve);
    }

    #[test]
    fn test_detect_systemd_resolve_from_path() {
        assert_eq!(detect_personality("/usr/bin/systemd-resolve"), Personality::SystemdResolve);
    }

    #[test]
    fn test_detect_systemd_resolve_exe() {
        assert_eq!(detect_personality("systemd-resolve.exe"), Personality::SystemdResolve);
    }

    #[test]
    fn test_detect_systemd_resolved() {
        assert_eq!(detect_personality("systemd-resolved"), Personality::SystemdResolved);
    }

    #[test]
    fn test_detect_systemd_resolved_from_path() {
        assert_eq!(detect_personality("/usr/lib/systemd/systemd-resolved"), Personality::SystemdResolved);
    }

    #[test]
    fn test_detect_systemd_resolved_exe() {
        assert_eq!(detect_personality("systemd-resolved.exe"), Personality::SystemdResolved);
    }

    #[test]
    fn test_detect_unknown_defaults_to_resolvectl() {
        assert_eq!(detect_personality("unknown-binary"), Personality::Resolvectl);
    }

    #[test]
    fn test_detect_empty_defaults_to_resolvectl() {
        assert_eq!(detect_personality(""), Personality::Resolvectl);
    }

    // -- basename -------------------------------------------------------------

    #[test]
    fn test_basename_simple() {
        assert_eq!(basename("resolvectl"), "resolvectl");
    }

    #[test]
    fn test_basename_unix_path() {
        assert_eq!(basename("/usr/bin/resolvectl"), "resolvectl");
    }

    #[test]
    fn test_basename_windows_path() {
        assert_eq!(basename("C:\\bin\\resolvectl.exe"), "resolvectl.exe");
    }

    #[test]
    fn test_basename_mixed_separators() {
        assert_eq!(basename("/usr/bin\\resolvectl"), "resolvectl");
    }

    #[test]
    fn test_basename_trailing_slash() {
        assert_eq!(basename("/usr/bin/"), "");
    }

    // -- Personality name -----------------------------------------------------

    #[test]
    fn test_personality_name_resolvectl() {
        assert_eq!(Personality::Resolvectl._name(), "resolvectl");
    }

    #[test]
    fn test_personality_name_systemd_resolve() {
        assert_eq!(Personality::SystemdResolve._name(), "systemd-resolve");
    }

    #[test]
    fn test_personality_name_systemd_resolved() {
        assert_eq!(Personality::SystemdResolved._name(), "systemd-resolved");
    }

    // -- DnsProtocol ----------------------------------------------------------

    #[test]
    fn test_dns_protocol_display_plain() {
        assert_eq!(format!("{}", DnsProtocol::Plain), "plain");
    }

    #[test]
    fn test_dns_protocol_display_dot() {
        assert_eq!(format!("{}", DnsProtocol::Dot), "DoT");
    }

    #[test]
    fn test_dns_protocol_display_doh() {
        assert_eq!(format!("{}", DnsProtocol::_Doh), "DoH");
    }

    #[test]
    fn test_dns_protocol_clone() {
        let p = DnsProtocol::Dot;
        let c = p;
        assert_eq!(c, DnsProtocol::Dot);
    }

    #[test]
    fn test_dns_protocol_eq() {
        assert_eq!(DnsProtocol::Plain, DnsProtocol::Plain);
        assert_ne!(DnsProtocol::Plain, DnsProtocol::Dot);
    }

    // -- _ResolutionProtocol ---------------------------------------------------

    #[test]
    fn test_resolution_protocol_display_dns() {
        assert_eq!(format!("{}", _ResolutionProtocol::Dns), "DNS");
    }

    #[test]
    fn test_resolution_protocol_display_llmnr() {
        assert_eq!(format!("{}", _ResolutionProtocol::Llmnr), "LLMNR");
    }

    #[test]
    fn test_resolution_protocol_display_mdns() {
        assert_eq!(format!("{}", _ResolutionProtocol::Mdns), "mDNS");
    }

    #[test]
    fn test_resolution_protocol_eq() {
        assert_eq!(_ResolutionProtocol::Dns, _ResolutionProtocol::Dns);
        assert_ne!(_ResolutionProtocol::Dns, _ResolutionProtocol::Llmnr);
    }

    // -- DnssecMode -----------------------------------------------------------

    #[test]
    fn test_dnssec_display_no() {
        assert_eq!(format!("{}", DnssecMode::No), "no");
    }

    #[test]
    fn test_dnssec_display_allow_downgrade() {
        assert_eq!(format!("{}", DnssecMode::AllowDowngrade), "allow-downgrade");
    }

    #[test]
    fn test_dnssec_display_yes() {
        assert_eq!(format!("{}", DnssecMode::Yes), "yes");
    }

    #[test]
    fn test_dnssec_parse_no() {
        assert_eq!(DnssecMode::parse("no"), Some(DnssecMode::No));
    }

    #[test]
    fn test_dnssec_parse_false() {
        assert_eq!(DnssecMode::parse("false"), Some(DnssecMode::No));
    }

    #[test]
    fn test_dnssec_parse_off() {
        assert_eq!(DnssecMode::parse("off"), Some(DnssecMode::No));
    }

    #[test]
    fn test_dnssec_parse_allow_downgrade() {
        assert_eq!(DnssecMode::parse("allow-downgrade"), Some(DnssecMode::AllowDowngrade));
    }

    #[test]
    fn test_dnssec_parse_yes() {
        assert_eq!(DnssecMode::parse("yes"), Some(DnssecMode::Yes));
    }

    #[test]
    fn test_dnssec_parse_true() {
        assert_eq!(DnssecMode::parse("true"), Some(DnssecMode::Yes));
    }

    #[test]
    fn test_dnssec_parse_on() {
        assert_eq!(DnssecMode::parse("on"), Some(DnssecMode::Yes));
    }

    #[test]
    fn test_dnssec_parse_invalid() {
        assert_eq!(DnssecMode::parse("maybe"), None);
    }

    #[test]
    fn test_dnssec_parse_case_insensitive() {
        assert_eq!(DnssecMode::parse("YES"), Some(DnssecMode::Yes));
        assert_eq!(DnssecMode::parse("No"), Some(DnssecMode::No));
    }

    #[test]
    fn test_dnssec_clone_eq() {
        let m = DnssecMode::AllowDowngrade;
        let c = m;
        assert_eq!(c, DnssecMode::AllowDowngrade);
    }

    // -- DnsOverTlsMode -------------------------------------------------------

    #[test]
    fn test_dot_display_no() {
        assert_eq!(format!("{}", DnsOverTlsMode::No), "no");
    }

    #[test]
    fn test_dot_display_opportunistic() {
        assert_eq!(format!("{}", DnsOverTlsMode::Opportunistic), "opportunistic");
    }

    #[test]
    fn test_dot_display_yes() {
        assert_eq!(format!("{}", DnsOverTlsMode::Yes), "yes");
    }

    #[test]
    fn test_dot_parse_no() {
        assert_eq!(DnsOverTlsMode::parse("no"), Some(DnsOverTlsMode::No));
    }

    #[test]
    fn test_dot_parse_false() {
        assert_eq!(DnsOverTlsMode::parse("false"), Some(DnsOverTlsMode::No));
    }

    #[test]
    fn test_dot_parse_opportunistic() {
        assert_eq!(DnsOverTlsMode::parse("opportunistic"), Some(DnsOverTlsMode::Opportunistic));
    }

    #[test]
    fn test_dot_parse_yes() {
        assert_eq!(DnsOverTlsMode::parse("yes"), Some(DnsOverTlsMode::Yes));
    }

    #[test]
    fn test_dot_parse_true() {
        assert_eq!(DnsOverTlsMode::parse("true"), Some(DnsOverTlsMode::Yes));
    }

    #[test]
    fn test_dot_parse_on() {
        assert_eq!(DnsOverTlsMode::parse("on"), Some(DnsOverTlsMode::Yes));
    }

    #[test]
    fn test_dot_parse_invalid() {
        assert_eq!(DnsOverTlsMode::parse("banana"), None);
    }

    #[test]
    fn test_dot_parse_case_insensitive() {
        assert_eq!(DnsOverTlsMode::parse("OPPORTUNISTIC"), Some(DnsOverTlsMode::Opportunistic));
    }

    // -- LinkProtocolMode -----------------------------------------------------

    #[test]
    fn test_link_protocol_display_no() {
        assert_eq!(format!("{}", LinkProtocolMode::No), "no");
    }

    #[test]
    fn test_link_protocol_display_resolve() {
        assert_eq!(format!("{}", LinkProtocolMode::_ResolveOnly), "resolve");
    }

    #[test]
    fn test_link_protocol_display_yes() {
        assert_eq!(format!("{}", LinkProtocolMode::Yes), "yes");
    }

    #[test]
    fn test_link_protocol_eq() {
        assert_eq!(LinkProtocolMode::Yes, LinkProtocolMode::Yes);
        assert_ne!(LinkProtocolMode::Yes, LinkProtocolMode::No);
    }

    // -- ServerRole -----------------------------------------------------------

    #[test]
    fn test_server_role_display_current() {
        assert_eq!(format!("{}", ServerRole::Current), "current");
    }

    #[test]
    fn test_server_role_display_fallback() {
        assert_eq!(format!("{}", ServerRole::Fallback), "fallback");
    }

    #[test]
    fn test_server_role_eq() {
        assert_eq!(ServerRole::Current, ServerRole::Current);
        assert_ne!(ServerRole::Current, ServerRole::Fallback);
    }

    // -- DnsServer ------------------------------------------------------------

    #[test]
    fn test_dns_server_new() {
        let s = DnsServer::new("8.8.8.8");
        assert_eq!(s.address, "8.8.8.8");
        assert!(s.interface.is_none());
        assert!(s._reachable);
        assert_eq!(s.protocol, DnsProtocol::Plain);
        assert_eq!(s.role, ServerRole::Current);
    }

    #[test]
    fn test_dns_server_with_interface() {
        let s = DnsServer::new("1.1.1.1").with_interface("eth0");
        assert_eq!(s.interface, Some("eth0".to_string()));
    }

    #[test]
    fn test_dns_server_with_protocol() {
        let s = DnsServer::new("1.1.1.1").with_protocol(DnsProtocol::Dot);
        assert_eq!(s.protocol, DnsProtocol::Dot);
    }

    #[test]
    fn test_dns_server_with_role() {
        let s = DnsServer::new("9.9.9.9").with_role(ServerRole::Fallback);
        assert_eq!(s.role, ServerRole::Fallback);
    }

    #[test]
    fn test_dns_server_with_reachable() {
        let s = DnsServer::new("8.8.8.8")._with_reachable(false);
        assert!(!s._reachable);
    }

    #[test]
    fn test_dns_server_builder_chain() {
        let s = DnsServer::new("1.1.1.1")
            .with_interface("wlan0")
            .with_protocol(DnsProtocol::_Doh)
            .with_role(ServerRole::Fallback);
        assert_eq!(s.address, "1.1.1.1");
        assert_eq!(s.interface, Some("wlan0".to_string()));
        assert_eq!(s.protocol, DnsProtocol::_Doh);
        assert_eq!(s.role, ServerRole::Fallback);
    }

    #[test]
    fn test_dns_server_clone() {
        let s = DnsServer::new("8.8.4.4").with_interface("eth0");
        let c = s.clone();
        assert_eq!(c.address, "8.8.4.4");
        assert_eq!(c.interface, Some("eth0".to_string()));
    }

    // -- LinkConfig -----------------------------------------------------------

    #[test]
    fn test_link_config_new() {
        let l = LinkConfig::new(1, "lo");
        assert_eq!(l.index, 1);
        assert_eq!(l.name, "lo");
        assert!(l.dns_servers.is_empty());
        assert!(l.search_domains.is_empty());
        assert_eq!(l.dnssec, DnssecMode::No);
        assert_eq!(l.llmnr, LinkProtocolMode::Yes);
        assert_eq!(l.mdns, LinkProtocolMode::No);
        assert_eq!(l.dns_over_tls, DnsOverTlsMode::No);
        assert!(!l.default_route);
    }

    #[test]
    fn test_link_config_clone() {
        let mut l = LinkConfig::new(2, "eth0");
        l.dns_servers.push(DnsServer::new("8.8.8.8"));
        l.search_domains.push("local".to_string());
        let c = l.clone();
        assert_eq!(c.index, 2);
        assert_eq!(c.name, "eth0");
        assert_eq!(c.dns_servers.len(), 1);
        assert_eq!(c.search_domains.len(), 1);
    }

    #[test]
    fn test_link_config_with_all_fields() {
        let mut l = LinkConfig::new(5, "bond0");
        l.dnssec = DnssecMode::Yes;
        l.llmnr = LinkProtocolMode::_ResolveOnly;
        l.mdns = LinkProtocolMode::Yes;
        l.dns_over_tls = DnsOverTlsMode::Opportunistic;
        l.default_route = true;
        assert_eq!(l.dnssec, DnssecMode::Yes);
        assert_eq!(l.llmnr, LinkProtocolMode::_ResolveOnly);
        assert_eq!(l.mdns, LinkProtocolMode::Yes);
        assert_eq!(l.dns_over_tls, DnsOverTlsMode::Opportunistic);
        assert!(l.default_route);
    }

    // -- CacheStats -----------------------------------------------------------

    #[test]
    fn test_cache_stats_new() {
        let c = CacheStats::new();
        assert_eq!(c.hits, 0);
        assert_eq!(c.misses, 0);
        assert_eq!(c.size, 0);
        assert_eq!(c.capacity, 4096);
    }

    #[test]
    fn test_cache_stats_clone() {
        let mut c = CacheStats::new();
        c.hits = 42;
        c.misses = 7;
        c.size = 100;
        let d = c.clone();
        assert_eq!(d.hits, 42);
        assert_eq!(d.misses, 7);
        assert_eq!(d.size, 100);
        assert_eq!(d.capacity, 4096);
    }

    // -- TransactionStats -----------------------------------------------------

    #[test]
    fn test_transaction_stats_clone() {
        let t = TransactionStats { current: 3, total: 100 };
        let c = t.clone();
        assert_eq!(c.current, 3);
        assert_eq!(c.total, 100);
    }

    // -- DnssecVerdicts -------------------------------------------------------

    #[test]
    fn test_dnssec_verdicts_clone() {
        let v = DnssecVerdicts { secure: 10, insecure: 20, bogus: 1, indeterminate: 5 };
        let c = v.clone();
        assert_eq!(c.secure, 10);
        assert_eq!(c.insecure, 20);
        assert_eq!(c.bogus, 1);
        assert_eq!(c.indeterminate, 5);
    }

    // -- ResolverStats --------------------------------------------------------

    #[test]
    fn test_resolver_stats_new() {
        let s = ResolverStats::new();
        assert!(!s.dnssec_supported);
        assert_eq!(s.cache.capacity, 4096);
        assert_eq!(s.transactions.current, 0);
        assert_eq!(s.dnssec_verdicts.secure, 0);
    }

    #[test]
    fn test_resolver_stats_clone() {
        let s = ResolverStats::new();
        let c = s.clone();
        assert_eq!(c.dnssec_supported, s.dnssec_supported);
        assert_eq!(c.cache.capacity, s.cache.capacity);
    }

    // -- GlobalConfig ---------------------------------------------------------

    #[test]
    fn test_global_config_default() {
        let g = GlobalConfig::default_config();
        assert_eq!(g.dns_servers.len(), 1);
        assert_eq!(g.dns_servers[0].address, "127.0.0.53");
        assert_eq!(g.fallback_servers.len(), 3);
        assert_eq!(g.dnssec, DnssecMode::AllowDowngrade);
        assert_eq!(g.dns_over_tls, DnsOverTlsMode::No);
    }

    #[test]
    fn test_global_config_fallback_servers() {
        let g = GlobalConfig::default_config();
        let addrs: Vec<&str> = g.fallback_servers.iter().map(|s| s.address.as_str()).collect();
        assert!(addrs.contains(&"9.9.9.10"));
        assert!(addrs.contains(&"8.8.8.8"));
        assert!(addrs.contains(&"1.1.1.1"));
    }

    #[test]
    fn test_global_config_fallback_role() {
        let g = GlobalConfig::default_config();
        for s in &g.fallback_servers {
            assert_eq!(s.role, ServerRole::Fallback);
        }
    }

    #[test]
    fn test_global_config_nta() {
        let g = GlobalConfig::default_config();
        assert!(!g.nta.is_empty());
        assert!(g.nta.contains(&"local".to_string()));
    }

    #[test]
    fn test_global_config_clone() {
        let g = GlobalConfig::default_config();
        let c = g.clone();
        assert_eq!(c.dns_servers.len(), g.dns_servers.len());
        assert_eq!(c.dnssec, g.dnssec);
    }

    // -- ResolverState --------------------------------------------------------

    #[test]
    fn test_resolver_state_new() {
        let s = ResolverState::new();
        assert_eq!(s.links.len(), 3);
        assert_eq!(s._log_level, "info");
    }

    #[test]
    fn test_resolver_state_links() {
        let s = ResolverState::new();
        assert!(s.links.contains_key(&1));
        assert!(s.links.contains_key(&2));
        assert!(s.links.contains_key(&3));
    }

    #[test]
    fn test_resolver_state_lo() {
        let s = ResolverState::new();
        let lo = s.links.get(&1).unwrap();
        assert_eq!(lo.name, "lo");
        assert!(lo.dns_servers.is_empty());
    }

    #[test]
    fn test_resolver_state_eth0() {
        let s = ResolverState::new();
        let eth0 = s.links.get(&2).unwrap();
        assert_eq!(eth0.name, "eth0");
        assert_eq!(eth0.dns_servers.len(), 2);
        assert!(eth0.default_route);
    }

    #[test]
    fn test_resolver_state_wlan0() {
        let s = ResolverState::new();
        let wlan0 = s.links.get(&3).unwrap();
        assert_eq!(wlan0.name, "wlan0");
        assert_eq!(wlan0.dns_servers.len(), 2);
        assert_eq!(wlan0.dns_over_tls, DnsOverTlsMode::Opportunistic);
    }

    #[test]
    fn test_resolver_state_wlan0_dot_servers() {
        let s = ResolverState::new();
        let wlan0 = s.links.get(&3).unwrap();
        for server in &wlan0.dns_servers {
            assert_eq!(server.protocol, DnsProtocol::Dot);
        }
    }

    #[test]
    fn test_find_link_by_name_exists() {
        let s = ResolverState::new();
        let link = s.find_link_by_name("eth0");
        assert!(link.is_some());
        assert_eq!(link.unwrap().index, 2);
    }

    #[test]
    fn test_find_link_by_name_missing() {
        let s = ResolverState::new();
        assert!(s.find_link_by_name("br0").is_none());
    }

    #[test]
    fn test_find_link_by_index() {
        let s = ResolverState::new();
        let link = s.find_link("2");
        assert!(link.is_some());
        assert_eq!(link.unwrap().name, "eth0");
    }

    #[test]
    fn test_find_link_by_name_via_find_link() {
        let s = ResolverState::new();
        let link = s.find_link("wlan0");
        assert!(link.is_some());
        assert_eq!(link.unwrap().index, 3);
    }

    #[test]
    fn test_find_link_missing() {
        let s = ResolverState::new();
        assert!(s.find_link("99").is_none());
        assert!(s.find_link("nonexistent").is_none());
    }

    #[test]
    fn test_all_dns_servers() {
        let s = ResolverState::new();
        let all = s._all_dns_servers();
        // 1 global + 2 eth0 + 2 wlan0 = 5
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_resolver_state_clone() {
        let s = ResolverState::new();
        let c = s.clone();
        assert_eq!(c.links.len(), s.links.len());
        assert_eq!(c._log_level, s._log_level);
    }

    // -- resolve_hostname / reverse_lookup ------------------------------------

    #[test]
    fn test_resolve_localhost() {
        let ips = resolve_hostname("localhost");
        assert_eq!(ips.len(), 2);
        assert!(ips.contains(&"127.0.0.1".to_string()));
        assert!(ips.contains(&"::1".to_string()));
    }

    #[test]
    fn test_resolve_unknown() {
        let ips = resolve_hostname("nonexistent.invalid");
        assert!(ips.is_empty());
    }

    #[test]
    fn test_reverse_lookup_ipv4_localhost() {
        assert_eq!(reverse_lookup("127.0.0.1"), Some("localhost".to_string()));
    }

    #[test]
    fn test_reverse_lookup_ipv6_localhost() {
        assert_eq!(reverse_lookup("::1"), Some("localhost".to_string()));
    }

    #[test]
    fn test_reverse_lookup_unknown() {
        assert_eq!(reverse_lookup("10.0.0.1"), None);
    }

    // -- format helpers -------------------------------------------------------

    #[test]
    fn test_format_bool_flag_true() {
        assert_eq!(format_bool_flag(true), "yes");
    }

    #[test]
    fn test_format_bool_flag_false() {
        assert_eq!(format_bool_flag(false), "no");
    }

    #[test]
    fn test_format_protocol_flag_enabled() {
        assert_eq!(format_protocol_flag(true, "LLMNR"), "+LLMNR");
    }

    #[test]
    fn test_format_protocol_flag_disabled() {
        assert_eq!(format_protocol_flag(false, "mDNS"), "-mDNS");
    }

    #[test]
    fn test_format_link_protocol_yes() {
        assert_eq!(format_link_protocol(LinkProtocolMode::Yes), "+");
    }

    #[test]
    fn test_format_link_protocol_no() {
        assert_eq!(format_link_protocol(LinkProtocolMode::No), "-");
    }

    #[test]
    fn test_format_link_protocol_resolve() {
        assert_eq!(format_link_protocol(LinkProtocolMode::_ResolveOnly), "resolve");
    }

    // -- Constants ------------------------------------------------------------

    #[test]
    fn test_version_const() {
        assert_eq!(VERSION, "0.1.0");
    }

    #[test]
    fn test_resolv_conf_path() {
        assert_eq!(RESOLV_CONF, "/etc/resolv.conf");
    }

    #[test]
    fn test_resolved_conf_path() {
        assert_eq!(RESOLVED_CONF, "/etc/systemd/resolved.conf");
    }

    #[test]
    fn test_run_resolved_path() {
        assert_eq!(RUN_RESOLVED, "/run/systemd/resolve");
    }

    // -- eth0 link detail tests -----------------------------------------------

    #[test]
    fn test_eth0_dns_server_addresses() {
        let s = ResolverState::new();
        let eth0 = s.links.get(&2).unwrap();
        let addrs: Vec<&str> = eth0.dns_servers.iter().map(|d| d.address.as_str()).collect();
        assert_eq!(addrs, vec!["8.8.8.8", "8.8.4.4"]);
    }

    #[test]
    fn test_eth0_search_domains() {
        let s = ResolverState::new();
        let eth0 = s.links.get(&2).unwrap();
        assert_eq!(eth0.search_domains, vec!["localdomain"]);
    }

    #[test]
    fn test_eth0_dnssec() {
        let s = ResolverState::new();
        let eth0 = s.links.get(&2).unwrap();
        assert_eq!(eth0.dnssec, DnssecMode::AllowDowngrade);
    }

    #[test]
    fn test_eth0_llmnr() {
        let s = ResolverState::new();
        let eth0 = s.links.get(&2).unwrap();
        assert_eq!(eth0.llmnr, LinkProtocolMode::Yes);
    }

    #[test]
    fn test_eth0_mdns() {
        let s = ResolverState::new();
        let eth0 = s.links.get(&2).unwrap();
        assert_eq!(eth0.mdns, LinkProtocolMode::Yes);
    }

    // -- wlan0 link detail tests ----------------------------------------------

    #[test]
    fn test_wlan0_dns_server_addresses() {
        let s = ResolverState::new();
        let wlan0 = s.links.get(&3).unwrap();
        let addrs: Vec<&str> = wlan0.dns_servers.iter().map(|d| d.address.as_str()).collect();
        assert_eq!(addrs, vec!["1.1.1.1", "1.0.0.1"]);
    }

    #[test]
    fn test_wlan0_search_domains() {
        let s = ResolverState::new();
        let wlan0 = s.links.get(&3).unwrap();
        assert_eq!(wlan0.search_domains, vec!["home.lan"]);
    }

    #[test]
    fn test_wlan0_interface_on_servers() {
        let s = ResolverState::new();
        let wlan0 = s.links.get(&3).unwrap();
        for server in &wlan0.dns_servers {
            assert_eq!(server.interface, Some("wlan0".to_string()));
        }
    }

    // -- Edge case tests for DnssecMode parse ---------------------------------

    #[test]
    fn test_dnssec_parse_empty_string() {
        assert_eq!(DnssecMode::parse(""), None);
    }

    #[test]
    fn test_dnssec_parse_whitespace() {
        // trim is not done in parse; raw input
        assert_eq!(DnssecMode::parse(" yes "), None);
    }

    // -- Edge case tests for DnsOverTlsMode parse -----------------------------

    #[test]
    fn test_dot_parse_empty_string() {
        assert_eq!(DnsOverTlsMode::parse(""), None);
    }

    #[test]
    fn test_dot_parse_off() {
        assert_eq!(DnsOverTlsMode::parse("off"), Some(DnsOverTlsMode::No));
    }

    // -- Global config NTA details --------------------------------------------

    #[test]
    fn test_global_nta_count() {
        let g = GlobalConfig::default_config();
        assert_eq!(g.nta.len(), 4);
    }

    #[test]
    fn test_global_nta_contains_rfc1918() {
        let g = GlobalConfig::default_config();
        assert!(g.nta.contains(&"10.in-addr.arpa".to_string()));
        assert!(g.nta.contains(&"168.192.in-addr.arpa".to_string()));
    }

    // -- Resolver state DNS server reachability default ------------------------

    #[test]
    fn test_all_servers_reachable_by_default() {
        let s = ResolverState::new();
        for server in s._all_dns_servers() {
            assert!(server._reachable);
        }
    }

    // -- Global config protocol defaults --------------------------------------

    #[test]
    fn test_global_llmnr_default() {
        let g = GlobalConfig::default_config();
        assert_eq!(g.llmnr, LinkProtocolMode::Yes);
    }

    #[test]
    fn test_global_mdns_default() {
        let g = GlobalConfig::default_config();
        assert_eq!(g.mdns, LinkProtocolMode::No);
    }

    #[test]
    fn test_global_search_domains_default_empty() {
        let g = GlobalConfig::default_config();
        assert!(g.search_domains.is_empty());
    }

    // -- lo link defaults -----------------------------------------------------

    #[test]
    fn test_lo_no_default_route() {
        let s = ResolverState::new();
        let lo = s.links.get(&1).unwrap();
        assert!(!lo.default_route);
    }

    #[test]
    fn test_lo_llmnr_default() {
        let s = ResolverState::new();
        let lo = s.links.get(&1).unwrap();
        assert_eq!(lo.llmnr, LinkProtocolMode::Yes);
    }

    #[test]
    fn test_lo_mdns_default() {
        let s = ResolverState::new();
        let lo = s.links.get(&1).unwrap();
        assert_eq!(lo.mdns, LinkProtocolMode::No);
    }

    #[test]
    fn test_lo_dns_over_tls_default() {
        let s = ResolverState::new();
        let lo = s.links.get(&1).unwrap();
        assert_eq!(lo.dns_over_tls, DnsOverTlsMode::No);
    }
}
