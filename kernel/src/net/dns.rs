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
//! ## Limitations
//!
//! - Forward resolution only supports A records (IPv4 addresses).
//! - CNAME chasing limited to 8 hops.
//! - No EDNS0 or DNSSEC.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

use super::interface::{self, Ipv4Addr};

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
/// DNS record class: IN (Internet).
const CLASS_IN: u16 = 1;

/// DNS flags: standard query, recursion desired.
const FLAGS_QUERY_RD: u16 = 0x0100;

/// Monotonically incrementing transaction ID for DNS queries.
///
/// Using a unique ID per query prevents spoofed responses from matching
/// a different query's transaction ID.  Starts at 1 (0 is reserved by
/// some implementations as invalid).
static NEXT_QUERY_ID: AtomicU16 = AtomicU16::new(1);

/// Allocate the next unique transaction ID.
fn next_query_id() -> u16 {
    let id = NEXT_QUERY_ID.fetch_add(1, Ordering::Relaxed);
    // Skip 0 on wrap-around (some resolvers treat 0 as invalid).
    if id == 0 {
        NEXT_QUERY_ID.fetch_add(1, Ordering::Relaxed)
    } else {
        id
    }
}

/// Ephemeral port counter for DNS queries.
///
/// Each query binds a unique local port to avoid collisions when
/// multiple resolutions are in flight (e.g., during CNAME chasing).
/// Range: 49152–65535 (IANA dynamic/private port range).
static NEXT_DNS_PORT: AtomicU16 = AtomicU16::new(49152);

/// Allocate the next ephemeral port for DNS.
fn next_dns_port() -> u16 {
    let port = NEXT_DNS_PORT.fetch_add(1, Ordering::Relaxed);
    // Wrap back to start of ephemeral range.
    if port == 0 || port < 49152 {
        NEXT_DNS_PORT.store(49153, Ordering::Relaxed);
        49152
    } else {
        port
    }
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
                if a.to_ascii_lowercase() != b.to_ascii_lowercase() {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }
}

/// Global DNS cache.
static DNS_CACHE: Mutex<DnsCache> = Mutex::new(DnsCache::new());

// ---------------------------------------------------------------------------
// DNS packet building
// ---------------------------------------------------------------------------

/// Build a DNS query packet for an A record.
///
/// `query_id` is the transaction ID for this query — used to match
/// the response and prevent spoofed replies.
///
/// Returns the raw UDP payload.
#[allow(clippy::arithmetic_side_effects)]
fn build_query(name: &str, query_id: u16) -> Vec<u8> {
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

    // Type: A.
    pkt.extend_from_slice(&TYPE_A.to_be_bytes());
    // Class: IN.
    pkt.extend_from_slice(&CLASS_IN.to_be_bytes());

    pkt
}

/// Build a DNS query packet for a PTR record (reverse DNS).
///
/// Converts the IP address to the `in-addr.arpa` domain format
/// (e.g., `192.168.1.1` → `1.1.168.192.in-addr.arpa`) and queries
/// for a PTR record.
#[allow(clippy::arithmetic_side_effects)]
fn build_ptr_query(ip: Ipv4Addr, query_id: u16) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // Header (12 bytes).
    pkt.extend_from_slice(&query_id.to_be_bytes());
    pkt.extend_from_slice(&FLAGS_QUERY_RD.to_be_bytes());
    pkt.extend_from_slice(&1u16.to_be_bytes());           // QDCOUNT = 1.
    pkt.extend_from_slice(&0u16.to_be_bytes());           // ANCOUNT = 0.
    pkt.extend_from_slice(&0u16.to_be_bytes());           // NSCOUNT = 0.
    pkt.extend_from_slice(&0u16.to_be_bytes());           // ARCOUNT = 0.

    // Build reverse name: octets in reverse order + "in-addr.arpa".
    // E.g., 192.168.1.1 → "1.1.168.192.in-addr.arpa"
    let arpa_name = alloc::format!(
        "{}.{}.{}.{}.in-addr.arpa",
        ip.0[3], ip.0[2], ip.0[1], ip.0[0]
    );
    encode_name(&mut pkt, &arpa_name);

    // Type: PTR.
    pkt.extend_from_slice(&TYPE_PTR.to_be_bytes());
    // Class: IN.
    pkt.extend_from_slice(&CLASS_IN.to_be_bytes());

    pkt
}

/// Encode a domain name as DNS wire-format labels.
///
/// E.g., `"example.com"` → `\x07example\x03com\x00`.
fn encode_name(pkt: &mut Vec<u8>, name: &str) {
    for label in name.split('.') {
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
fn parse_response(data: &[u8], expected_id: u16) -> KernelResult<DnsResult> {
    parse_response_inner(data, expected_id, None)
}

/// Inner response parser that can optionally look for an A record
/// matching a specific name (used during CNAME resolution).
#[allow(clippy::arithmetic_side_effects)]
fn parse_response_inner(
    data: &[u8],
    expected_id: u16,
    target_name: Option<&str>,
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
        offset += 4; // QTYPE + QCLASS.
    }

    // First pass: collect CNAME mappings and find A records.
    // We store up to MAX_CNAME_HOPS CNAME targets and check for A records.
    let mut cname_target: Option<String> = None;
    let mut a_results: Vec<(String, Ipv4Addr, u32)> = Vec::new();
    let answer_start = offset;

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
    // send a follow-up query.  Return NotFound — the caller checks
    // `last_cname_target()` for follow-up.
    if cname_target.is_some() {
        // Store the CNAME target for the caller to re-query.
        *LAST_CNAME.lock() = cname_target;
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
fn names_eq_case_insensitive(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

/// Storage for the last CNAME target from a response that had no A record.
///
/// Used by `resolve()` to send follow-up queries for CNAME chains
/// that span multiple DNS responses.
static LAST_CNAME: Mutex<Option<String>> = Mutex::new(None);

/// Decode a DNS name at the given offset into a dotted string.
///
/// Handles compression pointers (RFC 1035 §4.1.4).  Returns the
/// decoded name and the offset in `data` after the name encoding
/// (following the first occurrence, not following pointers).
#[allow(clippy::arithmetic_side_effects)]
fn decode_name(data: &[u8], mut offset: usize) -> KernelResult<(String, usize)> {
    let mut name = String::with_capacity(64);
    let mut jumped = false;
    let mut jump_return: usize = 0;
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

        if len & 0xC0 == 0xC0 {
            // Compression pointer — two bytes encode a 14-bit offset.
            if ptr_offset + 1 >= data.len() {
                return Err(KernelError::InvalidArgument);
            }
            if !jumped {
                offset = ptr_offset + 2;
                jumped = true;
            }
            let _ = jump_return; // Silence unused warning.
            jump_return = ptr_offset + 2;
            ptr_offset = (usize::from(len & 0x3F) << 8) | usize::from(data[ptr_offset + 1]);
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
/// Returns the offset after the name.
#[allow(clippy::arithmetic_side_effects)]
fn skip_name(data: &[u8], mut offset: usize) -> KernelResult<usize> {
    let mut jumped = false;
    let mut steps = 0;

    loop {
        if offset >= data.len() || steps > 128 {
            return Err(KernelError::InvalidArgument);
        }
        steps += 1;

        let len = data[offset];
        if len == 0 {
            if !jumped {
                offset += 1;
            }
            return Ok(offset);
        }

        if len & 0xC0 == 0xC0 {
            // Compression pointer.
            if !jumped {
                offset += 2; // Skip the 2-byte pointer.
                jumped = true;
            }
            // Follow the pointer for further label traversal,
            // but we only care about advancing the offset past
            // the name in the original data.
            if !jumped {
                return Ok(offset);
            }
            // For skip_name, we just need to advance past the pointer.
            return Ok(offset);
        }

        if !jumped {
            offset += 1 + len as usize;
        } else {
            break;
        }
    }

    Ok(offset)
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
    let mut current_name = String::from(name);

    for hop in 0..MAX_CNAME_HOPS {
        match resolve_single(&current_name) {
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
                // Check if parse_response stored a CNAME target to follow.
                let cname = LAST_CNAME.lock().take();
                if let Some(target) = cname {
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

/// Resolve a single name (with retry on timeout).
///
/// Sends a DNS A record query and waits for a response.  On timeout,
/// retransmits the query with an increasing wait window (1s → 2s → 4s).
/// Returns `Ok(ip)` on success, `Err(NotFound)` if no A record was
/// found (check `LAST_CNAME` for CNAME follow-up), or another error.
#[allow(clippy::arithmetic_side_effects)]
fn resolve_single(name: &str) -> KernelResult<Ipv4Addr> {
    let now_ns = crate::hrtimer::now_ns();

    // Check cache first (positive and negative entries).
    match DNS_CACHE.lock().lookup(name, now_ns) {
        Some(Some(ip)) => {
            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!("[dns] Cache hit: '{}' → {}", name, ip);
            return Ok(ip);
        }
        Some(None) => {
            // Negative cache hit — name was recently queried and not found.
            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!("[dns] Negative cache hit: '{}'", name);
            return Err(KernelError::NotFound);
        }
        None => {
            CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
        }
    }

    let dns_server = interface::info().dns;
    if dns_server.is_unspecified() {
        crate::serial_println!("[dns] No DNS server configured");
        return Err(KernelError::NotSupported);
    }

    crate::serial_println!("[dns] Resolving '{}' via {}...", name, dns_server);

    // Clear any previous CNAME target.
    *LAST_CNAME.lock() = None;

    // Use a unique transaction ID and ephemeral port per query.
    let query_id = next_query_id();
    let local_port = next_dns_port();

    // Build the query once — retransmits send the same bytes.
    let query = build_query(name, query_id);

    // Bind a UDP socket to receive the reply.
    let sock = super::udp::bind(local_port)?;

    // Retry loop with increasing timeouts.
    for attempt in 0..MAX_DNS_ATTEMPTS {
        // Send (or re-send) the query.
        if let Err(e) = super::udp::send(local_port, dns_server, DNS_PORT, &query) {
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

        // Poll for response.
        for _ in 0..polls {
            super::poll();

            if let Some(dgram) = super::udp::recv(sock) {
                // Validate source: must be from our DNS server on port 53.
                if dgram.src_ip != dns_server || dgram.src_port != DNS_PORT {
                    continue;
                }
                super::udp::close(sock);
                match parse_response(&dgram.data, query_id) {
                    Ok(result) => {
                        crate::serial_println!(
                            "[dns] Resolved '{}' → {} (TTL {}s)",
                            name, result.ip, result.ttl_secs
                        );
                        let cache_now = crate::hrtimer::now_ns();
                        DNS_CACHE.lock().insert(name, result.ip, result.ttl_secs, cache_now);
                        return Ok(result.ip);
                    }
                    Err(KernelError::NotFound) => {
                        // CNAME-only or NXDOMAIN — don't retry, it's a
                        // definitive answer from the server.
                        if LAST_CNAME.lock().is_none() {
                            let cache_now = crate::hrtimer::now_ns();
                            DNS_CACHE.lock().insert(
                                name,
                                Ipv4Addr::UNSPECIFIED,
                                NEGATIVE_CACHE_TTL,
                                cache_now,
                            );
                        }
                        return Err(KernelError::NotFound);
                    }
                    Err(e) => {
                        crate::serial_println!("[dns] Parse error: {:?}", e);
                        return Err(e);
                    }
                }
            }

            // Brief spin delay (~1ms per iteration).
            for _ in 0..10_000 {
                core::hint::spin_loop();
            }
        }

        // This attempt timed out — retry (unless last attempt).
    }

    super::udp::close(sock);
    crate::serial_println!(
        "[dns] Resolution timed out for '{}' after {} attempts",
        name, MAX_DNS_ATTEMPTS
    );
    Err(KernelError::TimedOut)
}

/// Flush the entire DNS cache.
///
/// Called when the network configuration changes (e.g., DHCP renewal
/// with a new DNS server) to avoid stale cached results.
pub fn flush_cache() {
    let mut cache = DNS_CACHE.lock();
    *cache = DnsCache::new();
    crate::serial_println!("[dns] Cache flushed");
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
#[allow(clippy::arithmetic_side_effects)]
pub fn reverse_resolve(ip: Ipv4Addr) -> KernelResult<String> {
    let dns_server = interface::info().dns;
    if dns_server.is_unspecified() {
        crate::serial_println!("[dns] No DNS server configured");
        return Err(KernelError::NotSupported);
    }

    crate::serial_println!("[dns] Reverse resolving {}...", ip);

    let query_id = next_query_id();
    let local_port = next_dns_port();
    let query = build_ptr_query(ip, query_id);

    // Bind a UDP socket to receive the reply.
    let sock = super::udp::bind(local_port)?;

    // Retry loop with increasing timeouts (same as forward resolution).
    for attempt in 0..MAX_DNS_ATTEMPTS {
        if let Err(e) = super::udp::send(local_port, dns_server, DNS_PORT, &query) {
            super::udp::close(sock);
            return Err(e);
        }

        if attempt > 0 {
            crate::serial_println!(
                "[dns] PTR retry {} for {} (timeout {}ms)",
                attempt, ip,
                DNS_ATTEMPT_POLLS.get(attempt).copied().unwrap_or(2000)
            );
        }

        let polls = DNS_ATTEMPT_POLLS.get(attempt).copied().unwrap_or(2000);

        for _ in 0..polls {
            super::poll();

            if let Some(dgram) = super::udp::recv(sock) {
                // Validate source: must be from our DNS server on port 53.
                if dgram.src_ip != dns_server || dgram.src_port != DNS_PORT {
                    continue;
                }
                super::udp::close(sock);
                match parse_ptr_response(&dgram.data, query_id) {
                    Ok(name) => {
                        crate::serial_println!(
                            "[dns] Reverse resolved {} → '{}'",
                            ip, name
                        );
                        return Ok(name);
                    }
                    Err(KernelError::NotFound) => {
                        // Definitive answer — no PTR record.
                        crate::serial_println!(
                            "[dns] No PTR record for {}",
                            ip
                        );
                        return Err(KernelError::NotFound);
                    }
                    Err(e) => {
                        crate::serial_println!("[dns] PTR parse error: {:?}", e);
                        return Err(e);
                    }
                }
            }

            // Brief spin delay (~1ms per iteration).
            for _ in 0..10_000 {
                core::hint::spin_loop();
            }
        }
    }

    super::udp::close(sock);
    crate::serial_println!(
        "[dns] PTR resolution timed out for {} after {} attempts",
        ip, MAX_DNS_ATTEMPTS
    );
    Err(KernelError::TimedOut)
}
