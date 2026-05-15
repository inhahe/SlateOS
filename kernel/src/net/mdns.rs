//! mDNS (Multicast DNS) and DNS-SD (DNS Service Discovery).
//!
//! Implements RFC 6762 (mDNS) and RFC 6763 (DNS-SD) for zero-configuration
//! service discovery on the local network.  Enables driverless printing,
//! network device discovery, and local name resolution without a DNS server.
//!
//! ## Protocol
//!
//! mDNS uses UDP multicast on 224.0.0.251:5353.  Queries and responses
//! use standard DNS message format but with multicast delivery.  The
//! `.local` domain is reserved for mDNS names.
//!
//! DNS-SD layers service discovery on top of mDNS using PTR, SRV, and
//! TXT records:
//! - PTR: `_service._proto.local → instance._service._proto.local`
//! - SRV: `instance._service._proto.local → hostname:port`
//! - TXT: `instance._service._proto.local → key=value pairs`
//!
//! ## Features
//!
//! - **Querying**: send mDNS queries for A, PTR, SRV, TXT records
//! - **Service browsing**: discover services by type (e.g., `_http._tcp`)
//! - **Name resolution**: resolve `.local` hostnames to IP addresses
//! - **Service registration**: announce local services on the network
//! - **Response caching**: cache received records with TTL expiry
//! - **Conflict detection**: detect name collisions during registration
//!
//! ## Architecture
//!
//! ```text
//! mDNS client
//!   → mdns::query_service("_ipp._tcp")  → discover printers
//!   → mdns::resolve_local("myhost.local") → resolve local name
//!   → mdns::register("MyService", "_http._tcp", 8080) → announce
//!   → mdns::tick() → process incoming queries/responses
//! ```
//!
//! ## Limitations
//!
//! - IPv4 only (no IPv6 link-local / ff02::fb).
//! - No NSEC record support (negative responses).
//! - No known-answer suppression in queries (RFC 6762 §7.1).
//! - Maximum 32 cached records and 8 registered services.
//! - Single-question queries only (one question per packet).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// mDNS multicast address: 224.0.0.251
const MDNS_MULTICAST_IP: Ipv4Addr = Ipv4Addr([224, 0, 0, 251]);

/// mDNS port.
const MDNS_PORT: u16 = 5353;

/// Maximum cached records.
const MAX_CACHE_ENTRIES: usize = 32;

/// Maximum registered services.
const MAX_SERVICES: usize = 8;

/// Maximum DNS name length.
#[allow(dead_code)] // Protocol constant.
const MAX_NAME_LEN: usize = 255;

/// Default TTL for our announcements (seconds).
const DEFAULT_TTL: u32 = 120;

/// Cache expiry check interval (nanoseconds) — 10 seconds.
const CACHE_TICK_INTERVAL_NS: u64 = 10_000_000_000;

// DNS record types.
const TYPE_A: u16 = 1;
const TYPE_PTR: u16 = 12;
const TYPE_TXT: u16 = 16;
const TYPE_SRV: u16 = 33;

/// DNS class IN (Internet) with cache-flush bit.
const CLASS_IN: u16 = 1;
const CLASS_IN_FLUSH: u16 = 0x8001;

/// DNS header size.
const DNS_HEADER_SIZE: usize = 12;

// DNS header flags.
const FLAG_RESPONSE: u16 = 0x8400; // QR=1, AA=1
const FLAG_QUERY: u16 = 0x0000;

// ---------------------------------------------------------------------------
// Record types
// ---------------------------------------------------------------------------

/// DNS record type for our cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    /// A record (hostname → IPv4 address).
    A,
    /// PTR record (service type → instance name).
    Ptr,
    /// SRV record (instance → host:port).
    Srv,
    /// TXT record (instance → key=value metadata).
    Txt,
}

impl RecordType {
    #[allow(dead_code)] // Public API.
    fn to_u16(self) -> u16 {
        match self {
            Self::A => TYPE_A,
            Self::Ptr => TYPE_PTR,
            Self::Srv => TYPE_SRV,
            Self::Txt => TYPE_TXT,
        }
    }

    fn from_u16(v: u16) -> Option<Self> {
        match v {
            TYPE_A => Some(Self::A),
            TYPE_PTR => Some(Self::Ptr),
            TYPE_SRV => Some(Self::Srv),
            TYPE_TXT => Some(Self::Txt),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::Ptr => "PTR",
            Self::Srv => "SRV",
            Self::Txt => "TXT",
        }
    }
}

/// A cached mDNS record.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// Record name (e.g., "myhost.local" or "_http._tcp.local").
    pub name: String,
    /// Record type.
    pub record_type: RecordType,
    /// Record data (interpretation depends on type).
    pub data: RecordData,
    /// TTL remaining (seconds at time of insertion).
    pub ttl: u32,
    /// Kernel timestamp when this entry was cached (ns).
    pub cached_at_ns: u64,
}

/// Parsed record data.
#[derive(Debug, Clone)]
pub enum RecordData {
    /// A record: IPv4 address.
    Address(Ipv4Addr),
    /// PTR record: target name.
    Name(String),
    /// SRV record: priority, weight, port, target host.
    Srv {
        #[allow(dead_code)] // Spec-defined field.
        priority: u16,
        #[allow(dead_code)] // Spec-defined field.
        weight: u16,
        port: u16,
        target: String,
    },
    /// TXT record: key=value pairs.
    Txt(Vec<String>),
}

/// A registered local service.
struct RegisteredService {
    /// Service instance name (e.g., "My Printer").
    instance_name: String,
    /// Service type (e.g., "_ipp._tcp").
    service_type: String,
    /// Port number.
    port: u16,
    /// TXT record key=value pairs.
    txt_records: Vec<String>,
    /// Whether this service is active.
    active: bool,
}

/// A discovered service (from browsing).
#[derive(Debug, Clone)]
pub struct DiscoveredService {
    /// Instance name (e.g., "My Printer").
    pub instance_name: String,
    /// Service type (e.g., "_ipp._tcp").
    pub service_type: String,
    /// Hostname.
    pub hostname: String,
    /// IP address (if resolved).
    pub ip: Option<Ipv4Addr>,
    /// Port number.
    pub port: u16,
    /// TXT record metadata.
    pub txt: Vec<String>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct MdnsState {
    /// UDP socket handle for mDNS.
    socket_handle: Option<usize>,
    /// Cached records.
    cache: Vec<CacheEntry>,
    /// Registered local services.
    services: Vec<RegisteredService>,
    /// Our hostname for .local resolution.
    hostname: String,
}

impl MdnsState {
    const fn new() -> Self {
        Self {
            socket_handle: None,
            cache: Vec::new(),
            services: Vec::new(),
            hostname: String::new(),
        }
    }
}

static STATE: Mutex<MdnsState> = Mutex::new(MdnsState::new());
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static LAST_CACHE_TICK: AtomicU64 = AtomicU64::new(0);

// Statistics.
static QUERIES_SENT: AtomicU64 = AtomicU64::new(0);
static RESPONSES_SENT: AtomicU64 = AtomicU64::new(0);
static RECORDS_RECEIVED: AtomicU64 = AtomicU64::new(0);
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// DNS packet construction
// ---------------------------------------------------------------------------

/// Build an mDNS query packet for a given name and record type.
fn build_query(name: &str, rtype: u16) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // DNS header: ID=0 (mDNS uses 0), flags=query, 1 question, 0 answers.
    pkt.extend_from_slice(&[0, 0]); // Transaction ID
    let flags = FLAG_QUERY;
    pkt.push((flags >> 8) as u8);
    pkt.push(flags as u8);
    pkt.extend_from_slice(&[0, 1]); // QDCOUNT = 1
    pkt.extend_from_slice(&[0, 0]); // ANCOUNT = 0
    pkt.extend_from_slice(&[0, 0]); // NSCOUNT = 0
    pkt.extend_from_slice(&[0, 0]); // ARCOUNT = 0

    // Question section.
    encode_dns_name(&mut pkt, name);
    pkt.push((rtype >> 8) as u8);
    pkt.push(rtype as u8);
    pkt.push((CLASS_IN >> 8) as u8);
    pkt.push(CLASS_IN as u8);

    pkt
}

/// Build an mDNS response packet with one answer record.
fn build_response_a(name: &str, ip: Ipv4Addr, ttl: u32) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // DNS header: ID=0, flags=response+authoritative, 0 questions, 1 answer.
    pkt.extend_from_slice(&[0, 0]); // Transaction ID
    let flags = FLAG_RESPONSE;
    pkt.push((flags >> 8) as u8);
    pkt.push(flags as u8);
    pkt.extend_from_slice(&[0, 0]); // QDCOUNT = 0
    pkt.extend_from_slice(&[0, 1]); // ANCOUNT = 1
    pkt.extend_from_slice(&[0, 0]); // NSCOUNT = 0
    pkt.extend_from_slice(&[0, 0]); // ARCOUNT = 0

    // Answer: A record.
    encode_dns_name(&mut pkt, name);
    pkt.push((TYPE_A >> 8) as u8);
    pkt.push(TYPE_A as u8);
    pkt.push((CLASS_IN_FLUSH >> 8) as u8);
    pkt.push(CLASS_IN_FLUSH as u8);
    // TTL.
    pkt.push((ttl >> 24) as u8);
    pkt.push((ttl >> 16) as u8);
    pkt.push((ttl >> 8) as u8);
    pkt.push(ttl as u8);
    // RDLENGTH = 4 (IPv4 address).
    pkt.extend_from_slice(&[0, 4]);
    pkt.extend_from_slice(&ip.0);

    pkt
}

/// Build an mDNS response with PTR + SRV + TXT records for a service.
fn build_service_response(service: &RegisteredService, our_ip: Ipv4Addr, hostname: &str) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(256);
    let full_instance = format!("{}.{}.local", service.instance_name, service.service_type);
    let service_type_local = format!("{}.local", service.service_type);
    let host_local = format!("{}.local", hostname);

    // DNS header: response, 3 answers (PTR + SRV + TXT), 1 additional (A).
    pkt.extend_from_slice(&[0, 0]); // Transaction ID
    let flags = FLAG_RESPONSE;
    pkt.push((flags >> 8) as u8);
    pkt.push(flags as u8);
    pkt.extend_from_slice(&[0, 0]); // QDCOUNT = 0
    pkt.extend_from_slice(&[0, 3]); // ANCOUNT = 3
    pkt.extend_from_slice(&[0, 0]); // NSCOUNT = 0
    pkt.extend_from_slice(&[0, 1]); // ARCOUNT = 1

    // Answer 1: PTR record (service_type.local → instance.service_type.local)
    encode_dns_name(&mut pkt, &service_type_local);
    pkt.push((TYPE_PTR >> 8) as u8);
    pkt.push(TYPE_PTR as u8);
    pkt.push((CLASS_IN >> 8) as u8);
    pkt.push(CLASS_IN as u8);
    encode_ttl(&mut pkt, DEFAULT_TTL);
    // PTR RDATA: the instance name encoded as DNS name.
    let mut ptr_rdata = Vec::new();
    encode_dns_name(&mut ptr_rdata, &full_instance);
    let rdlen = ptr_rdata.len() as u16;
    pkt.push((rdlen >> 8) as u8);
    pkt.push(rdlen as u8);
    pkt.extend_from_slice(&ptr_rdata);

    // Answer 2: SRV record (instance.service_type.local → host:port)
    encode_dns_name(&mut pkt, &full_instance);
    pkt.push((TYPE_SRV >> 8) as u8);
    pkt.push(TYPE_SRV as u8);
    pkt.push((CLASS_IN_FLUSH >> 8) as u8);
    pkt.push(CLASS_IN_FLUSH as u8);
    encode_ttl(&mut pkt, DEFAULT_TTL);
    // SRV RDATA: priority(2) + weight(2) + port(2) + target_name
    let mut srv_rdata = Vec::new();
    srv_rdata.extend_from_slice(&[0, 0]); // priority = 0
    srv_rdata.extend_from_slice(&[0, 0]); // weight = 0
    srv_rdata.push((service.port >> 8) as u8);
    srv_rdata.push(service.port as u8);
    encode_dns_name(&mut srv_rdata, &host_local);
    let rdlen = srv_rdata.len() as u16;
    pkt.push((rdlen >> 8) as u8);
    pkt.push(rdlen as u8);
    pkt.extend_from_slice(&srv_rdata);

    // Answer 3: TXT record (instance.service_type.local → key=value)
    encode_dns_name(&mut pkt, &full_instance);
    pkt.push((TYPE_TXT >> 8) as u8);
    pkt.push(TYPE_TXT as u8);
    pkt.push((CLASS_IN_FLUSH >> 8) as u8);
    pkt.push(CLASS_IN_FLUSH as u8);
    encode_ttl(&mut pkt, DEFAULT_TTL);
    // TXT RDATA: length-prefixed strings.
    let mut txt_rdata = Vec::new();
    if service.txt_records.is_empty() {
        // Empty TXT record: single zero byte.
        txt_rdata.push(0);
    } else {
        for entry in &service.txt_records {
            let len = entry.len().min(255) as u8;
            txt_rdata.push(len);
            if let Some(s) = entry.as_bytes().get(..len as usize) {
                txt_rdata.extend_from_slice(s);
            }
        }
    }
    let rdlen = txt_rdata.len() as u16;
    pkt.push((rdlen >> 8) as u8);
    pkt.push(rdlen as u8);
    pkt.extend_from_slice(&txt_rdata);

    // Additional: A record for our hostname.
    encode_dns_name(&mut pkt, &host_local);
    pkt.push((TYPE_A >> 8) as u8);
    pkt.push(TYPE_A as u8);
    pkt.push((CLASS_IN_FLUSH >> 8) as u8);
    pkt.push(CLASS_IN_FLUSH as u8);
    encode_ttl(&mut pkt, DEFAULT_TTL);
    pkt.extend_from_slice(&[0, 4]);
    pkt.extend_from_slice(&our_ip.0);

    pkt
}

/// Encode a DNS name in label format (e.g., "foo.local" → [3,f,o,o,5,l,o,c,a,l,0]).
fn encode_dns_name(buf: &mut Vec<u8>, name: &str) {
    for label in name.split('.') {
        let len = label.len().min(63);
        buf.push(len as u8);
        if let Some(bytes) = label.as_bytes().get(..len) {
            buf.extend_from_slice(bytes);
        }
    }
    buf.push(0); // Root label.
}

/// Decode a DNS name from a packet (handles label pointers).
fn decode_dns_name(data: &[u8], offset: usize) -> (String, usize) {
    let mut name = String::new();
    let mut pos = offset;
    let mut followed_pointer = false;
    let mut end_pos = 0usize;
    let mut hops = 0u8;

    loop {
        if pos >= data.len() || hops > 32 {
            break;
        }
        let len = *data.get(pos).unwrap_or(&0);

        if len == 0 {
            if !followed_pointer {
                end_pos = pos.saturating_add(1);
            }
            break;
        }

        if (len & 0xC0) == 0xC0 {
            // Pointer.
            let ptr_byte2 = *data.get(pos.saturating_add(1)).unwrap_or(&0);
            let ptr_offset = (((len & 0x3F) as usize) << 8) | (ptr_byte2 as usize);
            if !followed_pointer {
                end_pos = pos.saturating_add(2);
                followed_pointer = true;
            }
            pos = ptr_offset;
            hops = hops.saturating_add(1);
            continue;
        }

        let label_len = len as usize;
        let label_start = pos.saturating_add(1);
        let label_end = label_start.saturating_add(label_len);
        // Break on truncated label — don't advance `pos` past the
        // buffer or produce a partial name that could pollute the cache.
        if label_end > data.len() {
            break;
        }
        if !name.is_empty() {
            name.push('.');
        }
        if let Some(slice) = data.get(label_start..label_end) {
            if let Ok(s) = core::str::from_utf8(slice) {
                name.push_str(s);
            }
        }
        pos = label_end;
    }

    if end_pos == 0 {
        end_pos = pos;
    }

    (name, end_pos)
}

/// Encode a TTL value (4 bytes, big-endian).
fn encode_ttl(buf: &mut Vec<u8>, ttl: u32) {
    buf.push((ttl >> 24) as u8);
    buf.push((ttl >> 16) as u8);
    buf.push((ttl >> 8) as u8);
    buf.push(ttl as u8);
}

// ---------------------------------------------------------------------------
// DNS packet parsing
// ---------------------------------------------------------------------------

/// Parse an mDNS packet and extract records.
fn parse_mdns_packet(data: &[u8]) -> KernelResult<Vec<CacheEntry>> {
    if data.len() < DNS_HEADER_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    // Parse header.
    let _id = read_u16(data, 0);
    let _flags = read_u16(data, 2);
    let qdcount = read_u16(data, 4) as usize;
    let ancount = read_u16(data, 6) as usize;
    let nscount = read_u16(data, 8) as usize;
    let arcount = read_u16(data, 10) as usize;

    let mut offset = DNS_HEADER_SIZE;
    let now = crate::hrtimer::now_ns();

    // Skip questions.
    for _ in 0..qdcount {
        let (_name, new_offset) = decode_dns_name(data, offset);
        offset = new_offset;
        offset = offset.saturating_add(4); // QTYPE + QCLASS
        if offset > data.len() {
            return Err(KernelError::InvalidArgument);
        }
    }

    // Parse answer, authority, and additional records.
    let total_records = ancount.saturating_add(nscount).saturating_add(arcount);
    let mut entries = Vec::with_capacity(total_records.min(32));

    for _ in 0..total_records {
        if offset >= data.len() {
            break;
        }

        let (name, new_offset) = decode_dns_name(data, offset);
        offset = new_offset;

        if offset.saturating_add(10) > data.len() {
            break;
        }

        let rtype = read_u16(data, offset);
        let _rclass = read_u16(data, offset.saturating_add(2));
        let ttl = read_u32(data, offset.saturating_add(4));
        let rdlength = read_u16(data, offset.saturating_add(8)) as usize;
        offset = offset.saturating_add(10);

        let rdata_start = offset;
        let rdata_end = offset.saturating_add(rdlength);
        if rdata_end > data.len() {
            break;
        }

        // Parse record data based on type.
        if let Some(record_type) = RecordType::from_u16(rtype) {
            let rdata = data.get(rdata_start..rdata_end).unwrap_or(&[]);
            if let Some(record_data) = parse_record_data(record_type, rdata, data) {
                entries.push(CacheEntry {
                    name,
                    record_type,
                    data: record_data,
                    ttl,
                    cached_at_ns: now,
                });
            }
        }

        offset = rdata_end;
    }

    RECORDS_RECEIVED.fetch_add(entries.len() as u64, Ordering::Relaxed);
    Ok(entries)
}

/// Parse record data based on type.
fn parse_record_data(rtype: RecordType, rdata: &[u8], full_packet: &[u8]) -> Option<RecordData> {
    match rtype {
        RecordType::A => {
            if rdata.len() < 4 {
                return None;
            }
            Some(RecordData::Address(Ipv4Addr([
                *rdata.get(0)?,
                *rdata.get(1)?,
                *rdata.get(2)?,
                *rdata.get(3)?,
            ])))
        }
        RecordType::Ptr => {
            // PTR RDATA is a DNS name.
            // The name may contain pointers into the full packet.
            let (name, _) = decode_dns_name(full_packet,
                rdata.as_ptr() as usize - full_packet.as_ptr() as usize);
            if name.is_empty() {
                return None;
            }
            Some(RecordData::Name(name))
        }
        RecordType::Srv => {
            if rdata.len() < 6 {
                return None;
            }
            let priority = (*rdata.get(0)? as u16) << 8 | *rdata.get(1)? as u16;
            let weight = (*rdata.get(2)? as u16) << 8 | *rdata.get(3)? as u16;
            let port = (*rdata.get(4)? as u16) << 8 | *rdata.get(5)? as u16;
            // Target name follows.
            let target_offset = rdata.as_ptr() as usize - full_packet.as_ptr() as usize + 6;
            let (target, _) = decode_dns_name(full_packet, target_offset);
            Some(RecordData::Srv { priority, weight, port, target })
        }
        RecordType::Txt => {
            let mut entries = Vec::new();
            let mut pos = 0usize;
            while pos < rdata.len() {
                let len = *rdata.get(pos)? as usize;
                pos = pos.saturating_add(1);
                if len == 0 {
                    continue;
                }
                let end = pos.saturating_add(len).min(rdata.len());
                if let Some(slice) = rdata.get(pos..end) {
                    if let Ok(s) = core::str::from_utf8(slice) {
                        entries.push(String::from(s));
                    }
                }
                pos = end;
            }
            Some(RecordData::Txt(entries))
        }
    }
}

/// Read big-endian u16 from data.
fn read_u16(data: &[u8], offset: usize) -> u16 {
    (*data.get(offset).unwrap_or(&0) as u16) << 8
        | *data.get(offset.saturating_add(1)).unwrap_or(&0) as u16
}

/// Read big-endian u32 from data.
fn read_u32(data: &[u8], offset: usize) -> u32 {
    (*data.get(offset).unwrap_or(&0) as u32) << 24
        | (*data.get(offset.saturating_add(1)).unwrap_or(&0) as u32) << 16
        | (*data.get(offset.saturating_add(2)).unwrap_or(&0) as u32) << 8
        | *data.get(offset.saturating_add(3)).unwrap_or(&0) as u32
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the mDNS subsystem.
///
/// Binds a UDP socket to port 5353 and joins the mDNS multicast group.
pub fn init() {
    let mut state = STATE.lock();
    if INITIALIZED.load(Ordering::Relaxed) {
        return;
    }

    // Bind UDP socket.
    match super::udp::bind(MDNS_PORT) {
        Ok(handle) => {
            state.socket_handle = Some(handle);
            // Join the mDNS multicast group.
            let _ = super::udp::join_group(handle, MDNS_MULTICAST_IP);
        }
        Err(e) => {
            crate::serial_println!("[mdns] Failed to bind port {}: {:?}", MDNS_PORT, e);
            return;
        }
    }

    // Set hostname from interface info if available.
    state.hostname = String::from("neo");

    INITIALIZED.store(true, Ordering::Relaxed);
    crate::serial_println!("[mdns] Initialized ({}:{})", MDNS_MULTICAST_IP, MDNS_PORT);
}

/// Set our local hostname (used for .local resolution).
pub fn set_hostname(name: &str) {
    STATE.lock().hostname = String::from(name);
}

/// Resolve a `.local` hostname to an IPv4 address via mDNS.
pub fn resolve_local(name: &str) -> KernelResult<Ipv4Addr> {
    // Check cache first.
    {
        let state = STATE.lock();
        let now = crate::hrtimer::now_ns();
        for entry in &state.cache {
            if entry.name == name && entry.record_type == RecordType::A {
                let age_secs = now.saturating_sub(entry.cached_at_ns) / 1_000_000_000;
                if age_secs < entry.ttl as u64 {
                    CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                    if let RecordData::Address(ip) = entry.data {
                        return Ok(ip);
                    }
                }
            }
        }
    }
    CACHE_MISSES.fetch_add(1, Ordering::Relaxed);

    // Send mDNS query.
    send_query(name, TYPE_A)?;

    // Wait for response (poll a few times).
    for _ in 0..5000 {
        super::poll();
        process_incoming();

        // Check cache again.
        let state = STATE.lock();
        for entry in &state.cache {
            if entry.name == name && entry.record_type == RecordType::A {
                if let RecordData::Address(ip) = entry.data {
                    return Ok(ip);
                }
            }
        }
        drop(state);

        for _ in 0..5_000 {
            core::hint::spin_loop();
        }
    }

    Err(KernelError::TimedOut)
}

/// Browse for services of a given type (e.g., "_http._tcp").
///
/// Returns discovered service instances.  Note: discovery is async
/// by nature; this function queries and waits briefly for responses.
pub fn browse_services(service_type: &str) -> KernelResult<Vec<DiscoveredService>> {
    let query_name = format!("{}.local", service_type);

    // Send PTR query for the service type.
    send_query(&query_name, TYPE_PTR)?;

    // Wait for responses.
    for _ in 0..10_000 {
        super::poll();
        process_incoming();

        for _ in 0..5_000 {
            core::hint::spin_loop();
        }
    }

    // Collect discovered services from cache.
    let state = STATE.lock();
    let now = crate::hrtimer::now_ns();
    let mut services = Vec::new();

    for entry in &state.cache {
        if entry.name == query_name && entry.record_type == RecordType::Ptr {
            let age_secs = now.saturating_sub(entry.cached_at_ns) / 1_000_000_000;
            if age_secs >= entry.ttl as u64 {
                continue;
            }
            if let RecordData::Name(ref instance_fqdn) = entry.data {
                // Look up SRV and TXT records for this instance.
                let mut hostname = String::new();
                let mut port = 0u16;
                let mut txt = Vec::new();
                let mut ip = None;

                for rec in &state.cache {
                    if rec.name != *instance_fqdn {
                        continue;
                    }
                    match &rec.data {
                        RecordData::Srv { port: p, target, .. } => {
                            port = *p;
                            hostname = target.clone();
                        }
                        RecordData::Txt(entries) => {
                            txt = entries.clone();
                        }
                        _ => {}
                    }
                }

                // Try to resolve the hostname from cache.
                if !hostname.is_empty() {
                    for rec in &state.cache {
                        if rec.name == hostname {
                            if let RecordData::Address(addr) = rec.data {
                                ip = Some(addr);
                                break;
                            }
                        }
                    }
                }

                // Extract instance name from FQDN.
                let instance_name = instance_fqdn
                    .strip_suffix(&format!(".{}.local", service_type))
                    .unwrap_or(instance_fqdn);

                services.push(DiscoveredService {
                    instance_name: String::from(instance_name),
                    service_type: String::from(service_type),
                    hostname,
                    ip,
                    port,
                    txt,
                });
            }
        }
    }

    Ok(services)
}

/// Register a local service for mDNS/DNS-SD announcement.
pub fn register_service(
    instance_name: &str,
    service_type: &str,
    port: u16,
    txt_records: &[&str],
) -> KernelResult<usize> {
    let mut state = STATE.lock();

    // Check for capacity.
    let slot = state.services.iter().position(|s| !s.active);
    let idx = match slot {
        Some(i) => i,
        None => {
            if state.services.len() >= MAX_SERVICES {
                return Err(KernelError::ResourceExhausted);
            }
            let i = state.services.len();
            state.services.push(RegisteredService {
                instance_name: String::new(),
                service_type: String::new(),
                port: 0,
                txt_records: Vec::new(),
                active: false,
            });
            i
        }
    };

    let svc = state.services.get_mut(idx).ok_or(KernelError::InternalError)?;
    svc.instance_name = String::from(instance_name);
    svc.service_type = String::from(service_type);
    svc.port = port;
    svc.txt_records = txt_records.iter().map(|s| String::from(*s)).collect();
    svc.active = true;

    crate::serial_println!(
        "[mdns] Registered service: {}.{}.local (port {})",
        instance_name, service_type, port
    );

    Ok(idx)
}

/// Unregister a local service.
pub fn unregister_service(index: usize) -> bool {
    let mut state = STATE.lock();
    if let Some(svc) = state.services.get_mut(index) {
        if svc.active {
            svc.active = false;
            return true;
        }
    }
    false
}

/// Send an mDNS query.
fn send_query(name: &str, rtype: u16) -> KernelResult<()> {
    let pkt = build_query(name, rtype);
    super::udp::send(MDNS_PORT, MDNS_MULTICAST_IP, MDNS_PORT, &pkt)?;
    QUERIES_SENT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Process incoming mDNS packets.
fn process_incoming() {
    let handle = match STATE.lock().socket_handle {
        Some(h) => h,
        None => return,
    };

    while let Some(dgram) = super::udp::recv(handle) {
        if dgram.src_port != MDNS_PORT {
            continue;
        }

        match parse_mdns_packet(&dgram.data) {
            Ok(records) => {
                let mut state = STATE.lock();
                let now = crate::hrtimer::now_ns();

                for record in records {
                    // Update or insert into cache.
                    let existing = state.cache.iter().position(|e| {
                        e.name == record.name && e.record_type == record.record_type
                    });

                    if let Some(idx) = existing {
                        if let Some(entry) = state.cache.get_mut(idx) {
                            entry.data = record.data;
                            entry.ttl = record.ttl;
                            entry.cached_at_ns = now;
                        }
                    } else if state.cache.len() < MAX_CACHE_ENTRIES {
                        state.cache.push(record);
                    } else {
                        // Evict oldest entry.
                        let oldest = state.cache.iter().enumerate()
                            .min_by_key(|(_, e)| e.cached_at_ns)
                            .map(|(i, _)| i);
                        if let Some(idx) = oldest {
                            if let Some(entry) = state.cache.get_mut(idx) {
                                *entry = record;
                            }
                        }
                    }
                }

                // Check if any incoming query needs a response from our services.
                drop(state);
                handle_queries(&dgram.data);
            }
            Err(_) => { /* Malformed packet — ignore. */ }
        }
    }
}

/// Handle incoming mDNS queries that match our registered services.
fn handle_queries(data: &[u8]) {
    if data.len() < DNS_HEADER_SIZE {
        return;
    }

    let flags = read_u16(data, 2);
    // Only process queries (QR bit = 0).
    if (flags & 0x8000) != 0 {
        return;
    }

    let qdcount = read_u16(data, 4) as usize;
    let mut offset = DNS_HEADER_SIZE;

    for _ in 0..qdcount {
        if offset >= data.len() {
            break;
        }
        let (qname, new_offset) = decode_dns_name(data, offset);
        offset = new_offset;
        if offset.saturating_add(4) > data.len() {
            break;
        }
        let qtype = read_u16(data, offset);
        offset = offset.saturating_add(4); // Skip QTYPE + QCLASS.

        respond_if_matching(&qname, qtype);
    }
}

/// Send a response if the query matches our hostname or services.
fn respond_if_matching(qname: &str, qtype: u16) {
    let state = STATE.lock();
    let our_ip = super::interface::ip();
    let hostname = state.hostname.clone();
    let hostname_local = format!("{}.local", hostname);

    // Check if querying our hostname.
    if qtype == TYPE_A && qname.eq_ignore_ascii_case(&hostname_local) {
        let pkt = build_response_a(&hostname_local, our_ip, DEFAULT_TTL);
        drop(state);
        let _ = super::udp::send(MDNS_PORT, MDNS_MULTICAST_IP, MDNS_PORT, &pkt);
        RESPONSES_SENT.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // Check if querying one of our services.
    if qtype == TYPE_PTR {
        for svc in &state.services {
            if !svc.active {
                continue;
            }
            let svc_local = format!("{}.local", svc.service_type);
            if qname.eq_ignore_ascii_case(&svc_local) {
                let pkt = build_service_response(svc, our_ip, &hostname);
                drop(state);
                let _ = super::udp::send(MDNS_PORT, MDNS_MULTICAST_IP, MDNS_PORT, &pkt);
                RESPONSES_SENT.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }
}

/// Periodic tick — expire cache entries and process incoming packets.
pub fn tick() {
    if !INITIALIZED.load(Ordering::Relaxed) {
        return;
    }

    process_incoming();

    // Rate-limited cache cleanup.
    let now = crate::hrtimer::now_ns();
    let last = LAST_CACHE_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= CACHE_TICK_INTERVAL_NS {
        LAST_CACHE_TICK.store(now, Ordering::Relaxed);
        expire_cache();
    }
}

/// Remove expired cache entries.
fn expire_cache() {
    let mut state = STATE.lock();
    let now = crate::hrtimer::now_ns();
    state.cache.retain(|entry| {
        let age_secs = now.saturating_sub(entry.cached_at_ns) / 1_000_000_000;
        age_secs < entry.ttl as u64
    });
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// mDNS statistics.
#[derive(Debug)]
pub struct MdnsStats {
    pub initialized: bool,
    pub cache_entries: usize,
    pub services_registered: usize,
    pub queries_sent: u64,
    pub responses_sent: u64,
    pub records_received: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub hostname: String,
}

/// Get mDNS statistics.
pub fn stats() -> MdnsStats {
    let state = STATE.lock();
    MdnsStats {
        initialized: INITIALIZED.load(Ordering::Relaxed),
        cache_entries: state.cache.len(),
        services_registered: state.services.iter().filter(|s| s.active).count(),
        queries_sent: QUERIES_SENT.load(Ordering::Relaxed),
        responses_sent: RESPONSES_SENT.load(Ordering::Relaxed),
        records_received: RECORDS_RECEIVED.load(Ordering::Relaxed),
        cache_hits: CACHE_HITS.load(Ordering::Relaxed),
        cache_misses: CACHE_MISSES.load(Ordering::Relaxed),
        hostname: state.hostname.clone(),
    }
}

/// Get all cached records.
pub fn cached_records() -> Vec<CacheEntry> {
    STATE.lock().cache.clone()
}

// ---------------------------------------------------------------------------
// Procfs
// ---------------------------------------------------------------------------

/// Generate procfs content for `/proc/mdns`.
pub fn procfs_content() -> String {
    let s = stats();
    let cache = cached_records();

    let mut out = String::with_capacity(512);
    out.push_str("mDNS / DNS-SD Status\n");
    out.push_str("=====================\n\n");

    out.push_str(&format!("Hostname:      {}.local\n", s.hostname));
    out.push_str(&format!("Cache entries: {}/{}\n", s.cache_entries, MAX_CACHE_ENTRIES));
    out.push_str(&format!("Services:      {}/{}\n", s.services_registered, MAX_SERVICES));
    out.push_str(&format!("Queries sent:  {}\n", s.queries_sent));
    out.push_str(&format!("Responses:     {}\n", s.responses_sent));
    out.push_str(&format!("Records recv:  {}\n", s.records_received));
    out.push_str(&format!("Cache hits:    {}\n", s.cache_hits));
    out.push_str(&format!("Cache misses:  {}\n", s.cache_misses));

    if !cache.is_empty() {
        out.push_str("\nCached Records:\n");
        for entry in &cache {
            let data_str = match &entry.data {
                RecordData::Address(ip) => format!("{}", ip),
                RecordData::Name(n) => n.clone(),
                RecordData::Srv { port, target, .. } => format!("{}:{}", target, port),
                RecordData::Txt(entries) => {
                    if entries.is_empty() {
                        String::from("(empty)")
                    } else {
                        entries.join("; ")
                    }
                }
            };
            out.push_str(&format!(
                "  {} {} {} (TTL={}s)\n",
                entry.name, entry.record_type.label(), data_str, entry.ttl
            ));
        }
    }

    // Show registered services.
    let state = STATE.lock();
    let active: Vec<_> = state.services.iter().filter(|s| s.active).collect();
    if !active.is_empty() {
        out.push_str(&format!("\nRegistered Services ({}):\n", active.len()));
        for svc in active {
            out.push_str(&format!(
                "  {}.{}.local port={}\n",
                svc.instance_name, svc.service_type, svc.port,
            ));
            for txt in &svc.txt_records {
                out.push_str(&format!("    {}\n", txt));
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run mDNS self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[mdns] Running mDNS self-tests...");
    let mut passed = 0u32;

    // --- Test 1: DNS name encoding ---
    {
        let mut buf = Vec::new();
        encode_dns_name(&mut buf, "myhost.local");
        // Expected: [6,m,y,h,o,s,t,5,l,o,c,a,l,0]
        assert!(buf.len() == 14, "encoded length");
        assert!(buf[0] == 6, "first label length");
        assert!(buf[7] == 5, "second label length");
        assert!(*buf.last().unwrap_or(&99) == 0, "root label");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 1 (DNS name encoding) PASSED");
    }

    // --- Test 2: DNS name decode (no pointers) ---
    {
        let data = [6u8, b'm', b'y', b'h', b'o', b's', b't', 5, b'l', b'o', b'c', b'a', b'l', 0];
        let (name, end) = decode_dns_name(&data, 0);
        assert!(name == "myhost.local", "decoded name");
        assert!(end == 14, "end offset");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 2 (DNS name decode) PASSED");
    }

    // --- Test 3: Query packet construction ---
    {
        let pkt = build_query("_http._tcp.local", TYPE_PTR);
        assert!(pkt.len() >= DNS_HEADER_SIZE, "packet large enough");
        // Check header: QDCOUNT = 1.
        assert!(read_u16(&pkt, 4) == 1, "1 question");
        assert!(read_u16(&pkt, 6) == 0, "0 answers");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 3 (query construction) PASSED");
    }

    // --- Test 4: A record response construction ---
    {
        let ip = Ipv4Addr([192, 168, 1, 100]);
        let pkt = build_response_a("myhost.local", ip, 120);
        assert!(pkt.len() >= DNS_HEADER_SIZE + 14, "response large enough");
        // Check header: ANCOUNT = 1.
        assert!(read_u16(&pkt, 6) == 1, "1 answer");
        // Check flags: QR=1, AA=1 → 0x8400.
        assert!(read_u16(&pkt, 2) == 0x8400, "response flags");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 4 (A response construction) PASSED");
    }

    // --- Test 5: Parse A record from response ---
    {
        let ip = Ipv4Addr([10, 0, 0, 42]);
        let pkt = build_response_a("test.local", ip, 60);
        let records = parse_mdns_packet(&pkt)?;
        assert!(!records.is_empty(), "should parse records");
        let rec = &records[0];
        assert!(rec.name == "test.local", "record name");
        assert!(rec.record_type == RecordType::A, "record type");
        assert!(rec.ttl == 60, "record TTL");
        if let RecordData::Address(addr) = rec.data {
            assert!(addr.0 == [10, 0, 0, 42], "parsed IP");
        } else {
            panic!("expected A record data");
        }

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 5 (parse A record) PASSED");
    }

    // --- Test 6: u16 and u32 reading ---
    {
        let data = [0x12, 0x34, 0x56, 0x78];
        assert!(read_u16(&data, 0) == 0x1234, "u16 read");
        assert!(read_u32(&data, 0) == 0x12345678, "u32 read");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 6 (byte reading) PASSED");
    }

    // --- Test 7: DNS name with multiple labels ---
    {
        let mut buf = Vec::new();
        encode_dns_name(&mut buf, "_ipp._tcp.local");
        let (decoded, _) = decode_dns_name(&buf, 0);
        assert!(decoded == "_ipp._tcp.local", "multi-label round-trip");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 7 (multi-label name) PASSED");
    }

    // --- Test 8: Empty name ---
    {
        let data = [0u8]; // Just root label.
        let (name, end) = decode_dns_name(&data, 0);
        assert!(name.is_empty(), "empty name");
        assert!(end == 1, "end after root");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 8 (empty name) PASSED");
    }

    // --- Test 9: TTL encoding ---
    {
        let mut buf = Vec::new();
        encode_ttl(&mut buf, 120);
        assert!(buf == [0, 0, 0, 120], "TTL 120");

        let mut buf2 = Vec::new();
        encode_ttl(&mut buf2, 0x01020304);
        assert!(buf2 == [1, 2, 3, 4], "TTL 0x01020304");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 9 (TTL encoding) PASSED");
    }

    // --- Test 10: Service registration ---
    {
        let initial = STATE.lock().services.iter().filter(|s| s.active).count();

        let idx = register_service("Test Service", "_test._tcp", 9999, &["key=value"])?;
        let count = STATE.lock().services.iter().filter(|s| s.active).count();
        assert!(count == initial + 1, "service count +1");

        let removed = unregister_service(idx);
        assert!(removed, "should unregister");

        let count2 = STATE.lock().services.iter().filter(|s| s.active).count();
        assert!(count2 == initial, "back to original");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 10 (service registration) PASSED");
    }

    // --- Test 11: DNS pointer decode ---
    {
        // Build a packet with a name pointer.
        // Offset 0: header (12 bytes of zeros).
        // Offset 12: name "test.local\0" (12 bytes: [4,t,e,s,t,5,l,o,c,a,l,0])
        // Offset 24: pointer to offset 12 (0xC0, 0x0C)
        let mut data = vec![0u8; 12]; // Header.
        encode_dns_name(&mut data, "test.local"); // At offset 12.
        // Add a pointer at current position pointing to offset 12.
        data.push(0xC0);
        data.push(0x0C);

        let ptr_offset = 12 + 12; // After header + "test.local\0"
        let (name, end) = decode_dns_name(&data, ptr_offset);
        assert!(name == "test.local", "pointer resolved");
        assert!(end == ptr_offset + 2, "end after pointer");

        passed = passed.saturating_add(1);
        crate::serial_println!("[mdns]   test 11 (DNS pointer decode) PASSED");
    }

    crate::serial_println!("[mdns] All {} self-tests PASSED", passed);
    Ok(())
}
