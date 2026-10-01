//! DNS resolver (RFC 1035) with result caching.
//!
//! Simple recursive DNS stub resolver that sends queries to a
//! configured DNS server and parses A record responses.
//!
//! ## Answers
//!
//! [`lookup`] answers what a DNS answer carries, as glibc's resolver does:
//! every address of the kinds asked (A, AAAA, or both), the name at the end
//! of the CNAME chain, and -- when there is none -- why: the name does not
//! exist (NXDOMAIN, `NotFound`), it has no address of that kind (NODATA,
//! `NoAddress`), or no answer came (`TimedOut`, `WouldBlock` for SERVFAIL,
//! `ConnectionRefused`). [`resolve`] and [`resolve6`] are its first IPv4 and
//! IPv6 address. Until 2026-10-01 they were all there was, one address each,
//! and "no such name" and "no address" were one `NotFound` (lane D's request
//! `d-a-sys-dns-resolve-answers-one-ipv4-address`).
//!
//! The kernel's hosts table (`fs::nameservice`, `127.0.0.1 localhost` and
//! `::1 localhost` by default) and a container's peers are asked before the
//! network.
//!
//! ## DNS Cache
//!
//! Whole answers are cached, per name and record type, with the TTL of the
//! response (clamped to 60 s .. 1 h). A negative answer -- NXDOMAIN or
//! NODATA -- is cached for 60 seconds, as itself, so a name that does not
//! exist and one with no address of a kind stay told apart. The cache holds
//! 64 answers; when full, an expired one is replaced, else the one expiring
//! first. A timeout or a SERVFAIL is not cached: the next caller asks again.
//!
//! ## Protocol overview
//!
//! DNS uses UDP port 53.  A query contains a question section with
//! the domain name and record type.  The server responds with answer
//! records containing the resolved IP addresses.
//!
//! Each query carries a transaction ID and is sent from an ephemeral
//! source port (49152–65535).  An off-path attacker forging a response
//! has to guess both, so both are drawn from the kernel CSPRNG, giving
//! the ~30 bits RFC 5452 §9 asks for; a value that merely does not repeat
//! — a counter, say — is not the same as one he cannot predict.  The port
//! is assigned by `udp::bind`, which because it holds the socket table
//! also guarantees it is free rather than merely unlikely to collide.
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
//! - No EDNS0 or DNSSEC, and no TCP: a response truncated to 512 bytes
//!   (TC set) is read for what it holds.
//!
//! ## IPv6 support (AAAA records)
//!
//! AAAA records (RFC 3596) are asked as A records are, into the same cache.
//! [`reverse_resolve6`] queries PTR records in the `ip6.arpa` domain for
//! IPv6 reverse DNS.

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use crate::sync::Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    let entries = DNS_CACHE.lock().count(None, crate::hrtimer::now_ns());
    DnsCacheStats {
        hits: CACHE_HITS.load(Ordering::Relaxed),
        misses: CACHE_MISSES.load(Ordering::Relaxed),
        evictions: CACHE_EVICTIONS.load(Ordering::Relaxed),
        entries,
        capacity: CACHE_SIZE,
    }
}

// ---------------------------------------------------------------------------
// What a lookup answers
// ---------------------------------------------------------------------------

/// An address a name resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    /// From an A record.
    V4(Ipv4Addr),
    /// From an AAAA record.
    V6(Ipv6Addr),
}

/// Which addresses a [`lookup`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// IPv6 and IPv4: AAAA and A records (`AF_UNSPEC`).
    Any,
    /// IPv4: A records (`AF_INET`).
    V4,
    /// IPv6: AAAA records (`AF_INET6`).
    V6,
}

/// What a [`lookup`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// The name at the end of the CNAME chain, as the server spelled it --
    /// `getaddrinfo`'s `AI_CANONNAME` -- or the name asked, when there was no
    /// chain.
    pub canonical: String,
    /// Every address, in the order the answer gave them: IPv6 before IPv4
    /// when both were asked. Never empty. Sorting them for a connection (RFC
    /// 6724) is the caller's, as glibc's `getaddrinfo` sorts its own.
    pub addrs: Vec<Address>,
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

/// Generate a randomized DNS transaction ID.
///
/// Drawn from the kernel CSPRNG.  The transaction ID is one of the two
/// values an off-path attacker must guess to forge a response — the source
/// port is the other — so predicting it is the classic cache-poisoning
/// vector (CVE-2008-1447 / Kaminsky).  Together with the random source
/// port from `udp::bind` this gives the ~30 bits RFC 5452 §9 asks for.
///
/// This previously mixed a monotonic counter with `rdtsc`.  That is not a
/// CSPRNG: the counter contributes no entropy at all, and `rdtsc` is a
/// value an attacker who can run code on the machine — or merely time it
/// well — can narrow considerably.  Using the real generator costs
/// nothing here, since resolution already blocks on the network and the
/// RNG is callable from any thread context.
///
/// Never returns 0, which some resolvers treat as invalid.  The old
/// version tried to guarantee this by falling back to `counter + 1`, but
/// that is itself 0 when the counter sits at `u16::MAX`; redrawing has no
/// such edge, and terminates with probability 1.
fn next_query_id() -> u16 {
    /// Size of the 16-bit transaction ID space, i.e. one past `u16::MAX`.
    const ID_SPACE: u64 = 1 << 16;

    loop {
        // Drawn in the range 1..=u16::MAX by rejecting 0 rather than by
        // biasing it away (e.g. `1 + rand % 65535`), so every valid ID
        // stays equally likely.
        let id = crate::rng::next_bounded(ID_SPACE);
        if id != 0 {
            // Fits by construction: the bound is 65536.
            return u16::try_from(id).unwrap_or(1);
        }
    }
}

// The source port for a query is no longer chosen here.  `dns_query_raw`
// asks `udp::bind` for an ephemeral port and reads back what it was given;
// see the note on `udp::random_ephemeral_offset` for why the allocator now
// draws its start from the CSPRNG.
//
// This module used to pick its own port and bind it explicitly, because
// the allocator handed out 49152, 49153, ... in order and a predictable
// source port defeats cache-poisoning resistance.  That workaround had a
// defect of its own: an explicitly-bound port that is already taken makes
// `bind` return `AlreadyExists`, which failed the whole lookup, and the
// retry loop sits after the bind so it could not recover.  Asking the
// allocator instead removes the collision by construction -- it holds the
// socket table, so it only ever returns a port it has checked is free.

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

/// Maximum number of cached answers: one per name and record type, A and
/// AAAA alike.
const CACHE_SIZE: usize = 64;

/// What the cache keeps for a name and record type: the answer, or the
/// negative one -- `NotFound` for NXDOMAIN, `NoAddress` for NODATA.
type CachedOutcome = Result<Lookup, KernelError>;

/// One cached answer.
struct CacheEntry {
    /// The name asked, lowercased: DNS names compare without case.
    name: String,
    /// `TYPE_A` or `TYPE_AAAA`.
    qtype: u16,
    /// The answer.
    outcome: CachedOutcome,
    /// Absolute expiration time in nanoseconds (monotonic clock).
    expires_ns: u64,
}

/// The resolver's cache: whole answers -- every address and the canonical
/// name -- per name and record type, positive and negative.
///
/// It replaced, on 2026-10-01, an A cache and an AAAA cache that kept one
/// address a name and the address 0.0.0.0 for "not found", which could not
/// say whether a name was missing or merely had no address of the kind.
struct DnsCache {
    entries: Vec<CacheEntry>,
}

impl DnsCache {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// The live outcome cached for `name`'s `qtype` records, if any.
    fn get(&self, name: &str, qtype: u16, now_ns: u64) -> Option<CachedOutcome> {
        self.entries
            .iter()
            .find(|e| {
                e.qtype == qtype
                    && now_ns < e.expires_ns
                    && names_eq_case_insensitive(&e.name, name)
            })
            .map(|e| e.outcome.clone())
    }

    /// Keep `outcome` for `name`'s `qtype` records. An answer for its TTL,
    /// clamped to at least 60 s, so a name is not asked again at once, and at
    /// most an hour, as stale data is worse than a query; a negative one for
    /// [`NEGATIVE_CACHE_TTL`]. When full, an expired entry goes, else the one
    /// expiring first.
    fn put(&mut self, name: &str, qtype: u16, outcome: CachedOutcome, ttl_secs: u32, now_ns: u64) {
        let ttl = if outcome.is_ok() {
            ttl_secs.clamp(60, 3600)
        } else {
            NEGATIVE_CACHE_TTL
        };
        let entry = CacheEntry {
            name: name.to_ascii_lowercase(),
            qtype,
            outcome,
            expires_ns: now_ns.saturating_add(u64::from(ttl).saturating_mul(1_000_000_000)),
        };
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|e| e.qtype == qtype && names_eq_case_insensitive(&e.name, name))
        {
            *slot = entry;
            return;
        }
        if self.entries.len() < CACHE_SIZE {
            self.entries.push(entry);
            return;
        }
        let victim = self
            .entries
            .iter()
            .position(|e| e.expires_ns <= now_ns)
            .or_else(|| {
                self.entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.expires_ns)
                    .map(|(i, _)| i)
            });
        if let Some(slot) = victim.and_then(|i| self.entries.get_mut(i)) {
            if now_ns < slot.expires_ns {
                CACHE_EVICTIONS.fetch_add(1, Ordering::Relaxed);
            }
            *slot = entry;
        }
    }

    /// Live entries: of one record type, or of all (`None`).
    fn count(&self, qtype: Option<u16>, now_ns: u64) -> usize {
        self.entries
            .iter()
            .filter(|e| now_ns < e.expires_ns && qtype.is_none_or(|t| t == e.qtype))
            .count()
    }
}

/// The resolver's cache.
static DNS_CACHE: Mutex<DnsCache> = Mutex::new(DnsCache::new());

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
    pkt.extend_from_slice(&query_id.to_be_bytes()); // ID.
    pkt.extend_from_slice(&FLAGS_QUERY_RD.to_be_bytes()); // Flags.
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1.
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT = 0.
    pkt.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT = 0.
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT = 0.

    // Question section: encode domain name as labels.
    encode_name(&mut pkt, name);

    // Type + Class.
    pkt.extend_from_slice(&qtype.to_be_bytes());
    pkt.extend_from_slice(&CLASS_IN.to_be_bytes());

    pkt
}

/// Build a DNS query packet for an A record.
///
/// `pub(crate)` solely so `crate::bench` can time the real builder rather than
/// a copy of it. The copy differed *behaviourally*, not just in cost: it lacked
/// [`encode_name`]'s `.filter(|l| !l.is_empty())`, so it skipped the
/// trailing-dot FQDN handling this path does, and the figure it reported
/// described a builder that would emit an invalid name. Exposing this rather
/// than [`build_query_typed`] keeps `TYPE_A` and the qtype constants private —
/// the benchmark wants the A-record path, which is exactly what this is. See
/// design-decisions.md §251.
pub(crate) fn build_query(name: &str, query_id: u16) -> Vec<u8> {
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
        ip.0[3],
        ip.0[2],
        ip.0[1],
        ip.0[0]
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

/// What one DNS response says about a name's records of one type.
#[derive(Debug, PartialEq, Eq)]
enum Response {
    /// The records at the end of the CNAME chain the response holds, and the
    /// smallest TTL along the way.
    Records(Lookup, u32),
    /// The chain leaves the response at this name: ask again for it.
    Chase(String),
}

/// The big-endian `u16` at `at`, or `IoError` past the end.
fn be16(data: &[u8], at: usize) -> KernelResult<u16> {
    at.checked_add(2)
        .and_then(|end| data.get(at..end))
        .and_then(|b| <[u8; 2]>::try_from(b).ok())
        .map(u16::from_be_bytes)
        .ok_or(KernelError::IoError)
}

/// The big-endian `u32` at `at`, or `IoError` past the end.
fn be32(data: &[u8], at: usize) -> KernelResult<u32> {
    at.checked_add(4)
        .and_then(|end| data.get(at..end))
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_be_bytes)
        .ok_or(KernelError::IoError)
}

/// Read a response to the query for `qname`'s `qtype` records (`TYPE_A` or
/// `TYPE_AAAA`) sent with transaction ID `expected_id`.
///
/// Follows the CNAME chain the answer section holds, from `qname`, as far as
/// it goes, and takes every record of `qtype` its last name owns. Records of
/// other names and other types are passed over.
///
/// # Errors
///
/// - `NotFound`: NXDOMAIN -- the name does not exist.
/// - `NoAddress`: NODATA -- the name exists, with no record of `qtype`.
/// - `WouldBlock`: SERVFAIL, a temporary failure: ask again later.
/// - `ConnectionRefused`: REFUSED.
/// - `TooManyLinks`: a CNAME chain longer than [`MAX_CNAME_HOPS`], or a loop.
/// - `IoError`: not a response to this query, or one that cannot be read.
fn parse_answer(data: &[u8], expected_id: u16, qname: &str, qtype: u16) -> KernelResult<Response> {
    if be16(data, 0)? != expected_id {
        return Err(KernelError::IoError);
    }
    let flags = be16(data, 2)?;
    if flags & 0x8000 == 0 {
        return Err(KernelError::IoError); // A query, not a response.
    }
    match flags & 0x000F {
        0 => {}
        2 => return Err(KernelError::WouldBlock),
        3 => return Err(KernelError::NotFound),
        5 => return Err(KernelError::ConnectionRefused),
        _ => return Err(KernelError::IoError),
    }
    let qdcount = be16(data, 4)?;
    let ancount = be16(data, 6)?;

    let mut offset: usize = 12;
    for _ in 0..qdcount {
        offset = skip_name(data, offset).map_err(|_| KernelError::IoError)?;
        // QTYPE and QCLASS.
        offset = offset
            .checked_add(4)
            .filter(|&end| end <= data.len())
            .ok_or(KernelError::IoError)?;
    }

    // The answer section: the CNAMEs, and the records of `qtype`, by owner.
    let mut cnames: Vec<(String, String, u32)> = Vec::new();
    let mut records: Vec<(String, Address, u32)> = Vec::new();
    for _ in 0..ancount {
        let (owner, fields) = decode_name(data, offset).map_err(|_| KernelError::IoError)?;
        let at = |n: usize| fields.checked_add(n).ok_or(KernelError::IoError);
        let rtype = be16(data, fields)?;
        let rclass = be16(data, at(2)?)?;
        let ttl = be32(data, at(4)?)?;
        let rdlength = usize::from(be16(data, at(8)?)?);
        let rdata_at = at(10)?;
        let rd_end = rdata_at
            .checked_add(rdlength)
            .filter(|&end| end <= data.len())
            .ok_or(KernelError::IoError)?;
        let rdata = data.get(rdata_at..rd_end).ok_or(KernelError::IoError)?;
        if rclass == CLASS_IN {
            match rtype {
                TYPE_CNAME => {
                    let (target, _) =
                        decode_name(data, rdata_at).map_err(|_| KernelError::IoError)?;
                    cnames.push((owner, target, ttl));
                }
                TYPE_A if qtype == TYPE_A => {
                    if let Ok(ip) = <[u8; 4]>::try_from(rdata) {
                        records.push((owner, Address::V4(Ipv4Addr(ip)), ttl));
                    }
                }
                TYPE_AAAA if qtype == TYPE_AAAA => {
                    if let Ok(ip) = <[u8; 16]>::try_from(rdata) {
                        records.push((owner, Address::V6(Ipv6Addr(ip)), ttl));
                    }
                }
                _ => {}
            }
        }
        offset = rd_end;
    }

    // The chain from `qname`, as far as the response holds it.
    let mut name = String::from(qname);
    let mut ttl = u32::MAX;
    let mut hops = 0usize;
    while let Some((_, target, link_ttl)) = cnames
        .iter()
        .find(|(owner, _, _)| names_eq_case_insensitive(owner, &name))
    {
        if hops == MAX_CNAME_HOPS {
            return Err(KernelError::TooManyLinks);
        }
        hops = hops.saturating_add(1);
        ttl = ttl.min(*link_ttl);
        name.clone_from(target);
    }

    let mut addrs = Vec::new();
    for (owner, addr, record_ttl) in &records {
        if names_eq_case_insensitive(owner, &name) {
            addrs.push(*addr);
            ttl = ttl.min(*record_ttl);
        }
    }
    if !addrs.is_empty() {
        return Ok(Response::Records(
            Lookup {
                canonical: name,
                addrs,
            },
            ttl,
        ));
    }
    // A chain whose end has no records here: the end is asked next.
    if hops > 0 {
        return Ok(Response::Chase(name));
    }
    Err(KernelError::NoAddress)
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
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
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

/// Look `name` up: every address of the kinds `family` asks for, and the name
/// at the end of its CNAME chain.
///
/// Asked in order, as `fs::nameservice` declares it: a container's peers (the
/// embedded DNS of a user-defined network; IPv4), the kernel's hosts table,
/// the cache, then the DNS server -- each type's query following CNAMEs
/// across responses, up to [`MAX_CNAME_HOPS`]. `Family::Any` asks AAAA, then
/// A, and gives every address either found, IPv6 first.
///
/// # Errors
///
/// - `NotFound`: the name does not exist (NXDOMAIN).
/// - `NoAddress`: it exists, with no address of the kinds asked (NODATA).
/// - `TimedOut`: no answer; `WouldBlock`: the server failed for now
///   (SERVFAIL); `ConnectionRefused`: it refused.
/// - `TooManyLinks`: a CNAME chain too long, or a loop.
/// - `IoError`: an answer that could not be read; `pick_dns_server`'s own
///   when no server is known.
pub fn lookup(name: &str, family: Family) -> KernelResult<Lookup> {
    // Docker embedded DNS: if the caller is inside a container attached to a
    // user-defined network, a peer's container name / hostname / alias
    // resolves to that peer's address *before* any upstream query
    // (127.0.0.11 semantics). The host namespace (0) skips this.
    if family != Family::V6 {
        let caller_ns = crate::sched::current_task_net_ns();
        if let Some(ip) = crate::container::resolve_dns(caller_ns, name) {
            return Ok(Lookup {
                canonical: String::from(name),
                addrs: alloc::vec![Address::V4(Ipv4Addr(ip))],
            });
        }
    }
    if let Some(found) = hosts_lookup(name, family) {
        return Ok(found);
    }
    match family {
        Family::V4 => lookup_type(name, TYPE_A),
        Family::V6 => lookup_type(name, TYPE_AAAA),
        Family::Any => {
            let v6 = lookup_type(name, TYPE_AAAA);
            // NXDOMAIN for one type is NXDOMAIN for the name: the A query
            // would only say so again.
            if v6 == Err(KernelError::NotFound) {
                return v6;
            }
            combine(v6, lookup_type(name, TYPE_A))
        }
    }
}

/// `name` in the kernel's hosts table (`fs::nameservice`): every address of
/// the kinds `family` asks for, IPv6 first, under the entry's own name. An
/// entry's address that is neither IPv4 nor IPv6 text is passed over.
fn hosts_lookup(name: &str, family: Family) -> Option<Lookup> {
    crate::fs::nameservice::init_defaults();
    let hosts = crate::fs::nameservice::list_hosts();
    let matching = || {
        hosts.iter().filter(|h| {
            names_eq_case_insensitive(&h.hostname, name)
                || h.aliases.iter().any(|a| names_eq_case_insensitive(a, name))
        })
    };
    let canonical = matching().next()?.hostname.clone();
    let mut addrs = Vec::new();
    if family != Family::V4 {
        addrs.extend(
            matching()
                .filter_map(|h| Ipv6Addr::parse(&h.address))
                .map(Address::V6),
        );
    }
    if family != Family::V6 {
        addrs.extend(
            matching()
                .filter_map(|h| parse_ipv4(&h.address))
                .map(Address::V4),
        );
    }
    if addrs.is_empty() {
        return None;
    }
    Some(Lookup { canonical, addrs })
}

/// A dotted-quad IPv4 address, `None` for anything else (`1.2.3`, `1.2.3.4.5`,
/// a field over 255, an IPv6 address).
fn parse_ipv4(text: &str) -> Option<Ipv4Addr> {
    let mut octets = [0u8; 4];
    let mut fields = text.split('.');
    for slot in &mut octets {
        *slot = fields.next()?.parse::<u8>().ok()?;
    }
    if fields.next().is_some() {
        return None;
    }
    Some(Ipv4Addr(octets))
}

/// An `Any` lookup's answer from its AAAA and A halves: every address either
/// found, IPv6 first, under the AAAA half's canonical name; or, when neither
/// found one, the error that says most -- the name does not exist, then no
/// answer came, then no address.
fn combine(v6: KernelResult<Lookup>, v4: KernelResult<Lookup>) -> KernelResult<Lookup> {
    /// How much an error says: the lower, the more.
    fn rank(e: KernelError) -> u8 {
        match e {
            KernelError::NotFound => 0,
            KernelError::NoAddress => 2,
            _ => 1,
        }
    }
    match (v6, v4) {
        (Ok(mut both), Ok(v4)) => {
            both.addrs.extend(v4.addrs);
            Ok(both)
        }
        (Ok(one), Err(_)) | (Err(_), Ok(one)) => Ok(one),
        (Err(a), Err(b)) => Err(if rank(b) < rank(a) { b } else { a }),
    }
}

/// `name`'s records of one type (`TYPE_A` or `TYPE_AAAA`): from the cache,
/// else from the network, the answer then cached -- a negative one too, but
/// not a failure to get one.
fn lookup_type(name: &str, qtype: u16) -> KernelResult<Lookup> {
    if let Some(cached) = DNS_CACHE.lock().get(name, qtype, crate::hrtimer::now_ns()) {
        CACHE_HITS.fetch_add(1, Ordering::Relaxed);
        return cached;
    }
    CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    let outcome = query_chain(name, qtype);
    let now_ns = crate::hrtimer::now_ns();
    match &outcome {
        Ok((found, ttl)) => DNS_CACHE
            .lock()
            .put(name, qtype, Ok(found.clone()), *ttl, now_ns),
        Err(e @ (KernelError::NotFound | KernelError::NoAddress)) => {
            DNS_CACHE.lock().put(name, qtype, Err(*e), 0, now_ns);
        }
        Err(_) => {}
    }
    outcome.map(|(found, _)| found)
}

/// Ask the DNS server for `name`'s `qtype` records, a CNAME chain that leaves
/// one response chased into the next query, up to [`MAX_CNAME_HOPS`].
fn query_chain(name: &str, qtype: u16) -> KernelResult<(Lookup, u32)> {
    let server = pick_dns_server()?;
    let kind = if qtype == TYPE_AAAA { "AAAA" } else { "A" };
    let mut asked = String::from(name);
    for _ in 0..MAX_CNAME_HOPS {
        crate::serial_println!("[dns] Resolving '{}' ({}) via {}...", asked, kind, server);
        let query_id = next_query_id();
        let query = build_query_typed(&asked, query_id, qtype);
        let response = dns_query_raw(&server, &query, &asked)?;
        match parse_answer(&response, query_id, &asked, qtype) {
            Ok(Response::Records(found, ttl)) => {
                crate::serial_println!(
                    "[dns] '{}' ({}): {} address(es), canonical '{}', TTL {}s",
                    name,
                    kind,
                    found.addrs.len(),
                    found.canonical,
                    ttl
                );
                return Ok((found, ttl));
            }
            Ok(Response::Chase(target)) => {
                crate::serial_println!("[dns] Following CNAME: {} -> {}", asked, target);
                asked = target;
            }
            Err(e) => {
                crate::serial_println!("[dns] '{}' ({}): {:?}", asked, kind, e);
                return Err(e);
            }
        }
    }
    crate::serial_println!(
        "[dns] CNAME chain for '{}' longer than {} hops",
        name,
        MAX_CNAME_HOPS
    );
    Err(KernelError::TooManyLinks)
}

/// Resolve a domain name to its first IPv4 address ([`lookup`] for
/// `Family::V4`). A name with no IPv4 address is `NotFound`, as before
/// [`lookup`] could say `NoAddress`: callers here ask for one address and
/// take its absence for a name that does not resolve.
///
/// # Errors
///
/// As [`lookup`], `NoAddress` as `NotFound`.
pub fn resolve(name: &str) -> KernelResult<Ipv4Addr> {
    let found = lookup(name, Family::V4).map_err(no_address_as_not_found)?;
    found
        .addrs
        .iter()
        .find_map(|a| match a {
            Address::V4(ip) => Some(*ip),
            Address::V6(_) => None,
        })
        .ok_or(KernelError::NotFound)
}

/// `NoAddress` as `NotFound`, for the one-address calls.
fn no_address_as_not_found(e: KernelError) -> KernelError {
    if e == KernelError::NoAddress {
        KernelError::NotFound
    } else {
        e
    }
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
/// The source port is assigned by `udp::bind` rather than chosen here, so
/// it is both unpredictable and guaranteed free; see the note where
/// `next_dns_port` used to live.
///
/// Returns the raw DNS response payload on success, or `TimedOut` after
/// exhausting all retry attempts.
#[allow(clippy::arithmetic_side_effects)]
fn dns_query_raw(server: &DnsServer, query: &[u8], name: &str) -> KernelResult<Vec<u8>> {
    // Port 0 means "any free port", which the allocator draws from the
    // CSPRNG.  Reading it back is necessary because `udp::send` is keyed by
    // port rather than by socket handle.
    let sock = super::udp::bind(crate::netns::ROOT_NS, 0)?;
    let Some(local_port) = super::udp::local_port(sock) else {
        // Unreachable: the handle was just returned by a successful bind.
        // Closed rather than leaked, since an early return here would
        // otherwise strand the socket slot for the life of the system.
        super::udp::close(sock);
        return Err(KernelError::InternalError);
    };

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
                attempt,
                name,
                DNS_ATTEMPT_POLLS.get(attempt).copied().unwrap_or(2000)
            );
        }

        let polls = DNS_ATTEMPT_POLLS.get(attempt).copied().unwrap_or(2000);

        for _ in 0..polls {
            super::poll();

            // Check the appropriate receive queue based on server type.
            let response = match server {
                DnsServer::V4(ip) => super::udp::recv(sock).and_then(|dgram| {
                    if dgram.src_ip == *ip && dgram.src_port == DNS_PORT {
                        Some(dgram.data)
                    } else {
                        None
                    }
                }),
                DnsServer::V6(ip) => super::udp::recv_v6(sock).and_then(|dgram| {
                    if dgram.src_ip == *ip && dgram.src_port == DNS_PORT {
                        Some(dgram.data)
                    } else {
                        None
                    }
                }),
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
        name,
        MAX_DNS_ATTEMPTS,
        server
    );
    Err(KernelError::TimedOut)
}

/// Flush the entire DNS cache (both A and AAAA).
///
/// Called when the network configuration changes (e.g., DHCP renewal
/// with a new DNS server) to avoid stale cached results.
pub fn flush_cache() {
    *DNS_CACHE.lock() = DnsCache::new();
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
    let query = build_ptr_query(ip, query_id);
    let arpa_name = alloc::format!("{}", ip); // For timeout logging.

    let response_data = dns_query_raw(&server, &query, &arpa_name)?;

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

/// Resolve a domain name to its first IPv6 address ([`lookup`] for
/// `Family::V6`). A name with no IPv6 address is `NotFound`, as for
/// [`resolve`].
///
/// # Errors
///
/// As [`lookup`], `NoAddress` as `NotFound`.
pub fn resolve6(name: &str) -> KernelResult<Ipv6Addr> {
    let found = lookup(name, Family::V6).map_err(no_address_as_not_found)?;
    found
        .addrs
        .iter()
        .find_map(|a| match a {
            Address::V6(ip) => Some(*ip),
            Address::V4(_) => None,
        })
        .ok_or(KernelError::NotFound)
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
    let query = build_ptr6_query(ip, query_id);
    let name_for_log = alloc::format!("{}", ip);

    let response_data = dns_query_raw(&server, &query, &name_for_log)?;

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

/// Return the number of live AAAA answers in the cache.
pub fn aaaa_cache_count() -> usize {
    DNS_CACHE
        .lock()
        .count(Some(TYPE_AAAA), crate::hrtimer::now_ns())
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
    test_parse_answer_records()?;
    test_parse_answer_cname_chain()?;
    test_parse_answer_refusals()?;
    test_cache()?;
    test_combine()?;
    test_hosts_lookup()?;
    test_build_aaaa_query_structure()?;
    test_ipv6_reverse_name()?;
    test_ns_aware_dns_picker()?;

    crate::serial_println!("[dns] DNS self-test PASSED (14 tests)");
    Ok(())
}

/// Test encode_name produces correct DNS wire format.
fn test_encode_name() -> KernelResult<()> {
    let mut buf = Vec::new();
    encode_name(&mut buf, "example.com");

    // Expected: \x07example\x03com\x00
    let expected: &[u8] = &[
        7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
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
    let expected3: &[u8] = &[9, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0];
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
        7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
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
        3, b'w', b'w', b'w', // "www" label
        0xC0, 0x00, // compression pointer → offset 0
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
        7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
    ];
    let after = skip_name(wire, 0)?;
    if after != 13 {
        crate::serial_println!("[dns]   FAIL: skip simple name = {}", after);
        return Err(KernelError::InternalError);
    }

    // Name with compression pointer at the end.
    let wire2: &[u8] = &[
        3, b'w', b'w', b'w', 0xC0, 0x00, // pointer to offset 0
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

/// A synthetic response to `qname`'s `qtype` query: the header with `flags`,
/// the question, and `answers` as (owner, type, TTL, RDATA).
#[allow(clippy::cast_possible_truncation)]
fn test_response(
    id: u16,
    flags: u16,
    qname: &str,
    qtype: u16,
    answers: &[(&str, u16, u32, Vec<u8>)],
) -> Vec<u8> {
    let mut resp = Vec::new();
    resp.extend_from_slice(&id.to_be_bytes());
    resp.extend_from_slice(&flags.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&(answers.len() as u16).to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    encode_name(&mut resp, qname);
    resp.extend_from_slice(&qtype.to_be_bytes());
    resp.extend_from_slice(&CLASS_IN.to_be_bytes());
    for (owner, rtype, ttl, rdata) in answers {
        encode_name(&mut resp, owner);
        resp.extend_from_slice(&rtype.to_be_bytes());
        resp.extend_from_slice(&CLASS_IN.to_be_bytes());
        resp.extend_from_slice(&ttl.to_be_bytes());
        resp.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        resp.extend_from_slice(rdata);
    }
    resp
}

/// A CNAME's RDATA: `target` in wire form.
fn cname_rdata(target: &str) -> Vec<u8> {
    let mut rdata = Vec::new();
    encode_name(&mut rdata, target);
    rdata
}

/// The flags of a response with no error: QR, RD and RA set, RCODE 0.
const RESPONSE_OK: u16 = 0x8180;

/// Every A record of the name asked, the smallest TTL, nothing of another
/// name; the AAAA records of an AAAA query, A records passed over; a
/// response to another query refused.
fn test_parse_answer_records() -> KernelResult<()> {
    let v6 = |last: u8| {
        let mut ip = [0u8; 16];
        ip[0] = 0x20;
        ip[1] = 0x01;
        ip[15] = last;
        ip
    };
    let a = test_response(
        0x1234,
        RESPONSE_OK,
        "many.test",
        TYPE_A,
        &[
            ("many.test", TYPE_A, 300, alloc::vec![10, 0, 0, 1]),
            ("other.test", TYPE_A, 300, alloc::vec![10, 0, 0, 9]),
            ("MANY.test", TYPE_A, 120, alloc::vec![10, 0, 0, 2]),
        ],
    );
    let want_a = Response::Records(
        Lookup {
            canonical: String::from("many.test"),
            addrs: alloc::vec![
                Address::V4(Ipv4Addr([10, 0, 0, 1])),
                Address::V4(Ipv4Addr([10, 0, 0, 2])),
            ],
        },
        120,
    );
    let aaaa = test_response(
        0x4321,
        RESPONSE_OK,
        "six.test",
        TYPE_AAAA,
        &[
            ("six.test", TYPE_A, 60, alloc::vec![10, 0, 0, 3]),
            ("six.test", TYPE_AAAA, 600, v6(1).to_vec()),
            ("six.test", TYPE_AAAA, 600, v6(2).to_vec()),
        ],
    );
    let want_aaaa = Response::Records(
        Lookup {
            canonical: String::from("six.test"),
            addrs: alloc::vec![Address::V6(Ipv6Addr(v6(1))), Address::V6(Ipv6Addr(v6(2)))],
        },
        600,
    );
    let got_a = parse_answer(&a, 0x1234, "many.test", TYPE_A);
    let got_aaaa = parse_answer(&aaaa, 0x4321, "six.test", TYPE_AAAA);
    let wrong_id = parse_answer(&a, 0x9999, "many.test", TYPE_A);
    let short = parse_answer(a.get(..5).unwrap_or(&[]), 0x1234, "many.test", TYPE_A);
    if got_a.as_ref() != Ok(&want_a)
        || got_aaaa.as_ref() != Ok(&want_aaaa)
        || wrong_id != Err(KernelError::IoError)
        || short != Err(KernelError::IoError)
    {
        crate::serial_println!(
            "[dns]   FAIL: answers: A {:?}, AAAA {:?}, wrong id {:?}, short {:?}",
            got_a,
            got_aaaa,
            wrong_id,
            short
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[dns]   parse answer: every record of the name asked: OK");
    Ok(())
}

/// A CNAME chain in one response ends at the canonical name, with its
/// records; one that leaves the response is chased; a loop is refused.
fn test_parse_answer_cname_chain() -> KernelResult<()> {
    let chain = test_response(
        7,
        RESPONSE_OK,
        "www.test",
        TYPE_A,
        &[
            ("www.test", TYPE_CNAME, 3600, cname_rdata("cdn.test")),
            ("cdn.test", TYPE_CNAME, 90, cname_rdata("Edge.Test")),
            ("edge.test", TYPE_A, 300, alloc::vec![192, 0, 2, 7]),
        ],
    );
    let want = Response::Records(
        Lookup {
            canonical: String::from("Edge.Test"),
            addrs: alloc::vec![Address::V4(Ipv4Addr([192, 0, 2, 7]))],
        },
        90,
    );
    let leaves = test_response(
        8,
        RESPONSE_OK,
        "www.test",
        TYPE_A,
        &[("www.test", TYPE_CNAME, 3600, cname_rdata("far.test"))],
    );
    let looped = test_response(
        9,
        RESPONSE_OK,
        "a.test",
        TYPE_A,
        &[
            ("a.test", TYPE_CNAME, 60, cname_rdata("b.test")),
            ("b.test", TYPE_CNAME, 60, cname_rdata("a.test")),
        ],
    );
    let got = parse_answer(&chain, 7, "www.test", TYPE_A);
    let chased = parse_answer(&leaves, 8, "www.test", TYPE_A);
    let looping = parse_answer(&looped, 9, "a.test", TYPE_A);
    if got.as_ref() != Ok(&want)
        || chased != Ok(Response::Chase(String::from("far.test")))
        || looping != Err(KernelError::TooManyLinks)
    {
        crate::serial_println!(
            "[dns]   FAIL: CNAME: chain {:?}, leaving {:?}, loop {:?}",
            got,
            chased,
            looping
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[dns]   parse answer: CNAME chain, chase, loop: OK");
    Ok(())
}

/// What a response says when it gives no address: NXDOMAIN, NODATA,
/// SERVFAIL, REFUSED -- each its own error -- and a query echoed back.
fn test_parse_answer_refusals() -> KernelResult<()> {
    let with = |flags: u16| test_response(3, flags, "gone.test", TYPE_A, &[]);
    let cases = [
        (0x8183, KernelError::NotFound),
        (RESPONSE_OK, KernelError::NoAddress),
        (0x8182, KernelError::WouldBlock),
        (0x8185, KernelError::ConnectionRefused),
        (0x8181, KernelError::IoError),
        (0x0100, KernelError::IoError),
    ];
    for (flags, want) in cases {
        let got = parse_answer(&with(flags), 3, "gone.test", TYPE_A);
        if got != Err(want) {
            crate::serial_println!(
                "[dns]   FAIL: flags {:#06x}: {:?}, want {:?}",
                flags,
                got,
                want
            );
            return Err(KernelError::InternalError);
        }
    }
    // NODATA: the name exists, with records of another type only.
    let other_type = test_response(
        4,
        RESPONSE_OK,
        "mx.test",
        TYPE_AAAA,
        &[("mx.test", TYPE_A, 60, alloc::vec![10, 1, 1, 1])],
    );
    if parse_answer(&other_type, 4, "mx.test", TYPE_AAAA) != Err(KernelError::NoAddress) {
        crate::serial_println!("[dns]   FAIL: records of another type were not NODATA");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[dns]   parse answer: NXDOMAIN, NODATA, SERVFAIL, REFUSED told apart: OK"
    );
    Ok(())
}

/// The cache: whole answers per name and type, without case; negative
/// answers as themselves; expiry; a full cache replacing what expires first.
fn test_cache() -> KernelResult<()> {
    let mut cache = DnsCache::new();
    let now = crate::hrtimer::now_ns();
    let found = Lookup {
        canonical: String::from("edge.test"),
        addrs: alloc::vec![
            Address::V4(Ipv4Addr([1, 2, 3, 4])),
            Address::V4(Ipv4Addr([1, 2, 3, 5])),
        ],
    };
    cache.put("www.test", TYPE_A, Ok(found.clone()), 120, now);
    cache.put("gone.test", TYPE_A, Err(KernelError::NotFound), 0, now);
    cache.put(
        "v4only.test",
        TYPE_AAAA,
        Err(KernelError::NoAddress),
        0,
        now,
    );
    let later = now.saturating_add(1_000_000_000_000); // ~1000 s: all expired
    let ok = cache.get("WWW.test", TYPE_A, now) == Some(Ok(found))
        && cache.get("www.test", TYPE_AAAA, now).is_none()
        && cache.get("gone.test", TYPE_A, now) == Some(Err(KernelError::NotFound))
        && cache.get("v4only.test", TYPE_AAAA, now) == Some(Err(KernelError::NoAddress))
        && cache.get("unknown.test", TYPE_A, now).is_none()
        && cache.get("www.test", TYPE_A, later).is_none()
        && cache.count(None, now) == 3
        && cache.count(Some(TYPE_AAAA), now) == 1;
    if !ok {
        crate::serial_println!("[dns]   FAIL: cache: get, negative answers, expiry or count");
        return Err(KernelError::InternalError);
    }
    // Full: the next answer takes the slot that expires first.
    let mut full = DnsCache::new();
    for i in 0..CACHE_SIZE {
        let ttl = 3000u32.saturating_sub(u32::try_from(i).unwrap_or(0));
        let name = alloc::format!("n{i}.test");
        // Each a second shorter-lived than the last: the last expires first.
        full.put(
            &name,
            TYPE_A,
            Ok(Lookup {
                canonical: name.clone(),
                addrs: alloc::vec![Address::V4(Ipv4Addr([10, 0, 0, 1]))],
            }),
            ttl,
            now,
        );
    }
    full.put("newcomer.test", TYPE_A, Err(KernelError::NotFound), 0, now);
    let last = alloc::format!("n{}.test", CACHE_SIZE.saturating_sub(1));
    if full.count(None, now) != CACHE_SIZE
        || full.get("newcomer.test", TYPE_A, now).is_none()
        || full.get(&last, TYPE_A, now).is_some()
        || full.get("n0.test", TYPE_A, now).is_none()
    {
        crate::serial_println!(
            "[dns]   FAIL: cache: a full cache did not replace what expires first"
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[dns]   cache: whole answers, negative answers, expiry, eviction: OK");
    Ok(())
}

/// `Family::Any` from its two halves: both found, IPv6 first; one found;
/// neither, the error that says most.
fn test_combine() -> KernelResult<()> {
    let six = Lookup {
        canonical: String::from("dual.test"),
        addrs: alloc::vec![Address::V6(Ipv6Addr([0x20; 16]))],
    };
    let four = Lookup {
        canonical: String::from("dual.test"),
        addrs: alloc::vec![Address::V4(Ipv4Addr([10, 0, 0, 1]))],
    };
    let both = Lookup {
        canonical: String::from("dual.test"),
        addrs: alloc::vec![
            Address::V6(Ipv6Addr([0x20; 16])),
            Address::V4(Ipv4Addr([10, 0, 0, 1])),
        ],
    };
    let e = Err::<Lookup, KernelError>;
    let ok = combine(Ok(six.clone()), Ok(four.clone())) == Ok(both)
        && combine(e(KernelError::NoAddress), Ok(four.clone())) == Ok(four)
        && combine(Ok(six.clone()), e(KernelError::TimedOut)) == Ok(six)
        && combine(e(KernelError::NoAddress), e(KernelError::NoAddress))
            == Err(KernelError::NoAddress)
        && combine(e(KernelError::NoAddress), e(KernelError::TimedOut))
            == Err(KernelError::TimedOut)
        && combine(e(KernelError::TimedOut), e(KernelError::NotFound))
            == Err(KernelError::NotFound);
    if !ok {
        crate::serial_println!("[dns]   FAIL: combining the AAAA and A answers");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[dns]   Any: AAAA and A combined, IPv6 first: OK");
    Ok(())
}

/// The kernel's hosts table answers before the network: `localhost` and its
/// alias as both families, each family alone; a dotted quad read strictly.
fn test_hosts_lookup() -> KernelResult<()> {
    let loopback4 = Address::V4(Ipv4Addr([127, 0, 0, 1]));
    let mut one = [0u8; 16];
    one[15] = 1;
    let loopback6 = Address::V6(Ipv6Addr(one));
    let any = hosts_lookup("localhost", Family::Any);
    let v4 = hosts_lookup("LOCALHOST", Family::V4);
    let v6 = hosts_lookup("ip6-localhost", Family::V6);
    let ok = any
        .as_ref()
        .is_some_and(|l| l.canonical == "localhost" && l.addrs == [loopback6, loopback4])
        && v4.as_ref().is_some_and(|l| l.addrs == [loopback4])
        && v6.as_ref().is_some_and(|l| l.addrs == [loopback6])
        && hosts_lookup("ip6-localhost", Family::V4).is_none()
        && hosts_lookup("nowhere.test", Family::Any).is_none()
        && parse_ipv4("10.20.30.40") == Some(Ipv4Addr([10, 20, 30, 40]))
        && parse_ipv4("1.2.3").is_none()
        && parse_ipv4("1.2.3.4.5").is_none()
        && parse_ipv4("1.2.3.256").is_none()
        && parse_ipv4("::1").is_none();
    if !ok {
        crate::serial_println!(
            "[dns]   FAIL: hosts table: any {:?}, v4 {:?}, v6 {:?}",
            any,
            v4,
            v6
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[dns]   hosts table: localhost, both families, before the network: OK");
    Ok(())
}

/// Test IPv6 → ip6.arpa reverse name generation.
fn test_ipv6_reverse_name() -> KernelResult<()> {
    // 2001:0db8::1 = 2001:0db8:0000:0000:0000:0000:0000:0001
    let ip = Ipv6Addr([
        0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x01,
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
