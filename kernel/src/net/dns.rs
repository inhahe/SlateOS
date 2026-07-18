//! DNS resolver (RFC 1035) with result caching.
//!
//! Simple recursive DNS stub resolver that sends queries to a
//! configured DNS server and parses A record responses.
//!
//! ## DNS Cache
//!
//! Resolved names are cached with their TTL (from the DNS response).
//! Subsequent lookups for the same name return the cached result
//! without a network round-trip.  Cache entries expire when their
//! TTL elapses.  The cache holds up to 32 entries; when full, the
//! oldest entry is evicted.
//!
//! Negative caching: names that fail to resolve (NXDOMAIN, no A
//! record, no CNAME) are cached for 60 seconds with a sentinel IP
//! (0.0.0.0) to avoid repeated queries for non-existent domains.
//!
//! ## Protocol overview
//!
//! DNS uses UDP port 53.  A query contains a question section with
//! the domain name and record type.  The server responds with answer
//! records containing the resolved IP addresses.
//!
//! Each query uses a unique transaction ID (monotonically incrementing
//! AtomicU16) and a unique ephemeral source port (from the 49152–65535
//! range) to prevent spoofed responses and port collisions.
//!
//! ## CNAME chasing
//!
//! When a query returns CNAME records instead of (or before) A records,
//! the resolver follows the CNAME chain within the same response packet.
//! Most DNS servers include both the CNAME and the final A record in a
//! single response, so this avoids extra round-trips for CDN and load-
//! balancer domains.  If the response contains only CNAMEs and no A
//! record for the final name, a second query is sent for the CNAME
//! target (up to 8 CNAME hops to prevent loops).
//!
//! ## Retry
//!
//! On timeout, the resolver retransmits the query with increasing wait
//! windows (1s → 2s → 4s, up to 3 attempts).  Definitive answers
//! (NXDOMAIN, parse errors) are not retried — only network-level
//! timeouts trigger retransmission.
//!
//! ## Reverse DNS (PTR records)
//!
//! The [`reverse_resolve`] function queries PTR records for an IPv4
//! address.  It converts the address to the `in-addr.arpa` domain
//! (e.g., `192.168.1.1` → `1.1.168.192.in-addr.arpa`) and returns
//! the associated hostname.  PTR results are not cached (reverse
//! lookups are comparatively rare).
//!
//! ## Transport
//!
//! DNS queries are sent over IPv4 UDP by default (using the DHCP-provided
//! DNS server).  When no IPv4 DNS server is configured, the resolver falls
//! back to IPv6 UDP transport using the DNS server address from Router
//! Advertisement RDNSS options (RFC 8106).  This enables name resolution
//! in IPv6-only networks.
//!
//! ## Limitations
//!
//! - CNAME chasing limited to 8 hops.
//! - No EDNS0 or DNSSEC.
//!
//! ## IPv6 support (AAAA records)
//!
//! The [`resolve6`] function queries AAAA records (RFC 3596) to resolve
//! domain names to IPv6 addresses.  AAAA results are cached in a
//! separate 16-entry cache with the same TTL/eviction behaviour as the
//! A record cache.  [`reverse_resolve6`] queries PTR records in the
//! `ip6.arpa` domain for IPv6 reverse DNS.

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

use super::interface::{self, Ipv4Addr};
use super::ipv6::Ipv6Addr;

// ---------------------------------------------------------------------------
// DNS cache statistics (lock-free atomic counters)
// ---------------------------------------------------------------------------

/// Number of cache hits (both positive and negative).
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
/// Number of cache misses (queries sent to the network).
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
/// Number of cache evictions (when a slot is reused for a new entry).
static CACHE_EVICTIONS: AtomicU64 = AtomicU64::new(0);

/// DNS cache statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct DnsCacheStats {
    /// Total cache hits (positive + negative).
    pub hits: u64,
    /// Total cache misses.
    pub misses: u64,
    /// Total evictions (replaced entries).
    pub evictions: u64,
    /// Current number of occupied cache slots.
    pub entries: usize,
    /// Maximum cache capacity.
    pub capacity: usize,
}

/// Return a snapshot of DNS cache statistics.
pub fn cache_stats() -> DnsCacheStats {
    let entries = DNS_CACHE.lock().count;
    DnsCacheStats {
        hits: CACHE_HITS.load(Ordering::Relaxed),
        misses: CACHE_MISSES.load(Ordering::Relaxed),
        evictions: CACHE_EVICTIONS.load(Ordering::Relaxed),
        entries,
        capacity: CACHE_SIZE,
    }
}

// ---------------------------------------------------------------------------
// DNS constants
// ---------------------------------------------------------------------------

/// DNS server port.
const DNS_PORT: u16 = 53;

/// DNS record type: A (IPv4 address).
const TYPE_A: u16 = 1;
/// DNS record type: CNAME (canonical name alias).
const TYPE_CNAME: u16 = 5;
/// DNS record type: PTR (pointer / reverse DNS).
const TYPE_PTR: u16 = 12;
/// DNS record type: AAAA (IPv6 address, RFC 3596).
const TYPE_AAAA: u16 = 28;
/// DNS record class: IN (Internet).
const CLASS_IN: u16 = 1;

/// DNS flags: standard query, recursion desired.
const FLAGS_QUERY_RD: u16 = 0x0100;

/// Counter mixed with TSC for query ID generation.
///
/// Combined with TSC jitter to produce unpredictable 16-bit transaction
/// IDs.  Monotonic predictability is a classic DNS cache poisoning vector
/// (CVE-2008-1447 / Kaminsky attack).
static QUERY_ID_COUNTER: AtomicU16 = AtomicU16::new(1);

/// Generate a randomized DNS transaction ID.
///
/// Mixes a monotonic counter with the TSC (timestamp counter) to produce
/// IDs that are:
/// - Unique (counter prevents collisions within ~65K queries)
/// - Unpredictable (TSC timing jitter varies per call)
/// - Never zero (some resolvers treat 0 as invalid)
fn next_query_id() -> u16 {
    let counter = QUERY_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    // SAFETY: rdtsc reads the hardware timestamp counter, always
    // available on x86_64 CPUs.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    // Mix lower TSC bits (high jitter) with the counter.
    // Rotate the TSC bits to spread entropy across all 16 bits.
    let tsc16 = (tsc as u16) ^ ((tsc >> 16) as u16) ^ ((tsc >> 5) as u16);
    let id = counter ^ tsc16;
    // Skip 0 on collision.
    if id == 0 { counter.wrapping_add(1) } else { id }
}

/// Counter mixed with TSC for ephemeral port allocation.
///
/// Each query binds a unique local port to avoid collisions when
/// multiple resolutions are in flight (e.g., during CNAME chasing).
/// Range: 49152–65535 (IANA dynamic/private port range, 16384 ports).
///
/// Random port selection prevents an attacker from predicting the source
/// port of a DNS query, which is essential for cache poisoning resistance
/// alongside random query IDs.
static PORT_COUNTER: AtomicU16 = AtomicU16::new(0);

/// Ephemeral port range start (IANA dynamic/private range).
const EPHEMERAL_PORT_START: u16 = 49152;
/// Ephemeral port range size.
const EPHEMERAL_PORT_RANGE: u16 = 16384; // 65535 - 49152 + 1

/// Allocate a randomized ephemeral port for DNS.
fn next_dns_port() -> u16 {
    let counter = PORT_COUNTER.fetch_add(1, Ordering::Relaxed);
    // SAFETY: rdtsc is always available on x86_64.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    let tsc16 = (tsc as u16) ^ ((tsc >> 11) as u16) ^ ((tsc >> 23) as u16);
    let mixed = counter ^ tsc16;
    // Map to ephemeral range [49152, 65535].
    EPHEMERAL_PORT_START.wrapping_add(mixed % EPHEMERAL_PORT_RANGE)
}

/// Maximum CNAME hops to follow before giving up.
const MAX_CNAME_HOPS: usize = 8;

/// TTL (in seconds) for negative cache entries (NXDOMAIN / not found).
///
/// Prevents repeated queries for names that don't exist.  Short TTL
/// ensures we retry promptly if the name is created.
const NEGATIVE_CACHE_TTL: u32 = 60;

// ---------------------------------------------------------------------------
// DNS cache
// ---------------------------------------------------------------------------

/// Maximum number of cached DNS entries.
const CACHE_SIZE: usize = 32;

/// Maximum domain name length stored in cache (bytes, not including null).
const MAX_NAME_LEN: usize = 128;

/// A cached DNS resolution result.
#[derive(Clone)]
struct CacheEntry {
    /// Domain name (lowercased, null-terminated in storage).
    name: [u8; MAX_NAME_LEN],
    /// Length of the name (bytes, not including null).
    name_len: usize,
    /// Resolved IPv4 address.
    ip: Ipv4Addr,
    /// Absolute expiration time in nanoseconds (monotonic clock).
    expires_ns: u64,
}

impl CacheEntry {
    /// An empty (unused) cache entry.
    const fn empty() -> Self {
        Self {
            name: [0u8; MAX_NAME_LEN],
            name_len: 0,
            ip: Ipv4Addr([0, 0, 0, 0]),
            expires_ns: 0,
        }
    }

    /// Check whether this entry is active (non-empty and not expired).
    fn is_valid(&self, now_ns: u64) -> bool {
        self.name_len > 0 && now_ns < self.expires_ns
    }
}

/// DNS resolution cache.
///
/// Fixed-size array of entries protected by a spinlock.  When full,
/// the oldest entry (lowest `expires_ns`) is evicted.
struct DnsCache {
    entries: [CacheEntry; CACHE_SIZE],
    /// Number of occupied slots (including expired — they get
    /// overwritten on next insert).
    count: usize,
}

impl DnsCache {
    const fn new() -> Self {
        Self {
            entries: [const { CacheEntry::empty() }; CACHE_SIZE],
            count: 0,
        }
    }

    /// Look up a name in the cache.
    ///
    /// Returns `Some(Some(ip))` for a positive cache hit,
    /// `Some(None)` for a negative cache hit (name was previously
    /// queried and returned NXDOMAIN), or `None` for a cache miss.
    fn lookup(&self, name: &str, now_ns: u64) -> Option<Option<Ipv4Addr>> {
        let name_bytes = name.as_bytes();
        for entry in &self.entries {
            if entry.name_len == name_bytes.len()
                && entry.is_valid(now_ns)
                && Self::names_match(&entry.name, entry.name_len, name_bytes)
            {
                // Unspecified IP (0.0.0.0) is our sentinel for negative cache entries.
                if entry.ip.is_unspecified() {
                    return Some(None);
                }
                return Some(Some(entry.ip));
            }
        }
        None
    }

    /// Insert or update a cache entry.
    fn insert(&mut self, name: &str, ip: Ipv4Addr, ttl_secs: u32, now_ns: u64) {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > MAX_NAME_LEN {
            return; // Name too long for cache — skip silently.
        }

        // Clamp TTL to a reasonable range.
        // Minimum 60s (avoid re-querying immediately).
        // Maximum 1 hour (stale data is worse than a re-query).
        let ttl_clamped = ttl_secs.clamp(60, 3600);
        let ttl_ns = u64::from(ttl_clamped).wrapping_mul(1_000_000_000);
        let expires = now_ns.wrapping_add(ttl_ns);

        // Check if already cached — update in place.
        for entry in &mut self.entries {
            if entry.name_len == name_bytes.len()
                && Self::names_match(&entry.name, entry.name_len, name_bytes)
            {
                entry.ip = ip;
                entry.expires_ns = expires;
                return;
            }
        }

        // Find a slot: prefer expired entries, then oldest entry.
        let mut best_idx: usize = 0;
        let mut best_expires: u64 = u64::MAX;

        for (i, entry) in self.entries.iter().enumerate() {
            // Empty slot — use immediately.
            if entry.name_len == 0 {
                best_idx = i;
                break;
            }
            // Expired — good candidate.
            if !entry.is_valid(now_ns) {
                best_idx = i;
                break;
            }
            // Otherwise, track the oldest for LRU-like eviction.
            if entry.expires_ns < best_expires {
                best_expires = entry.expires_ns;
                best_idx = i;
            }
        }

        // Write the new entry.
        if let Some(slot) = self.entries.get_mut(best_idx) {
            // Track evictions: replacing a valid (non-empty, non-expired) entry.
            if slot.name_len > 0 && slot.is_valid(now_ns) {
                CACHE_EVICTIONS.fetch_add(1, Ordering::Relaxed);
            }
            slot.name = [0u8; MAX_NAME_LEN];
            let copy_len = name_bytes.len().min(MAX_NAME_LEN);
            slot.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            slot.name_len = name_bytes.len();
            slot.ip = ip;
            slot.expires_ns = expires;
            if self.count < CACHE_SIZE {
                self.count = self.count.wrapping_add(1);
            }
        }
    }

    /// Case-insensitive name comparison (DNS is case-insensitive).
    fn names_match(cached: &[u8; MAX_NAME_LEN], cached_len: usize, query: &[u8]) -> bool {
        if cached_len != query.len() {
            return false;
        }
        for i in 0..cached_len {
            if let (Some(&a), Some(&b)) = (cached.get(i), query.get(i)) {
                if !a.eq_ignore_ascii_case(&b) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }
}

/// Global DNS cache (A records).
static DNS_CACHE: Mutex<DnsCache> = Mutex::new(DnsCache::new());

// ---------------------------------------------------------------------------
// AAAA (IPv6) DNS cache
// ---------------------------------------------------------------------------

/// Maximum number of cached AAAA entries.
///
/// Smaller than the A cache because IPv6 is less commonly used.
const AAAA_CACHE_SIZE: usize = 16;

/// A cached DNS AAAA resolution result.
#[derive(Clone)]
struct AaaaCacheEntry {
    /// Domain name (lowercased).
    name: [u8; MAX_NAME_LEN],
    /// Length of the name.
    name_len: usize,
    /// Resolved IPv6 address.
    ip: Ipv6Addr,
    /// Absolute expiration time in nanoseconds (monotonic clock).
    expires_ns: u64,
}

impl AaaaCacheEntry {
    const fn empty() -> Self {
        Self {
            name: [0u8; MAX_NAME_LEN],
            name_len: 0,
            ip: Ipv6Addr::UNSPECIFIED,
            expires_ns: 0,
        }
    }

    fn is_valid(&self, now_ns: u64) -> bool {
        self.name_len > 0 && now_ns < self.expires_ns
    }
}

/// DNS AAAA resolution cache.
///
/// Same eviction strategy as the A record cache.
struct DnsAaaaCache {
    entries: [AaaaCacheEntry; AAAA_CACHE_SIZE],
    count: usize,
}

impl DnsAaaaCache {
    const fn new() -> Self {
        Self {
            entries: [const { AaaaCacheEntry::empty() }; AAAA_CACHE_SIZE],
            count: 0,
        }
    }

    /// Look up a name.  Returns `Some(Some(ip))` for positive hit,
    /// `Some(None)` for negative hit, `None` for cache miss.
    fn lookup(&self, name: &str, now_ns: u64) -> Option<Option<Ipv6Addr>> {
        let name_bytes = name.as_bytes();
        for entry in &self.entries {
            if entry.name_len == name_bytes.len()
                && entry.is_valid(now_ns)
                && DnsCache::names_match(&entry.name, entry.name_len, name_bytes)
            {
                if entry.ip.is_unspecified() {
                    return Some(None); // Negative cache entry.
                }
                return Some(Some(entry.ip));
            }
        }
        None
    }

    /// Insert or update an entry.
    fn insert(&mut self, name: &str, ip: Ipv6Addr, ttl_secs: u32, now_ns: u64) {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > MAX_NAME_LEN {
            return;
        }

        let ttl_clamped = ttl_secs.clamp(60, 3600);
        let ttl_ns = u64::from(ttl_clamped).wrapping_mul(1_000_000_000);
        let expires = now_ns.wrapping_add(ttl_ns);

        // Update in place if already cached.
        for entry in &mut self.entries {
            if entry.name_len == name_bytes.len()
                && DnsCache::names_match(&entry.name, entry.name_len, name_bytes)
            {
                entry.ip = ip;
                entry.expires_ns = expires;
                return;
            }
        }

        // Find a slot: prefer empty/expired, then evict oldest.
        let mut best_idx: usize = 0;
        let mut best_expires: u64 = u64::MAX;

        for (i, entry) in self.entries.iter().enumerate() {
            if entry.name_len == 0 {
                best_idx = i;
                break;
            }
            if !entry.is_valid(now_ns) {
                best_idx = i;
                break;
            }
            if entry.expires_ns < best_expires {
                best_expires = entry.expires_ns;
                best_idx = i;
            }
        }

        if let Some(slot) = self.entries.get_mut(best_idx) {
            if slot.name_len > 0 && slot.is_valid(now_ns) {
                CACHE_EVICTIONS.fetch_add(1, Ordering::Relaxed);
            }
            slot.name = [0u8; MAX_NAME_LEN];
            let copy_len = name_bytes.len().min(MAX_NAME_LEN);
            slot.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            slot.name_len = name_bytes.len();
            slot.ip = ip;
            slot.expires_ns = expires;
            if self.count < AAAA_CACHE_SIZE {
                self.count = self.count.wrapping_add(1);
            }
        }
    }
}

/// Global AAAA (IPv6) DNS cache.
static AAAA_CACHE: Mutex<DnsAaaaCache> = Mutex::new(DnsAaaaCache::new());

// ---------------------------------------------------------------------------
// DNS packet building
// ---------------------------------------------------------------------------

/// Build a DNS query packet for any record type.
///
/// `query_id` is the transaction ID for this query — used to match
/// the response and prevent spoofed replies.
/// `qtype` is the DNS record type (TYPE_A, TYPE_AAAA, TYPE_PTR, etc.).
///
/// Returns the raw UDP payload.
#[allow(clippy::arithmetic_side_effects)]
fn build_query_typed(name: &str, query_id: u16, qtype: u16) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // Header (12 bytes).
    pkt.extend_from_slice(&query_id.to_be_bytes());       // ID.
    pkt.extend_from_slice(&FLAGS_QUERY_RD.to_be_bytes()); // Flags.
    pkt.extend_from_slice(&1u16.to_be_bytes());           // QDCOUNT = 1.
    pkt.extend_from_slice(&0u16.to_be_bytes());           // ANCOUNT = 0.
    pkt.extend_from_slice(&0u16.to_be_bytes());           // NSCOUNT = 0.
    pkt.extend_from_slice(&0u16.to_be_bytes());           // ARCOUNT = 0.

    // Question section: encode domain name as labels.
    encode_name(&mut pkt, name);

    // Type + Class.
    pkt.extend_from_slice(&qtype.to_be_bytes());
    pkt.extend_from_slice(&CLASS_IN.to_be_bytes());

    pkt
}

/// Build a DNS query packet for an A record.
fn build_query(name: &str, query_id: u16) -> Vec<u8> {
    build_query_typed(name, query_id, TYPE_A)
}

/// Build a DNS query packet for an AAAA record (IPv6, RFC 3596).
fn build_aaaa_query(name: &str, query_id: u16) -> Vec<u8> {
    build_query_typed(name, query_id, TYPE_AAAA)
}

/// Build a DNS query packet for a PTR record (reverse DNS, IPv4).
///
/// Converts the IP address to the `in-addr.arpa` domain format
/// (e.g., `192.168.1.1` → `1.1.168.192.in-addr.arpa`) and queries
/// for a PTR record.
fn build_ptr_query(ip: Ipv4Addr, query_id: u16) -> Vec<u8> {
    let arpa_name = alloc::format!(
        "{}.{}.{}.{}.in-addr.arpa",
        ip.0[3], ip.0[2], ip.0[1], ip.0[0]
    );
    build_query_typed(&arpa_name, query_id, TYPE_PTR)
}

/// Build a DNS query packet for a PTR record (reverse DNS, IPv6).
///
/// Converts the IPv6 address to the `ip6.arpa` domain format:
/// each nibble of the 128-bit address is reversed and separated by dots.
///
/// E.g., `2001:db8::1` → `1.0.0.0.…0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa`
fn build_ptr6_query(ip: &Ipv6Addr, query_id: u16) -> Vec<u8> {
    let arpa_name = ipv6_to_ip6_arpa(ip);
    build_query_typed(&arpa_name, query_id, TYPE_PTR)
}

/// Convert an IPv6 address to the ip6.arpa reverse DNS format.
///
/// Each nibble (4 bits) of the address is represented as a hex digit,
/// reversed, and separated by dots.  For example:
///
/// `2001:0db8:0000:0000:0000:0000:0000:0001` →
/// `1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa`
fn ipv6_to_ip6_arpa(ip: &Ipv6Addr) -> String {
    // 32 nibbles * 2 chars each (nibble + '.') + "ip6.arpa" = ~74 bytes.
    let mut name = String::with_capacity(80);

    // Process each byte from the end, low nibble first.
    for i in (0..16).rev() {
        let byte = ip.0[i];
        let lo = byte & 0x0F;
        let hi = (byte >> 4) & 0x0F;

        // Low nibble first (reversed order).
        let hex_chars = b"0123456789abcdef";
        name.push(hex_chars[lo as usize] as char);
        name.push('.');
        name.push(hex_chars[hi as usize] as char);
        name.push('.');
    }
    name.push_str("ip6.arpa");
    name
}

/// Encode a domain name as DNS wire-format labels.
///
/// E.g., `"example.com"` → `\x07example\x03com\x00`.
///
/// Handles fully-qualified domain names (trailing dot, e.g.,
/// `"example.com."`) by filtering out empty labels.  Without this,
/// `split('.')` on a trailing-dot name produces an empty final label,
/// which encodes as a zero-length label *before* the root terminator —
/// an invalid DNS name that servers may reject.
fn encode_name(pkt: &mut Vec<u8>, name: &str) {
    for label in name.split('.').filter(|l| !l.is_empty()) {
        let len = label.len().min(63);
        pkt.push(len as u8);
        pkt.extend_from_slice(&label.as_bytes()[..len]);
    }
    pkt.push(0); // Root label.
}

// ---------------------------------------------------------------------------
// DNS response parsing
// ---------------------------------------------------------------------------

/// Parsed DNS A record result: IP address and TTL in seconds.
struct DnsResult {
    ip: Ipv4Addr,
    ttl_secs: u32,
}

/// Parse a DNS response and extract the first A record IP and TTL.
///
/// `expected_id` is the transaction ID from the query — responses
/// with a different ID are rejected (prevents spoofed replies).
///
/// If the answer section contains CNAME records, follows the chain
/// to find the A record for the final canonical name.  Most DNS
/// servers include both the CNAME and the A record for the target
/// in the same response, so this usually succeeds without additional
/// queries.
///
/// Returns `Err(NotFound)` with the CNAME target name if the response
/// has CNAMEs but no matching A record (caller should re-query for
/// the CNAME target).
#[allow(clippy::arithmetic_side_effects)]
fn parse_response(
    data: &[u8],
    expected_id: u16,
    cname_out: &mut Option<String>,
) -> KernelResult<DnsResult> {
    parse_response_inner(data, expected_id, None, cname_out)
}

/// Inner response parser that can optionally look for an A record
/// matching a specific name (used during CNAME resolution).
///
/// If the response contains only a CNAME (no A record), the CNAME
/// target is written to `cname_out` for the caller to chase.
#[allow(clippy::arithmetic_side_effects)]
fn parse_response_inner(
    data: &[u8],
    expected_id: u16,
    target_name: Option<&str>,
    cname_out: &mut Option<String>,
) -> KernelResult<DnsResult> {
    if data.len() < 12 {
        return Err(KernelError::InvalidArgument);
    }

    let id = u16::from_be_bytes([data[0], data[1]]);
    if id != expected_id {
        return Err(KernelError::InvalidArgument);
    }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    // Check QR bit (response) and RCODE (no error).
    if flags & 0x8000 == 0 {
        return Err(KernelError::InvalidArgument); // Not a response.
    }
    let rcode = flags & 0x000F;
    if rcode != 0 {
        return Err(KernelError::NotFound); // Server returned an error.
    }

    let qdcount = u16::from_be_bytes([data[4], data[5]]);
    let ancount = u16::from_be_bytes([data[6], data[7]]);

    if ancount == 0 {
        return Err(KernelError::NotFound);
    }

    // Skip the question section.
    let mut offset = 12;
    for _ in 0..qdcount {
        offset = skip_name(data, offset)?;
        if offset.checked_add(4).is_none_or(|end| end > data.len()) {
            return Err(KernelError::InvalidArgument);
        }
        offset += 4; // QTYPE + QCLASS.
    }

    // First pass: collect CNAME mappings and find A records.
    // We store up to MAX_CNAME_HOPS CNAME targets and check for A records.
    let mut cname_target: Option<String> = None;
    let mut a_results: Vec<(String, Ipv4Addr, u32)> = Vec::new();

    for _ in 0..ancount {
        if offset >= data.len() {
            break;
        }

        let (rr_name, new_offset) = decode_name(data, offset)?;
        offset = new_offset;

        if offset + 10 > data.len() {
            break;
        }

        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let rclass = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        let ttl = u32::from_be_bytes([
            data[offset + 4], data[offset + 5],
            data[offset + 6], data[offset + 7],
        ]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]);
        offset += 10;

        let rd_end = offset + rdlength as usize;
        if rd_end > data.len() {
            break;
        }

        if rclass == CLASS_IN {
            if rtype == TYPE_A && rdlength == 4 {
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&data[offset..offset + 4]);
                a_results.push((rr_name, Ipv4Addr(ip), ttl));
            } else if rtype == TYPE_CNAME {
                let (cname, _) = decode_name(data, offset)?;
                crate::serial_println!(
                    "[dns] CNAME: {} → {}", rr_name, cname
                );
                cname_target = Some(cname);
            }
        }

        offset = rd_end;
    }

    // If we have a specific target name, look for its A record first.
    if let Some(target) = target_name {
        for (name, ip, ttl) in &a_results {
            if names_eq_case_insensitive(name, target) {
                return Ok(DnsResult { ip: *ip, ttl_secs: *ttl });
            }
        }
    }

    // If we have a CNAME, check if any A record resolves the CNAME
    // target (common in responses that include the full CNAME chain).
    if let Some(ref cname) = cname_target {
        for (name, ip, ttl) in &a_results {
            if names_eq_case_insensitive(name, cname) {
                return Ok(DnsResult { ip: *ip, ttl_secs: *ttl });
            }
        }
    }

    // If we have any A record at all, return the first one.
    if let Some((_, ip, ttl)) = a_results.first() {
        return Ok(DnsResult { ip: *ip, ttl_secs: *ttl });
    }

    // No A record found.  If we got a CNAME, the caller may need to
    // send a follow-up query.  Return NotFound — the caller uses
    // `cname_out` for follow-up.
    if cname_target.is_some() {
        *cname_out = cname_target;
        return Err(KernelError::NotFound);
    }

    Err(KernelError::NotFound)
}

/// Parse a DNS PTR response and extract the hostname.
///
/// `expected_id` is the transaction ID from the query.
/// Returns the PTR name (e.g., "router.local") on success.
#[allow(clippy::arithmetic_side_effects)]
fn parse_ptr_response(data: &[u8], expected_id: u16) -> KernelResult<String> {
    if data.len() < 12 {
        return Err(KernelError::InvalidArgument);
    }

    let id = u16::from_be_bytes([data[0], data[1]]);
    if id != expected_id {
        return Err(KernelError::InvalidArgument);
    }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 {
        return Err(KernelError::InvalidArgument); // Not a response.
    }
    let rcode = flags & 0x000F;
    if rcode != 0 {
        return Err(KernelError::NotFound); // Server returned an error.
    }

    let qdcount = u16::from_be_bytes([data[4], data[5]]);
    let ancount = u16::from_be_bytes([data[6], data[7]]);

    if ancount == 0 {
        return Err(KernelError::NotFound);
    }

    // Skip the question section.
    let mut offset = 12;
    for _ in 0..qdcount {
        offset = skip_name(data, offset)?;
        if offset.checked_add(4).is_none_or(|end| end > data.len()) {
            return Err(KernelError::InvalidArgument);
        }
        offset += 4; // QTYPE + QCLASS.
    }

    // Scan answer section for PTR records.
    for _ in 0..ancount {
        if offset >= data.len() {
            break;
        }

        let (_rr_name, new_offset) = decode_name(data, offset)?;
        offset = new_offset;

        if offset + 10 > data.len() {
            break;
        }

        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let rclass = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        let _ttl = u32::from_be_bytes([
            data[offset + 4], data[offset + 5],
            data[offset + 6], data[offset + 7],
        ]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]);
        offset += 10;

        let rd_end = offset + rdlength as usize;
        if rd_end > data.len() {
            break;
        }

        if rclass == CLASS_IN && rtype == TYPE_PTR {
            let (ptr_name, _) = decode_name(data, offset)?;
            return Ok(ptr_name);
        }

        offset = rd_end;
    }

    Err(KernelError::NotFound)
}

/// Case-insensitive DNS name comparison.
///
/// Strips trailing dots before comparing so that `"example.com."`
/// matches `"example.com"` (FQDN vs non-FQDN forms of the same name).
fn names_eq_case_insensitive(a: &str, b: &str) -> bool {
    let a = a.strip_suffix('.').unwrap_or(a);
    let b = b.strip_suffix('.').unwrap_or(b);
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .all(|(x, y)| x.eq_ignore_ascii_case(&y))
}

// CNAME targets are now passed directly via `cname_out` parameters
// instead of global state, eliminating races between concurrent
// DNS resolutions.

/// Decode a DNS name at the given offset into a dotted string.
///
/// Handles compression pointers (RFC 1035 §4.1.4).  Returns the
/// decoded name and the offset in `data` after the name encoding
/// (following the first occurrence, not following pointers).
#[allow(clippy::arithmetic_side_effects)]
fn decode_name(data: &[u8], mut offset: usize) -> KernelResult<(String, usize)> {
    let mut name = String::with_capacity(64);
    let mut jumped = false;
    let mut ptr_offset = offset;
    let mut steps = 0;

    loop {
        if ptr_offset >= data.len() || steps > 128 {
            return Err(KernelError::InvalidArgument);
        }
        steps += 1;

        let len = data[ptr_offset];
        if len == 0 {
            if !jumped {
                offset = ptr_offset + 1;
            }
            break;
        }

        if len & 0xC0 != 0 {
            if len & 0xC0 != 0xC0 {
                // Reserved label type (0x40-0xBF) — reject.
                return Err(KernelError::InvalidArgument);
            }
            // Compression pointer — two bytes encode a 14-bit offset.
            if ptr_offset + 1 >= data.len() {
                return Err(KernelError::InvalidArgument);
            }
            if !jumped {
                // Save the position just past the pointer — this is where
                // the name ends in the original wire data.
                offset = ptr_offset + 2;
                jumped = true;
            }
            let target = (usize::from(len & 0x3F) << 8) | usize::from(data[ptr_offset + 1]);
            // Pointers must point strictly backward to prevent loops.
            if target >= ptr_offset {
                return Err(KernelError::InvalidArgument);
            }
            ptr_offset = target;
            continue;
        }

        // Regular label.
        let label_len = len as usize;
        let label_start = ptr_offset + 1;
        let label_end = label_start + label_len;
        if label_end > data.len() {
            return Err(KernelError::InvalidArgument);
        }

        if !name.is_empty() {
            name.push('.');
        }
        for &b in &data[label_start..label_end] {
            name.push(b as char);
        }

        ptr_offset = label_end;
    }

    Ok((name, offset))
}

/// Skip a DNS name at the given offset (handles compression pointers).
///
/// A DNS name in wire format ends when we encounter either:
/// - A null byte (root label terminator): consume 1 byte.
/// - A compression pointer (2 bytes, top 2 bits set): consume 2 bytes.
///
/// We don't need to follow compression pointers — we just need to know
/// where the name ends in the original data stream.
///
/// Returns the offset after the name.
#[allow(clippy::arithmetic_side_effects)]
fn skip_name(data: &[u8], mut offset: usize) -> KernelResult<usize> {
    let mut steps = 0;

    loop {
        if offset >= data.len() || steps > 128 {
            return Err(KernelError::InvalidArgument);
        }
        steps += 1;

        let len = data[offset];

        if len == 0 {
            // Root label — name ends here.
            return Ok(offset + 1);
        }

        if len & 0xC0 != 0 {
            if len & 0xC0 != 0xC0 {
                // Reserved label type (0x40-0xBF) — reject.
                return Err(KernelError::InvalidArgument);
            }
            // Compression pointer (2 bytes) — name ends after the pointer.
            // We don't follow it; we just advance past it.
            if offset + 1 >= data.len() {
                return Err(KernelError::InvalidArgument);
            }
            return Ok(offset + 2);
        }

        // Regular label: 1 length byte + `len` content bytes.
        offset += 1 + len as usize;
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve a domain name to an IPv4 address.
///
/// Checks the DNS cache first.  On a cache miss, sends a DNS A record
/// query to the configured DNS server and waits for a response
/// (blocking with polling, up to ~2 seconds).  If the response contains
/// CNAME records without a matching A record, follows the CNAME chain
/// with additional queries (up to [`MAX_CNAME_HOPS`] hops).
///
/// Successful results are cached with the TTL from the DNS response.
#[allow(clippy::arithmetic_side_effects)]
pub fn resolve(name: &str) -> KernelResult<Ipv4Addr> {
    // Docker embedded DNS: if the caller is inside a container attached to a
    // user-defined network, a peer's container name / hostname / alias resolves
    // to that peer's address *before* any upstream query (127.0.0.11 semantics).
    // The host namespace (0) skips this; a non-matching name falls through.
    let caller_ns = crate::sched::current_task_net_ns();
    if let Some(ip) = crate::container::resolve_dns(caller_ns, name) {
        return Ok(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]));
    }

    let mut current_name = String::from(name);
    let mut cname_out = None;

    for hop in 0..MAX_CNAME_HOPS {
        match resolve_single(&current_name, &mut cname_out) {
            Ok(ip) => {
                // Cache under the original name too, if we followed CNAMEs.
                if hop > 0 {
                    let cache_now = crate::hrtimer::now_ns();
                    // Use a reasonable TTL for the original name (60s).
                    DNS_CACHE.lock().insert(name, ip, 60, cache_now);
                }
                return Ok(ip);
            }
            Err(KernelError::NotFound) => {
                // Check if parse_response returned a CNAME target to follow.
                if let Some(target) = cname_out.take() {
                    crate::serial_println!(
                        "[dns] Following CNAME: {} → {} (hop {})",
                        current_name, target, hop + 1
                    );
                    // Check cache for the CNAME target before querying.
                    let now_ns = crate::hrtimer::now_ns();
                    match DNS_CACHE.lock().lookup(&target, now_ns) {
                        Some(Some(ip)) => {
                            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                            crate::serial_println!(
                                "[dns] Cache hit for CNAME target: '{}' → {}",
                                target, ip
                            );
                            // Also cache under the original name.
                            let cache_now = crate::hrtimer::now_ns();
                            DNS_CACHE.lock().insert(name, ip, 60, cache_now);
                            return Ok(ip);
                        }
                        Some(None) => {
                            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                            // CNAME target is negatively cached — fail.
                            return Err(KernelError::NotFound);
                        }
                        None => {
                            CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    current_name = target;
                    continue;
                }
                return Err(KernelError::NotFound);
            }
            Err(e) => return Err(e),
        }
    }

    crate::serial_println!(
        "[dns] CNAME loop detected for '{}' (>{} hops)",
        name, MAX_CNAME_HOPS
    );
    Err(KernelError::InvalidArgument)
}

/// Maximum number of query attempts before giving up.
///
/// Each attempt uses an increasing timeout: 1s, 2s, 4s.  Total worst-case
/// wait is ~7 seconds, which matches typical resolver behavior.
const MAX_DNS_ATTEMPTS: usize = 3;

/// Poll iterations per attempt.  Each iteration is ~1ms of spin delay,
/// so these correspond to roughly 1s, 2s, 4s timeouts.
const DNS_ATTEMPT_POLLS: [usize; MAX_DNS_ATTEMPTS] = [1000, 2000, 4000];

// ---------------------------------------------------------------------------
// Transport-agnostic DNS query helper
// ---------------------------------------------------------------------------

/// DNS server address — either IPv4 (from DHCP) or IPv6 (from SLAAC RDNSS).
///
/// Allows DNS queries to be sent over either IPv4 or IPv6 transport
/// depending on the available network configuration.
#[derive(Debug, Clone, Copy)]
enum DnsServer {
    /// IPv4 DNS server (typically from DHCP).
    V4(Ipv4Addr),
    /// IPv6 DNS server (typically from Router Advertisement RDNSS option).
    V6(Ipv6Addr),
}

impl core::fmt::Display for DnsServer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DnsServer::V4(ip) => write!(f, "{}", ip),
            DnsServer::V6(ip) => write!(f, "{}", ip),
        }
    }
}

/// Pick the best available DNS server for the root (host) namespace.
///
/// Prefers IPv4 (from DHCP) since it's more widely deployed.  Falls back
/// to IPv6 (from SLAAC RDNSS) when no IPv4 DNS server is configured.
fn pick_dns_server_root() -> KernelResult<DnsServer> {
    let v4 = interface::info().dns;
    if !v4.is_unspecified() {
        return Ok(DnsServer::V4(v4));
    }
    if let Some(v6) = super::icmpv6::slaac_rdnss() {
        return Ok(DnsServer::V6(v6));
    }
    Err(KernelError::NotSupported)
}

/// Pick the DNS server for a specific network namespace.
///
/// For non-root namespaces, checks the namespace's configured DNS server
/// first.  Falls back to the root namespace DNS if the namespace has no
/// DNS configured or the namespace is the root.
///
/// Safe to call before netns subsystem is initialized (falls back to root).
fn pick_dns_server_for_ns(ns_id: crate::netns::NetNsId) -> KernelResult<DnsServer> {
    if ns_id != crate::netns::ROOT_NS && crate::netns::is_initialized() {
        if let Some(cfg) = crate::netns::interface_config(ns_id) {
            let dns_bytes = cfg.dns.0;
            // Non-zero means a DNS server is configured for this namespace.
            if dns_bytes != [0, 0, 0, 0] {
                let dns = Ipv4Addr::new(dns_bytes[0], dns_bytes[1], dns_bytes[2], dns_bytes[3]);
                return Ok(DnsServer::V4(dns));
            }
        }
    }
    // Fallback to root namespace DNS.
    pick_dns_server_root()
}

/// Pick the DNS server for the current task's network namespace.
///
/// Queries the calling task's net_ns and uses its configured DNS server.
/// If the task is in the root namespace (default), or its namespace has no
/// DNS configured, falls back to the host's DHCP/SLAAC DNS.
fn pick_dns_server() -> KernelResult<DnsServer> {
    let ns_id = crate::sched::current_task_net_ns();
    pick_dns_server_for_ns(ns_id)
}

/// Send a DNS query and wait for a response with retry/backoff.
///
/// Abstracts the transport (IPv4 or IPv6 UDP) based on the DNS server
/// address family.  The DNS query format is identical regardless of
/// transport — only the UDP send/receive changes.
///
/// Returns the raw DNS response payload on success, or `TimedOut` after
/// exhausting all retry attempts.
#[allow(clippy::arithmetic_side_effects)]
fn dns_query_raw(
    server: &DnsServer,
    local_port: u16,
    query: &[u8],
    name: &str,
) -> KernelResult<Vec<u8>> {
    let sock = super::udp::bind(crate::netns::ROOT_NS, local_port)?;

    for attempt in 0..MAX_DNS_ATTEMPTS {
        // Send (or re-send) the query via the appropriate transport.
        let send_result = match server {
            DnsServer::V4(ip) => super::udp::send(local_port, *ip, DNS_PORT, query),
            DnsServer::V6(ip) => super::udp::send_v6(local_port, *ip, DNS_PORT, query),
        };
        if let Err(e) = send_result {
            super::udp::close(sock);
            return Err(e);
        }

        if attempt > 0 {
            crate::serial_println!(
                "[dns] Retry {} for '{}' (timeout {}ms)",
                attempt, name,
                DNS_ATTEMPT_POLLS.get(attempt).copied().unwrap_or(2000)
            );
        }

        let polls = DNS_ATTEMPT_POLLS.get(attempt).copied().unwrap_or(2000);

        for _ in 0..polls {
            super::poll();

            // Check the appropriate receive queue based on server type.
            let response = match server {
                DnsServer::V4(ip) => {
                    super::udp::recv(sock).and_then(|dgram| {
                        if dgram.src_ip == *ip && dgram.src_port == DNS_PORT {
                            Some(dgram.data)
                        } else {
                            None
                        }
                    })
                }
                DnsServer::V6(ip) => {
                    super::udp::recv_v6(sock).and_then(|dgram| {
                        if dgram.src_ip == *ip && dgram.src_port == DNS_PORT {
                            Some(dgram.data)
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some(data) = response {
                super::udp::close(sock);
                return Ok(data);
            }

            // Brief spin delay (~1ms per iteration).
            for _ in 0..10_000 {
                core::hint::spin_loop();
            }
        }
    }

    super::udp::close(sock);
    crate::serial_println!(
        "[dns] Query timed out for '{}' after {} attempts via {}",
        name, MAX_DNS_ATTEMPTS, server
    );
    Err(KernelError::TimedOut)
}

/// Resolve a single name (with retry on timeout).
///
/// Sends a DNS A record query and waits for a response.  On timeout,
/// retransmits the query with an increasing wait window (1s → 2s → 4s).
/// Uses [`pick_dns_server`] to select IPv4 or IPv6 DNS transport.
/// Returns `Ok(ip)` on success, `Err(NotFound)` if no A record was
/// found (check `cname_out` for CNAME follow-up), or another error.
#[allow(clippy::arithmetic_side_effects)]
fn resolve_single(name: &str, cname_out: &mut Option<String>) -> KernelResult<Ipv4Addr> {
    let now_ns = crate::hrtimer::now_ns();

    // Check cache first (positive and negative entries).
    match DNS_CACHE.lock().lookup(name, now_ns) {
        Some(Some(ip)) => {
            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!("[dns] Cache hit: '{}' → {}", name, ip);
            return Ok(ip);
        }
        Some(None) => {
            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!("[dns] Negative cache hit: '{}'", name);
            return Err(KernelError::NotFound);
        }
        None => {
            CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
        }
    }

    let server = pick_dns_server()?;
    crate::serial_println!("[dns] Resolving '{}' via {}...", name, server);

    *cname_out = None;

    let query_id = next_query_id();
    let local_port = next_dns_port();
    let query = build_query(name, query_id);

    let response_data = dns_query_raw(&server, local_port, &query, name)?;

    match parse_response(&response_data, query_id, cname_out) {
        Ok(result) => {
            crate::serial_println!(
                "[dns] Resolved '{}' → {} (TTL {}s)",
                name, result.ip, result.ttl_secs
            );
            let cache_now = crate::hrtimer::now_ns();
            DNS_CACHE.lock().insert(name, result.ip, result.ttl_secs, cache_now);
            Ok(result.ip)
        }
        Err(KernelError::NotFound) => {
            if cname_out.is_none() {
                let cache_now = crate::hrtimer::now_ns();
                DNS_CACHE.lock().insert(
                    name,
                    Ipv4Addr::UNSPECIFIED,
                    NEGATIVE_CACHE_TTL,
                    cache_now,
                );
            }
            Err(KernelError::NotFound)
        }
        Err(e) => {
            crate::serial_println!("[dns] Parse error: {:?}", e);
            Err(e)
        }
    }
}

/// Flush the entire DNS cache (both A and AAAA).
///
/// Called when the network configuration changes (e.g., DHCP renewal
/// with a new DNS server) to avoid stale cached results.
pub fn flush_cache() {
    {
        let mut cache = DNS_CACHE.lock();
        *cache = DnsCache::new();
    }
    {
        let mut cache = AAAA_CACHE.lock();
        *cache = DnsAaaaCache::new();
    }
    crate::serial_println!("[dns] Cache flushed (A + AAAA)");
}

/// Resolve a domain name and return it as a formatted string.
pub fn resolve_str(name: &str) -> KernelResult<String> {
    let ip = resolve(name)?;
    Ok(alloc::format!("{}", ip))
}

/// Reverse-resolve an IPv4 address to a hostname (PTR record).
///
/// Sends a PTR query to the configured DNS server for the `in-addr.arpa`
/// domain corresponding to the IP address.  For example, `192.168.1.1`
/// queries for `1.1.168.192.in-addr.arpa`.
///
/// Returns the hostname string on success (e.g., "router.local"),
/// or `Err(NotFound)` if no PTR record exists.
///
/// Results are not cached (PTR records change less frequently and
/// reverse lookups are comparatively rare).
///
/// Uses [`pick_dns_server`] for transport selection (IPv4 or IPv6).
#[allow(clippy::arithmetic_side_effects)]
pub fn reverse_resolve(ip: Ipv4Addr) -> KernelResult<String> {
    let server = pick_dns_server()?;
    crate::serial_println!("[dns] Reverse resolving {} via {}...", ip, server);

    let query_id = next_query_id();
    let local_port = next_dns_port();
    let query = build_ptr_query(ip, query_id);
    let arpa_name = alloc::format!("{}", ip); // For timeout logging.

    let response_data = dns_query_raw(&server, local_port, &query, &arpa_name)?;

    match parse_ptr_response(&response_data, query_id) {
        Ok(name) => {
            crate::serial_println!("[dns] Reverse resolved {} → '{}'", ip, name);
            Ok(name)
        }
        Err(KernelError::NotFound) => {
            crate::serial_println!("[dns] No PTR record for {}", ip);
            Err(KernelError::NotFound)
        }
        Err(e) => {
            crate::serial_println!("[dns] PTR parse error: {:?}", e);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// AAAA (IPv6) resolution — RFC 3596
// ---------------------------------------------------------------------------

/// Parsed DNS AAAA record result: IPv6 address and TTL in seconds.
struct DnsResult6 {
    ip: Ipv6Addr,
    ttl_secs: u32,
}

/// Parse a DNS response and extract the first AAAA record.
///
/// Follows CNAME chains within the response, same as the A record parser.
/// Returns `Err(NotFound)` with the CNAME target in `cname_out` if only
/// CNAMEs are present (caller should re-query).
#[allow(clippy::arithmetic_side_effects)]
fn parse_aaaa_response(
    data: &[u8],
    expected_id: u16,
    cname_out: &mut Option<String>,
) -> KernelResult<DnsResult6> {
    if data.len() < 12 {
        return Err(KernelError::InvalidArgument);
    }

    let id = u16::from_be_bytes([data[0], data[1]]);
    if id != expected_id {
        return Err(KernelError::InvalidArgument);
    }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & 0x8000 == 0 {
        return Err(KernelError::InvalidArgument); // Not a response.
    }
    let rcode = flags & 0x000F;
    if rcode != 0 {
        return Err(KernelError::NotFound); // Server error (NXDOMAIN, etc.).
    }

    let qdcount = u16::from_be_bytes([data[4], data[5]]);
    let ancount = u16::from_be_bytes([data[6], data[7]]);

    if ancount == 0 {
        return Err(KernelError::NotFound);
    }

    // Skip the question section.
    let mut offset = 12;
    for _ in 0..qdcount {
        offset = skip_name(data, offset)?;
        if offset.checked_add(4).is_none_or(|end| end > data.len()) {
            return Err(KernelError::InvalidArgument);
        }
        offset += 4; // QTYPE + QCLASS.
    }

    // Collect CNAME mappings and AAAA records.
    let mut cname_target: Option<String> = None;
    let mut aaaa_results: Vec<(String, Ipv6Addr, u32)> = Vec::new();

    for _ in 0..ancount {
        if offset >= data.len() {
            break;
        }

        let (rr_name, new_offset) = decode_name(data, offset)?;
        offset = new_offset;

        if offset + 10 > data.len() {
            break;
        }

        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let rclass = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        let ttl = u32::from_be_bytes([
            data[offset + 4], data[offset + 5],
            data[offset + 6], data[offset + 7],
        ]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]);
        offset += 10;

        let rd_end = offset + rdlength as usize;
        if rd_end > data.len() {
            break;
        }

        if rclass == CLASS_IN {
            if rtype == TYPE_AAAA && rdlength == 16 {
                let mut addr = [0u8; 16];
                addr.copy_from_slice(&data[offset..offset + 16]);
                aaaa_results.push((rr_name, Ipv6Addr(addr), ttl));
            } else if rtype == TYPE_CNAME {
                let (cname, _) = decode_name(data, offset)?;
                crate::serial_println!(
                    "[dns] CNAME (AAAA): {} → {}", rr_name, cname
                );
                cname_target = Some(cname);
            }
        }

        offset = rd_end;
    }

    // If we have a CNAME, check if any AAAA record resolves the target.
    if let Some(ref cname) = cname_target {
        for (name, ip, ttl) in &aaaa_results {
            if names_eq_case_insensitive(name, cname) {
                return Ok(DnsResult6 { ip: *ip, ttl_secs: *ttl });
            }
        }
    }

    // Return the first AAAA record found.
    if let Some((_, ip, ttl)) = aaaa_results.first() {
        return Ok(DnsResult6 { ip: *ip, ttl_secs: *ttl });
    }

    // No AAAA record — propagate CNAME for follow-up.
    if cname_target.is_some() {
        *cname_out = cname_target;
        return Err(KernelError::NotFound);
    }

    Err(KernelError::NotFound)
}

/// Resolve a domain name to an IPv6 address (AAAA record, RFC 3596).
///
/// Checks the AAAA cache first.  On a cache miss, sends a DNS AAAA
/// query and waits for a response.  Follows CNAME chains.
///
/// Successful results are cached with the TTL from the DNS response.
#[allow(dead_code)] // Public API, not yet called from other modules.
#[allow(clippy::arithmetic_side_effects)]
pub fn resolve6(name: &str) -> KernelResult<Ipv6Addr> {
    let mut current_name = String::from(name);
    let mut cname_out = None;

    for hop in 0..MAX_CNAME_HOPS {
        match resolve6_single(&current_name, &mut cname_out) {
            Ok(ip) => {
                // Cache under the original name too, if we followed CNAMEs.
                if hop > 0 {
                    let cache_now = crate::hrtimer::now_ns();
                    AAAA_CACHE.lock().insert(name, ip, 60, cache_now);
                }
                return Ok(ip);
            }
            Err(KernelError::NotFound) => {
                if let Some(target) = cname_out.take() {
                    crate::serial_println!(
                        "[dns] Following CNAME (AAAA): {} → {} (hop {})",
                        current_name, target, hop + 1
                    );
                    // Check AAAA cache for the CNAME target.
                    let now_ns = crate::hrtimer::now_ns();
                    match AAAA_CACHE.lock().lookup(&target, now_ns) {
                        Some(Some(ip)) => {
                            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                            let cache_now = crate::hrtimer::now_ns();
                            AAAA_CACHE.lock().insert(name, ip, 60, cache_now);
                            return Ok(ip);
                        }
                        Some(None) => {
                            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                            return Err(KernelError::NotFound);
                        }
                        None => {
                            CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    current_name = target;
                    continue;
                }
                return Err(KernelError::NotFound);
            }
            Err(e) => return Err(e),
        }
    }

    crate::serial_println!(
        "[dns] AAAA CNAME loop for '{}' (>{} hops)",
        name, MAX_CNAME_HOPS
    );
    Err(KernelError::InvalidArgument)
}

/// Resolve a single AAAA name (with retry on timeout).
///
/// Uses [`pick_dns_server`] for transport selection (IPv4 or IPv6).
#[allow(clippy::arithmetic_side_effects)]
fn resolve6_single(name: &str, cname_out: &mut Option<String>) -> KernelResult<Ipv6Addr> {
    let now_ns = crate::hrtimer::now_ns();

    // Check AAAA cache first.
    match AAAA_CACHE.lock().lookup(name, now_ns) {
        Some(Some(ip)) => {
            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!("[dns] AAAA cache hit: '{}' → {}", name, ip);
            return Ok(ip);
        }
        Some(None) => {
            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!("[dns] AAAA negative cache hit: '{}'", name);
            return Err(KernelError::NotFound);
        }
        None => {
            CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
        }
    }

    let server = pick_dns_server()?;
    crate::serial_println!("[dns] AAAA resolving '{}' via {}...", name, server);

    *cname_out = None;

    let query_id = next_query_id();
    let local_port = next_dns_port();
    let query = build_aaaa_query(name, query_id);

    let response_data = dns_query_raw(&server, local_port, &query, name)?;

    match parse_aaaa_response(&response_data, query_id, cname_out) {
        Ok(result) => {
            crate::serial_println!(
                "[dns] AAAA resolved '{}' → {} (TTL {}s)",
                name, result.ip, result.ttl_secs
            );
            let cache_now = crate::hrtimer::now_ns();
            AAAA_CACHE.lock().insert(name, result.ip, result.ttl_secs, cache_now);
            Ok(result.ip)
        }
        Err(KernelError::NotFound) => {
            if cname_out.is_none() {
                let cache_now = crate::hrtimer::now_ns();
                AAAA_CACHE.lock().insert(
                    name,
                    Ipv6Addr::UNSPECIFIED,
                    NEGATIVE_CACHE_TTL,
                    cache_now,
                );
            }
            Err(KernelError::NotFound)
        }
        Err(e) => {
            crate::serial_println!("[dns] AAAA parse error: {:?}", e);
            Err(e)
        }
    }
}

/// Resolve a domain name to an IPv6 address and return it as a string.
#[allow(dead_code)] // Public API.
pub fn resolve6_str(name: &str) -> KernelResult<String> {
    let ip = resolve6(name)?;
    Ok(alloc::format!("{}", ip))
}

/// Reverse-resolve an IPv6 address to a hostname (PTR record via ip6.arpa).
///
/// Converts the address to the nibble-reversed ip6.arpa domain and sends
/// a PTR query.  For example, `2001:db8::1` queries for
/// `1.0.0.0.…0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa`.
///
/// Results are not cached (reverse lookups are rare).
///
/// Uses [`pick_dns_server`] for transport selection (IPv4 or IPv6).
#[allow(dead_code)] // Public API.
#[allow(clippy::arithmetic_side_effects)]
pub fn reverse_resolve6(ip: &Ipv6Addr) -> KernelResult<String> {
    let server = pick_dns_server()?;
    crate::serial_println!("[dns] Reverse resolving {} via {}...", ip, server);

    let query_id = next_query_id();
    let local_port = next_dns_port();
    let query = build_ptr6_query(ip, query_id);
    let name_for_log = alloc::format!("{}", ip);

    let response_data = dns_query_raw(&server, local_port, &query, &name_for_log)?;

    match parse_ptr_response(&response_data, query_id) {
        Ok(name) => {
            crate::serial_println!("[dns] PTR6 resolved {} → '{}'", ip, name);
            Ok(name)
        }
        Err(KernelError::NotFound) => {
            crate::serial_println!("[dns] No PTR record for {}", ip);
            Err(KernelError::NotFound)
        }
        Err(e) => {
            crate::serial_println!("[dns] PTR6 parse error: {:?}", e);
            Err(e)
        }
    }
}

/// Return the number of entries in the AAAA cache.
pub fn aaaa_cache_count() -> usize {
    AAAA_CACHE.lock().count
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// DNS unit tests — exercises name encoding/decoding, query building,
/// response parsing, cache insert/lookup, and case-insensitive matching.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[dns] Running DNS self-test...");

    test_encode_name()?;
    test_decode_name()?;
    test_skip_name()?;
    test_names_case_insensitive()?;
    test_build_query_structure()?;
    test_parse_response_a_record()?;
    test_cache_insert_lookup()?;
    test_cache_negative_entry()?;
    test_build_aaaa_query_structure()?;
    test_parse_aaaa_response()?;
    test_aaaa_cache()?;
    test_ipv6_reverse_name()?;
    test_ns_aware_dns_picker()?;

    crate::serial_println!("[dns] DNS self-test PASSED (13 tests)");
    Ok(())
}

/// Test encode_name produces correct DNS wire format.
fn test_encode_name() -> KernelResult<()> {
    let mut buf = Vec::new();
    encode_name(&mut buf, "example.com");

    // Expected: \x07example\x03com\x00
    let expected: &[u8] = &[
        7, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
        3, b'c', b'o', b'm',
        0,
    ];
    if buf.as_slice() != expected {
        crate::serial_println!("[dns]   FAIL: encode_name mismatch (len={})", buf.len());
        return Err(KernelError::InternalError);
    }

    // FQDN with trailing dot should produce the same output.
    let mut buf2 = Vec::new();
    encode_name(&mut buf2, "example.com.");
    if buf2.as_slice() != expected {
        crate::serial_println!("[dns]   FAIL: FQDN encode mismatch");
        return Err(KernelError::InternalError);
    }

    // Single-label name.
    let mut buf3 = Vec::new();
    encode_name(&mut buf3, "localhost");
    let expected3: &[u8] = &[
        9, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't',
        0,
    ];
    if buf3.as_slice() != expected3 {
        crate::serial_println!("[dns]   FAIL: single-label encode mismatch");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   encode_name: OK");
    Ok(())
}

/// Test decode_name on known wire-format data.
fn test_decode_name() -> KernelResult<()> {
    // Wire data: \x07example\x03com\x00
    let wire: &[u8] = &[
        7, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
        3, b'c', b'o', b'm',
        0,
    ];
    let (name, end_offset) = decode_name(wire, 0)?;
    if name != "example.com" {
        crate::serial_println!("[dns]   FAIL: decoded '{}', expected 'example.com'", name);
        return Err(KernelError::InternalError);
    }
    if end_offset != 13 {
        crate::serial_println!("[dns]   FAIL: end_offset = {}, expected 13", end_offset);
        return Err(KernelError::InternalError);
    }

    // Test with compression pointer: build wire data where a name points back.
    // Layout: offset 0 = \x07example\x03com\x00 (13 bytes)
    //         offset 13 = \x03www + compression pointer to offset 0
    let mut wire2 = Vec::from(wire);
    wire2.extend_from_slice(&[
        3, b'w', b'w', b'w',       // "www" label
        0xC0, 0x00,                 // compression pointer → offset 0
    ]);
    let (name2, end2) = decode_name(&wire2, 13)?;
    if name2 != "www.example.com" {
        crate::serial_println!("[dns]   FAIL: compressed name = '{}'", name2);
        return Err(KernelError::InternalError);
    }
    // end_offset should be right after the pointer (13 + 4 label bytes + 2 pointer bytes = 19).
    if end2 != 19 {
        crate::serial_println!("[dns]   FAIL: compressed end_offset = {}", end2);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   decode_name: OK");
    Ok(())
}

/// Test skip_name correctly advances past names.
fn test_skip_name() -> KernelResult<()> {
    // Simple name: \x07example\x03com\x00 (13 bytes).
    let wire: &[u8] = &[
        7, b'e', b'x', b'a', b'm', b'p', b'l', b'e',
        3, b'c', b'o', b'm',
        0,
    ];
    let after = skip_name(wire, 0)?;
    if after != 13 {
        crate::serial_println!("[dns]   FAIL: skip simple name = {}", after);
        return Err(KernelError::InternalError);
    }

    // Name with compression pointer at the end.
    let wire2: &[u8] = &[
        3, b'w', b'w', b'w',
        0xC0, 0x00, // pointer to offset 0
    ];
    let after2 = skip_name(wire2, 0)?;
    // Should advance past the 4-byte label + 2-byte pointer = 6.
    if after2 != 6 {
        crate::serial_println!("[dns]   FAIL: skip compressed name = {}", after2);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   skip_name: OK");
    Ok(())
}

/// Test case-insensitive name comparison.
fn test_names_case_insensitive() -> KernelResult<()> {
    if !names_eq_case_insensitive("example.com", "EXAMPLE.COM") {
        crate::serial_println!("[dns]   FAIL: case-insensitive match failed");
        return Err(KernelError::InternalError);
    }
    if !names_eq_case_insensitive("Example.Com.", "example.com.") {
        crate::serial_println!("[dns]   FAIL: mixed case + FQDN match failed");
        return Err(KernelError::InternalError);
    }
    if !names_eq_case_insensitive("example.com.", "example.com") {
        crate::serial_println!("[dns]   FAIL: FQDN vs non-FQDN match failed");
        return Err(KernelError::InternalError);
    }
    if names_eq_case_insensitive("example.com", "example.org") {
        crate::serial_println!("[dns]   FAIL: different names matched");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   names case-insensitive: OK");
    Ok(())
}

/// Test build_query produces valid DNS query structure.
#[allow(clippy::arithmetic_side_effects)]
fn test_build_query_structure() -> KernelResult<()> {
    let query = build_query("test.dev", 0x1234);

    // Minimum: 12 header + name encoding + 4 (qtype+qclass).
    if query.len() < 12 + 4 {
        crate::serial_println!("[dns]   FAIL: query too short ({})", query.len());
        return Err(KernelError::InternalError);
    }

    // Check transaction ID.
    let id = u16::from_be_bytes([query[0], query[1]]);
    if id != 0x1234 {
        crate::serial_println!("[dns]   FAIL: query ID = {:#06x}", id);
        return Err(KernelError::InternalError);
    }

    // Check flags: standard query, RD set.
    let flags = u16::from_be_bytes([query[2], query[3]]);
    if flags != FLAGS_QUERY_RD {
        crate::serial_println!("[dns]   FAIL: flags = {:#06x}", flags);
        return Err(KernelError::InternalError);
    }

    // QDCOUNT = 1.
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if qdcount != 1 {
        crate::serial_println!("[dns]   FAIL: QDCOUNT = {}", qdcount);
        return Err(KernelError::InternalError);
    }

    // Check that name is encoded starting at offset 12.
    // "test.dev" → \x04test\x03dev\x00 (10 bytes).
    if query.len() < 12 + 10 + 4 {
        crate::serial_println!("[dns]   FAIL: query too short for name");
        return Err(KernelError::InternalError);
    }
    if query[12] != 4 || query[17] != 3 {
        crate::serial_println!("[dns]   FAIL: name label lengths wrong");
        return Err(KernelError::InternalError);
    }

    // QTYPE = A (1) at end-2, QCLASS = IN (1) at end.
    let qtype = u16::from_be_bytes([query[query.len() - 4], query[query.len() - 3]]);
    let qclass = u16::from_be_bytes([query[query.len() - 2], query[query.len() - 1]]);
    if qtype != TYPE_A {
        crate::serial_println!("[dns]   FAIL: QTYPE = {}", qtype);
        return Err(KernelError::InternalError);
    }
    if qclass != CLASS_IN {
        crate::serial_println!("[dns]   FAIL: QCLASS = {}", qclass);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   build_query structure: OK");
    Ok(())
}

/// Test parse_response with a hand-crafted A record response.
#[allow(clippy::arithmetic_side_effects)]
fn test_parse_response_a_record() -> KernelResult<()> {
    // Build a synthetic DNS response for "test.dev" → 93.184.216.34.
    let query_id: u16 = 0xABCD;
    let mut resp = Vec::new();

    // Header.
    resp.extend_from_slice(&query_id.to_be_bytes()); // ID
    resp.extend_from_slice(&0x8180u16.to_be_bytes()); // Flags: QR=1, RD=1, RA=1
    resp.extend_from_slice(&1u16.to_be_bytes());      // QDCOUNT = 1
    resp.extend_from_slice(&1u16.to_be_bytes());      // ANCOUNT = 1
    resp.extend_from_slice(&0u16.to_be_bytes());      // NSCOUNT = 0
    resp.extend_from_slice(&0u16.to_be_bytes());      // ARCOUNT = 0

    // Question section: "test.dev" IN A
    encode_name(&mut resp, "test.dev");
    resp.extend_from_slice(&TYPE_A.to_be_bytes());
    resp.extend_from_slice(&CLASS_IN.to_be_bytes());

    // Answer section: test.dev A 300 93.184.216.34
    encode_name(&mut resp, "test.dev");
    resp.extend_from_slice(&TYPE_A.to_be_bytes());     // TYPE
    resp.extend_from_slice(&CLASS_IN.to_be_bytes());    // CLASS
    resp.extend_from_slice(&300u32.to_be_bytes());      // TTL = 300s
    resp.extend_from_slice(&4u16.to_be_bytes());        // RDLENGTH = 4
    resp.extend_from_slice(&[93, 184, 216, 34]);        // RDATA

    let mut cname_out = None;
    let result = parse_response(&resp, query_id, &mut cname_out)?;

    if result.ip != Ipv4Addr([93, 184, 216, 34]) {
        crate::serial_println!("[dns]   FAIL: parsed IP = {}", result.ip);
        return Err(KernelError::InternalError);
    }
    if result.ttl_secs != 300 {
        crate::serial_println!("[dns]   FAIL: parsed TTL = {}", result.ttl_secs);
        return Err(KernelError::InternalError);
    }

    // Wrong query ID should be rejected.
    let mut cname_out2 = None;
    if parse_response(&resp, 0x9999, &mut cname_out2).is_ok() {
        crate::serial_println!("[dns]   FAIL: accepted wrong query ID");
        return Err(KernelError::InternalError);
    }

    // Too-short response should be rejected.
    let mut cname_out3 = None;
    if parse_response(&resp[..5], query_id, &mut cname_out3).is_ok() {
        crate::serial_println!("[dns]   FAIL: accepted too-short response");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   parse response A record: OK");
    Ok(())
}

/// Test DNS cache insert and lookup.
fn test_cache_insert_lookup() -> KernelResult<()> {
    let mut cache = DnsCache::new();
    let now = crate::hrtimer::now_ns();
    let ip = Ipv4Addr([1, 2, 3, 4]);

    // Insert and lookup.
    cache.insert("cached.example", ip, 120, now);

    match cache.lookup("cached.example", now) {
        Some(Some(found)) if found == ip => {} // OK.
        Some(Some(found)) => {
            crate::serial_println!("[dns]   FAIL: cache lookup = {}", found);
            return Err(KernelError::InternalError);
        }
        Some(None) => {
            crate::serial_println!("[dns]   FAIL: cache returned negative for positive entry");
            return Err(KernelError::InternalError);
        }
        None => {
            crate::serial_println!("[dns]   FAIL: cache miss after insert");
            return Err(KernelError::InternalError);
        }
    }

    // Case-insensitive lookup.
    match cache.lookup("CACHED.EXAMPLE", now) {
        Some(Some(found)) if found == ip => {} // OK.
        _ => {
            crate::serial_println!("[dns]   FAIL: case-insensitive lookup failed");
            return Err(KernelError::InternalError);
        }
    }

    // Entry not present.
    if cache.lookup("nonexistent.test", now).is_some() {
        crate::serial_println!("[dns]   FAIL: found nonexistent entry");
        return Err(KernelError::InternalError);
    }

    // Expired entry (far future timestamp).
    let far_future = now.wrapping_add(1_000_000_000_000); // ~1000s
    if cache.lookup("cached.example", far_future).is_some() {
        crate::serial_println!("[dns]   FAIL: expired entry still returned");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   cache insert/lookup: OK");
    Ok(())
}

/// Test DNS negative cache entry.
fn test_cache_negative_entry() -> KernelResult<()> {
    let mut cache = DnsCache::new();
    let now = crate::hrtimer::now_ns();

    // Insert a negative entry (NXDOMAIN sentinel: 0.0.0.0).
    cache.insert("nxdomain.test", Ipv4Addr::UNSPECIFIED, 60, now);

    match cache.lookup("nxdomain.test", now) {
        Some(None) => {} // Negative hit — correct.
        Some(Some(ip)) => {
            crate::serial_println!("[dns]   FAIL: negative entry returned positive IP {}", ip);
            return Err(KernelError::InternalError);
        }
        None => {
            crate::serial_println!("[dns]   FAIL: negative entry not found");
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[dns]   cache negative entry: OK");
    Ok(())
}

/// Test AAAA query building produces correct structure.
#[allow(clippy::arithmetic_side_effects)]
fn test_build_aaaa_query_structure() -> KernelResult<()> {
    let query = build_aaaa_query("test.dev", 0x5678);

    // Minimum: 12 header + name + 4 (qtype+qclass).
    if query.len() < 12 + 4 {
        crate::serial_println!("[dns]   FAIL: AAAA query too short ({})", query.len());
        return Err(KernelError::InternalError);
    }

    // Transaction ID.
    let id = u16::from_be_bytes([query[0], query[1]]);
    if id != 0x5678 {
        crate::serial_println!("[dns]   FAIL: AAAA query ID = {:#06x}", id);
        return Err(KernelError::InternalError);
    }

    // QTYPE at end-4 should be AAAA (28).
    let qtype = u16::from_be_bytes([query[query.len() - 4], query[query.len() - 3]]);
    if qtype != TYPE_AAAA {
        crate::serial_println!("[dns]   FAIL: QTYPE = {} (expected {})", qtype, TYPE_AAAA);
        return Err(KernelError::InternalError);
    }

    // QCLASS at end-2 should be IN (1).
    let qclass = u16::from_be_bytes([query[query.len() - 2], query[query.len() - 1]]);
    if qclass != CLASS_IN {
        crate::serial_println!("[dns]   FAIL: QCLASS = {}", qclass);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   build AAAA query: OK");
    Ok(())
}

/// Test AAAA response parsing with a hand-crafted response.
#[allow(clippy::arithmetic_side_effects)]
fn test_parse_aaaa_response() -> KernelResult<()> {
    // Build a synthetic DNS response for "test.dev" → 2001:db8::1.
    let query_id: u16 = 0xBEEF;
    let mut resp = Vec::new();

    // Header.
    resp.extend_from_slice(&query_id.to_be_bytes());       // ID.
    resp.extend_from_slice(&0x8180u16.to_be_bytes());      // Flags: QR=1, RD=1, RA=1.
    resp.extend_from_slice(&1u16.to_be_bytes());            // QDCOUNT = 1.
    resp.extend_from_slice(&1u16.to_be_bytes());            // ANCOUNT = 1.
    resp.extend_from_slice(&0u16.to_be_bytes());            // NSCOUNT = 0.
    resp.extend_from_slice(&0u16.to_be_bytes());            // ARCOUNT = 0.

    // Question section: "test.dev" IN AAAA.
    encode_name(&mut resp, "test.dev");
    resp.extend_from_slice(&TYPE_AAAA.to_be_bytes());
    resp.extend_from_slice(&CLASS_IN.to_be_bytes());

    // Answer section: test.dev AAAA 600 2001:db8::1.
    encode_name(&mut resp, "test.dev");
    resp.extend_from_slice(&TYPE_AAAA.to_be_bytes());      // TYPE.
    resp.extend_from_slice(&CLASS_IN.to_be_bytes());        // CLASS.
    resp.extend_from_slice(&600u32.to_be_bytes());          // TTL = 600s.
    resp.extend_from_slice(&16u16.to_be_bytes());           // RDLENGTH = 16.
    // 2001:0db8::1 = 2001:0db8:0000:0000:0000:0000:0000:0001
    resp.extend_from_slice(&[
        0x20, 0x01, 0x0d, 0xb8,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01,
    ]);

    let expected_ip = Ipv6Addr([
        0x20, 0x01, 0x0d, 0xb8,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01,
    ]);

    let mut cname_out = None;
    let result = parse_aaaa_response(&resp, query_id, &mut cname_out)?;

    if result.ip != expected_ip {
        crate::serial_println!("[dns]   FAIL: parsed AAAA IP = {}", result.ip);
        return Err(KernelError::InternalError);
    }
    if result.ttl_secs != 600 {
        crate::serial_println!("[dns]   FAIL: parsed AAAA TTL = {}", result.ttl_secs);
        return Err(KernelError::InternalError);
    }

    // Wrong query ID should be rejected.
    let mut cname_out2 = None;
    if parse_aaaa_response(&resp, 0x1111, &mut cname_out2).is_ok() {
        crate::serial_println!("[dns]   FAIL: AAAA accepted wrong query ID");
        return Err(KernelError::InternalError);
    }

    // Too-short response should be rejected.
    let mut cname_out3 = None;
    if parse_aaaa_response(&resp[..8], query_id, &mut cname_out3).is_ok() {
        crate::serial_println!("[dns]   FAIL: AAAA accepted too-short response");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   parse AAAA response: OK");
    Ok(())
}

/// Test AAAA cache insert/lookup/negative.
fn test_aaaa_cache() -> KernelResult<()> {
    let mut cache = DnsAaaaCache::new();
    let now = crate::hrtimer::now_ns();
    let ip = Ipv6Addr([
        0x20, 0x01, 0x0d, 0xb8,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01,
    ]);

    // Insert and lookup.
    cache.insert("v6.example", ip, 120, now);

    match cache.lookup("v6.example", now) {
        Some(Some(found)) if found == ip => {} // OK.
        Some(Some(found)) => {
            crate::serial_println!("[dns]   FAIL: AAAA cache lookup = {}", found);
            return Err(KernelError::InternalError);
        }
        Some(None) => {
            crate::serial_println!("[dns]   FAIL: AAAA cache returned negative for positive");
            return Err(KernelError::InternalError);
        }
        None => {
            crate::serial_println!("[dns]   FAIL: AAAA cache miss after insert");
            return Err(KernelError::InternalError);
        }
    }

    // Case-insensitive lookup.
    match cache.lookup("V6.EXAMPLE", now) {
        Some(Some(found)) if found == ip => {} // OK.
        _ => {
            crate::serial_println!("[dns]   FAIL: AAAA case-insensitive lookup");
            return Err(KernelError::InternalError);
        }
    }

    // Negative entry.
    cache.insert("nx.v6.test", Ipv6Addr::UNSPECIFIED, 60, now);
    match cache.lookup("nx.v6.test", now) {
        Some(None) => {} // Correct — negative hit.
        _ => {
            crate::serial_println!("[dns]   FAIL: AAAA negative entry");
            return Err(KernelError::InternalError);
        }
    }

    // Miss for unknown name.
    if cache.lookup("unknown.v6", now).is_some() {
        crate::serial_println!("[dns]   FAIL: AAAA found nonexistent entry");
        return Err(KernelError::InternalError);
    }

    // Expired entry.
    let far_future = now.wrapping_add(1_000_000_000_000);
    if cache.lookup("v6.example", far_future).is_some() {
        crate::serial_println!("[dns]   FAIL: AAAA expired entry still returned");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   AAAA cache: OK");
    Ok(())
}

/// Test IPv6 → ip6.arpa reverse name generation.
fn test_ipv6_reverse_name() -> KernelResult<()> {
    // 2001:0db8::1 = 2001:0db8:0000:0000:0000:0000:0000:0001
    let ip = Ipv6Addr([
        0x20, 0x01, 0x0d, 0xb8,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01,
    ]);
    let arpa = ipv6_to_ip6_arpa(&ip);

    // Expected: 1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa
    let expected = "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa";
    if arpa != expected {
        crate::serial_println!("[dns]   FAIL: ip6.arpa = '{}'", arpa);
        crate::serial_println!("[dns]   expected:         '{}'", expected);
        return Err(KernelError::InternalError);
    }

    // Loopback (::1) should produce all zeros except the last nibble.
    let lo = Ipv6Addr::LOOPBACK;
    let lo_arpa = ipv6_to_ip6_arpa(&lo);
    if !lo_arpa.ends_with("ip6.arpa") {
        crate::serial_println!("[dns]   FAIL: loopback ip6.arpa missing suffix");
        return Err(KernelError::InternalError);
    }
    if !lo_arpa.starts_with("1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0") {
        crate::serial_println!("[dns]   FAIL: loopback ip6.arpa prefix wrong");
        return Err(KernelError::InternalError);
    }

    // Unspecified (::) should be all zeros.
    let zero = Ipv6Addr::UNSPECIFIED;
    let zero_arpa = ipv6_to_ip6_arpa(&zero);
    let expected_zero = "0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.ip6.arpa";
    if zero_arpa != expected_zero {
        crate::serial_println!("[dns]   FAIL: :: ip6.arpa = '{}'", zero_arpa);
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[dns]   IPv6 reverse name: OK");
    Ok(())
}

/// Test namespace-aware DNS server selection.
///
/// Verifies that:
/// - `pick_dns_server_for_ns(ROOT_NS)` falls back to root DNS.
/// - `pick_dns_server_for_ns(nonexistent)` falls back to root DNS.
/// - `pick_dns_server_for_ns(ns_with_dns)` returns that namespace's DNS (if netns initialized).
fn test_ns_aware_dns_picker() -> KernelResult<()> {
    use crate::netns;

    // Root NS should always fall back to the global DNS configuration.
    // (May be Ok or Err depending on whether DHCP ran, but must not panic.)
    let root_direct = pick_dns_server_root();
    let root_via_ns = pick_dns_server_for_ns(netns::ROOT_NS);
    match (&root_direct, &root_via_ns) {
        (Ok(DnsServer::V4(a)), Ok(DnsServer::V4(b))) => {
            if a.0 != b.0 {
                crate::serial_println!("[dns]   FAIL: ROOT_NS DNS mismatch");
                return Err(KernelError::InternalError);
            }
        }
        (Ok(DnsServer::V6(a)), Ok(DnsServer::V6(b))) => {
            if a.0 != b.0 {
                crate::serial_println!("[dns]   FAIL: ROOT_NS DNS6 mismatch");
                return Err(KernelError::InternalError);
            }
        }
        (Err(_), Err(_)) => { /* Both not configured — OK. */ }
        _ => {
            crate::serial_println!("[dns]   FAIL: ROOT_NS DNS type mismatch");
            return Err(KernelError::InternalError);
        }
    }

    // Non-existent namespace (when netns not initialized) should fall back to root.
    let bad_ns_result = pick_dns_server_for_ns(255);
    match (&root_direct, &bad_ns_result) {
        (Ok(_), Ok(_)) | (Err(_), Err(_)) => { /* Same fallback behaviour. */ }
        _ => {
            crate::serial_println!("[dns]   FAIL: bad NS should fallback to root");
            return Err(KernelError::InternalError);
        }
    }

    // Test with actual netns subsystem only if it's initialized.
    if netns::is_initialized() {
        if let Ok(ns_id) = netns::create() {
            let custom_dns = netns::Ipv4Addr::new(1, 2, 3, 4);
            let ip = netns::Ipv4Addr::new(10, 200, 0, 2);
            let mask = netns::Ipv4Addr::new(255, 255, 255, 0);
            let gw = netns::Ipv4Addr::new(10, 200, 0, 1);
            let _ = netns::configure_interface(ns_id, ip, mask, gw, custom_dns);

            match pick_dns_server_for_ns(ns_id) {
                Ok(DnsServer::V4(dns_ip)) => {
                    if dns_ip.0 != [1, 2, 3, 4] {
                        crate::serial_println!(
                            "[dns]   FAIL: NS DNS = {}, expected 1.2.3.4",
                            dns_ip
                        );
                        let _ = netns::delete(ns_id);
                        return Err(KernelError::InternalError);
                    }
                }
                other => {
                    crate::serial_println!("[dns]   FAIL: NS DNS picker = {:?}", other);
                    let _ = netns::delete(ns_id);
                    return Err(KernelError::InternalError);
                }
            }

            let _ = netns::delete(ns_id);
        }
    }

    crate::serial_println!("[dns]   Namespace-aware DNS picker: OK");
    Ok(())
}
