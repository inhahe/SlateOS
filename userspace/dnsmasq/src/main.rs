//! OurOS Lightweight DNS/DHCP/TFTP Server (dnsmasq)
//!
//! Multi-personality binary providing:
//! - **dnsmasq** (default) -- DNS forwarder + DHCP server + TFTP server
//! - **dnsmasq-dhcp** -- DHCP-only mode
//!
//! Personality is detected from `argv[0]` basename.
//!
//! # Usage
//!
//! ```text
//! dnsmasq [OPTIONS]
//!
//! DNS options:
//!   --port=PORT              DNS listen port (default: 53)
//!   --listen-address=ADDR    Bind to specific address
//!   --no-resolv              Don't read /etc/resolv.conf
//!   --server=IP              Upstream DNS server
//!   --address=/DOMAIN/IP     Return IP for DOMAIN (and subdomains)
//!   --log-queries            Log DNS queries
//!
//! DHCP options:
//!   --dhcp-range=START,END,NETMASK,LEASE
//!                            DHCP address pool
//!   --dhcp-host=MAC,IP       Static DHCP reservation
//!   --dhcp-option=NUM,VALUE  DHCP option to send
//!   --log-dhcp               Log DHCP transactions
//!
//! TFTP options:
//!   --enable-tftp            Enable TFTP server
//!   --tftp-root=DIR          TFTP root directory
//!
//! General options:
//!   --interface=IFACE        Listen on specific interface
//!   --bind-interfaces        Bind to interfaces explicitly
//!   --no-daemon              Run in foreground
//!   --conf-file=FILE         Configuration file path
//!   --conf-dir=DIR           Additional config directory
//!   --pid-file=FILE          PID file path
//!   --help                   Show this help
//!   --version                Show version
//! ```

#![deny(clippy::all)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::env;
use std::fmt;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Default DNS port.
const DEFAULT_DNS_PORT: u16 = 53;

/// Default DHCP server port.
const _DEFAULT_DHCP_SERVER_PORT: u16 = 67;

/// Default DHCP client port.
const _DEFAULT_DHCP_CLIENT_PORT: u16 = 68;

/// Default TFTP port.
const _DEFAULT_TFTP_PORT: u16 = 69;

/// Default configuration file path.
const DEFAULT_CONF_FILE: &str = "/etc/dnsmasq.conf";

/// Default hosts file.
const DEFAULT_HOSTS_FILE: &str = "/etc/hosts";

/// Default resolv.conf path.
const DEFAULT_RESOLV_CONF: &str = "/etc/resolv.conf";

/// Default blocklist directory.
const _DEFAULT_BLOCKLIST_DIR: &str = "/etc/dnsmasq.d/blocklist";

/// Default lease file path.
const DEFAULT_LEASE_FILE: &str = "/var/lib/dnsmasq/dnsmasq.leases";

/// Default DHCP lease time in seconds (1 hour).
const DEFAULT_LEASE_TIME: u64 = 3600;

/// DNS header length.
const DNS_HEADER_LEN: usize = 12;

/// Maximum DNS packet size.
const MAX_DNS_PACKET: usize = 512;

/// Maximum DNS label length.
const MAX_LABEL_LEN: usize = 63;

/// Maximum DNS name length.
const MAX_NAME_LEN: usize = 253;

/// DHCP magic cookie.
const DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

/// DHCP minimum packet length (including magic cookie).
const DHCP_MIN_PACKET_LEN: usize = 240;

/// TFTP data block size.
const TFTP_BLOCK_SIZE: usize = 512;

// ============================================================================
// Personality detection
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Personality {
    /// Full dnsmasq: DNS + DHCP + TFTP
    Dnsmasq,
    /// DHCP-only mode
    DnsmasqDhcp,
}

fn detect_personality(prog: &str) -> Personality {
    if prog.contains("dnsmasq-dhcp") {
        Personality::DnsmasqDhcp
    } else {
        Personality::Dnsmasq
    }
}

// ============================================================================
// IPv4 address (simple, no external deps)
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Ipv4Addr {
    octets: [u8; 4],
}

impl Ipv4Addr {
    const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self { octets: [a, b, c, d] }
    }

    fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 4 {
            return None;
        }
        let mut octets = [0u8; 4];
        for (i, part) in parts.iter().enumerate() {
            octets[i] = part.parse::<u8>().ok()?;
        }
        Some(Self { octets })
    }

    fn to_u32(self) -> u32 {
        u32::from_be_bytes(self.octets)
    }

    fn from_u32(val: u32) -> Self {
        Self { octets: val.to_be_bytes() }
    }
}

impl fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.octets[0], self.octets[1], self.octets[2], self.octets[3])
    }
}

// ============================================================================
// MAC address
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct MacAddr {
    bytes: [u8; 6],
}

impl MacAddr {
    fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 6 {
            return None;
        }
        let mut bytes = [0u8; 6];
        for (i, part) in parts.iter().enumerate() {
            bytes[i] = u8::from_str_radix(part, 16).ok()?;
        }
        Some(Self { bytes })
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.bytes[0], self.bytes[1], self.bytes[2],
            self.bytes[3], self.bytes[4], self.bytes[5]
        )
    }
}

// ============================================================================
// DNS Record Types
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u16)]
enum DnsRecordType {
    A = 1,
    NS = 2,
    CNAME = 5,
    SOA = 6,
    PTR = 12,
    MX = 15,
    TXT = 16,
    AAAA = 28,
    Unknown = 0,
}

impl DnsRecordType {
    fn from_u16(v: u16) -> Self {
        match v {
            1 => Self::A,
            2 => Self::NS,
            5 => Self::CNAME,
            6 => Self::SOA,
            12 => Self::PTR,
            15 => Self::MX,
            16 => Self::TXT,
            28 => Self::AAAA,
            _ => Self::Unknown,
        }
    }

    fn to_u16(self) -> u16 {
        match self {
            Self::A => 1,
            Self::NS => 2,
            Self::CNAME => 5,
            Self::SOA => 6,
            Self::PTR => 12,
            Self::MX => 15,
            Self::TXT => 16,
            Self::AAAA => 28,
            Self::Unknown => 0,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::NS => "NS",
            Self::CNAME => "CNAME",
            Self::SOA => "SOA",
            Self::PTR => "PTR",
            Self::MX => "MX",
            Self::TXT => "TXT",
            Self::AAAA => "AAAA",
            Self::Unknown => "UNKNOWN",
        }
    }
}

// ============================================================================
// DNS Class
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
enum DnsClass {
    IN = 1,
    _CH = 3,
    _HS = 4,
    Unknown = 0,
}

impl DnsClass {
    fn from_u16(v: u16) -> Self {
        match v {
            1 => Self::IN,
            3 => Self::_CH,
            4 => Self::_HS,
            _ => Self::Unknown,
        }
    }

    fn to_u16(self) -> u16 {
        match self {
            Self::IN => 1,
            Self::_CH => 3,
            Self::_HS => 4,
            Self::Unknown => 0,
        }
    }
}

// ============================================================================
// DNS Response Codes
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum DnsRcode {
    NoError = 0,
    FormErr = 1,
    ServFail = 2,
    NxDomain = 3,
    NotImp = 4,
    Refused = 5,
}

impl DnsRcode {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::NoError,
            1 => Self::FormErr,
            2 => Self::ServFail,
            3 => Self::NxDomain,
            4 => Self::NotImp,
            5 => Self::Refused,
        _ => Self::ServFail,
        }
    }
}

// ============================================================================
// DNS Header
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
struct DnsHeader {
    id: u16,
    qr: bool,      // false = query, true = response
    opcode: u8,
    aa: bool,       // authoritative answer
    tc: bool,       // truncated
    rd: bool,       // recursion desired
    ra: bool,       // recursion available
    rcode: DnsRcode,
    qdcount: u16,
    ancount: u16,
    nscount: u16,
    arcount: u16,
}

impl DnsHeader {
    fn new_query(id: u16) -> Self {
        Self {
            id,
            qr: false,
            opcode: 0,
            aa: false,
            tc: false,
            rd: true,
            ra: false,
            rcode: DnsRcode::NoError,
            qdcount: 1,
            ancount: 0,
            nscount: 0,
            arcount: 0,
        }
    }

    fn new_response(id: u16, rcode: DnsRcode, ancount: u16) -> Self {
        Self {
            id,
            qr: true,
            opcode: 0,
            aa: false,
            tc: false,
            rd: true,
            ra: true,
            rcode,
            qdcount: 1,
            ancount,
            nscount: 0,
            arcount: 0,
        }
    }

    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < DNS_HEADER_LEN {
            return None;
        }
        let id = u16::from_be_bytes([data[0], data[1]]);
        let flags1 = data[2];
        let flags2 = data[3];
        let qr = (flags1 & 0x80) != 0;
        let opcode = (flags1 >> 3) & 0x0F;
        let aa = (flags1 & 0x04) != 0;
        let tc = (flags1 & 0x02) != 0;
        let rd = (flags1 & 0x01) != 0;
        let ra = (flags2 & 0x80) != 0;
        let rcode = DnsRcode::from_u8(flags2 & 0x0F);
        let qdcount = u16::from_be_bytes([data[4], data[5]]);
        let ancount = u16::from_be_bytes([data[6], data[7]]);
        let nscount = u16::from_be_bytes([data[8], data[9]]);
        let arcount = u16::from_be_bytes([data[10], data[11]]);
        Some(Self {
            id, qr, opcode, aa, tc, rd, ra, rcode,
            qdcount, ancount, nscount, arcount,
        })
    }

    fn serialize(&self) -> [u8; DNS_HEADER_LEN] {
        let mut buf = [0u8; DNS_HEADER_LEN];
        buf[0..2].copy_from_slice(&self.id.to_be_bytes());

        let mut flags1: u8 = 0;
        if self.qr { flags1 |= 0x80; }
        flags1 |= (self.opcode & 0x0F) << 3;
        if self.aa { flags1 |= 0x04; }
        if self.tc { flags1 |= 0x02; }
        if self.rd { flags1 |= 0x01; }
        buf[2] = flags1;

        let mut flags2: u8 = 0;
        if self.ra { flags2 |= 0x80; }
        flags2 |= self.rcode as u8 & 0x0F;
        buf[3] = flags2;

        buf[4..6].copy_from_slice(&self.qdcount.to_be_bytes());
        buf[6..8].copy_from_slice(&self.ancount.to_be_bytes());
        buf[8..10].copy_from_slice(&self.nscount.to_be_bytes());
        buf[10..12].copy_from_slice(&self.arcount.to_be_bytes());
        buf
    }
}

// ============================================================================
// DNS Name encoding/decoding
// ============================================================================

/// Encode a domain name into DNS wire format (label-length encoding).
fn dns_encode_name(name: &str) -> Option<Vec<u8>> {
    if name.is_empty() {
        return Some(vec![0]);
    }
    let name = name.trim_end_matches('.');
    if name.len() > MAX_NAME_LEN {
        return None;
    }
    let mut result = Vec::new();
    for label in name.split('.') {
        if label.is_empty() || label.len() > MAX_LABEL_LEN {
            return None;
        }
        result.push(label.len() as u8);
        result.extend_from_slice(label.as_bytes());
    }
    result.push(0);
    Some(result)
}

/// Decode a DNS wire-format name starting at `offset` in `data`.
/// Returns the decoded name string and the new offset after the name.
fn dns_decode_name(data: &[u8], offset: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut pos = offset;
    let mut jumped = false;
    let mut end_pos = 0;
    let mut seen_pointers = 0;

    loop {
        if pos >= data.len() {
            return None;
        }
        let len = data[pos] as usize;

        if len == 0 {
            if !jumped {
                end_pos = pos + 1;
            }
            break;
        }

        // Compression pointer
        if (len & 0xC0) == 0xC0 {
            if pos + 1 >= data.len() {
                return None;
            }
            if !jumped {
                end_pos = pos + 2;
            }
            let ptr = ((len & 0x3F) << 8) | (data[pos + 1] as usize);
            if ptr >= data.len() {
                return None;
            }
            pos = ptr;
            jumped = true;
            seen_pointers += 1;
            if seen_pointers > 10 {
                // Prevent infinite pointer loops
                return None;
            }
            continue;
        }

        if len > MAX_LABEL_LEN {
            return None;
        }
        pos += 1;
        if pos + len > data.len() {
            return None;
        }
        let label = std::str::from_utf8(&data[pos..pos + len]).ok()?;
        labels.push(label.to_string());
        pos += len;
    }

    let name = labels.join(".");
    Some((name, end_pos))
}

// ============================================================================
// DNS Question
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
struct DnsQuestion {
    name: String,
    qtype: DnsRecordType,
    qclass: DnsClass,
}

impl DnsQuestion {
    fn parse(data: &[u8], offset: usize) -> Option<(Self, usize)> {
        let (name, pos) = dns_decode_name(data, offset)?;
        if pos + 4 > data.len() {
            return None;
        }
        let qtype = DnsRecordType::from_u16(u16::from_be_bytes([data[pos], data[pos + 1]]));
        let qclass = DnsClass::from_u16(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        Some((Self { name, qtype, qclass }, pos + 4))
    }

    fn serialize(&self) -> Option<Vec<u8>> {
        let mut buf = dns_encode_name(&self.name)?;
        buf.extend_from_slice(&self.qtype.to_u16().to_be_bytes());
        buf.extend_from_slice(&self.qclass.to_u16().to_be_bytes());
        Some(buf)
    }
}

// ============================================================================
// DNS Resource Record
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
struct DnsRecord {
    name: String,
    rtype: DnsRecordType,
    rclass: DnsClass,
    ttl: u32,
    rdata: Vec<u8>,
}

impl DnsRecord {
    fn new_a(name: &str, ttl: u32, addr: Ipv4Addr) -> Self {
        Self {
            name: name.to_string(),
            rtype: DnsRecordType::A,
            rclass: DnsClass::IN,
            ttl,
            rdata: addr.octets.to_vec(),
        }
    }

    fn new_aaaa(name: &str, ttl: u32, addr: &[u8; 16]) -> Self {
        Self {
            name: name.to_string(),
            rtype: DnsRecordType::AAAA,
            rclass: DnsClass::IN,
            ttl,
            rdata: addr.to_vec(),
        }
    }

    fn new_cname(name: &str, ttl: u32, target: &str) -> Option<Self> {
        let rdata = dns_encode_name(target)?;
        Some(Self {
            name: name.to_string(),
            rtype: DnsRecordType::CNAME,
            rclass: DnsClass::IN,
            ttl,
            rdata,
        })
    }

    fn new_mx(name: &str, ttl: u32, priority: u16, exchange: &str) -> Option<Self> {
        let mut rdata = priority.to_be_bytes().to_vec();
        rdata.extend(dns_encode_name(exchange)?);
        Some(Self {
            name: name.to_string(),
            rtype: DnsRecordType::MX,
            rclass: DnsClass::IN,
            ttl,
            rdata,
        })
    }

    fn new_ns(name: &str, ttl: u32, nsdname: &str) -> Option<Self> {
        let rdata = dns_encode_name(nsdname)?;
        Some(Self {
            name: name.to_string(),
            rtype: DnsRecordType::NS,
            rclass: DnsClass::IN,
            ttl,
            rdata,
        })
    }

    fn new_ptr(name: &str, ttl: u32, ptrdname: &str) -> Option<Self> {
        let rdata = dns_encode_name(ptrdname)?;
        Some(Self {
            name: name.to_string(),
            rtype: DnsRecordType::PTR,
            rclass: DnsClass::IN,
            ttl,
            rdata,
        })
    }

    fn new_txt(name: &str, ttl: u32, text: &str) -> Self {
        let mut rdata = Vec::new();
        // TXT records: one or more length-prefixed strings
        let bytes = text.as_bytes();
        let mut offset = 0;
        while offset < bytes.len() {
            let chunk_len = std::cmp::min(255, bytes.len() - offset);
            rdata.push(chunk_len as u8);
            rdata.extend_from_slice(&bytes[offset..offset + chunk_len]);
            offset += chunk_len;
        }
        if rdata.is_empty() {
            rdata.push(0);
        }
        Self {
            name: name.to_string(),
            rtype: DnsRecordType::TXT,
            rclass: DnsClass::IN,
            ttl,
            rdata,
        }
    }

    fn new_soa(
        name: &str, ttl: u32,
        mname: &str, rname: &str,
        serial: u32, refresh: u32, retry: u32,
        expire: u32, minimum: u32,
    ) -> Option<Self> {
        let mut rdata = dns_encode_name(mname)?;
        rdata.extend(dns_encode_name(rname)?);
        rdata.extend_from_slice(&serial.to_be_bytes());
        rdata.extend_from_slice(&refresh.to_be_bytes());
        rdata.extend_from_slice(&retry.to_be_bytes());
        rdata.extend_from_slice(&expire.to_be_bytes());
        rdata.extend_from_slice(&minimum.to_be_bytes());
        Some(Self {
            name: name.to_string(),
            rtype: DnsRecordType::SOA,
            rclass: DnsClass::IN,
            ttl,
            rdata,
        })
    }

    fn parse(data: &[u8], offset: usize) -> Option<(Self, usize)> {
        let (name, pos) = dns_decode_name(data, offset)?;
        if pos + 10 > data.len() {
            return None;
        }
        let rtype = DnsRecordType::from_u16(u16::from_be_bytes([data[pos], data[pos + 1]]));
        let rclass = DnsClass::from_u16(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        let ttl = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let rdlength = u16::from_be_bytes([data[pos + 8], data[pos + 9]]) as usize;
        let rdata_start = pos + 10;
        if rdata_start + rdlength > data.len() {
            return None;
        }
        let rdata = data[rdata_start..rdata_start + rdlength].to_vec();
        Some((Self { name, rtype, rclass, ttl, rdata }, rdata_start + rdlength))
    }

    fn serialize(&self) -> Option<Vec<u8>> {
        let mut buf = dns_encode_name(&self.name)?;
        buf.extend_from_slice(&self.rtype.to_u16().to_be_bytes());
        buf.extend_from_slice(&self.rclass.to_u16().to_be_bytes());
        buf.extend_from_slice(&self.ttl.to_be_bytes());
        buf.extend_from_slice(&(self.rdata.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.rdata);
        Some(buf)
    }
}

// ============================================================================
// DNS Packet
// ============================================================================

#[derive(Clone, Debug)]
struct DnsPacket {
    header: DnsHeader,
    questions: Vec<DnsQuestion>,
    answers: Vec<DnsRecord>,
    authorities: Vec<DnsRecord>,
    additionals: Vec<DnsRecord>,
}

impl DnsPacket {
    fn new_query(id: u16, name: &str, qtype: DnsRecordType) -> Self {
        Self {
            header: DnsHeader::new_query(id),
            questions: vec![DnsQuestion {
                name: name.to_string(),
                qtype,
                qclass: DnsClass::IN,
            }],
            answers: vec![],
            authorities: vec![],
            additionals: vec![],
        }
    }

    fn new_response(query: &DnsPacket, rcode: DnsRcode, answers: Vec<DnsRecord>) -> Self {
        Self {
            header: DnsHeader::new_response(
                query.header.id,
                rcode,
                answers.len() as u16,
            ),
            questions: query.questions.clone(),
            answers,
            authorities: vec![],
            additionals: vec![],
        }
    }

    fn parse(data: &[u8]) -> Option<Self> {
        let header = DnsHeader::parse(data)?;
        let mut offset = DNS_HEADER_LEN;

        let mut questions = Vec::new();
        for _ in 0..header.qdcount {
            let (q, new_offset) = DnsQuestion::parse(data, offset)?;
            questions.push(q);
            offset = new_offset;
        }

        let mut answers = Vec::new();
        for _ in 0..header.ancount {
            let (r, new_offset) = DnsRecord::parse(data, offset)?;
            answers.push(r);
            offset = new_offset;
        }

        let mut authorities = Vec::new();
        for _ in 0..header.nscount {
            let (r, new_offset) = DnsRecord::parse(data, offset)?;
            authorities.push(r);
            offset = new_offset;
        }

        let mut additionals = Vec::new();
        for _ in 0..header.arcount {
            let (r, new_offset) = DnsRecord::parse(data, offset)?;
            additionals.push(r);
            offset = new_offset;
        }

        Some(Self { header, questions, answers, authorities, additionals })
    }

    fn serialize(&self) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.header.serialize());
        for q in &self.questions {
            buf.extend(q.serialize()?);
        }
        for r in &self.answers {
            buf.extend(r.serialize()?);
        }
        for r in &self.authorities {
            buf.extend(r.serialize()?);
        }
        for r in &self.additionals {
            buf.extend(r.serialize()?);
        }
        if buf.len() > MAX_DNS_PACKET {
            // Truncation: in a real server we would set TC bit
        }
        Some(buf)
    }
}

// ============================================================================
// DNS Cache
// ============================================================================

#[derive(Clone, Debug)]
struct DnsCacheEntry {
    records: Vec<DnsRecord>,
    /// Timestamp (simulated seconds since epoch) when this entry was cached.
    cached_at: u64,
    /// Minimum TTL among the records, used for expiration.
    min_ttl: u32,
}

struct DnsCache {
    entries: HashMap<(String, DnsRecordType), DnsCacheEntry>,
    max_entries: usize,
}

impl DnsCache {
    fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
        }
    }

    fn insert(&mut self, name: &str, rtype: DnsRecordType, records: Vec<DnsRecord>, now: u64) {
        if records.is_empty() {
            return;
        }
        let min_ttl = records.iter().map(|r| r.ttl).min().unwrap_or(0);
        if min_ttl == 0 {
            return; // Don't cache zero-TTL records
        }

        // Evict oldest entries if at capacity
        if self.entries.len() >= self.max_entries {
            self.evict_expired(now);
        }
        // If still full, remove the oldest entry
        if self.entries.len() >= self.max_entries {
            let oldest_key = self.entries.iter()
                .min_by_key(|(_, v)| v.cached_at)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest_key {
                self.entries.remove(&key);
            }
        }

        let key = (name.to_lowercase(), rtype);
        self.entries.insert(key, DnsCacheEntry {
            records,
            cached_at: now,
            min_ttl,
        });
    }

    fn lookup(&self, name: &str, rtype: DnsRecordType, now: u64) -> Option<Vec<DnsRecord>> {
        let key = (name.to_lowercase(), rtype);
        let entry = self.entries.get(&key)?;
        let age = now.saturating_sub(entry.cached_at);
        if age >= entry.min_ttl as u64 {
            return None; // Expired
        }
        // Adjust TTLs to reflect remaining time
        let remaining = (entry.min_ttl as u64).saturating_sub(age) as u32;
        let records: Vec<DnsRecord> = entry.records.iter().map(|r| {
            let adj_ttl = if r.ttl > remaining { remaining } else { r.ttl };
            DnsRecord {
                ttl: adj_ttl,
                ..r.clone()
            }
        }).collect();
        Some(records)
    }

    fn evict_expired(&mut self, now: u64) {
        self.entries.retain(|_, entry| {
            let age = now.saturating_sub(entry.cached_at);
            age < entry.min_ttl as u64
        });
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

// ============================================================================
// Hosts file parsing
// ============================================================================

#[derive(Clone, Debug)]
struct HostsEntry {
    ip: Ipv4Addr,
    hostnames: Vec<String>,
}

fn parse_hosts_content(content: &str) -> Vec<HostsEntry> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        // Strip comments
        let line = match line.find('#') {
            Some(pos) => &line[..pos],
            None => line,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let ip_str = match parts.next() {
            Some(s) => s,
            None => continue,
        };
        // Only handle IPv4 for now
        let ip = match Ipv4Addr::from_str(ip_str) {
            Some(ip) => ip,
            None => continue,
        };
        let hostnames: Vec<String> = parts.map(|s| s.to_lowercase()).collect();
        if hostnames.is_empty() {
            continue;
        }
        entries.push(HostsEntry { ip, hostnames });
    }
    entries
}

fn hosts_lookup(entries: &[HostsEntry], name: &str) -> Option<Ipv4Addr> {
    let name_lower = name.to_lowercase();
    for entry in entries {
        for hostname in &entry.hostnames {
            if hostname == &name_lower {
                return Some(entry.ip);
            }
        }
    }
    None
}

// ============================================================================
// Resolv.conf parsing
// ============================================================================

fn parse_resolv_conf(content: &str) -> Vec<Ipv4Addr> {
    let mut servers = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("nameserver") {
            let addr_str = rest.trim();
            if let Some(addr) = Ipv4Addr::from_str(addr_str) {
                servers.push(addr);
            }
        }
    }
    servers
}

// ============================================================================
// Domain blocking
// ============================================================================

struct DomainBlocklist {
    domains: Vec<String>,
}

impl DomainBlocklist {
    fn new() -> Self {
        Self { domains: Vec::new() }
    }

    fn load_from_content(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            self.domains.push(line.to_lowercase());
        }
    }

    fn is_blocked(&self, name: &str) -> bool {
        let name_lower = name.to_lowercase();
        for blocked in &self.domains {
            if name_lower == *blocked {
                return true;
            }
            // Also block subdomains
            if name_lower.ends_with(&format!(".{}", blocked)) {
                return true;
            }
        }
        false
    }

    fn len(&self) -> usize {
        self.domains.len()
    }
}

// ============================================================================
// Address overrides (--address=/domain/ip)
// ============================================================================

#[derive(Clone, Debug)]
struct AddressOverride {
    domain: String,
    ip: Ipv4Addr,
}

fn parse_address_option(opt: &str) -> Option<AddressOverride> {
    // Format: /domain/IP
    let opt = opt.trim_start_matches('/');
    let slash_pos = opt.find('/')?;
    let domain = &opt[..slash_pos];
    let ip_str = &opt[slash_pos + 1..];
    let ip = Ipv4Addr::from_str(ip_str)?;
    Some(AddressOverride {
        domain: domain.to_lowercase(),
        ip,
    })
}

fn check_address_overrides(overrides: &[AddressOverride], name: &str) -> Option<Ipv4Addr> {
    let name_lower = name.to_lowercase();
    for ovr in overrides {
        if name_lower == ovr.domain || name_lower.ends_with(&format!(".{}", ovr.domain)) {
            return Some(ovr.ip);
        }
    }
    None
}

// ============================================================================
// DHCP Message Types
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum DhcpMessageType {
    Discover = 1,
    Offer = 2,
    Request = 3,
    Decline = 4,
    Ack = 5,
    Nak = 6,
    Release = 7,
    Inform = 8,
}

impl DhcpMessageType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Discover),
            2 => Some(Self::Offer),
            3 => Some(Self::Request),
            4 => Some(Self::Decline),
            5 => Some(Self::Ack),
            6 => Some(Self::Nak),
            7 => Some(Self::Release),
            8 => Some(Self::Inform),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Discover => "DISCOVER",
            Self::Offer => "OFFER",
            Self::Request => "REQUEST",
            Self::Decline => "DECLINE",
            Self::Ack => "ACK",
            Self::Nak => "NAK",
            Self::Release => "RELEASE",
            Self::Inform => "INFORM",
        }
    }
}

// ============================================================================
// DHCP Options
// ============================================================================

/// Standard DHCP option codes.
const DHCP_OPT_SUBNET_MASK: u8 = 1;
const DHCP_OPT_ROUTER: u8 = 3;
const DHCP_OPT_DNS_SERVER: u8 = 6;
const DHCP_OPT_DOMAIN_NAME: u8 = 15;
const DHCP_OPT_REQUESTED_IP: u8 = 50;
const DHCP_OPT_LEASE_TIME: u8 = 51;
const DHCP_OPT_MESSAGE_TYPE: u8 = 53;
const DHCP_OPT_SERVER_ID: u8 = 54;
const DHCP_OPT_END: u8 = 255;

#[derive(Clone, Debug, PartialEq, Eq)]
struct DhcpOption {
    code: u8,
    data: Vec<u8>,
}

impl DhcpOption {
    fn new_byte(code: u8, val: u8) -> Self {
        Self { code, data: vec![val] }
    }

    fn new_u32(code: u8, val: u32) -> Self {
        Self { code, data: val.to_be_bytes().to_vec() }
    }

    fn new_ip(code: u8, addr: Ipv4Addr) -> Self {
        Self { code, data: addr.octets.to_vec() }
    }

    fn new_string(code: u8, s: &str) -> Self {
        Self { code, data: s.as_bytes().to_vec() }
    }

    fn get_u8(&self) -> Option<u8> {
        self.data.first().copied()
    }

    fn get_u32(&self) -> Option<u32> {
        if self.data.len() >= 4 {
            Some(u32::from_be_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]))
        } else {
            None
        }
    }

    fn get_ip(&self) -> Option<Ipv4Addr> {
        if self.data.len() >= 4 {
            Some(Ipv4Addr::new(self.data[0], self.data[1], self.data[2], self.data[3]))
        } else {
            None
        }
    }
}

// ============================================================================
// DHCP Packet
// ============================================================================

#[derive(Clone, Debug)]
struct DhcpPacket {
    op: u8,          // 1 = request, 2 = reply
    htype: u8,       // hardware type (1 = Ethernet)
    hlen: u8,        // hardware address length (6 for Ethernet)
    hops: u8,
    xid: u32,        // transaction ID
    secs: u16,
    flags: u16,
    ciaddr: Ipv4Addr, // client IP
    yiaddr: Ipv4Addr, // 'your' (client) IP
    siaddr: Ipv4Addr, // server IP
    giaddr: Ipv4Addr, // gateway IP
    chaddr: [u8; 16], // client hardware address
    sname: [u8; 64],  // server host name
    file: [u8; 128],  // boot file name
    options: Vec<DhcpOption>,
}

impl DhcpPacket {
    fn new() -> Self {
        Self {
            op: 1,
            htype: 1,
            hlen: 6,
            hops: 0,
            xid: 0,
            secs: 0,
            flags: 0,
            ciaddr: Ipv4Addr::new(0, 0, 0, 0),
            yiaddr: Ipv4Addr::new(0, 0, 0, 0),
            siaddr: Ipv4Addr::new(0, 0, 0, 0),
            giaddr: Ipv4Addr::new(0, 0, 0, 0),
            chaddr: [0u8; 16],
            sname: [0u8; 64],
            file: [0u8; 128],
            options: Vec::new(),
        }
    }

    fn client_mac(&self) -> MacAddr {
        let mut bytes = [0u8; 6];
        bytes.copy_from_slice(&self.chaddr[..6]);
        MacAddr { bytes }
    }

    fn set_client_mac(&mut self, mac: MacAddr) {
        self.chaddr[..6].copy_from_slice(&mac.bytes);
    }

    fn get_message_type(&self) -> Option<DhcpMessageType> {
        for opt in &self.options {
            if opt.code == DHCP_OPT_MESSAGE_TYPE {
                return opt.get_u8().and_then(DhcpMessageType::from_u8);
            }
        }
        None
    }

    fn get_requested_ip(&self) -> Option<Ipv4Addr> {
        for opt in &self.options {
            if opt.code == DHCP_OPT_REQUESTED_IP {
                return opt.get_ip();
            }
        }
        None
    }

    fn get_option(&self, code: u8) -> Option<&DhcpOption> {
        self.options.iter().find(|o| o.code == code)
    }

    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < DHCP_MIN_PACKET_LEN {
            return None;
        }
        // Verify magic cookie at offset 236
        if data[236..240] != DHCP_MAGIC_COOKIE {
            return None;
        }

        let mut pkt = Self::new();
        pkt.op = data[0];
        pkt.htype = data[1];
        pkt.hlen = data[2];
        pkt.hops = data[3];
        pkt.xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        pkt.secs = u16::from_be_bytes([data[8], data[9]]);
        pkt.flags = u16::from_be_bytes([data[10], data[11]]);
        pkt.ciaddr = Ipv4Addr::new(data[12], data[13], data[14], data[15]);
        pkt.yiaddr = Ipv4Addr::new(data[16], data[17], data[18], data[19]);
        pkt.siaddr = Ipv4Addr::new(data[20], data[21], data[22], data[23]);
        pkt.giaddr = Ipv4Addr::new(data[24], data[25], data[26], data[27]);
        pkt.chaddr.copy_from_slice(&data[28..44]);
        pkt.sname.copy_from_slice(&data[44..108]);
        pkt.file.copy_from_slice(&data[108..236]);

        // Parse options starting at 240
        let mut pos = 240;
        while pos < data.len() {
            let code = data[pos];
            if code == DHCP_OPT_END {
                break;
            }
            if code == 0 {
                // Padding
                pos += 1;
                continue;
            }
            pos += 1;
            if pos >= data.len() {
                break;
            }
            let len = data[pos] as usize;
            pos += 1;
            if pos + len > data.len() {
                break;
            }
            pkt.options.push(DhcpOption {
                code,
                data: data[pos..pos + len].to_vec(),
            });
            pos += len;
        }

        Some(pkt)
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = vec![0u8; 240];
        buf[0] = self.op;
        buf[1] = self.htype;
        buf[2] = self.hlen;
        buf[3] = self.hops;
        buf[4..8].copy_from_slice(&self.xid.to_be_bytes());
        buf[8..10].copy_from_slice(&self.secs.to_be_bytes());
        buf[10..12].copy_from_slice(&self.flags.to_be_bytes());
        buf[12..16].copy_from_slice(&self.ciaddr.octets);
        buf[16..20].copy_from_slice(&self.yiaddr.octets);
        buf[20..24].copy_from_slice(&self.siaddr.octets);
        buf[24..28].copy_from_slice(&self.giaddr.octets);
        buf[28..44].copy_from_slice(&self.chaddr);
        buf[44..108].copy_from_slice(&self.sname);
        buf[108..236].copy_from_slice(&self.file);
        // Magic cookie
        buf[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);

        // Options
        for opt in &self.options {
            buf.push(opt.code);
            buf.push(opt.data.len() as u8);
            buf.extend_from_slice(&opt.data);
        }
        buf.push(DHCP_OPT_END);

        // Pad to minimum size (300 bytes typical)
        while buf.len() < 300 {
            buf.push(0);
        }
        buf
    }
}

// ============================================================================
// DHCP Lease
// ============================================================================

#[derive(Clone, Debug)]
struct DhcpLease {
    ip: Ipv4Addr,
    mac: MacAddr,
    expires: u64,   // seconds since epoch
    hostname: Option<String>,
}

impl DhcpLease {
    fn is_expired(&self, now: u64) -> bool {
        now >= self.expires
    }

    fn to_lease_line(&self) -> String {
        let hostname = self.hostname.as_deref().unwrap_or("*");
        format!("{} {} {} {}", self.expires, self.mac, self.ip, hostname)
    }

    fn from_lease_line(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return None;
        }
        let expires = parts[0].parse::<u64>().ok()?;
        let mac = MacAddr::from_str(parts[1])?;
        let ip = Ipv4Addr::from_str(parts[2])?;
        let hostname = if parts[3] == "*" { None } else { Some(parts[3].to_string()) };
        Some(Self { ip, mac, expires, hostname })
    }
}

// ============================================================================
// DHCP IP Pool
// ============================================================================

struct DhcpPool {
    range_start: Ipv4Addr,
    range_end: Ipv4Addr,
    subnet_mask: Ipv4Addr,
    lease_time: u64,
    leases: Vec<DhcpLease>,
    static_hosts: HashMap<MacAddr, Ipv4Addr>,
}

impl DhcpPool {
    fn new(start: Ipv4Addr, end: Ipv4Addr, mask: Ipv4Addr, lease_time: u64) -> Self {
        Self {
            range_start: start,
            range_end: end,
            subnet_mask: mask,
            lease_time,
            leases: Vec::new(),
            static_hosts: HashMap::new(),
        }
    }

    fn pool_size(&self) -> u32 {
        let start = self.range_start.to_u32();
        let end = self.range_end.to_u32();
        if end >= start {
            end - start + 1
        } else {
            0
        }
    }

    fn is_in_range(&self, ip: Ipv4Addr) -> bool {
        let val = ip.to_u32();
        val >= self.range_start.to_u32() && val <= self.range_end.to_u32()
    }

    fn add_static_host(&mut self, mac: MacAddr, ip: Ipv4Addr) {
        self.static_hosts.insert(mac, ip);
    }

    fn find_lease_by_mac(&self, mac: &MacAddr) -> Option<&DhcpLease> {
        self.leases.iter().find(|l| l.mac == *mac)
    }

    fn find_lease_by_ip(&self, ip: Ipv4Addr) -> Option<&DhcpLease> {
        self.leases.iter().find(|l| l.ip == ip)
    }

    fn allocate_ip(&self, mac: &MacAddr, now: u64) -> Option<Ipv4Addr> {
        // Check static reservations first
        if let Some(&ip) = self.static_hosts.get(mac) {
            return Some(ip);
        }

        // Check for existing lease for this MAC
        if let Some(lease) = self.find_lease_by_mac(mac)
            && !lease.is_expired(now) {
                return Some(lease.ip);
            }

        // Find a free IP in the range
        let start = self.range_start.to_u32();
        let end = self.range_end.to_u32();
        for addr_u32 in start..=end {
            let addr = Ipv4Addr::from_u32(addr_u32);
            // Check if this IP is in use by a non-expired lease
            let in_use = self.leases.iter().any(|l| l.ip == addr && !l.is_expired(now));
            // Check if it's a static reservation for a different MAC
            let reserved = self.static_hosts.iter().any(|(m, &ip)| ip == addr && m != mac);
            if !in_use && !reserved {
                return Some(addr);
            }
        }
        None
    }

    fn create_or_renew_lease(&mut self, mac: MacAddr, ip: Ipv4Addr, now: u64, hostname: Option<String>) {
        let expires = now + self.lease_time;
        // Remove any existing lease for this MAC
        self.leases.retain(|l| l.mac != mac);
        self.leases.push(DhcpLease { ip, mac, expires, hostname });
    }

    fn release_lease(&mut self, mac: &MacAddr) -> bool {
        let len_before = self.leases.len();
        self.leases.retain(|l| l.mac != *mac);
        self.leases.len() < len_before
    }

    fn cleanup_expired(&mut self, now: u64) -> usize {
        let len_before = self.leases.len();
        self.leases.retain(|l| !l.is_expired(now));
        len_before - self.leases.len()
    }

    fn active_lease_count(&self, now: u64) -> usize {
        self.leases.iter().filter(|l| !l.is_expired(now)).count()
    }

    fn serialize_leases(&self) -> String {
        let mut output = String::new();
        for lease in &self.leases {
            output.push_str(&lease.to_lease_line());
            output.push('\n');
        }
        output
    }

    fn load_leases(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(lease) = DhcpLease::from_lease_line(line) {
                self.leases.push(lease);
            }
        }
    }
}

// ============================================================================
// DHCP Server State Machine
// ============================================================================

struct DhcpServer {
    pool: DhcpPool,
    server_ip: Ipv4Addr,
    router: Option<Ipv4Addr>,
    dns_servers: Vec<Ipv4Addr>,
    domain_name: Option<String>,
    extra_options: Vec<DhcpOption>,
    log_dhcp: bool,
    log_messages: Vec<String>,
}

impl DhcpServer {
    fn new(pool: DhcpPool, server_ip: Ipv4Addr) -> Self {
        Self {
            pool,
            server_ip,
            router: None,
            dns_servers: Vec::new(),
            domain_name: None,
            extra_options: Vec::new(),
            log_dhcp: false,
            log_messages: Vec::new(),
        }
    }

    fn log(&mut self, msg: String) {
        if self.log_dhcp {
            self.log_messages.push(msg);
        }
    }

    fn handle_packet(&mut self, packet: &DhcpPacket, now: u64) -> Option<DhcpPacket> {
        let msg_type = packet.get_message_type()?;
        let mac = packet.client_mac();

        self.log(format!("DHCP {} from {}", msg_type.name(), mac));

        match msg_type {
            DhcpMessageType::Discover => self.handle_discover(packet, now),
            DhcpMessageType::Request => self.handle_request(packet, now),
            DhcpMessageType::Release => {
                self.handle_release(packet);
                None
            }
            DhcpMessageType::Inform => self.handle_inform(packet),
            DhcpMessageType::Decline => {
                self.handle_decline(packet, now);
                None
            }
            _ => None,
        }
    }

    fn handle_discover(&mut self, packet: &DhcpPacket, now: u64) -> Option<DhcpPacket> {
        let mac = packet.client_mac();
        let offered_ip = self.pool.allocate_ip(&mac, now)?;

        self.log(format!("DHCP OFFER {} to {}", offered_ip, mac));

        let mut reply = DhcpPacket::new();
        reply.op = 2;
        reply.xid = packet.xid;
        reply.yiaddr = offered_ip;
        reply.siaddr = self.server_ip;
        reply.chaddr = packet.chaddr;
        reply.flags = packet.flags;

        reply.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Offer as u8));
        reply.options.push(DhcpOption::new_ip(DHCP_OPT_SERVER_ID, self.server_ip));
        reply.options.push(DhcpOption::new_u32(DHCP_OPT_LEASE_TIME, self.pool.lease_time as u32));
        self.add_common_options(&mut reply);

        Some(reply)
    }

    fn handle_request(&mut self, packet: &DhcpPacket, now: u64) -> Option<DhcpPacket> {
        let mac = packet.client_mac();

        // Determine which IP the client is requesting
        let requested_ip = packet.get_requested_ip()
            .or(if packet.ciaddr != Ipv4Addr::new(0, 0, 0, 0) { Some(packet.ciaddr) } else { None });

        let requested_ip = match requested_ip {
            Some(ip) => ip,
            None => {
                // No IP requested, try to allocate one
                match self.pool.allocate_ip(&mac, now) {
                    Some(ip) => ip,
                    None => return self.send_nak(packet, "No IP available"),
                }
            }
        };

        // Validate: is the IP in our range or a static assignment?
        let is_valid = self.pool.is_in_range(requested_ip)
            || self.pool.static_hosts.values().any(|&ip| ip == requested_ip);

        if !is_valid {
            return self.send_nak(packet, "Requested IP not in range");
        }

        // Check if the IP is taken by someone else
        if let Some(existing) = self.pool.find_lease_by_ip(requested_ip)
            && existing.mac != mac && !existing.is_expired(now) {
                return self.send_nak(packet, "IP already in use");
            }

        // Create or renew the lease
        self.pool.create_or_renew_lease(mac, requested_ip, now, None);
        self.log(format!("DHCP ACK {} to {}", requested_ip, mac));

        let mut reply = DhcpPacket::new();
        reply.op = 2;
        reply.xid = packet.xid;
        reply.yiaddr = requested_ip;
        reply.siaddr = self.server_ip;
        reply.chaddr = packet.chaddr;
        reply.flags = packet.flags;

        reply.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Ack as u8));
        reply.options.push(DhcpOption::new_ip(DHCP_OPT_SERVER_ID, self.server_ip));
        reply.options.push(DhcpOption::new_u32(DHCP_OPT_LEASE_TIME, self.pool.lease_time as u32));
        self.add_common_options(&mut reply);

        Some(reply)
    }

    fn handle_release(&mut self, packet: &DhcpPacket) {
        let mac = packet.client_mac();
        if self.pool.release_lease(&mac) {
            self.log(format!("DHCP RELEASE from {}", mac));
        }
    }

    fn handle_decline(&mut self, packet: &DhcpPacket, now: u64) {
        let mac = packet.client_mac();
        if let Some(ip) = packet.get_requested_ip() {
            self.log(format!("DHCP DECLINE of {} from {}", ip, mac));
            // Mark the IP as unavailable by creating a dummy lease with long expiry
            let dummy_mac = MacAddr { bytes: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF] };
            self.pool.create_or_renew_lease(dummy_mac, ip, now, Some("declined".into()));
        }
    }

    fn handle_inform(&mut self, packet: &DhcpPacket) -> Option<DhcpPacket> {
        let mac = packet.client_mac();
        self.log(format!("DHCP INFORM from {} ({})", mac, packet.ciaddr));

        let mut reply = DhcpPacket::new();
        reply.op = 2;
        reply.xid = packet.xid;
        reply.ciaddr = packet.ciaddr;
        reply.siaddr = self.server_ip;
        reply.chaddr = packet.chaddr;
        reply.flags = packet.flags;

        reply.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Ack as u8));
        reply.options.push(DhcpOption::new_ip(DHCP_OPT_SERVER_ID, self.server_ip));
        self.add_common_options(&mut reply);

        Some(reply)
    }

    fn send_nak(&mut self, packet: &DhcpPacket, _reason: &str) -> Option<DhcpPacket> {
        self.log(format!("DHCP NAK to {} ({})", packet.client_mac(), _reason));

        let mut reply = DhcpPacket::new();
        reply.op = 2;
        reply.xid = packet.xid;
        reply.chaddr = packet.chaddr;
        reply.flags = packet.flags;

        reply.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Nak as u8));
        reply.options.push(DhcpOption::new_ip(DHCP_OPT_SERVER_ID, self.server_ip));

        Some(reply)
    }

    fn add_common_options(&self, reply: &mut DhcpPacket) {
        reply.options.push(DhcpOption::new_ip(DHCP_OPT_SUBNET_MASK, self.pool.subnet_mask));

        if let Some(router) = self.router {
            reply.options.push(DhcpOption::new_ip(DHCP_OPT_ROUTER, router));
        }

        if !self.dns_servers.is_empty() {
            let mut data = Vec::new();
            for srv in &self.dns_servers {
                data.extend_from_slice(&srv.octets);
            }
            reply.options.push(DhcpOption { code: DHCP_OPT_DNS_SERVER, data });
        }

        if let Some(ref domain) = self.domain_name {
            reply.options.push(DhcpOption::new_string(DHCP_OPT_DOMAIN_NAME, domain));
        }

        for opt in &self.extra_options {
            reply.options.push(opt.clone());
        }
    }
}

// ============================================================================
// TFTP Packet Types
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
enum TftpOpcode {
    Rrq = 1,
    Wrq = 2,
    Data = 3,
    Ack = 4,
    Error = 5,
}

impl TftpOpcode {
    fn from_u16(v: u16) -> Option<Self> {
        match v {
            1 => Some(Self::Rrq),
            2 => Some(Self::Wrq),
            3 => Some(Self::Data),
            4 => Some(Self::Ack),
            5 => Some(Self::Error),
            _ => None,
        }
    }
}

/// TFTP error codes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
enum TftpError {
    NotDefined = 0,
    FileNotFound = 1,
    AccessViolation = 2,
    DiskFull = 3,
    IllegalOp = 4,
    UnknownTid = 5,
    FileExists = 6,
    NoSuchUser = 7,
}

// ============================================================================
// TFTP Packet
// ============================================================================

#[derive(Clone, Debug)]
enum TftpPacket {
    ReadRequest { filename: String, mode: String },
    WriteRequest { filename: String, mode: String },
    Data { block: u16, data: Vec<u8> },
    Ack { block: u16 },
    Error { code: TftpError, message: String },
}

impl TftpPacket {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 2 {
            return None;
        }
        let opcode = u16::from_be_bytes([data[0], data[1]]);
        let opcode = TftpOpcode::from_u16(opcode)?;

        match opcode {
            TftpOpcode::Rrq | TftpOpcode::Wrq => {
                // filename\0mode\0
                let rest = &data[2..];
                let null1 = rest.iter().position(|&b| b == 0)?;
                let filename = std::str::from_utf8(&rest[..null1]).ok()?.to_string();
                let after_null1 = null1 + 1;
                if after_null1 >= rest.len() {
                    return None;
                }
                let null2 = rest[after_null1..].iter().position(|&b| b == 0)?;
                let mode = std::str::from_utf8(&rest[after_null1..after_null1 + null2]).ok()?.to_string();
                if opcode == TftpOpcode::Rrq {
                    Some(TftpPacket::ReadRequest { filename, mode })
                } else {
                    Some(TftpPacket::WriteRequest { filename, mode })
                }
            }
            TftpOpcode::Data => {
                if data.len() < 4 {
                    return None;
                }
                let block = u16::from_be_bytes([data[2], data[3]]);
                let payload = data[4..].to_vec();
                Some(TftpPacket::Data { block, data: payload })
            }
            TftpOpcode::Ack => {
                if data.len() < 4 {
                    return None;
                }
                let block = u16::from_be_bytes([data[2], data[3]]);
                Some(TftpPacket::Ack { block })
            }
            TftpOpcode::Error => {
                if data.len() < 4 {
                    return None;
                }
                let code_val = u16::from_be_bytes([data[2], data[3]]);
                let code = match code_val {
                    0 => TftpError::NotDefined,
                    1 => TftpError::FileNotFound,
                    2 => TftpError::AccessViolation,
                    3 => TftpError::DiskFull,
                    4 => TftpError::IllegalOp,
                    5 => TftpError::UnknownTid,
                    6 => TftpError::FileExists,
                    7 => TftpError::NoSuchUser,
                    _ => TftpError::NotDefined,
                };
                let msg = if data.len() > 4 {
                    let end = data[4..].iter().position(|&b| b == 0).unwrap_or(data.len() - 4);
                    std::str::from_utf8(&data[4..4 + end]).unwrap_or("").to_string()
                } else {
                    String::new()
                };
                Some(TftpPacket::Error { code, message: msg })
            }
        }
    }

    fn serialize(&self) -> Vec<u8> {
        match self {
            TftpPacket::ReadRequest { filename, mode } => {
                let mut buf = vec![0, 1]; // opcode 1
                buf.extend_from_slice(filename.as_bytes());
                buf.push(0);
                buf.extend_from_slice(mode.as_bytes());
                buf.push(0);
                buf
            }
            TftpPacket::WriteRequest { filename, mode } => {
                let mut buf = vec![0, 2]; // opcode 2
                buf.extend_from_slice(filename.as_bytes());
                buf.push(0);
                buf.extend_from_slice(mode.as_bytes());
                buf.push(0);
                buf
            }
            TftpPacket::Data { block, data } => {
                let mut buf = vec![0, 3]; // opcode 3
                buf.extend_from_slice(&block.to_be_bytes());
                buf.extend_from_slice(data);
                buf
            }
            TftpPacket::Ack { block } => {
                let mut buf = vec![0, 4]; // opcode 4
                buf.extend_from_slice(&block.to_be_bytes());
                buf
            }
            TftpPacket::Error { code, message } => {
                let mut buf = vec![0, 5]; // opcode 5
                buf.extend_from_slice(&(*code as u16).to_be_bytes());
                buf.extend_from_slice(message.as_bytes());
                buf.push(0);
                buf
            }
        }
    }
}

// ============================================================================
// TFTP Server (simulated file serving)
// ============================================================================

struct TftpServer {
    root_dir: String,
    /// Simulated filesystem: filename -> content
    files: HashMap<String, Vec<u8>>,
    enabled: bool,
}

impl TftpServer {
    fn new(root_dir: &str) -> Self {
        Self {
            root_dir: root_dir.to_string(),
            files: HashMap::new(),
            enabled: false,
        }
    }

    fn add_file(&mut self, name: &str, content: Vec<u8>) {
        self.files.insert(name.to_string(), content);
    }

    fn handle_request(&self, packet: &TftpPacket) -> Vec<TftpPacket> {
        if !self.enabled {
            return vec![TftpPacket::Error {
                code: TftpError::NotDefined,
                message: "TFTP server not enabled".to_string(),
            }];
        }

        match packet {
            TftpPacket::ReadRequest { filename, mode } => {
                self.handle_read(filename, mode)
            }
            TftpPacket::WriteRequest { .. } => {
                vec![TftpPacket::Error {
                    code: TftpError::AccessViolation,
                    message: "Write not supported".to_string(),
                }]
            }
            _ => {
                vec![TftpPacket::Error {
                    code: TftpError::IllegalOp,
                    message: "Unexpected packet type".to_string(),
                }]
            }
        }
    }

    fn handle_read(&self, filename: &str, mode: &str) -> Vec<TftpPacket> {
        let mode_lower = mode.to_lowercase();
        if mode_lower != "octet" && mode_lower != "netascii" {
            return vec![TftpPacket::Error {
                code: TftpError::NotDefined,
                message: format!("Unsupported mode: {}", mode),
            }];
        }

        // Prevent path traversal
        if filename.contains("..") {
            return vec![TftpPacket::Error {
                code: TftpError::AccessViolation,
                message: "Path traversal not allowed".to_string(),
            }];
        }

        // Strip leading slashes
        let clean_name = filename.trim_start_matches('/').trim_start_matches('\\');

        let content = match self.files.get(clean_name) {
            Some(data) => data,
            None => {
                return vec![TftpPacket::Error {
                    code: TftpError::FileNotFound,
                    message: format!("File not found: {}", clean_name),
                }];
            }
        };

        // Split content into TFTP_BLOCK_SIZE chunks
        let mut packets = Vec::new();
        let mut block: u16 = 1;
        let mut offset = 0;

        loop {
            let end = std::cmp::min(offset + TFTP_BLOCK_SIZE, content.len());
            let chunk = content[offset..end].to_vec();
            let is_last = chunk.len() < TFTP_BLOCK_SIZE;

            packets.push(TftpPacket::Data { block, data: chunk });

            if is_last {
                break;
            }
            offset = end;
            block = block.wrapping_add(1);
        }

        packets
    }
}

// ============================================================================
// Configuration
// ============================================================================

#[derive(Clone, Debug)]
struct Config {
    port: u16,
    listen_address: Option<String>,
    no_resolv: bool,
    upstream_servers: Vec<Ipv4Addr>,
    address_overrides: Vec<AddressOverride>,
    dhcp_ranges: Vec<DhcpRangeConfig>,
    dhcp_hosts: Vec<DhcpHostConfig>,
    dhcp_options: Vec<DhcpOption>,
    enable_tftp: bool,
    tftp_root: String,
    interface: Option<String>,
    bind_interfaces: bool,
    no_daemon: bool,
    log_queries: bool,
    log_dhcp: bool,
    conf_file: String,
    conf_dir: Option<String>,
    pid_file: Option<String>,
    hosts_file: String,
    resolv_conf: String,
    lease_file: String,
}

#[derive(Clone, Debug)]
struct DhcpRangeConfig {
    start: Ipv4Addr,
    end: Ipv4Addr,
    netmask: Ipv4Addr,
    lease_time: u64,
}

#[derive(Clone, Debug)]
struct DhcpHostConfig {
    mac: MacAddr,
    ip: Ipv4Addr,
}

impl Config {
    fn default_config() -> Self {
        Self {
            port: DEFAULT_DNS_PORT,
            listen_address: None,
            no_resolv: false,
            upstream_servers: Vec::new(),
            address_overrides: Vec::new(),
            dhcp_ranges: Vec::new(),
            dhcp_hosts: Vec::new(),
            dhcp_options: Vec::new(),
            enable_tftp: false,
            tftp_root: "/var/lib/tftpboot".to_string(),
            interface: None,
            bind_interfaces: false,
            no_daemon: false,
            log_queries: false,
            log_dhcp: false,
            conf_file: DEFAULT_CONF_FILE.to_string(),
            conf_dir: None,
            pid_file: None,
            hosts_file: DEFAULT_HOSTS_FILE.to_string(),
            resolv_conf: DEFAULT_RESOLV_CONF.to_string(),
            lease_file: DEFAULT_LEASE_FILE.to_string(),
        }
    }
}

// ============================================================================
// Config file parsing
// ============================================================================

fn parse_config_content(content: &str, config: &mut Config) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        parse_config_line(line, config);
    }
}

fn parse_config_line(line: &str, config: &mut Config) {
    // Handle key=value or key value
    let (key, value) = if let Some(eq_pos) = line.find('=') {
        (&line[..eq_pos], line[eq_pos + 1..].trim())
    } else {
        let mut parts = line.splitn(2, char::is_whitespace);
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("").trim();
        (key, val)
    };

    match key {
        "port" => {
            if let Ok(p) = value.parse::<u16>() {
                config.port = p;
            }
        }
        "listen-address" => {
            config.listen_address = Some(value.to_string());
        }
        "no-resolv" => {
            config.no_resolv = true;
        }
        "server" => {
            if let Some(addr) = Ipv4Addr::from_str(value) {
                config.upstream_servers.push(addr);
            }
        }
        "address" => {
            if let Some(ovr) = parse_address_option(value) {
                config.address_overrides.push(ovr);
            }
        }
        "dhcp-range" => {
            if let Some(range) = parse_dhcp_range(value) {
                config.dhcp_ranges.push(range);
            }
        }
        "dhcp-host" => {
            if let Some(host) = parse_dhcp_host(value) {
                config.dhcp_hosts.push(host);
            }
        }
        "dhcp-option" => {
            if let Some(opt) = parse_dhcp_option(value) {
                config.dhcp_options.push(opt);
            }
        }
        "enable-tftp" => {
            config.enable_tftp = true;
        }
        "tftp-root" => {
            config.tftp_root = value.to_string();
        }
        "interface" => {
            config.interface = Some(value.to_string());
        }
        "bind-interfaces" => {
            config.bind_interfaces = true;
        }
        "no-daemon" => {
            config.no_daemon = true;
        }
        "log-queries" => {
            config.log_queries = true;
        }
        "log-dhcp" => {
            config.log_dhcp = true;
        }
        "conf-file" => {
            config.conf_file = value.to_string();
        }
        "conf-dir" => {
            config.conf_dir = Some(value.to_string());
        }
        "pid-file" => {
            config.pid_file = Some(value.to_string());
        }
        _ => {
            // Unknown directive, skip
        }
    }
}

fn parse_dhcp_range(value: &str) -> Option<DhcpRangeConfig> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() < 2 {
        return None;
    }
    let start = Ipv4Addr::from_str(parts[0].trim())?;
    let end = Ipv4Addr::from_str(parts[1].trim())?;
    let netmask = if parts.len() > 2 {
        Ipv4Addr::from_str(parts[2].trim()).unwrap_or(Ipv4Addr::new(255, 255, 255, 0))
    } else {
        Ipv4Addr::new(255, 255, 255, 0)
    };
    let lease_time = if parts.len() > 3 {
        parse_lease_time(parts[3].trim())
    } else {
        DEFAULT_LEASE_TIME
    };
    Some(DhcpRangeConfig { start, end, netmask, lease_time })
}

fn parse_dhcp_host(value: &str) -> Option<DhcpHostConfig> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() < 2 {
        return None;
    }
    let mac = MacAddr::from_str(parts[0].trim())?;
    let ip = Ipv4Addr::from_str(parts[1].trim())?;
    Some(DhcpHostConfig { mac, ip })
}

fn parse_dhcp_option(value: &str) -> Option<DhcpOption> {
    let parts: Vec<&str> = value.splitn(2, ',').collect();
    if parts.len() < 2 {
        return None;
    }
    let code = parts[0].trim().parse::<u8>().ok()?;
    let val = parts[1].trim();

    // Try parsing as an IP address
    if let Some(addr) = Ipv4Addr::from_str(val) {
        return Some(DhcpOption::new_ip(code, addr));
    }
    // Try parsing as a number
    if let Ok(num) = val.parse::<u32>() {
        return Some(DhcpOption::new_u32(code, num));
    }
    // Treat as string
    Some(DhcpOption::new_string(code, val))
}

fn parse_lease_time(s: &str) -> u64 {
    let s = s.trim();
    if s.is_empty() {
        return DEFAULT_LEASE_TIME;
    }
    let last = s.as_bytes()[s.len() - 1];
    if last.is_ascii_digit() {
        return s.parse::<u64>().unwrap_or(DEFAULT_LEASE_TIME);
    }
    let num_str = &s[..s.len() - 1];
    let num = num_str.parse::<u64>().unwrap_or(DEFAULT_LEASE_TIME);
    match last {
        b's' => num,
        b'm' => num * 60,
        b'h' => num * 3600,
        b'd' => num * 86400,
        b'w' => num * 604800,
        _ => num,
    }
}

// ============================================================================
// CLI argument parsing
// ============================================================================

fn parse_cli_args(args: &[String], config: &mut Config) -> Result<(), String> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" || arg == "-h" {
            return Err("HELP".to_string());
        }
        if arg == "--version" || arg == "-V" {
            return Err("VERSION".to_string());
        }

        // Handle --key=value and --key value
        let (key, value) = if let Some(eq_pos) = arg.find('=') {
            (&arg[..eq_pos], Some(arg[eq_pos + 1..].to_string()))
        } else {
            (arg.as_str(), None)
        };

        match key {
            "--port" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--port requires a value")?;
                config.port = val.parse::<u16>().map_err(|_| "Invalid port number")?;
            }
            "--listen-address" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--listen-address requires a value")?;
                config.listen_address = Some(val);
            }
            "--no-resolv" => {
                config.no_resolv = true;
            }
            "--server" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--server requires a value")?;
                if let Some(addr) = Ipv4Addr::from_str(&val) {
                    config.upstream_servers.push(addr);
                }
            }
            "--address" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--address requires a value")?;
                if let Some(ovr) = parse_address_option(&val) {
                    config.address_overrides.push(ovr);
                }
            }
            "--dhcp-range" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--dhcp-range requires a value")?;
                if let Some(range) = parse_dhcp_range(&val) {
                    config.dhcp_ranges.push(range);
                }
            }
            "--dhcp-host" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--dhcp-host requires a value")?;
                if let Some(host) = parse_dhcp_host(&val) {
                    config.dhcp_hosts.push(host);
                }
            }
            "--dhcp-option" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--dhcp-option requires a value")?;
                if let Some(opt) = parse_dhcp_option(&val) {
                    config.dhcp_options.push(opt);
                }
            }
            "--enable-tftp" => {
                config.enable_tftp = true;
            }
            "--tftp-root" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--tftp-root requires a value")?;
                config.tftp_root = val;
            }
            "--interface" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--interface requires a value")?;
                config.interface = Some(val);
            }
            "--bind-interfaces" => {
                config.bind_interfaces = true;
            }
            "--no-daemon" => {
                config.no_daemon = true;
            }
            "--log-queries" => {
                config.log_queries = true;
            }
            "--log-dhcp" => {
                config.log_dhcp = true;
            }
            "--conf-file" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--conf-file requires a value")?;
                config.conf_file = val;
            }
            "--conf-dir" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--conf-dir requires a value")?;
                config.conf_dir = Some(val);
            }
            "--pid-file" => {
                let val = value.or_else(|| { i += 1; args.get(i).cloned() })
                    .ok_or("--pid-file requires a value")?;
                config.pid_file = Some(val);
            }
            other => {
                return Err(format!("Unknown option: {}", other));
            }
        }

        i += 1;
    }
    Ok(())
}

// ============================================================================
// DNS Query Resolution (simulated)
// ============================================================================

struct DnsResolver {
    cache: DnsCache,
    hosts_entries: Vec<HostsEntry>,
    blocklist: DomainBlocklist,
    address_overrides: Vec<AddressOverride>,
    upstream_servers: Vec<Ipv4Addr>,
    log_queries: bool,
    log_messages: Vec<String>,
}

impl DnsResolver {
    fn new() -> Self {
        Self {
            cache: DnsCache::new(1024),
            hosts_entries: Vec::new(),
            blocklist: DomainBlocklist::new(),
            address_overrides: Vec::new(),
            upstream_servers: Vec::new(),
            log_queries: false,
            log_messages: Vec::new(),
        }
    }

    fn log(&mut self, msg: String) {
        if self.log_queries {
            self.log_messages.push(msg);
        }
    }

    fn resolve(&mut self, packet: &DnsPacket, now: u64) -> DnsPacket {
        if packet.questions.is_empty() {
            return DnsPacket::new_response(packet, DnsRcode::FormErr, vec![]);
        }

        let question = &packet.questions[0];
        let name = &question.name;
        let qtype = question.qtype;

        self.log(format!("query[{}] {} from client", qtype.name(), name));

        // Check blocklist
        if self.blocklist.is_blocked(name) {
            self.log(format!("blocked: {}", name));
            return DnsPacket::new_response(packet, DnsRcode::NxDomain, vec![]);
        }

        // Check address overrides
        if qtype == DnsRecordType::A
            && let Some(ip) = check_address_overrides(&self.address_overrides, name) {
                self.log(format!("address override: {} -> {}", name, ip));
                let record = DnsRecord::new_a(name, 300, ip);
                return DnsPacket::new_response(packet, DnsRcode::NoError, vec![record]);
            }

        // Check /etc/hosts
        if qtype == DnsRecordType::A
            && let Some(ip) = hosts_lookup(&self.hosts_entries, name) {
                self.log(format!("hosts: {} -> {}", name, ip));
                let record = DnsRecord::new_a(name, 0, ip);
                return DnsPacket::new_response(packet, DnsRcode::NoError, vec![record]);
            }

        // Check cache
        if let Some(records) = self.cache.lookup(name, qtype, now) {
            self.log(format!("cached: {} {}", qtype.name(), name));
            return DnsPacket::new_response(packet, DnsRcode::NoError, records);
        }

        // In a real server, we would forward to upstream. Simulate an NXDOMAIN.
        self.log(format!("forwarding: {} {} (simulated nxdomain)", qtype.name(), name));
        DnsPacket::new_response(packet, DnsRcode::NxDomain, vec![])
    }
}

// ============================================================================
// Help / version display
// ============================================================================

fn print_help(personality: Personality) {
    match personality {
        Personality::Dnsmasq => {
            println!("dnsmasq v{} - lightweight DNS/DHCP/TFTP server", VERSION);
            println!();
            println!("Usage: dnsmasq [OPTIONS]");
            println!();
            println!("DNS options:");
            println!("  --port=PORT              DNS listen port (default: 53)");
            println!("  --listen-address=ADDR    Bind to specific address");
            println!("  --no-resolv              Don't read /etc/resolv.conf");
            println!("  --server=IP              Upstream DNS server");
            println!("  --address=/DOMAIN/IP     Return IP for DOMAIN");
            println!("  --log-queries            Log DNS queries");
            println!();
            println!("DHCP options:");
            println!("  --dhcp-range=START,END,NETMASK,LEASE");
            println!("                           DHCP address pool");
            println!("  --dhcp-host=MAC,IP       Static DHCP reservation");
            println!("  --dhcp-option=NUM,VALUE  DHCP option to send");
            println!("  --log-dhcp               Log DHCP transactions");
            println!();
            println!("TFTP options:");
            println!("  --enable-tftp            Enable TFTP server");
            println!("  --tftp-root=DIR          TFTP root directory");
            println!();
            println!("General options:");
            println!("  --interface=IFACE        Listen on specific interface");
            println!("  --bind-interfaces        Bind to interfaces explicitly");
            println!("  --no-daemon              Run in foreground");
            println!("  --conf-file=FILE         Configuration file (default: /etc/dnsmasq.conf)");
            println!("  --conf-dir=DIR           Additional config directory");
            println!("  --pid-file=FILE          PID file path");
            println!("  --help                   Show this help");
            println!("  --version                Show version");
        }
        Personality::DnsmasqDhcp => {
            println!("dnsmasq-dhcp v{} - lightweight DHCP server", VERSION);
            println!();
            println!("Usage: dnsmasq-dhcp [OPTIONS]");
            println!();
            println!("DHCP options:");
            println!("  --dhcp-range=START,END,NETMASK,LEASE");
            println!("                           DHCP address pool");
            println!("  --dhcp-host=MAC,IP       Static DHCP reservation");
            println!("  --dhcp-option=NUM,VALUE  DHCP option to send");
            println!("  --log-dhcp               Log DHCP transactions");
            println!();
            println!("General options:");
            println!("  --interface=IFACE        Listen on specific interface");
            println!("  --no-daemon              Run in foreground");
            println!("  --conf-file=FILE         Configuration file");
            println!("  --pid-file=FILE          PID file path");
            println!("  --help                   Show this help");
            println!("  --version                Show version");
        }
    }
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("dnsmasq");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let personality = detect_personality(&prog_name);

    let mut config = Config::default_config();

    // Parse CLI arguments (skip argv[0])
    let cli_args: Vec<String> = args.into_iter().skip(1).collect();
    match parse_cli_args(&cli_args, &mut config) {
        Ok(()) => {}
        Err(ref e) if e == "HELP" => {
            print_help(personality);
            return;
        }
        Err(ref e) if e == "VERSION" => {
            match personality {
                Personality::Dnsmasq => println!("dnsmasq v{}", VERSION),
                Personality::DnsmasqDhcp => println!("dnsmasq-dhcp v{}", VERSION),
            }
            return;
        }
        Err(e) => {
            eprintln!("dnsmasq: {}", e);
            std::process::exit(1);
        }
    }

    // Print startup banner
    match personality {
        Personality::Dnsmasq => {
            println!("dnsmasq v{} started", VERSION);
            println!("  DNS port: {}", config.port);
            if config.enable_tftp {
                println!("  TFTP root: {}", config.tftp_root);
            }
        }
        Personality::DnsmasqDhcp => {
            println!("dnsmasq-dhcp v{} started", VERSION);
        }
    }

    if !config.dhcp_ranges.is_empty() {
        for range in &config.dhcp_ranges {
            println!("  DHCP range: {} - {} (mask {}, lease {}s)",
                range.start, range.end, range.netmask, range.lease_time);
        }
    }

    if !config.upstream_servers.is_empty() {
        for srv in &config.upstream_servers {
            println!("  Upstream DNS: {}", srv);
        }
    }

    // In a real server, we would bind sockets and enter event loop.
    // For simulation, just report that we're ready.
    println!("dnsmasq: ready (simulation mode - no real network sockets)");
    if config.no_daemon {
        println!("dnsmasq: running in foreground (no-daemon mode)");
    }

    if let Some(ref pid_file) = config.pid_file {
        println!("dnsmasq: pid file would be written to {}", pid_file);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // IPv4 Address tests
    // ========================================================================

    #[test]
    fn ipv4_parse_valid() {
        let addr = Ipv4Addr::from_str("192.168.1.1").unwrap();
        assert_eq!(addr.octets, [192, 168, 1, 1]);
    }

    #[test]
    fn ipv4_parse_zeros() {
        let addr = Ipv4Addr::from_str("0.0.0.0").unwrap();
        assert_eq!(addr.octets, [0, 0, 0, 0]);
    }

    #[test]
    fn ipv4_parse_max() {
        let addr = Ipv4Addr::from_str("255.255.255.255").unwrap();
        assert_eq!(addr.octets, [255, 255, 255, 255]);
    }

    #[test]
    fn ipv4_parse_invalid_octet() {
        assert!(Ipv4Addr::from_str("256.1.1.1").is_none());
    }

    #[test]
    fn ipv4_parse_too_few_parts() {
        assert!(Ipv4Addr::from_str("192.168.1").is_none());
    }

    #[test]
    fn ipv4_parse_too_many_parts() {
        assert!(Ipv4Addr::from_str("192.168.1.1.1").is_none());
    }

    #[test]
    fn ipv4_parse_non_numeric() {
        assert!(Ipv4Addr::from_str("abc.def.ghi.jkl").is_none());
    }

    #[test]
    fn ipv4_display() {
        let addr = Ipv4Addr::new(10, 0, 0, 1);
        assert_eq!(format!("{}", addr), "10.0.0.1");
    }

    #[test]
    fn ipv4_to_u32_and_back() {
        let addr = Ipv4Addr::new(192, 168, 1, 100);
        let val = addr.to_u32();
        let back = Ipv4Addr::from_u32(val);
        assert_eq!(addr, back);
    }

    #[test]
    fn ipv4_u32_ordering() {
        let a = Ipv4Addr::new(192, 168, 1, 1);
        let b = Ipv4Addr::new(192, 168, 1, 2);
        assert!(a.to_u32() < b.to_u32());
    }

    // ========================================================================
    // MAC Address tests
    // ========================================================================

    #[test]
    fn mac_parse_valid() {
        let mac = MacAddr::from_str("aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(mac.bytes, [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    }

    #[test]
    fn mac_parse_uppercase() {
        let mac = MacAddr::from_str("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(mac.bytes, [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    }

    #[test]
    fn mac_parse_invalid_too_short() {
        assert!(MacAddr::from_str("aa:bb:cc").is_none());
    }

    #[test]
    fn mac_parse_invalid_hex() {
        assert!(MacAddr::from_str("gg:hh:ii:jj:kk:ll").is_none());
    }

    #[test]
    fn mac_display() {
        let mac = MacAddr { bytes: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab] };
        assert_eq!(format!("{}", mac), "01:23:45:67:89:ab");
    }

    // ========================================================================
    // DNS Record Type tests
    // ========================================================================

    #[test]
    fn dns_record_type_roundtrip() {
        let types = [
            DnsRecordType::A, DnsRecordType::NS, DnsRecordType::CNAME,
            DnsRecordType::SOA, DnsRecordType::PTR, DnsRecordType::MX,
            DnsRecordType::TXT, DnsRecordType::AAAA,
        ];
        for t in &types {
            assert_eq!(DnsRecordType::from_u16(t.to_u16()), *t);
        }
    }

    #[test]
    fn dns_record_type_unknown() {
        assert_eq!(DnsRecordType::from_u16(999), DnsRecordType::Unknown);
    }

    #[test]
    fn dns_record_type_names() {
        assert_eq!(DnsRecordType::A.name(), "A");
        assert_eq!(DnsRecordType::AAAA.name(), "AAAA");
        assert_eq!(DnsRecordType::CNAME.name(), "CNAME");
        assert_eq!(DnsRecordType::MX.name(), "MX");
    }

    // ========================================================================
    // DNS Class tests
    // ========================================================================

    #[test]
    fn dns_class_in() {
        assert_eq!(DnsClass::from_u16(1), DnsClass::IN);
        assert_eq!(DnsClass::IN.to_u16(), 1);
    }

    #[test]
    fn dns_class_unknown() {
        assert_eq!(DnsClass::from_u16(999), DnsClass::Unknown);
    }

    // ========================================================================
    // DNS Name encoding/decoding tests
    // ========================================================================

    #[test]
    fn dns_encode_simple_name() {
        let encoded = dns_encode_name("example.com").unwrap();
        assert_eq!(encoded, vec![7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]);
    }

    #[test]
    fn dns_encode_root() {
        let encoded = dns_encode_name("").unwrap();
        assert_eq!(encoded, vec![0]);
    }

    #[test]
    fn dns_encode_trailing_dot() {
        let a = dns_encode_name("example.com").unwrap();
        let b = dns_encode_name("example.com.").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn dns_encode_label_too_long() {
        let long_label = "a".repeat(64);
        assert!(dns_encode_name(&long_label).is_none());
    }

    #[test]
    fn dns_encode_empty_label() {
        assert!(dns_encode_name("example..com").is_none());
    }

    #[test]
    fn dns_decode_roundtrip() {
        let name = "www.example.com";
        let encoded = dns_encode_name(name).unwrap();
        let (decoded, end) = dns_decode_name(&encoded, 0).unwrap();
        assert_eq!(decoded, name);
        assert_eq!(end, encoded.len());
    }

    #[test]
    fn dns_decode_with_pointer() {
        // Construct a packet with a pointer
        let mut data = Vec::new();
        // At offset 0: encode "example.com"
        data.extend(dns_encode_name("example.com").unwrap());
        let ptr_offset = data.len();
        // At ptr_offset: encode "www" + pointer to offset 0
        data.push(3); // label length
        data.extend_from_slice(b"www");
        data.push(0xC0); // pointer marker
        data.push(0x00); // pointer to offset 0

        let (decoded, _end) = dns_decode_name(&data, ptr_offset).unwrap();
        assert_eq!(decoded, "www.example.com");
    }

    #[test]
    fn dns_decode_truncated() {
        let data = vec![3, b'w', b'w']; // label says 3 bytes but only 2 available
        assert!(dns_decode_name(&data, 0).is_none());
    }

    #[test]
    fn dns_decode_pointer_loop_protection() {
        // Self-referencing pointer
        let data = vec![0xC0, 0x00];
        assert!(dns_decode_name(&data, 0).is_none());
    }

    // ========================================================================
    // DNS Header tests
    // ========================================================================

    #[test]
    fn dns_header_query_roundtrip() {
        let hdr = DnsHeader::new_query(0x1234);
        let bytes = hdr.serialize();
        let parsed = DnsHeader::parse(&bytes).unwrap();
        assert_eq!(parsed.id, 0x1234);
        assert!(!parsed.qr);
        assert!(parsed.rd);
        assert_eq!(parsed.qdcount, 1);
    }

    #[test]
    fn dns_header_response_roundtrip() {
        let hdr = DnsHeader::new_response(0xABCD, DnsRcode::NoError, 2);
        let bytes = hdr.serialize();
        let parsed = DnsHeader::parse(&bytes).unwrap();
        assert_eq!(parsed.id, 0xABCD);
        assert!(parsed.qr);
        assert!(parsed.ra);
        assert_eq!(parsed.ancount, 2);
    }

    #[test]
    fn dns_header_flags_all_set() {
        let hdr = DnsHeader {
            id: 1, qr: true, opcode: 0, aa: true, tc: true, rd: true,
            ra: true, rcode: DnsRcode::NxDomain,
            qdcount: 1, ancount: 0, nscount: 0, arcount: 0,
        };
        let bytes = hdr.serialize();
        let parsed = DnsHeader::parse(&bytes).unwrap();
        assert!(parsed.qr);
        assert!(parsed.aa);
        assert!(parsed.tc);
        assert!(parsed.rd);
        assert!(parsed.ra);
        assert_eq!(parsed.rcode, DnsRcode::NxDomain);
    }

    #[test]
    fn dns_header_parse_too_short() {
        let data = [0u8; 11]; // need 12
        assert!(DnsHeader::parse(&data).is_none());
    }

    // ========================================================================
    // DNS Question tests
    // ========================================================================

    #[test]
    fn dns_question_roundtrip() {
        let q = DnsQuestion {
            name: "example.com".to_string(),
            qtype: DnsRecordType::A,
            qclass: DnsClass::IN,
        };
        let bytes = q.serialize().unwrap();
        let (parsed, _end) = DnsQuestion::parse(&bytes, 0).unwrap();
        assert_eq!(parsed.name, "example.com");
        assert_eq!(parsed.qtype, DnsRecordType::A);
        assert_eq!(parsed.qclass, DnsClass::IN);
    }

    #[test]
    fn dns_question_aaaa() {
        let q = DnsQuestion {
            name: "ipv6.example.com".to_string(),
            qtype: DnsRecordType::AAAA,
            qclass: DnsClass::IN,
        };
        let bytes = q.serialize().unwrap();
        let (parsed, _) = DnsQuestion::parse(&bytes, 0).unwrap();
        assert_eq!(parsed.qtype, DnsRecordType::AAAA);
    }

    // ========================================================================
    // DNS Record tests
    // ========================================================================

    #[test]
    fn dns_record_a_roundtrip() {
        let rec = DnsRecord::new_a("test.com", 300, Ipv4Addr::new(1, 2, 3, 4));
        let bytes = rec.serialize().unwrap();
        let (parsed, _) = DnsRecord::parse(&bytes, 0).unwrap();
        assert_eq!(parsed.name, "test.com");
        assert_eq!(parsed.rtype, DnsRecordType::A);
        assert_eq!(parsed.ttl, 300);
        assert_eq!(parsed.rdata, vec![1, 2, 3, 4]);
    }

    #[test]
    fn dns_record_aaaa() {
        let addr: [u8; 16] = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let rec = DnsRecord::new_aaaa("v6.test.com", 600, &addr);
        let bytes = rec.serialize().unwrap();
        let (parsed, _) = DnsRecord::parse(&bytes, 0).unwrap();
        assert_eq!(parsed.rtype, DnsRecordType::AAAA);
        assert_eq!(parsed.rdata.len(), 16);
    }

    #[test]
    fn dns_record_cname() {
        let rec = DnsRecord::new_cname("alias.com", 120, "real.com").unwrap();
        assert_eq!(rec.rtype, DnsRecordType::CNAME);
        let bytes = rec.serialize().unwrap();
        let (parsed, _) = DnsRecord::parse(&bytes, 0).unwrap();
        assert_eq!(parsed.name, "alias.com");
        assert_eq!(parsed.ttl, 120);
    }

    #[test]
    fn dns_record_mx() {
        let rec = DnsRecord::new_mx("mail.com", 300, 10, "smtp.mail.com").unwrap();
        assert_eq!(rec.rtype, DnsRecordType::MX);
        let bytes = rec.serialize().unwrap();
        let (parsed, _) = DnsRecord::parse(&bytes, 0).unwrap();
        assert_eq!(parsed.rtype, DnsRecordType::MX);
        // First 2 bytes of rdata = priority
        let priority = u16::from_be_bytes([parsed.rdata[0], parsed.rdata[1]]);
        assert_eq!(priority, 10);
    }

    #[test]
    fn dns_record_ns() {
        let rec = DnsRecord::new_ns("example.com", 3600, "ns1.example.com").unwrap();
        assert_eq!(rec.rtype, DnsRecordType::NS);
        let bytes = rec.serialize().unwrap();
        let (parsed, _) = DnsRecord::parse(&bytes, 0).unwrap();
        assert_eq!(parsed.name, "example.com");
    }

    #[test]
    fn dns_record_ptr() {
        let rec = DnsRecord::new_ptr("1.168.192.in-addr.arpa", 300, "host.local").unwrap();
        assert_eq!(rec.rtype, DnsRecordType::PTR);
    }

    #[test]
    fn dns_record_txt() {
        let rec = DnsRecord::new_txt("info.com", 120, "v=spf1 include:example.com");
        assert_eq!(rec.rtype, DnsRecordType::TXT);
        // TXT rdata starts with length byte
        assert_eq!(rec.rdata[0] as usize, "v=spf1 include:example.com".len());
    }

    #[test]
    fn dns_record_txt_empty() {
        let rec = DnsRecord::new_txt("empty.com", 60, "");
        assert_eq!(rec.rdata, vec![0]);
    }

    #[test]
    fn dns_record_soa() {
        let rec = DnsRecord::new_soa(
            "example.com", 86400,
            "ns1.example.com", "admin.example.com",
            2024010101, 3600, 1800, 604800, 86400,
        ).unwrap();
        assert_eq!(rec.rtype, DnsRecordType::SOA);
    }

    // ========================================================================
    // DNS Packet tests
    // ========================================================================

    #[test]
    fn dns_packet_query_roundtrip() {
        let pkt = DnsPacket::new_query(0x4321, "www.example.com", DnsRecordType::A);
        let bytes = pkt.serialize().unwrap();
        let parsed = DnsPacket::parse(&bytes).unwrap();
        assert_eq!(parsed.header.id, 0x4321);
        assert!(!parsed.header.qr);
        assert_eq!(parsed.questions.len(), 1);
        assert_eq!(parsed.questions[0].name, "www.example.com");
    }

    #[test]
    fn dns_packet_response_roundtrip() {
        let query = DnsPacket::new_query(0x1111, "test.com", DnsRecordType::A);
        let answer = DnsRecord::new_a("test.com", 300, Ipv4Addr::new(10, 0, 0, 1));
        let response = DnsPacket::new_response(&query, DnsRcode::NoError, vec![answer]);
        let bytes = response.serialize().unwrap();
        let parsed = DnsPacket::parse(&bytes).unwrap();
        assert!(parsed.header.qr);
        assert_eq!(parsed.answers.len(), 1);
        assert_eq!(parsed.answers[0].rdata, vec![10, 0, 0, 1]);
    }

    #[test]
    fn dns_packet_multiple_answers() {
        let query = DnsPacket::new_query(0x2222, "multi.com", DnsRecordType::A);
        let answers = vec![
            DnsRecord::new_a("multi.com", 300, Ipv4Addr::new(1, 1, 1, 1)),
            DnsRecord::new_a("multi.com", 300, Ipv4Addr::new(1, 1, 1, 2)),
        ];
        let response = DnsPacket::new_response(&query, DnsRcode::NoError, answers);
        let bytes = response.serialize().unwrap();
        let parsed = DnsPacket::parse(&bytes).unwrap();
        assert_eq!(parsed.answers.len(), 2);
    }

    #[test]
    fn dns_packet_nxdomain() {
        let query = DnsPacket::new_query(0x3333, "nonexistent.com", DnsRecordType::A);
        let response = DnsPacket::new_response(&query, DnsRcode::NxDomain, vec![]);
        let bytes = response.serialize().unwrap();
        let parsed = DnsPacket::parse(&bytes).unwrap();
        assert_eq!(parsed.header.rcode, DnsRcode::NxDomain);
        assert_eq!(parsed.answers.len(), 0);
    }

    #[test]
    fn dns_packet_parse_empty() {
        assert!(DnsPacket::parse(&[]).is_none());
    }

    #[test]
    fn dns_packet_parse_too_short() {
        assert!(DnsPacket::parse(&[0u8; 5]).is_none());
    }

    // ========================================================================
    // DNS Cache tests
    // ========================================================================

    #[test]
    fn cache_insert_and_lookup() {
        let mut cache = DnsCache::new(100);
        let records = vec![DnsRecord::new_a("test.com", 300, Ipv4Addr::new(1, 2, 3, 4))];
        cache.insert("test.com", DnsRecordType::A, records, 1000);
        let result = cache.lookup("test.com", DnsRecordType::A, 1000);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn cache_case_insensitive() {
        let mut cache = DnsCache::new(100);
        let records = vec![DnsRecord::new_a("Test.COM", 300, Ipv4Addr::new(1, 2, 3, 4))];
        cache.insert("Test.COM", DnsRecordType::A, records, 1000);
        assert!(cache.lookup("test.com", DnsRecordType::A, 1000).is_some());
        assert!(cache.lookup("TEST.COM", DnsRecordType::A, 1000).is_some());
    }

    #[test]
    fn cache_ttl_expiry() {
        let mut cache = DnsCache::new(100);
        let records = vec![DnsRecord::new_a("expire.com", 60, Ipv4Addr::new(1, 1, 1, 1))];
        cache.insert("expire.com", DnsRecordType::A, records, 1000);
        // Not expired yet
        assert!(cache.lookup("expire.com", DnsRecordType::A, 1059).is_some());
        // Expired
        assert!(cache.lookup("expire.com", DnsRecordType::A, 1060).is_none());
    }

    #[test]
    fn cache_ttl_adjustment() {
        let mut cache = DnsCache::new(100);
        let records = vec![DnsRecord::new_a("adj.com", 300, Ipv4Addr::new(1, 1, 1, 1))];
        cache.insert("adj.com", DnsRecordType::A, records, 1000);
        let result = cache.lookup("adj.com", DnsRecordType::A, 1100).unwrap();
        // TTL should be adjusted: 300 - 100 = 200
        assert!(result[0].ttl <= 200);
    }

    #[test]
    fn cache_zero_ttl_not_cached() {
        let mut cache = DnsCache::new(100);
        let records = vec![DnsRecord::new_a("zero.com", 0, Ipv4Addr::new(1, 1, 1, 1))];
        cache.insert("zero.com", DnsRecordType::A, records, 1000);
        assert!(cache.lookup("zero.com", DnsRecordType::A, 1000).is_none());
    }

    #[test]
    fn cache_eviction_when_full() {
        let mut cache = DnsCache::new(2);
        let r1 = vec![DnsRecord::new_a("a.com", 300, Ipv4Addr::new(1, 1, 1, 1))];
        let r2 = vec![DnsRecord::new_a("b.com", 300, Ipv4Addr::new(2, 2, 2, 2))];
        let r3 = vec![DnsRecord::new_a("c.com", 300, Ipv4Addr::new(3, 3, 3, 3))];
        cache.insert("a.com", DnsRecordType::A, r1, 1000);
        cache.insert("b.com", DnsRecordType::A, r2, 1001);
        cache.insert("c.com", DnsRecordType::A, r3, 1002);
        // Oldest (a.com) should have been evicted
        assert!(cache.lookup("a.com", DnsRecordType::A, 1002).is_none());
        assert!(cache.lookup("c.com", DnsRecordType::A, 1002).is_some());
    }

    #[test]
    fn cache_different_types_same_name() {
        let mut cache = DnsCache::new(100);
        let a = vec![DnsRecord::new_a("dual.com", 300, Ipv4Addr::new(1, 1, 1, 1))];
        let aaaa_addr: [u8; 16] = [0; 16];
        let aaaa = vec![DnsRecord::new_aaaa("dual.com", 300, &aaaa_addr)];
        cache.insert("dual.com", DnsRecordType::A, a, 1000);
        cache.insert("dual.com", DnsRecordType::AAAA, aaaa, 1000);
        assert!(cache.lookup("dual.com", DnsRecordType::A, 1000).is_some());
        assert!(cache.lookup("dual.com", DnsRecordType::AAAA, 1000).is_some());
    }

    #[test]
    fn cache_evict_expired() {
        let mut cache = DnsCache::new(100);
        let r1 = vec![DnsRecord::new_a("old.com", 10, Ipv4Addr::new(1, 1, 1, 1))];
        let r2 = vec![DnsRecord::new_a("new.com", 600, Ipv4Addr::new(2, 2, 2, 2))];
        cache.insert("old.com", DnsRecordType::A, r1, 1000);
        cache.insert("new.com", DnsRecordType::A, r2, 1000);
        assert_eq!(cache.len(), 2);
        cache.evict_expired(1011);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_clear() {
        let mut cache = DnsCache::new(100);
        let r = vec![DnsRecord::new_a("x.com", 300, Ipv4Addr::new(1, 1, 1, 1))];
        cache.insert("x.com", DnsRecordType::A, r, 1000);
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn cache_empty_records_not_inserted() {
        let mut cache = DnsCache::new(100);
        cache.insert("empty.com", DnsRecordType::A, vec![], 1000);
        assert_eq!(cache.len(), 0);
    }

    // ========================================================================
    // Hosts file tests
    // ========================================================================

    #[test]
    fn hosts_parse_basic() {
        let content = "127.0.0.1 localhost\n192.168.1.1 router gateway\n";
        let entries = parse_hosts_content(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].ip, Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(entries[0].hostnames, vec!["localhost"]);
        assert_eq!(entries[1].hostnames, vec!["router", "gateway"]);
    }

    #[test]
    fn hosts_parse_with_comments() {
        let content = "# This is a comment\n127.0.0.1 localhost # loopback\n";
        let entries = parse_hosts_content(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hostnames, vec!["localhost"]);
    }

    #[test]
    fn hosts_parse_empty_lines() {
        let content = "\n\n127.0.0.1 host1\n\n10.0.0.1 host2\n\n";
        let entries = parse_hosts_content(content);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn hosts_parse_skips_ipv6() {
        let content = "::1 localhost\n127.0.0.1 localhost4\n";
        let entries = parse_hosts_content(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hostnames, vec!["localhost4"]);
    }

    #[test]
    fn hosts_lookup_found() {
        let content = "192.168.1.100 myhost.local\n";
        let entries = parse_hosts_content(content);
        let ip = hosts_lookup(&entries, "myhost.local");
        assert_eq!(ip, Some(Ipv4Addr::new(192, 168, 1, 100)));
    }

    #[test]
    fn hosts_lookup_case_insensitive() {
        let content = "10.0.0.1 MyHost.Local\n";
        let entries = parse_hosts_content(content);
        assert!(hosts_lookup(&entries, "myhost.local").is_some());
        assert!(hosts_lookup(&entries, "MYHOST.LOCAL").is_some());
    }

    #[test]
    fn hosts_lookup_not_found() {
        let content = "127.0.0.1 localhost\n";
        let entries = parse_hosts_content(content);
        assert!(hosts_lookup(&entries, "unknown.host").is_none());
    }

    #[test]
    fn hosts_lookup_multiple_aliases() {
        let content = "10.0.0.1 server srv web\n";
        let entries = parse_hosts_content(content);
        assert!(hosts_lookup(&entries, "server").is_some());
        assert!(hosts_lookup(&entries, "srv").is_some());
        assert!(hosts_lookup(&entries, "web").is_some());
    }

    // ========================================================================
    // Resolv.conf tests
    // ========================================================================

    #[test]
    fn resolv_parse_basic() {
        let content = "nameserver 8.8.8.8\nnameserver 8.8.4.4\n";
        let servers = parse_resolv_conf(content);
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0], Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(servers[1], Ipv4Addr::new(8, 8, 4, 4));
    }

    #[test]
    fn resolv_parse_with_comments() {
        let content = "# DNS\nnameserver 1.1.1.1\n; old\n;nameserver 9.9.9.9\n";
        let servers = parse_resolv_conf(content);
        assert_eq!(servers.len(), 1);
    }

    #[test]
    fn resolv_parse_empty() {
        let servers = parse_resolv_conf("");
        assert!(servers.is_empty());
    }

    #[test]
    fn resolv_parse_with_extra_whitespace() {
        let content = "nameserver   1.1.1.1  \n";
        let servers = parse_resolv_conf(content);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0], Ipv4Addr::new(1, 1, 1, 1));
    }

    // ========================================================================
    // Domain blocklist tests
    // ========================================================================

    #[test]
    fn blocklist_basic_block() {
        let mut bl = DomainBlocklist::new();
        bl.load_from_content("ads.example.com\ntracker.net\n");
        assert!(bl.is_blocked("ads.example.com"));
        assert!(bl.is_blocked("tracker.net"));
        assert!(!bl.is_blocked("good.example.com"));
    }

    #[test]
    fn blocklist_subdomain_block() {
        let mut bl = DomainBlocklist::new();
        bl.load_from_content("doubleclick.net\n");
        assert!(bl.is_blocked("ad.doubleclick.net"));
        assert!(bl.is_blocked("sub.ad.doubleclick.net"));
    }

    #[test]
    fn blocklist_case_insensitive() {
        let mut bl = DomainBlocklist::new();
        bl.load_from_content("ADS.example.COM\n");
        assert!(bl.is_blocked("ads.example.com"));
        assert!(bl.is_blocked("ADS.EXAMPLE.COM"));
    }

    #[test]
    fn blocklist_comments_and_empty() {
        let mut bl = DomainBlocklist::new();
        bl.load_from_content("# comment\n\nads.com\n# another\ntracker.com\n");
        assert_eq!(bl.len(), 2);
    }

    #[test]
    fn blocklist_not_blocked() {
        let mut bl = DomainBlocklist::new();
        bl.load_from_content("bad.com\n");
        assert!(!bl.is_blocked("good.com"));
        assert!(!bl.is_blocked("notbad.com")); // not a subdomain of bad.com
    }

    // ========================================================================
    // Address override tests
    // ========================================================================

    #[test]
    fn address_override_parse() {
        let ovr = parse_address_option("/example.com/10.0.0.1").unwrap();
        assert_eq!(ovr.domain, "example.com");
        assert_eq!(ovr.ip, Ipv4Addr::new(10, 0, 0, 1));
    }

    #[test]
    fn address_override_check_exact() {
        let overrides = vec![parse_address_option("/test.com/1.2.3.4").unwrap()];
        assert_eq!(check_address_overrides(&overrides, "test.com"), Some(Ipv4Addr::new(1, 2, 3, 4)));
    }

    #[test]
    fn address_override_check_subdomain() {
        let overrides = vec![parse_address_option("/example.com/5.6.7.8").unwrap()];
        assert_eq!(check_address_overrides(&overrides, "sub.example.com"), Some(Ipv4Addr::new(5, 6, 7, 8)));
    }

    #[test]
    fn address_override_no_match() {
        let overrides = vec![parse_address_option("/example.com/1.1.1.1").unwrap()];
        assert!(check_address_overrides(&overrides, "other.com").is_none());
    }

    #[test]
    fn address_override_parse_invalid() {
        assert!(parse_address_option("noslashes").is_none());
        assert!(parse_address_option("/domain/notanip").is_none());
    }

    // ========================================================================
    // DHCP Message Type tests
    // ========================================================================

    #[test]
    fn dhcp_message_type_roundtrip() {
        for i in 1..=8u8 {
            let mt = DhcpMessageType::from_u8(i).unwrap();
            assert_eq!(mt as u8, i);
        }
    }

    #[test]
    fn dhcp_message_type_invalid() {
        assert!(DhcpMessageType::from_u8(0).is_none());
        assert!(DhcpMessageType::from_u8(9).is_none());
    }

    #[test]
    fn dhcp_message_type_names() {
        assert_eq!(DhcpMessageType::Discover.name(), "DISCOVER");
        assert_eq!(DhcpMessageType::Ack.name(), "ACK");
        assert_eq!(DhcpMessageType::Nak.name(), "NAK");
    }

    // ========================================================================
    // DHCP Option tests
    // ========================================================================

    #[test]
    fn dhcp_option_byte() {
        let opt = DhcpOption::new_byte(53, 1);
        assert_eq!(opt.get_u8(), Some(1));
    }

    #[test]
    fn dhcp_option_u32() {
        let opt = DhcpOption::new_u32(51, 3600);
        assert_eq!(opt.get_u32(), Some(3600));
    }

    #[test]
    fn dhcp_option_ip() {
        let addr = Ipv4Addr::new(192, 168, 1, 1);
        let opt = DhcpOption::new_ip(1, addr);
        assert_eq!(opt.get_ip(), Some(addr));
    }

    #[test]
    fn dhcp_option_string() {
        let opt = DhcpOption::new_string(15, "example.local");
        assert_eq!(opt.data, b"example.local");
    }

    #[test]
    fn dhcp_option_get_u8_empty() {
        let opt = DhcpOption { code: 1, data: vec![] };
        assert!(opt.get_u8().is_none());
    }

    #[test]
    fn dhcp_option_get_u32_too_short() {
        let opt = DhcpOption { code: 1, data: vec![1, 2] };
        assert!(opt.get_u32().is_none());
    }

    #[test]
    fn dhcp_option_get_ip_too_short() {
        let opt = DhcpOption { code: 1, data: vec![1, 2, 3] };
        assert!(opt.get_ip().is_none());
    }

    // ========================================================================
    // DHCP Packet tests
    // ========================================================================

    #[test]
    fn dhcp_packet_serialize_parse_roundtrip() {
        let mut pkt = DhcpPacket::new();
        pkt.xid = 0xDEADBEEF;
        pkt.yiaddr = Ipv4Addr::new(192, 168, 1, 100);
        pkt.set_client_mac(MacAddr { bytes: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF] });
        pkt.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Discover as u8));

        let bytes = pkt.serialize();
        let parsed = DhcpPacket::parse(&bytes).unwrap();
        assert_eq!(parsed.xid, 0xDEADBEEF);
        assert_eq!(parsed.yiaddr, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(parsed.client_mac(), MacAddr { bytes: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF] });
        assert_eq!(parsed.get_message_type(), Some(DhcpMessageType::Discover));
    }

    #[test]
    fn dhcp_packet_parse_too_short() {
        assert!(DhcpPacket::parse(&[0u8; 100]).is_none());
    }

    #[test]
    fn dhcp_packet_parse_bad_cookie() {
        let mut data = vec![0u8; 300];
        data[236] = 0; // wrong cookie
        assert!(DhcpPacket::parse(&data).is_none());
    }

    #[test]
    fn dhcp_packet_multiple_options() {
        let mut pkt = DhcpPacket::new();
        pkt.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Offer as u8));
        pkt.options.push(DhcpOption::new_ip(DHCP_OPT_SUBNET_MASK, Ipv4Addr::new(255, 255, 255, 0)));
        pkt.options.push(DhcpOption::new_ip(DHCP_OPT_ROUTER, Ipv4Addr::new(192, 168, 1, 1)));

        let bytes = pkt.serialize();
        let parsed = DhcpPacket::parse(&bytes).unwrap();
        assert_eq!(parsed.options.len(), 3);
    }

    #[test]
    fn dhcp_packet_get_requested_ip() {
        let mut pkt = DhcpPacket::new();
        pkt.options.push(DhcpOption::new_ip(DHCP_OPT_REQUESTED_IP, Ipv4Addr::new(10, 0, 0, 50)));
        assert_eq!(pkt.get_requested_ip(), Some(Ipv4Addr::new(10, 0, 0, 50)));
    }

    #[test]
    fn dhcp_packet_get_option() {
        let mut pkt = DhcpPacket::new();
        pkt.options.push(DhcpOption::new_string(DHCP_OPT_DOMAIN_NAME, "test.local"));
        let opt = pkt.get_option(DHCP_OPT_DOMAIN_NAME);
        assert!(opt.is_some());
        assert_eq!(opt.unwrap().data, b"test.local");
    }

    // ========================================================================
    // DHCP Lease tests
    // ========================================================================

    #[test]
    fn lease_not_expired() {
        let lease = DhcpLease {
            ip: Ipv4Addr::new(10, 0, 0, 1),
            mac: MacAddr { bytes: [1, 2, 3, 4, 5, 6] },
            expires: 2000,
            hostname: None,
        };
        assert!(!lease.is_expired(1999));
        assert!(lease.is_expired(2000));
        assert!(lease.is_expired(2001));
    }

    #[test]
    fn lease_line_roundtrip() {
        let lease = DhcpLease {
            ip: Ipv4Addr::new(192, 168, 1, 50),
            mac: MacAddr { bytes: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff] },
            expires: 1700000000,
            hostname: Some("myhost".to_string()),
        };
        let line = lease.to_lease_line();
        let parsed = DhcpLease::from_lease_line(&line).unwrap();
        assert_eq!(parsed.ip, lease.ip);
        assert_eq!(parsed.mac, lease.mac);
        assert_eq!(parsed.expires, 1700000000);
        assert_eq!(parsed.hostname.as_deref(), Some("myhost"));
    }

    #[test]
    fn lease_line_no_hostname() {
        let lease = DhcpLease {
            ip: Ipv4Addr::new(10, 0, 0, 1),
            mac: MacAddr { bytes: [1, 2, 3, 4, 5, 6] },
            expires: 9999,
            hostname: None,
        };
        let line = lease.to_lease_line();
        assert!(line.contains(" * ") || line.ends_with(" *"));
        let parsed = DhcpLease::from_lease_line(&line).unwrap();
        assert!(parsed.hostname.is_none());
    }

    #[test]
    fn lease_line_parse_invalid() {
        assert!(DhcpLease::from_lease_line("").is_none());
        assert!(DhcpLease::from_lease_line("only two").is_none());
        assert!(DhcpLease::from_lease_line("notanumber aa:bb:cc:dd:ee:ff 1.2.3.4 host").is_none());
    }

    // ========================================================================
    // DHCP Pool tests
    // ========================================================================

    #[test]
    fn pool_size() {
        let pool = DhcpPool::new(
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 200),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        assert_eq!(pool.pool_size(), 101);
    }

    #[test]
    fn pool_is_in_range() {
        let pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        assert!(pool.is_in_range(Ipv4Addr::new(10, 0, 0, 10)));
        assert!(pool.is_in_range(Ipv4Addr::new(10, 0, 0, 15)));
        assert!(pool.is_in_range(Ipv4Addr::new(10, 0, 0, 20)));
        assert!(!pool.is_in_range(Ipv4Addr::new(10, 0, 0, 9)));
        assert!(!pool.is_in_range(Ipv4Addr::new(10, 0, 0, 21)));
    }

    #[test]
    fn pool_allocate_first_available() {
        let pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mac = MacAddr { bytes: [1, 2, 3, 4, 5, 6] };
        let ip = pool.allocate_ip(&mac, 1000).unwrap();
        assert_eq!(ip, Ipv4Addr::new(10, 0, 0, 10));
    }

    #[test]
    fn pool_allocate_static_host() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mac = MacAddr { bytes: [0xAA, 0xBB, 0xCC, 0, 0, 1] };
        pool.add_static_host(mac, Ipv4Addr::new(10, 0, 0, 50));
        let ip = pool.allocate_ip(&mac, 1000).unwrap();
        assert_eq!(ip, Ipv4Addr::new(10, 0, 0, 50));
    }

    #[test]
    fn pool_allocate_returns_existing_lease() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mac = MacAddr { bytes: [1, 2, 3, 4, 5, 6] };
        pool.create_or_renew_lease(mac, Ipv4Addr::new(10, 0, 0, 15), 1000, None);
        let ip = pool.allocate_ip(&mac, 1000).unwrap();
        assert_eq!(ip, Ipv4Addr::new(10, 0, 0, 15));
    }

    #[test]
    fn pool_allocate_skips_used_ips() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 12),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mac1 = MacAddr { bytes: [1, 1, 1, 1, 1, 1] };
        let mac2 = MacAddr { bytes: [2, 2, 2, 2, 2, 2] };
        pool.create_or_renew_lease(mac1, Ipv4Addr::new(10, 0, 0, 10), 1000, None);
        let ip = pool.allocate_ip(&mac2, 1000).unwrap();
        assert_eq!(ip, Ipv4Addr::new(10, 0, 0, 11));
    }

    #[test]
    fn pool_allocate_exhausted() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mac1 = MacAddr { bytes: [1, 1, 1, 1, 1, 1] };
        let mac2 = MacAddr { bytes: [2, 2, 2, 2, 2, 2] };
        pool.create_or_renew_lease(mac1, Ipv4Addr::new(10, 0, 0, 10), 1000, None);
        assert!(pool.allocate_ip(&mac2, 1000).is_none());
    }

    #[test]
    fn pool_release_lease() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mac = MacAddr { bytes: [1, 2, 3, 4, 5, 6] };
        pool.create_or_renew_lease(mac, Ipv4Addr::new(10, 0, 0, 10), 1000, None);
        assert!(pool.release_lease(&mac));
        assert!(!pool.release_lease(&mac)); // Already released
    }

    #[test]
    fn pool_cleanup_expired() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            100,
        );
        let mac1 = MacAddr { bytes: [1, 1, 1, 1, 1, 1] };
        let mac2 = MacAddr { bytes: [2, 2, 2, 2, 2, 2] };
        pool.create_or_renew_lease(mac1, Ipv4Addr::new(10, 0, 0, 10), 1000, None);
        pool.create_or_renew_lease(mac2, Ipv4Addr::new(10, 0, 0, 11), 1050, None);
        // At time 1101, first lease expired, second still valid
        let expired = pool.cleanup_expired(1101);
        assert_eq!(expired, 1);
        assert_eq!(pool.active_lease_count(1101), 1);
    }

    #[test]
    fn pool_serialize_and_load_leases() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mac = MacAddr { bytes: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF] };
        pool.create_or_renew_lease(mac, Ipv4Addr::new(10, 0, 0, 10), 1000, Some("test-host".into()));
        let serialized = pool.serialize_leases();

        let mut pool2 = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        pool2.load_leases(&serialized);
        assert_eq!(pool2.leases.len(), 1);
        assert_eq!(pool2.leases[0].mac, mac);
        assert_eq!(pool2.leases[0].hostname.as_deref(), Some("test-host"));
    }

    #[test]
    fn pool_active_lease_count() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            100,
        );
        let mac1 = MacAddr { bytes: [1, 1, 1, 1, 1, 1] };
        let mac2 = MacAddr { bytes: [2, 2, 2, 2, 2, 2] };
        pool.create_or_renew_lease(mac1, Ipv4Addr::new(10, 0, 0, 10), 1000, None);
        pool.create_or_renew_lease(mac2, Ipv4Addr::new(10, 0, 0, 11), 1000, None);
        assert_eq!(pool.active_lease_count(1000), 2);
        assert_eq!(pool.active_lease_count(1101), 0);
    }

    // ========================================================================
    // DHCP Server state machine tests
    // ========================================================================

    fn make_test_server() -> DhcpServer {
        let pool = DhcpPool::new(
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(192, 168, 1, 200),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mut server = DhcpServer::new(pool, Ipv4Addr::new(192, 168, 1, 1));
        server.router = Some(Ipv4Addr::new(192, 168, 1, 1));
        server.dns_servers = vec![Ipv4Addr::new(8, 8, 8, 8)];
        server.domain_name = Some("test.local".to_string());
        server.log_dhcp = true;
        server
    }

    fn make_discover(mac: MacAddr, xid: u32) -> DhcpPacket {
        let mut pkt = DhcpPacket::new();
        pkt.op = 1;
        pkt.xid = xid;
        pkt.set_client_mac(mac);
        pkt.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Discover as u8));
        pkt
    }

    fn make_request(mac: MacAddr, xid: u32, requested_ip: Ipv4Addr) -> DhcpPacket {
        let mut pkt = DhcpPacket::new();
        pkt.op = 1;
        pkt.xid = xid;
        pkt.set_client_mac(mac);
        pkt.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Request as u8));
        pkt.options.push(DhcpOption::new_ip(DHCP_OPT_REQUESTED_IP, requested_ip));
        pkt
    }

    #[test]
    fn dhcp_discover_offers_ip() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0, 1] };
        let discover = make_discover(mac, 1);
        let reply = server.handle_packet(&discover, 1000).unwrap();
        assert_eq!(reply.get_message_type(), Some(DhcpMessageType::Offer));
        assert_eq!(reply.yiaddr, Ipv4Addr::new(192, 168, 1, 100));
    }

    #[test]
    fn dhcp_request_acks_ip() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0, 2] };
        let request = make_request(mac, 2, Ipv4Addr::new(192, 168, 1, 100));
        let reply = server.handle_packet(&request, 1000).unwrap();
        assert_eq!(reply.get_message_type(), Some(DhcpMessageType::Ack));
        assert_eq!(reply.yiaddr, Ipv4Addr::new(192, 168, 1, 100));
    }

    #[test]
    fn dhcp_request_out_of_range_naks() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0, 3] };
        let request = make_request(mac, 3, Ipv4Addr::new(10, 0, 0, 1));
        let reply = server.handle_packet(&request, 1000).unwrap();
        assert_eq!(reply.get_message_type(), Some(DhcpMessageType::Nak));
    }

    #[test]
    fn dhcp_release_frees_ip() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0, 4] };

        // First, get a lease
        let request = make_request(mac, 4, Ipv4Addr::new(192, 168, 1, 100));
        server.handle_packet(&request, 1000);

        // Release it
        let mut release = DhcpPacket::new();
        release.op = 1;
        release.xid = 5;
        release.ciaddr = Ipv4Addr::new(192, 168, 1, 100);
        release.set_client_mac(mac);
        release.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Release as u8));
        let reply = server.handle_packet(&release, 1001);
        assert!(reply.is_none()); // Release doesn't generate a reply

        // IP should be available again
        let mac2 = MacAddr { bytes: [0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA] };
        let ip = server.pool.allocate_ip(&mac2, 1002).unwrap();
        assert_eq!(ip, Ipv4Addr::new(192, 168, 1, 100));
    }

    #[test]
    fn dhcp_inform_returns_ack() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0, 5] };
        let mut inform = DhcpPacket::new();
        inform.op = 1;
        inform.xid = 6;
        inform.ciaddr = Ipv4Addr::new(192, 168, 1, 50);
        inform.set_client_mac(mac);
        inform.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Inform as u8));
        let reply = server.handle_packet(&inform, 1000).unwrap();
        assert_eq!(reply.get_message_type(), Some(DhcpMessageType::Ack));
    }

    #[test]
    fn dhcp_common_options_included() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0, 6] };
        let discover = make_discover(mac, 7);
        let reply = server.handle_packet(&discover, 1000).unwrap();

        // Should have subnet mask, router, DNS
        assert!(reply.get_option(DHCP_OPT_SUBNET_MASK).is_some());
        assert!(reply.get_option(DHCP_OPT_ROUTER).is_some());
        assert!(reply.get_option(DHCP_OPT_DNS_SERVER).is_some());
        assert!(reply.get_option(DHCP_OPT_DOMAIN_NAME).is_some());
    }

    #[test]
    fn dhcp_static_host_reservation() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66] };
        server.pool.add_static_host(mac, Ipv4Addr::new(192, 168, 1, 50));

        let discover = make_discover(mac, 8);
        let reply = server.handle_packet(&discover, 1000).unwrap();
        assert_eq!(reply.yiaddr, Ipv4Addr::new(192, 168, 1, 50));
    }

    #[test]
    fn dhcp_decline_marks_ip_unavailable() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0xAA, 0xBB, 0xCC, 0, 0, 1] };

        let mut decline = DhcpPacket::new();
        decline.op = 1;
        decline.xid = 9;
        decline.set_client_mac(mac);
        decline.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Decline as u8));
        decline.options.push(DhcpOption::new_ip(DHCP_OPT_REQUESTED_IP, Ipv4Addr::new(192, 168, 1, 100)));
        server.handle_packet(&decline, 1000);

        // The declined IP should not be offered to others
        let mac2 = MacAddr { bytes: [0xDD, 0xEE, 0xFF, 0, 0, 1] };
        let ip = server.pool.allocate_ip(&mac2, 1001).unwrap();
        assert_ne!(ip, Ipv4Addr::new(192, 168, 1, 100));
    }

    #[test]
    fn dhcp_logging() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0xDE, 0xAD, 0xBE, 0xEF, 0, 7] };
        let discover = make_discover(mac, 10);
        server.handle_packet(&discover, 1000);
        assert!(!server.log_messages.is_empty());
        assert!(server.log_messages.iter().any(|m| m.contains("DISCOVER")));
    }

    // ========================================================================
    // TFTP Packet tests
    // ========================================================================

    #[test]
    fn tftp_rrq_roundtrip() {
        let pkt = TftpPacket::ReadRequest {
            filename: "boot.img".to_string(),
            mode: "octet".to_string(),
        };
        let bytes = pkt.serialize();
        let parsed = TftpPacket::parse(&bytes).unwrap();
        match parsed {
            TftpPacket::ReadRequest { filename, mode } => {
                assert_eq!(filename, "boot.img");
                assert_eq!(mode, "octet");
            }
            _ => panic!("Expected ReadRequest"),
        }
    }

    #[test]
    fn tftp_wrq_roundtrip() {
        let pkt = TftpPacket::WriteRequest {
            filename: "upload.bin".to_string(),
            mode: "octet".to_string(),
        };
        let bytes = pkt.serialize();
        let parsed = TftpPacket::parse(&bytes).unwrap();
        match parsed {
            TftpPacket::WriteRequest { filename, mode } => {
                assert_eq!(filename, "upload.bin");
                assert_eq!(mode, "octet");
            }
            _ => panic!("Expected WriteRequest"),
        }
    }

    #[test]
    fn tftp_data_roundtrip() {
        let pkt = TftpPacket::Data {
            block: 42,
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };
        let bytes = pkt.serialize();
        let parsed = TftpPacket::parse(&bytes).unwrap();
        match parsed {
            TftpPacket::Data { block, data } => {
                assert_eq!(block, 42);
                assert_eq!(data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
            }
            _ => panic!("Expected Data"),
        }
    }

    #[test]
    fn tftp_ack_roundtrip() {
        let pkt = TftpPacket::Ack { block: 7 };
        let bytes = pkt.serialize();
        let parsed = TftpPacket::parse(&bytes).unwrap();
        match parsed {
            TftpPacket::Ack { block } => assert_eq!(block, 7),
            _ => panic!("Expected Ack"),
        }
    }

    #[test]
    fn tftp_error_roundtrip() {
        let pkt = TftpPacket::Error {
            code: TftpError::FileNotFound,
            message: "File not found".to_string(),
        };
        let bytes = pkt.serialize();
        let parsed = TftpPacket::parse(&bytes).unwrap();
        match parsed {
            TftpPacket::Error { code, message } => {
                assert_eq!(code, TftpError::FileNotFound);
                assert_eq!(message, "File not found");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn tftp_parse_too_short() {
        assert!(TftpPacket::parse(&[0]).is_none());
        assert!(TftpPacket::parse(&[]).is_none());
    }

    #[test]
    fn tftp_parse_invalid_opcode() {
        assert!(TftpPacket::parse(&[0, 99]).is_none());
    }

    #[test]
    fn tftp_data_empty_payload() {
        let pkt = TftpPacket::Data { block: 1, data: vec![] };
        let bytes = pkt.serialize();
        let parsed = TftpPacket::parse(&bytes).unwrap();
        match parsed {
            TftpPacket::Data { block, data } => {
                assert_eq!(block, 1);
                assert!(data.is_empty());
            }
            _ => panic!("Expected Data"),
        }
    }

    // ========================================================================
    // TFTP Server tests
    // ========================================================================

    #[test]
    fn tftp_server_disabled() {
        let server = TftpServer::new("/tftpboot");
        let req = TftpPacket::ReadRequest { filename: "test".to_string(), mode: "octet".to_string() };
        let responses = server.handle_request(&req);
        assert_eq!(responses.len(), 1);
        match &responses[0] {
            TftpPacket::Error { message, .. } => assert!(message.contains("not enabled")),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn tftp_server_read_file() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;
        server.add_file("boot.img", vec![1, 2, 3, 4, 5]);

        let req = TftpPacket::ReadRequest { filename: "boot.img".to_string(), mode: "octet".to_string() };
        let responses = server.handle_request(&req);
        assert_eq!(responses.len(), 1);
        match &responses[0] {
            TftpPacket::Data { block, data } => {
                assert_eq!(*block, 1);
                assert_eq!(data, &vec![1, 2, 3, 4, 5]);
            }
            _ => panic!("Expected Data packet"),
        }
    }

    #[test]
    fn tftp_server_read_large_file() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;
        // Create a file larger than one TFTP block
        let content: Vec<u8> = (0..1500).map(|i| (i % 256) as u8).collect();
        server.add_file("large.bin", content.clone());

        let req = TftpPacket::ReadRequest { filename: "large.bin".to_string(), mode: "octet".to_string() };
        let responses = server.handle_request(&req);
        // 1500 bytes = 3 blocks (512 + 512 + 476)
        assert_eq!(responses.len(), 3);

        // Reassemble and verify
        let mut reassembled = Vec::new();
        for (i, pkt) in responses.iter().enumerate() {
            match pkt {
                TftpPacket::Data { block, data } => {
                    assert_eq!(*block as usize, i + 1);
                    reassembled.extend_from_slice(data);
                }
                _ => panic!("Expected Data"),
            }
        }
        assert_eq!(reassembled, content);
    }

    #[test]
    fn tftp_server_file_not_found() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;

        let req = TftpPacket::ReadRequest { filename: "missing.bin".to_string(), mode: "octet".to_string() };
        let responses = server.handle_request(&req);
        assert_eq!(responses.len(), 1);
        match &responses[0] {
            TftpPacket::Error { code, .. } => assert_eq!(*code, TftpError::FileNotFound),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn tftp_server_write_rejected() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;

        let req = TftpPacket::WriteRequest { filename: "test".to_string(), mode: "octet".to_string() };
        let responses = server.handle_request(&req);
        match &responses[0] {
            TftpPacket::Error { code, .. } => assert_eq!(*code, TftpError::AccessViolation),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn tftp_server_path_traversal_blocked() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;
        server.add_file("secret.txt", vec![0]);

        let req = TftpPacket::ReadRequest { filename: "../../etc/passwd".to_string(), mode: "octet".to_string() };
        let responses = server.handle_request(&req);
        match &responses[0] {
            TftpPacket::Error { code, .. } => assert_eq!(*code, TftpError::AccessViolation),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn tftp_server_unsupported_mode() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;
        server.add_file("test.txt", vec![0]);

        let req = TftpPacket::ReadRequest { filename: "test.txt".to_string(), mode: "mail".to_string() };
        let responses = server.handle_request(&req);
        match &responses[0] {
            TftpPacket::Error { message, .. } => assert!(message.contains("Unsupported mode")),
            _ => panic!("Expected error"),
        }
    }

    #[test]
    fn tftp_server_leading_slash_stripped() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;
        server.add_file("file.txt", vec![42]);

        let req = TftpPacket::ReadRequest { filename: "/file.txt".to_string(), mode: "octet".to_string() };
        let responses = server.handle_request(&req);
        assert_eq!(responses.len(), 1);
        match &responses[0] {
            TftpPacket::Data { data, .. } => assert_eq!(data, &vec![42]),
            _ => panic!("Expected Data"),
        }
    }

    #[test]
    fn tftp_server_exact_block_size_file() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;
        // Exactly 512 bytes = 1 full block + 1 empty block
        let content: Vec<u8> = vec![0xAB; TFTP_BLOCK_SIZE];
        server.add_file("exact.bin", content);

        let req = TftpPacket::ReadRequest { filename: "exact.bin".to_string(), mode: "octet".to_string() };
        let responses = server.handle_request(&req);
        // With our implementation, if data is exactly block size, we get 1 packet (last chunk is
        // exactly TFTP_BLOCK_SIZE which is not < TFTP_BLOCK_SIZE, so we continue) then a second
        // empty packet. Actually let's verify:
        // offset=0, end=512, chunk=512 bytes, is_last = (512 < 512) = false
        // offset=512, end=512, chunk=0 bytes, is_last = (0 < 512) = true
        assert_eq!(responses.len(), 2);
        match &responses[1] {
            TftpPacket::Data { data, .. } => assert!(data.is_empty()),
            _ => panic!("Expected empty Data"),
        }
    }

    // ========================================================================
    // Configuration parsing tests
    // ========================================================================

    #[test]
    fn config_parse_port() {
        let mut config = Config::default_config();
        parse_config_content("port=5353\n", &mut config);
        assert_eq!(config.port, 5353);
    }

    #[test]
    fn config_parse_server() {
        let mut config = Config::default_config();
        parse_config_content("server=8.8.8.8\nserver=1.1.1.1\n", &mut config);
        assert_eq!(config.upstream_servers.len(), 2);
    }

    #[test]
    fn config_parse_no_resolv() {
        let mut config = Config::default_config();
        parse_config_content("no-resolv\n", &mut config);
        assert!(config.no_resolv);
    }

    #[test]
    fn config_parse_dhcp_range() {
        let mut config = Config::default_config();
        parse_config_content("dhcp-range=192.168.1.100,192.168.1.200,255.255.255.0,1h\n", &mut config);
        assert_eq!(config.dhcp_ranges.len(), 1);
        assert_eq!(config.dhcp_ranges[0].start, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(config.dhcp_ranges[0].lease_time, 3600);
    }

    #[test]
    fn config_parse_dhcp_host() {
        let mut config = Config::default_config();
        parse_config_content("dhcp-host=aa:bb:cc:dd:ee:ff,192.168.1.50\n", &mut config);
        assert_eq!(config.dhcp_hosts.len(), 1);
        assert_eq!(config.dhcp_hosts[0].ip, Ipv4Addr::new(192, 168, 1, 50));
    }

    #[test]
    fn config_parse_address() {
        let mut config = Config::default_config();
        parse_config_content("address=/doubleclick.net/0.0.0.0\n", &mut config);
        assert_eq!(config.address_overrides.len(), 1);
        assert_eq!(config.address_overrides[0].domain, "doubleclick.net");
    }

    #[test]
    fn config_parse_tftp() {
        let mut config = Config::default_config();
        parse_config_content("enable-tftp\ntftp-root=/srv/tftp\n", &mut config);
        assert!(config.enable_tftp);
        assert_eq!(config.tftp_root, "/srv/tftp");
    }

    #[test]
    fn config_parse_comments_and_blanks() {
        let mut config = Config::default_config();
        parse_config_content("# comment\n\nport=1234\n# another comment\n", &mut config);
        assert_eq!(config.port, 1234);
    }

    #[test]
    fn config_parse_boolean_flags() {
        let mut config = Config::default_config();
        parse_config_content(
            "bind-interfaces\nno-daemon\nlog-queries\nlog-dhcp\n",
            &mut config,
        );
        assert!(config.bind_interfaces);
        assert!(config.no_daemon);
        assert!(config.log_queries);
        assert!(config.log_dhcp);
    }

    #[test]
    fn config_parse_interface() {
        let mut config = Config::default_config();
        parse_config_content("interface=eth0\n", &mut config);
        assert_eq!(config.interface.as_deref(), Some("eth0"));
    }

    #[test]
    fn config_parse_listen_address() {
        let mut config = Config::default_config();
        parse_config_content("listen-address=127.0.0.1\n", &mut config);
        assert_eq!(config.listen_address.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn config_parse_pid_file() {
        let mut config = Config::default_config();
        parse_config_content("pid-file=/var/run/dnsmasq.pid\n", &mut config);
        assert_eq!(config.pid_file.as_deref(), Some("/var/run/dnsmasq.pid"));
    }

    #[test]
    fn config_parse_conf_dir() {
        let mut config = Config::default_config();
        parse_config_content("conf-dir=/etc/dnsmasq.d\n", &mut config);
        assert_eq!(config.conf_dir.as_deref(), Some("/etc/dnsmasq.d"));
    }

    // ========================================================================
    // CLI argument parsing tests
    // ========================================================================

    #[test]
    fn cli_port_equals() {
        let mut config = Config::default_config();
        let args = vec!["--port=5353".to_string()];
        parse_cli_args(&args, &mut config).unwrap();
        assert_eq!(config.port, 5353);
    }

    #[test]
    fn cli_port_separate() {
        let mut config = Config::default_config();
        let args = vec!["--port".to_string(), "5353".to_string()];
        parse_cli_args(&args, &mut config).unwrap();
        assert_eq!(config.port, 5353);
    }

    #[test]
    fn cli_help() {
        let mut config = Config::default_config();
        let args = vec!["--help".to_string()];
        let result = parse_cli_args(&args, &mut config);
        assert_eq!(result.unwrap_err(), "HELP");
    }

    #[test]
    fn cli_version() {
        let mut config = Config::default_config();
        let args = vec!["--version".to_string()];
        let result = parse_cli_args(&args, &mut config);
        assert_eq!(result.unwrap_err(), "VERSION");
    }

    #[test]
    fn cli_unknown_option() {
        let mut config = Config::default_config();
        let args = vec!["--bogus".to_string()];
        assert!(parse_cli_args(&args, &mut config).is_err());
    }

    #[test]
    fn cli_no_resolv() {
        let mut config = Config::default_config();
        let args = vec!["--no-resolv".to_string()];
        parse_cli_args(&args, &mut config).unwrap();
        assert!(config.no_resolv);
    }

    #[test]
    fn cli_multiple_servers() {
        let mut config = Config::default_config();
        let args = vec![
            "--server=8.8.8.8".to_string(),
            "--server=1.1.1.1".to_string(),
        ];
        parse_cli_args(&args, &mut config).unwrap();
        assert_eq!(config.upstream_servers.len(), 2);
    }

    #[test]
    fn cli_combined_options() {
        let mut config = Config::default_config();
        let args = vec![
            "--port=5353".to_string(),
            "--no-daemon".to_string(),
            "--log-queries".to_string(),
            "--log-dhcp".to_string(),
            "--enable-tftp".to_string(),
            "--tftp-root=/srv/tftp".to_string(),
        ];
        parse_cli_args(&args, &mut config).unwrap();
        assert_eq!(config.port, 5353);
        assert!(config.no_daemon);
        assert!(config.log_queries);
        assert!(config.log_dhcp);
        assert!(config.enable_tftp);
        assert_eq!(config.tftp_root, "/srv/tftp");
    }

    // ========================================================================
    // Lease time parsing tests
    // ========================================================================

    #[test]
    fn lease_time_seconds() {
        assert_eq!(parse_lease_time("3600"), 3600);
    }

    #[test]
    fn lease_time_with_s() {
        assert_eq!(parse_lease_time("120s"), 120);
    }

    #[test]
    fn lease_time_with_m() {
        assert_eq!(parse_lease_time("30m"), 1800);
    }

    #[test]
    fn lease_time_with_h() {
        assert_eq!(parse_lease_time("1h"), 3600);
    }

    #[test]
    fn lease_time_with_d() {
        assert_eq!(parse_lease_time("1d"), 86400);
    }

    #[test]
    fn lease_time_with_w() {
        assert_eq!(parse_lease_time("1w"), 604800);
    }

    #[test]
    fn lease_time_empty() {
        assert_eq!(parse_lease_time(""), DEFAULT_LEASE_TIME);
    }

    // ========================================================================
    // DHCP range/host config parsing tests
    // ========================================================================

    #[test]
    fn dhcp_range_parse_full() {
        let r = parse_dhcp_range("192.168.1.100,192.168.1.200,255.255.255.0,12h").unwrap();
        assert_eq!(r.start, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(r.end, Ipv4Addr::new(192, 168, 1, 200));
        assert_eq!(r.netmask, Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(r.lease_time, 43200);
    }

    #[test]
    fn dhcp_range_parse_minimal() {
        let r = parse_dhcp_range("10.0.0.10,10.0.0.20").unwrap();
        assert_eq!(r.start, Ipv4Addr::new(10, 0, 0, 10));
        assert_eq!(r.netmask, Ipv4Addr::new(255, 255, 255, 0)); // default
    }

    #[test]
    fn dhcp_range_parse_invalid() {
        assert!(parse_dhcp_range("only_one").is_none());
        assert!(parse_dhcp_range("").is_none());
    }

    #[test]
    fn dhcp_host_parse() {
        let h = parse_dhcp_host("aa:bb:cc:dd:ee:ff,192.168.1.50").unwrap();
        assert_eq!(h.mac, MacAddr::from_str("aa:bb:cc:dd:ee:ff").unwrap());
        assert_eq!(h.ip, Ipv4Addr::new(192, 168, 1, 50));
    }

    #[test]
    fn dhcp_host_parse_invalid() {
        assert!(parse_dhcp_host("invalid").is_none());
        assert!(parse_dhcp_host("notmac,1.2.3.4").is_none());
    }

    // ========================================================================
    // DHCP option parsing tests
    // ========================================================================

    #[test]
    fn dhcp_option_parse_ip() {
        let opt = parse_dhcp_option("3,192.168.1.1").unwrap();
        assert_eq!(opt.code, 3);
        assert_eq!(opt.get_ip(), Some(Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn dhcp_option_parse_number() {
        let opt = parse_dhcp_option("51,7200").unwrap();
        assert_eq!(opt.code, 51);
        assert_eq!(opt.get_u32(), Some(7200));
    }

    #[test]
    fn dhcp_option_parse_string() {
        let opt = parse_dhcp_option("15,example.local").unwrap();
        assert_eq!(opt.code, 15);
        assert_eq!(opt.data, b"example.local");
    }

    #[test]
    fn dhcp_option_parse_invalid() {
        assert!(parse_dhcp_option("no_comma").is_none());
        assert!(parse_dhcp_option("notanumber,value").is_none());
    }

    // ========================================================================
    // DNS Resolver tests
    // ========================================================================

    #[test]
    fn resolver_blocklist_query() {
        let mut resolver = DnsResolver::new();
        resolver.blocklist.load_from_content("ads.com\n");
        resolver.log_queries = true;

        let query = DnsPacket::new_query(1, "ads.com", DnsRecordType::A);
        let response = resolver.resolve(&query, 1000);
        assert_eq!(response.header.rcode, DnsRcode::NxDomain);
    }

    #[test]
    fn resolver_address_override() {
        let mut resolver = DnsResolver::new();
        resolver.address_overrides.push(AddressOverride {
            domain: "override.com".to_string(),
            ip: Ipv4Addr::new(10, 10, 10, 10),
        });

        let query = DnsPacket::new_query(2, "override.com", DnsRecordType::A);
        let response = resolver.resolve(&query, 1000);
        assert_eq!(response.header.rcode, DnsRcode::NoError);
        assert_eq!(response.answers.len(), 1);
        assert_eq!(response.answers[0].rdata, vec![10, 10, 10, 10]);
    }

    #[test]
    fn resolver_hosts_lookup() {
        let mut resolver = DnsResolver::new();
        resolver.hosts_entries = parse_hosts_content("10.0.0.1 myhost.local\n");

        let query = DnsPacket::new_query(3, "myhost.local", DnsRecordType::A);
        let response = resolver.resolve(&query, 1000);
        assert_eq!(response.header.rcode, DnsRcode::NoError);
        assert_eq!(response.answers[0].rdata, vec![10, 0, 0, 1]);
    }

    #[test]
    fn resolver_cache_hit() {
        let mut resolver = DnsResolver::new();
        let records = vec![DnsRecord::new_a("cached.com", 300, Ipv4Addr::new(1, 2, 3, 4))];
        resolver.cache.insert("cached.com", DnsRecordType::A, records, 1000);

        let query = DnsPacket::new_query(4, "cached.com", DnsRecordType::A);
        let response = resolver.resolve(&query, 1050);
        assert_eq!(response.header.rcode, DnsRcode::NoError);
        assert_eq!(response.answers.len(), 1);
    }

    #[test]
    fn resolver_cache_miss_nxdomain() {
        let mut resolver = DnsResolver::new();
        let query = DnsPacket::new_query(5, "unknown.com", DnsRecordType::A);
        let response = resolver.resolve(&query, 1000);
        assert_eq!(response.header.rcode, DnsRcode::NxDomain);
    }

    #[test]
    fn resolver_empty_query() {
        let mut resolver = DnsResolver::new();
        let pkt = DnsPacket {
            header: DnsHeader::new_query(6),
            questions: vec![],
            answers: vec![],
            authorities: vec![],
            additionals: vec![],
        };
        let response = resolver.resolve(&pkt, 1000);
        assert_eq!(response.header.rcode, DnsRcode::FormErr);
    }

    #[test]
    fn resolver_priority_order() {
        let mut resolver = DnsResolver::new();
        // Set up blocklist, override, and hosts all for the same domain
        resolver.blocklist.load_from_content("priority.com\n");
        resolver.address_overrides.push(AddressOverride {
            domain: "priority.com".to_string(),
            ip: Ipv4Addr::new(1, 1, 1, 1),
        });
        resolver.hosts_entries = parse_hosts_content("2.2.2.2 priority.com\n");

        // Blocklist should win (checked first)
        let query = DnsPacket::new_query(7, "priority.com", DnsRecordType::A);
        let response = resolver.resolve(&query, 1000);
        assert_eq!(response.header.rcode, DnsRcode::NxDomain);
    }

    // ========================================================================
    // Personality detection tests
    // ========================================================================

    #[test]
    fn personality_default() {
        assert_eq!(detect_personality("dnsmasq"), Personality::Dnsmasq);
    }

    #[test]
    fn personality_dhcp_only() {
        assert_eq!(detect_personality("dnsmasq-dhcp"), Personality::DnsmasqDhcp);
    }

    #[test]
    fn personality_path_prefix() {
        assert_eq!(detect_personality("dnsmasq"), Personality::Dnsmasq);
    }

    #[test]
    fn personality_unknown_defaults() {
        assert_eq!(detect_personality("something-else"), Personality::Dnsmasq);
    }

    // ========================================================================
    // DNS Rcode tests
    // ========================================================================

    #[test]
    fn rcode_roundtrip() {
        assert_eq!(DnsRcode::from_u8(0), DnsRcode::NoError);
        assert_eq!(DnsRcode::from_u8(1), DnsRcode::FormErr);
        assert_eq!(DnsRcode::from_u8(2), DnsRcode::ServFail);
        assert_eq!(DnsRcode::from_u8(3), DnsRcode::NxDomain);
        assert_eq!(DnsRcode::from_u8(4), DnsRcode::NotImp);
        assert_eq!(DnsRcode::from_u8(5), DnsRcode::Refused);
    }

    #[test]
    fn rcode_unknown_defaults_to_servfail() {
        assert_eq!(DnsRcode::from_u8(99), DnsRcode::ServFail);
    }

    // ========================================================================
    // Edge case and integration tests
    // ========================================================================

    #[test]
    fn dns_packet_with_authority_and_additional() {
        let mut pkt = DnsPacket::new_query(100, "test.com", DnsRecordType::A);
        pkt.header.qr = true;
        pkt.header.nscount = 1;
        pkt.header.arcount = 1;
        pkt.authorities.push(DnsRecord::new_ns("test.com", 3600, "ns1.test.com").unwrap());
        pkt.additionals.push(DnsRecord::new_a("ns1.test.com", 3600, Ipv4Addr::new(1, 1, 1, 1)));

        let bytes = pkt.serialize().unwrap();
        let parsed = DnsPacket::parse(&bytes).unwrap();
        assert_eq!(parsed.authorities.len(), 1);
        assert_eq!(parsed.additionals.len(), 1);
    }

    #[test]
    fn dns_encode_name_max_label() {
        let label = "a".repeat(63);
        let name = format!("{}.com", label);
        assert!(dns_encode_name(&name).is_some());
    }

    #[test]
    fn dhcp_full_workflow() {
        let mut server = make_test_server();
        let mac = MacAddr { bytes: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55] };

        // 1. DISCOVER
        let discover = make_discover(mac, 100);
        let offer = server.handle_packet(&discover, 1000).unwrap();
        assert_eq!(offer.get_message_type(), Some(DhcpMessageType::Offer));
        let offered_ip = offer.yiaddr;

        // 2. REQUEST
        let request = make_request(mac, 101, offered_ip);
        let ack = server.handle_packet(&request, 1001).unwrap();
        assert_eq!(ack.get_message_type(), Some(DhcpMessageType::Ack));
        assert_eq!(ack.yiaddr, offered_ip);

        // 3. Verify lease exists
        assert_eq!(server.pool.active_lease_count(1001), 1);

        // 4. RELEASE
        let mut release = DhcpPacket::new();
        release.op = 1;
        release.xid = 102;
        release.ciaddr = offered_ip;
        release.set_client_mac(mac);
        release.options.push(DhcpOption::new_byte(DHCP_OPT_MESSAGE_TYPE, DhcpMessageType::Release as u8));
        server.handle_packet(&release, 1002);

        // 5. Verify lease released
        assert!(server.pool.find_lease_by_mac(&mac).is_none());
    }

    #[test]
    fn tftp_full_workflow() {
        let mut server = TftpServer::new("/tftpboot");
        server.enabled = true;

        // Add a multi-block file
        let data: Vec<u8> = (0u8..=255).cycle().take(1024).collect();
        server.add_file("firmware.bin", data.clone());

        // Request the file
        let req = TftpPacket::ReadRequest {
            filename: "firmware.bin".to_string(),
            mode: "octet".to_string(),
        };
        let packets = server.handle_request(&req);

        // Should be 2 data packets (512 + 512)
        // Actually 1024 / 512 = 2 full blocks, then since last chunk is exactly 512,
        // we get 2 blocks + 1 empty block = 3 packets
        assert_eq!(packets.len(), 3);

        let mut reassembled = Vec::new();
        for pkt in &packets {
            if let TftpPacket::Data { data: d, .. } = pkt {
                reassembled.extend_from_slice(d);
            }
        }
        assert_eq!(reassembled, data);
    }

    #[test]
    fn config_default_values() {
        let config = Config::default_config();
        assert_eq!(config.port, DEFAULT_DNS_PORT);
        assert!(!config.no_resolv);
        assert!(!config.enable_tftp);
        assert!(!config.no_daemon);
        assert!(!config.log_queries);
        assert!(!config.log_dhcp);
        assert_eq!(config.conf_file, DEFAULT_CONF_FILE);
        assert_eq!(config.hosts_file, DEFAULT_HOSTS_FILE);
        assert_eq!(config.resolv_conf, DEFAULT_RESOLV_CONF);
        assert_eq!(config.lease_file, DEFAULT_LEASE_FILE);
    }

    #[test]
    fn pool_find_lease_by_ip() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        let mac = MacAddr { bytes: [1, 2, 3, 4, 5, 6] };
        pool.create_or_renew_lease(mac, Ipv4Addr::new(10, 0, 0, 15), 1000, None);
        let lease = pool.find_lease_by_ip(Ipv4Addr::new(10, 0, 0, 15));
        assert!(lease.is_some());
        assert_eq!(lease.unwrap().mac, mac);
    }

    #[test]
    fn pool_find_lease_by_ip_not_found() {
        let pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            3600,
        );
        assert!(pool.find_lease_by_ip(Ipv4Addr::new(10, 0, 0, 15)).is_none());
    }

    #[test]
    fn pool_renew_lease_updates_expiry() {
        let mut pool = DhcpPool::new(
            Ipv4Addr::new(10, 0, 0, 10),
            Ipv4Addr::new(10, 0, 0, 20),
            Ipv4Addr::new(255, 255, 255, 0),
            100,
        );
        let mac = MacAddr { bytes: [1, 2, 3, 4, 5, 6] };
        pool.create_or_renew_lease(mac, Ipv4Addr::new(10, 0, 0, 10), 1000, None);
        assert_eq!(pool.leases[0].expires, 1100);

        // Renew at time 1050
        pool.create_or_renew_lease(mac, Ipv4Addr::new(10, 0, 0, 10), 1050, None);
        assert_eq!(pool.leases[0].expires, 1150);
    }
}
