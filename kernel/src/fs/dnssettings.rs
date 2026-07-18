//! DNS Settings — DNS resolver configuration and management.
//!
//! Manages DNS server lists, search domains, DNS-over-HTTPS/TLS
//! settings, and per-interface DNS configuration.
//!
//! ## Architecture
//!
//! ```text
//! DNS configuration
//!   → dnssettings::set_servers(servers) → configure resolvers
//!   → dnssettings::add_search_domain(domain) → add search suffix
//!   → dnssettings::resolve(name) → cached lookup simulation
//!
//! Integration:
//!   → netsettings (network configuration)
//!   → dyndns (dynamic DNS)
//!   → vpn (VPN connections)
//!   → netdiag (network diagnostics)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// DNS protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsProtocol {
    Plain,       // Standard DNS (port 53).
    Doh,         // DNS-over-HTTPS.
    Dot,         // DNS-over-TLS.
    Dnscrypt,    // DNSCrypt.
}

impl DnsProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Plain => "DNS",
            Self::Doh => "DoH",
            Self::Dot => "DoT",
            Self::Dnscrypt => "DNSCrypt",
        }
    }
}

/// A DNS server entry.
#[derive(Debug, Clone)]
pub struct DnsServer {
    pub address: String,
    pub protocol: DnsProtocol,
    pub priority: u32,
    pub is_active: bool,
    pub queries_sent: u64,
    pub failures: u64,
}

/// A cached DNS record.
#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub address: String,
    pub ttl_sec: u32,
    pub cached_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SERVERS: usize = 20;
const MAX_SEARCH_DOMAINS: usize = 10;
const MAX_CACHE: usize = 500;

struct State {
    servers: Vec<DnsServer>,
    search_domains: Vec<String>,
    cache: Vec<DnsRecord>,
    total_queries: u64,
    total_cache_hits: u64,
    total_failures: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        servers: alloc::vec![
            DnsServer { address: String::from("1.1.1.1"), protocol: DnsProtocol::Plain, priority: 1, is_active: true, queries_sent: 0, failures: 0 },
            DnsServer { address: String::from("8.8.8.8"), protocol: DnsProtocol::Plain, priority: 2, is_active: true, queries_sent: 0, failures: 0 },
            DnsServer { address: String::from("https://dns.cloudflare.com/dns-query"), protocol: DnsProtocol::Doh, priority: 3, is_active: false, queries_sent: 0, failures: 0 },
        ],
        search_domains: alloc::vec![String::from("local")],
        cache: Vec::new(),
        total_queries: 0,
        total_cache_hits: 0,
        total_failures: 0,
        ops: 0,
    });
}

/// Add a DNS server.
pub fn add_server(address: &str, protocol: DnsProtocol, priority: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.servers.len() >= MAX_SERVERS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.servers.iter().any(|s| s.address == address) {
            return Err(KernelError::AlreadyExists);
        }
        state.servers.push(DnsServer {
            address: String::from(address), protocol, priority,
            is_active: true, queries_sent: 0, failures: 0,
        });
        state.servers.sort_by_key(|s| s.priority);
        Ok(())
    })
}

/// Remove a DNS server.
pub fn remove_server(address: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.servers.len();
        state.servers.retain(|s| s.address != address);
        if state.servers.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Set server active/inactive.
pub fn set_server_active(address: &str, active: bool) -> KernelResult<()> {
    with_state(|state| {
        let srv = state.servers.iter_mut().find(|s| s.address == address)
            .ok_or(KernelError::NotFound)?;
        srv.is_active = active;
        Ok(())
    })
}

/// Add a search domain.
pub fn add_search_domain(domain: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.search_domains.len() >= MAX_SEARCH_DOMAINS {
            return Err(KernelError::ResourceExhausted);
        }
        state.search_domains.push(String::from(domain));
        Ok(())
    })
}

/// Remove a search domain.
pub fn remove_search_domain(domain: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.search_domains.len();
        state.search_domains.retain(|d| d != domain);
        if state.search_domains.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Simulate a DNS resolve (check cache, then "query" first active server).
pub fn resolve(name: &str) -> KernelResult<String> {
    with_state(|state| {
        state.total_queries += 1;
        // Check cache.
        if let Some(entry) = state.cache.iter().find(|r| r.name == name) {
            state.total_cache_hits += 1;
            return Ok(entry.address.clone());
        }
        // Find active server.
        let server = state.servers.iter_mut().find(|s| s.is_active)
            .ok_or(KernelError::NotFound)?;
        server.queries_sent += 1;
        // Simulate resolution: generate a fake IP based on name hash.
        let hash: u32 = name.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
        let ip = format!("{}.{}.{}.{}", (hash >> 24) & 0xFF, (hash >> 16) & 0xFF, (hash >> 8) & 0xFF, hash & 0xFF);
        let now = crate::hpet::elapsed_ns();
        if state.cache.len() >= MAX_CACHE { state.cache.remove(0); }
        state.cache.push(DnsRecord {
            name: String::from(name), address: ip.clone(), ttl_sec: 300, cached_ns: now,
        });
        Ok(ip)
    })
}

/// Flush DNS cache.
pub fn flush_cache() -> KernelResult<usize> {
    with_state(|state| {
        let count = state.cache.len();
        state.cache.clear();
        Ok(count)
    })
}

/// List DNS servers.
pub fn list_servers() -> Vec<DnsServer> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.servers.clone())
}

/// List search domains.
pub fn list_search_domains() -> Vec<String> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.search_domains.clone())
}

/// Cache size.
pub fn cache_size() -> usize {
    STATE.lock().as_ref().map_or(0, |s| s.cache.len())
}

/// Statistics: (server_count, cache_size, total_queries, total_cache_hits, total_failures, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.servers.len(), s.cache.len(), s.total_queries, s.total_cache_hits, s.total_failures, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("dnssettings::self_test() — running tests...");
    init_defaults();

    // 1: Default servers.
    assert_eq!(list_servers().len(), 3);
    assert_eq!(list_search_domains().len(), 1);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Resolve (cache miss).
    let ip = resolve("example.com").expect("resolve");
    assert!(!ip.is_empty());
    assert_eq!(cache_size(), 1);
    crate::serial_println!("  [2/8] resolve: OK");

    // 3: Cache hit.
    let ip2 = resolve("example.com").expect("resolve2");
    assert_eq!(ip, ip2);
    crate::serial_println!("  [3/8] cache hit: OK");

    // 4: Add server.
    add_server("9.9.9.9", DnsProtocol::Dot, 0).expect("add");
    assert_eq!(list_servers().len(), 4);
    crate::serial_println!("  [4/8] add server: OK");

    // 5: Remove server.
    remove_server("9.9.9.9").expect("remove");
    assert_eq!(list_servers().len(), 3);
    crate::serial_println!("  [5/8] remove server: OK");

    // 6: Search domains.
    add_search_domain("corp.local").expect("add_sd");
    assert_eq!(list_search_domains().len(), 2);
    remove_search_domain("corp.local").expect("rm_sd");
    assert_eq!(list_search_domains().len(), 1);
    crate::serial_println!("  [6/8] search domains: OK");

    // 7: Flush cache.
    let flushed = flush_cache().expect("flush");
    assert!(flushed >= 1);
    assert_eq!(cache_size(), 0);
    crate::serial_println!("  [7/8] flush cache: OK");

    // 8: Stats.
    let (servers, cache, queries, hits, _failures, ops) = stats();
    assert_eq!(servers, 3);
    assert_eq!(cache, 0);
    assert_eq!(queries, 2);
    assert_eq!(hits, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("dnssettings::self_test() — all 8 tests passed");
}
