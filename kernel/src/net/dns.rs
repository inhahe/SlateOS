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
//! ## Protocol overview
//!
//! DNS uses UDP port 53.  A query contains a question section with
//! the domain name and record type.  The server responds with answer
//! records containing the resolved IP addresses.
//!
//! ## Limitations
//!
//! - Only supports A records (IPv4 addresses).
//! - No CNAME chasing (returns first A record found).
//! - No EDNS0 or DNSSEC.

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

use super::interface::{self, Ipv4Addr};

// ---------------------------------------------------------------------------
// DNS constants
// ---------------------------------------------------------------------------

/// DNS server port.
const DNS_PORT: u16 = 53;

/// DNS record type: A (IPv4 address).
const TYPE_A: u16 = 1;
/// DNS record class: IN (Internet).
const CLASS_IN: u16 = 1;

/// DNS flags: standard query, recursion desired.
const FLAGS_QUERY_RD: u16 = 0x0100;

/// Transaction ID for our queries.
const QUERY_ID: u16 = 0xBEEF;

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
    /// Returns the cached IP if the entry exists and hasn't expired.
    fn lookup(&self, name: &str, now_ns: u64) -> Option<Ipv4Addr> {
        let name_bytes = name.as_bytes();
        for entry in &self.entries {
            if entry.name_len == name_bytes.len()
                && entry.is_valid(now_ns)
                && Self::names_match(&entry.name, entry.name_len, name_bytes)
            {
                return Some(entry.ip);
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
/// Returns the raw UDP payload.
#[allow(clippy::arithmetic_side_effects)]
fn build_query(name: &str) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // Header (12 bytes).
    pkt.extend_from_slice(&QUERY_ID.to_be_bytes());   // ID.
    pkt.extend_from_slice(&FLAGS_QUERY_RD.to_be_bytes()); // Flags.
    pkt.extend_from_slice(&1u16.to_be_bytes());         // QDCOUNT = 1.
    pkt.extend_from_slice(&0u16.to_be_bytes());         // ANCOUNT = 0.
    pkt.extend_from_slice(&0u16.to_be_bytes());         // NSCOUNT = 0.
    pkt.extend_from_slice(&0u16.to_be_bytes());         // ARCOUNT = 0.

    // Question section: encode domain name as labels.
    encode_name(&mut pkt, name);

    // Type: A.
    pkt.extend_from_slice(&TYPE_A.to_be_bytes());
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
#[allow(clippy::arithmetic_side_effects)]
fn parse_response(data: &[u8]) -> KernelResult<DnsResult> {
    if data.len() < 12 {
        return Err(KernelError::InvalidArgument);
    }

    let id = u16::from_be_bytes([data[0], data[1]]);
    if id != QUERY_ID {
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

    // Parse answer records, looking for the first A record.
    for _ in 0..ancount {
        if offset >= data.len() {
            break;
        }

        offset = skip_name(data, offset)?;

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

        if rtype == TYPE_A && rclass == CLASS_IN && rdlength == 4 {
            let mut ip = [0u8; 4];
            ip.copy_from_slice(&data[offset..offset + 4]);
            return Ok(DnsResult { ip: Ipv4Addr(ip), ttl_secs: ttl });
        }

        offset = rd_end;
    }

    Err(KernelError::NotFound)
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
/// (blocking with polling, up to ~2 seconds).  Successful results are
/// cached with the TTL from the DNS response.
#[allow(clippy::arithmetic_side_effects)]
pub fn resolve(name: &str) -> KernelResult<Ipv4Addr> {
    let now_ns = crate::hrtimer::now_ns();

    // Check cache first.
    if let Some(ip) = DNS_CACHE.lock().lookup(name, now_ns) {
        crate::serial_println!("[dns] Cache hit: '{}' → {}", name, ip);
        return Ok(ip);
    }

    let dns_server = interface::info().dns;
    if dns_server.is_unspecified() {
        crate::serial_println!("[dns] No DNS server configured");
        return Err(KernelError::NotSupported);
    }

    crate::serial_println!("[dns] Resolving '{}' via {}...", name, dns_server);

    // Build and send the query.
    let query = build_query(name);

    // Use a local port for the DNS query.
    let local_port: u16 = 49152; // Ephemeral port.

    // Bind a UDP socket to receive the reply.
    let sock = super::udp::bind(local_port)?;

    // Send the query.
    if let Err(e) = super::udp::send(local_port, dns_server, DNS_PORT, &query) {
        super::udp::close(sock);
        return Err(e);
    }

    // Poll for response (up to ~2 seconds).
    for _ in 0..2000 {
        // Poll the NIC.
        super::poll();

        // Check for a response.
        if let Some(dgram) = super::udp::recv(sock) {
            super::udp::close(sock);
            match parse_response(&dgram.data) {
                Ok(result) => {
                    crate::serial_println!(
                        "[dns] Resolved '{}' → {} (TTL {}s)",
                        name, result.ip, result.ttl_secs
                    );
                    // Cache the result.
                    let cache_now = crate::hrtimer::now_ns();
                    DNS_CACHE.lock().insert(name, result.ip, result.ttl_secs, cache_now);
                    return Ok(result.ip);
                }
                Err(e) => {
                    crate::serial_println!("[dns] Parse error: {:?}", e);
                    return Err(e);
                }
            }
        }

        // Brief spin delay.
        for _ in 0..10_000 {
            core::hint::spin_loop();
        }
    }

    super::udp::close(sock);
    crate::serial_println!("[dns] Resolution timed out for '{}'", name);
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
