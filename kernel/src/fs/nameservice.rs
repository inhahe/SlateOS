//! Name Service — hostname/DNS name resolution configuration.
//!
//! Manages system hostname, domain, search order for name resolution
//! (files, DNS, mDNS), and static host entries (like /etc/hosts).
//!
//! ## Architecture
//!
//! ```text
//! Name service
//!   → nameservice::resolve(name) → resolve hostname
//!   → nameservice::add_host(addr, name) → add static entry
//!   → nameservice::set_hostname(name) → set system hostname
//!   → nameservice::set_order(order) → set resolution order
//!
//! Integration:
//!   → dnssettings (DNS configuration)
//!   → netsettings (network settings)
//!   → timesync (NTP server resolution)
//!   → sysinfo (system information)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Name resolution source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveSource {
    Files,    // /etc/hosts
    Dns,      // DNS server
    Mdns,     // mDNS (multicast)
    Cache,    // Local cache
}

impl ResolveSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Dns => "dns",
            Self::Mdns => "mdns",
            Self::Cache => "cache",
        }
    }
}

/// A static host entry.
#[derive(Debug, Clone)]
pub struct HostEntry {
    pub address: String,
    pub hostname: String,
    pub aliases: Vec<String>,
}

/// A resolve result.
#[derive(Debug, Clone)]
pub struct ResolveResult {
    pub hostname: String,
    pub address: String,
    pub source: ResolveSource,
    pub ttl_secs: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HOSTS: usize = 256;
const MAX_CACHE: usize = 512;

struct State {
    hostname: String,
    domain: String,
    resolve_order: Vec<ResolveSource>,
    hosts: Vec<HostEntry>,
    cache: Vec<ResolveResult>,
    total_lookups: u64,
    cache_hits: u64,
    cache_misses: u64,
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
        hostname: String::from("localhost"),
        domain: String::from("localdomain"),
        resolve_order: alloc::vec![ResolveSource::Cache, ResolveSource::Files, ResolveSource::Dns],
        hosts: alloc::vec![
            HostEntry {
                address: String::from("127.0.0.1"),
                hostname: String::from("localhost"),
                aliases: alloc::vec![String::from("loopback")],
            },
            HostEntry {
                address: String::from("::1"),
                hostname: String::from("localhost"),
                aliases: alloc::vec![String::from("ip6-localhost")],
            },
        ],
        cache: Vec::new(),
        total_lookups: 0,
        cache_hits: 0,
        cache_misses: 0,
        ops: 0,
    });
}

/// Get system hostname.
pub fn get_hostname() -> String {
    STATE.lock().as_ref().map_or(String::from("unknown"), |s| s.hostname.clone())
}

/// Set system hostname.
///
/// Empty names are accepted: this models Linux's
/// `sethostname(_, 0)` "clear the nodename field" semantics
/// (`memset(u->nodename + len, 0, sizeof(u->nodename) - len)` with
/// `len == 0` zeros the entire 64-byte field, leaving the kernel
/// observing an empty hostname).  The Linux ABI translator relies
/// on this so it can mirror Linux's len==0 → 0 success contract
/// without a translator-side hack (batch 479).  Mirrors
/// [`set_domain`]'s already-permissive empty-string policy.
pub fn set_hostname(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if name.len() > 253 {
            return Err(KernelError::InvalidArgument);
        }
        state.hostname = String::from(name);
        Ok(())
    })
}

/// Get domain.
pub fn get_domain() -> String {
    STATE.lock().as_ref().map_or(String::from(""), |s| s.domain.clone())
}

/// Set domain.
pub fn set_domain(domain: &str) -> KernelResult<()> {
    with_state(|state| {
        state.domain = String::from(domain);
        Ok(())
    })
}

/// Resolve a hostname (checks hosts file, returns first match).
pub fn resolve(hostname: &str) -> KernelResult<ResolveResult> {
    with_state(|state| {
        state.total_lookups += 1;
        // Check cache first.
        if let Some(cached) = state.cache.iter().find(|r| r.hostname == hostname) {
            state.cache_hits += 1;
            return Ok(cached.clone());
        }
        state.cache_misses += 1;
        // Check hosts file.
        for entry in &state.hosts {
            if entry.hostname == hostname || entry.aliases.iter().any(|a| a == hostname) {
                let result = ResolveResult {
                    hostname: String::from(hostname),
                    address: entry.address.clone(),
                    source: ResolveSource::Files,
                    ttl_secs: 0,
                };
                // Add to cache.
                if state.cache.len() >= MAX_CACHE { state.cache.remove(0); }
                state.cache.push(result.clone());
                return Ok(result);
            }
        }
        Err(KernelError::NotFound)
    })
}

/// Add a static host entry.
pub fn add_host(address: &str, hostname: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.hosts.len() >= MAX_HOSTS { return Err(KernelError::ResourceExhausted); }
        if state.hosts.iter().any(|h| h.hostname == hostname && h.address == address) {
            return Err(KernelError::AlreadyExists);
        }
        state.hosts.push(HostEntry {
            address: String::from(address),
            hostname: String::from(hostname),
            aliases: Vec::new(),
        });
        Ok(())
    })
}

/// Remove a host entry.
pub fn remove_host(hostname: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.hosts.len();
        state.hosts.retain(|h| h.hostname != hostname);
        if state.hosts.len() == before { return Err(KernelError::NotFound); }
        // Also invalidate cache.
        state.cache.retain(|r| r.hostname != hostname);
        Ok(())
    })
}

/// List all host entries.
pub fn list_hosts() -> Vec<HostEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.hosts.clone())
}

/// Get resolution order.
pub fn get_order() -> Vec<ResolveSource> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.resolve_order.clone())
}

/// Flush resolve cache.
pub fn flush_cache() -> KernelResult<u64> {
    with_state(|state| {
        let count = state.cache.len() as u64;
        state.cache.clear();
        Ok(count)
    })
}

/// Statistics: (hosts_count, total_lookups, cache_hits, cache_misses, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.hosts.len(), s.total_lookups, s.cache_hits, s.cache_misses, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("nameservice::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(get_hostname(), "localhost");
    assert_eq!(list_hosts().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Set hostname.  Empty names are accepted (mirrors Linux's
    // sethostname(_, 0) "clear the nodename field" semantics; see
    // set_hostname's doc comment).  Over-long names still reject.
    set_hostname("myhost").expect("set");
    assert_eq!(get_hostname(), "myhost");
    set_hostname("").expect("clear");
    assert_eq!(get_hostname(), "");
    // Restore a sane value so subsequent tests' resolve/hosts paths
    // don't surface an empty hostname through any incidental reader.
    set_hostname("myhost").expect("restore");
    crate::serial_println!("  [2/8] hostname: OK");

    // 3: Resolve.
    let result = resolve("localhost").expect("resolve");
    assert_eq!(result.address, "127.0.0.1");
    assert_eq!(result.source, ResolveSource::Files);
    crate::serial_println!("  [3/8] resolve: OK");

    // 4: Cache hit.
    let result2 = resolve("localhost").expect("resolve2");
    assert_eq!(result2.source, ResolveSource::Files); // Cached entry keeps original source.
    crate::serial_println!("  [4/8] cache: OK");

    // 5: Add host.
    add_host("192.168.1.100", "server1").expect("add");
    assert_eq!(list_hosts().len(), 3);
    let r = resolve("server1").expect("resolve3");
    assert_eq!(r.address, "192.168.1.100");
    crate::serial_println!("  [5/8] add host: OK");

    // 6: Remove host.
    remove_host("server1").expect("remove");
    assert_eq!(list_hosts().len(), 2);
    assert!(resolve("server1").is_err());
    crate::serial_println!("  [6/8] remove: OK");

    // 7: Flush cache.
    let flushed = flush_cache().expect("flush");
    assert!(flushed >= 1);
    crate::serial_println!("  [7/8] flush: OK");

    // 8: Stats.
    let (hosts, lookups, hits, misses, ops) = stats();
    assert_eq!(hosts, 2);
    assert!(lookups >= 3);
    assert!(hits >= 1);
    assert!(misses >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("nameservice::self_test() — all 8 tests passed");
}
