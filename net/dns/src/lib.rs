//! `OurOS` DNS Resolver Library
//!
//! Provides hostname-to-IP-address resolution via the DNS protocol (RFC 1035).
//! Includes a caching resolver, hosts file support, and full DNS message
//! serialization/deserialization with name compression pointer handling.
//!
//! # Architecture
//!
//! The resolver checks sources in order:
//! 1. Local hosts file (`/etc/hosts`)
//! 2. Internal cache (TTL-based expiry)
//! 3. Configured nameservers via UDP queries
//!
//! # Example
//!
//! ```no_run
//! use dns::Resolver;
//! let mut resolver = Resolver::system();
//! let addrs = resolver.resolve("example.com")?;
//! # Ok::<(), dns::DnsError>(())
//! ```

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]

use std::fmt;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// IP Address Types
// ---------------------------------------------------------------------------

/// An IPv4 address represented as four octets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    /// Creates a new IPv4 address from four octets.
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }

    /// Returns the four octets of the address.
    pub const fn octets(&self) -> [u8; 4] {
        self.0
    }
}

impl fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

impl FromStr for Ipv4Addr {
    type Err = DnsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 4 {
            return Err(DnsError::InvalidName(format!("invalid IPv4 address: {s}")));
        }
        let mut octets = [0u8; 4];
        for (i, part) in parts.iter().enumerate() {
            octets[i] = part
                .parse::<u8>()
                .map_err(|_| DnsError::InvalidName(format!("invalid IPv4 octet: {part}")))?;
        }
        Ok(Self(octets))
    }
}

/// An IPv6 address represented as sixteen octets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ipv6Addr(pub [u8; 16]);

impl Ipv6Addr {
    /// Creates a new IPv6 address from sixteen octets.
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Creates an IPv6 address from eight 16-bit segments.
    pub const fn from_segments(segments: [u16; 8]) -> Self {
        let mut bytes = [0u8; 16];
        let mut i = 0;
        while i < 8 {
            bytes[i * 2] = (segments[i] >> 8) as u8;
            // Truncation is intentional: low byte of the segment.
            #[allow(clippy::cast_possible_truncation)]
            {
                bytes[i * 2 + 1] = segments[i] as u8;
            }
            i += 1;
        }
        Self(bytes)
    }

    /// Returns the sixteen octets of the address.
    pub const fn octets(&self) -> [u8; 16] {
        self.0
    }

    /// Returns the address as eight 16-bit segments.
    pub fn segments(&self) -> [u16; 8] {
        let mut segs = [0u16; 8];
        for (i, seg) in segs.iter_mut().enumerate() {
            *seg = u16::from_be_bytes([self.0[i * 2], self.0[i * 2 + 1]]);
        }
        segs
    }
}

impl fmt::Display for Ipv6Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let segs = self.segments();
        write!(
            f,
            "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
            segs[0], segs[1], segs[2], segs[3], segs[4], segs[5], segs[6], segs[7]
        )
    }
}

impl FromStr for Ipv6Addr {
    type Err = DnsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Handle :: (compressed zeros)
        let (left, right) = if let Some(pos) = s.find("::") {
            let l = &s[..pos];
            let r = &s[pos + 2..];
            (l, Some(r))
        } else {
            (s, None)
        };

        let mut segments = [0u16; 8];
        let left_parts: Vec<&str> = if left.is_empty() {
            Vec::new()
        } else {
            left.split(':').collect()
        };

        let right_parts: Vec<&str> = match right {
            Some("") | None => Vec::new(),
            Some(r) => r.split(':').collect(),
        };

        let total = left_parts.len() + right_parts.len();
        if right.is_some() {
            if total > 8 {
                return Err(DnsError::InvalidName(format!("invalid IPv6 address: {s}")));
            }
        } else if total != 8 {
            return Err(DnsError::InvalidName(format!("invalid IPv6 address: {s}")));
        }

        for (i, part) in left_parts.iter().enumerate() {
            segments[i] = u16::from_str_radix(part, 16)
                .map_err(|_| DnsError::InvalidName(format!("invalid IPv6 segment: {part}")))?;
        }

        let right_start = 8 - right_parts.len();
        for (i, part) in right_parts.iter().enumerate() {
            segments[right_start + i] = u16::from_str_radix(part, 16)
                .map_err(|_| DnsError::InvalidName(format!("invalid IPv6 segment: {part}")))?;
        }

        Ok(Self::from_segments(segments))
    }
}

/// A unified IP address type supporting both IPv4 and IPv6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IpAddr {
    /// An IPv4 address.
    V4(Ipv4Addr),
    /// An IPv6 address.
    V6(Ipv6Addr),
}

impl fmt::Display for IpAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V4(addr) => addr.fmt(f),
            Self::V6(addr) => addr.fmt(f),
        }
    }
}

impl FromStr for IpAddr {
    type Err = DnsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try IPv4 first (more common), then IPv6
        if let Ok(v4) = Ipv4Addr::from_str(s) {
            return Ok(Self::V4(v4));
        }
        Ipv6Addr::from_str(s).map(Self::V6)
    }
}

// ---------------------------------------------------------------------------
// DNS Error Type
// ---------------------------------------------------------------------------

/// Errors that can occur during DNS resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsError {
    /// The hostname contains invalid characters or is malformed.
    InvalidName(String),
    /// The query timed out waiting for a response.
    Timeout,
    /// The DNS server reported an internal failure (RCODE 2).
    ServerFailure,
    /// The queried name does not exist (NXDOMAIN, RCODE 3).
    NameNotFound,
    /// The DNS server refused the query (RCODE 5).
    Refused,
    /// The response could not be parsed.
    InvalidResponse(String),
    /// A network-level error occurred.
    NetworkError(String),
}

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(msg) => write!(f, "invalid DNS name: {msg}"),
            Self::Timeout => write!(f, "DNS query timed out"),
            Self::ServerFailure => write!(f, "DNS server failure"),
            Self::NameNotFound => write!(f, "name not found (NXDOMAIN)"),
            Self::Refused => write!(f, "DNS query refused"),
            Self::InvalidResponse(msg) => write!(f, "invalid DNS response: {msg}"),
            Self::NetworkError(msg) => write!(f, "network error: {msg}"),
        }
    }
}

// ---------------------------------------------------------------------------
// DNS Record Types and Classes
// ---------------------------------------------------------------------------

/// DNS record type (QTYPE values per RFC 1035 and extensions).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum DnsType {
    /// IPv4 host address.
    A = 1,
    /// Authoritative name server.
    NS = 2,
    /// Canonical name (alias).
    CNAME = 5,
    /// Start of authority.
    SOA = 6,
    /// Domain name pointer (reverse DNS).
    PTR = 12,
    /// Mail exchange.
    MX = 15,
    /// Text record.
    TXT = 16,
    /// IPv6 host address.
    AAAA = 28,
    /// Service locator.
    SRV = 33,
}

impl DnsType {
    /// Converts a raw u16 value to a `DnsType`, if recognized.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::A),
            2 => Some(Self::NS),
            5 => Some(Self::CNAME),
            6 => Some(Self::SOA),
            12 => Some(Self::PTR),
            15 => Some(Self::MX),
            16 => Some(Self::TXT),
            28 => Some(Self::AAAA),
            33 => Some(Self::SRV),
            _ => None,
        }
    }
}

/// DNS record class (QCLASS values).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum DnsClass {
    /// Internet class.
    IN = 1,
}

impl DnsClass {
    /// Converts a raw u16 value to a `DnsClass`, if recognized.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::IN),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// DNS Message Structures
// ---------------------------------------------------------------------------

/// A complete DNS message (query or response).
pub struct DnsMessage {
    pub header: DnsHeader,
    pub questions: Vec<DnsQuestion>,
    pub answers: Vec<DnsRecord>,
    pub authority: Vec<DnsRecord>,
    pub additional: Vec<DnsRecord>,
}

/// The 12-byte fixed header of a DNS message.
pub struct DnsHeader {
    /// Transaction ID, used to match queries with responses.
    pub id: u16,
    /// Flags field: QR, Opcode, AA, TC, RD, RA, Z, RCODE.
    pub flags: u16,
    /// Number of entries in the question section.
    pub question_count: u16,
    /// Number of entries in the answer section.
    pub answer_count: u16,
    /// Number of entries in the authority section.
    pub authority_count: u16,
    /// Number of entries in the additional section.
    pub additional_count: u16,
}

impl DnsHeader {
    /// Returns the RCODE (response code) from the flags field.
    pub fn rcode(&self) -> u16 {
        self.flags & 0x000F
    }

    /// Returns true if this is a response (QR bit set).
    pub fn is_response(&self) -> bool {
        (self.flags & 0x8000) != 0
    }

    /// Returns true if the message is truncated (TC bit set).
    pub fn is_truncated(&self) -> bool {
        (self.flags & 0x0200) != 0
    }
}

/// A question entry in a DNS message.
pub struct DnsQuestion {
    /// The domain name being queried.
    pub name: String,
    /// The query type (A, AAAA, etc.).
    pub qtype: DnsType,
    /// The query class (usually IN).
    pub qclass: DnsClass,
}

/// A resource record in a DNS message.
#[derive(Clone, Debug)]
pub struct DnsRecord {
    /// The domain name this record applies to.
    pub name: String,
    /// The record type.
    pub rtype: DnsType,
    /// The record class.
    pub class: DnsClass,
    /// Time-to-live in seconds.
    pub ttl: u32,
    /// The record data.
    pub rdata: RData,
}

/// Resource record data, parsed according to record type.
#[derive(Clone, Debug)]
pub enum RData {
    /// IPv4 address (A record).
    A(Ipv4Addr),
    /// IPv6 address (AAAA record).
    AAAA(Ipv6Addr),
    /// Canonical name (CNAME record).
    CName(String),
    /// Mail exchange (MX record).
    Mx { priority: u16, exchange: String },
    /// Name server (NS record).
    Ns(String),
    /// Pointer record (PTR).
    Ptr(String),
    /// Text record (TXT).
    Txt(String),
    /// Service locator (SRV record).
    Srv {
        priority: u16,
        weight: u16,
        port: u16,
        target: String,
    },
    /// Unknown or unimplemented record type.
    Unknown(Vec<u8>),
}

// ---------------------------------------------------------------------------
// DNS Message Serialization
// ---------------------------------------------------------------------------

/// Encodes a domain name into DNS wire format (length-prefixed labels).
///
/// Each label is preceded by its length byte, terminated by a zero byte.
/// For example, "example.com" becomes [7, 'e', 'x', 'a', 'm', 'p', 'l', 'e', 3, 'c', 'o', 'm', 0].
///
/// # Errors
///
/// Returns [`DnsError::InvalidName`] if the name contains an empty label
/// or any label longer than 63 bytes (the DNS label-length limit).
pub fn encode_name(name: &str) -> Result<Vec<u8>, DnsError> {
    if name.is_empty() {
        return Ok(vec![0]);
    }

    let mut result = Vec::new();
    let name = name.strip_suffix('.').unwrap_or(name);

    for label in name.split('.') {
        let len = label.len();
        if len == 0 {
            return Err(DnsError::InvalidName("empty label in domain name".to_string()));
        }
        if len > 63 {
            return Err(DnsError::InvalidName(format!(
                "label exceeds 63 bytes: {label}"
            )));
        }
        // Safe: len checked to be <= 63, so fits in u8.
        #[allow(clippy::cast_possible_truncation)]
        {
            result.push(len as u8);
        }
        result.extend_from_slice(label.as_bytes());
    }
    result.push(0); // Root label terminator
    Ok(result)
}

/// Decodes a domain name from DNS wire format, handling compression pointers.
///
/// DNS name compression uses a two-byte pointer (high bits 11) to reference
/// a previously-seen name at an offset within the message. This function
/// follows up to 16 pointers to prevent infinite loops.
///
/// # Errors
///
/// Returns [`DnsError::InvalidResponse`] if the encoded name runs off the
/// end of `data`, a compression pointer is truncated, or the chain of
/// compression pointers exceeds the maximum jump count.
pub fn decode_name(data: &[u8], offset: &mut usize) -> Result<String, DnsError> {
    const MAX_JUMPS: usize = 16;
    let mut labels: Vec<String> = Vec::new();
    let mut current = *offset;
    let mut jumped = false;
    let mut jump_count = 0;

    loop {
        if current >= data.len() {
            return Err(DnsError::InvalidResponse(
                "name extends past end of message".to_string(),
            ));
        }

        let length = data[current];

        if length == 0 {
            // End of name
            if !jumped {
                *offset = current + 1;
            }
            break;
        }

        // Check for compression pointer (two high bits set)
        if (length & 0xC0) == 0xC0 {
            if current + 1 >= data.len() {
                return Err(DnsError::InvalidResponse(
                    "truncated compression pointer".to_string(),
                ));
            }
            jump_count += 1;
            if jump_count > MAX_JUMPS {
                return Err(DnsError::InvalidResponse(
                    "too many compression pointer jumps".to_string(),
                ));
            }
            if !jumped {
                *offset = current + 2;
                jumped = true;
            }
            let pointer = u16::from_be_bytes([length & 0x3F, data[current + 1]]) as usize;
            current = pointer;
            continue;
        }

        // Regular label
        let label_len = length as usize;
        let label_start = current + 1;
        let label_end = label_start + label_len;

        if label_end > data.len() {
            return Err(DnsError::InvalidResponse(
                "label extends past end of message".to_string(),
            ));
        }

        let label = String::from_utf8_lossy(&data[label_start..label_end]).to_string();
        labels.push(label);
        current = label_end;
    }

    Ok(labels.join("."))
}

/// Serializes a DNS query for the given name and record type.
///
/// Returns the raw bytes suitable for sending over UDP to a DNS server.
/// Uses transaction ID 0x1234 (caller should randomize for production use).
///
/// # Errors
///
/// Propagates [`DnsError::InvalidName`] from [`encode_name`].
pub fn serialize_query(name: &str, qtype: DnsType) -> Result<Vec<u8>, DnsError> {
    serialize_query_with_id(0x1234, name, qtype)
}

/// Serializes a DNS query with a specified transaction ID.
///
/// # Errors
///
/// Propagates [`DnsError::InvalidName`] from [`encode_name`].
pub fn serialize_query_with_id(id: u16, name: &str, qtype: DnsType) -> Result<Vec<u8>, DnsError> {
    let mut buf = Vec::with_capacity(64);

    // Header (12 bytes)
    buf.extend_from_slice(&id.to_be_bytes()); // Transaction ID
    buf.extend_from_slice(&0x0100u16.to_be_bytes()); // Flags: RD=1 (recursion desired)
    buf.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    buf.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT = 0
    buf.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT = 0
    buf.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT = 0

    // Question section
    let encoded_name = encode_name(name)?;
    buf.extend_from_slice(&encoded_name);
    buf.extend_from_slice(&(qtype as u16).to_be_bytes()); // QTYPE
    buf.extend_from_slice(&(DnsClass::IN as u16).to_be_bytes()); // QCLASS = IN

    Ok(buf)
}

/// Parses a DNS response from raw wire-format bytes.
///
/// Handles all standard sections (question, answer, authority, additional)
/// and supports name compression pointers throughout.
///
/// # Errors
///
/// Returns [`DnsError::InvalidResponse`] if `data` is shorter than the
/// DNS header, is truncated mid-record, or contains malformed names.
/// Returns [`DnsError::ServerFailure`], [`DnsError::NameNotFound`], or
/// [`DnsError::Refused`] when the response RCODE indicates a server-side
/// failure.
pub fn parse_response(data: &[u8]) -> Result<DnsMessage, DnsError> {
    if data.len() < 12 {
        return Err(DnsError::InvalidResponse(
            "message too short for header".to_string(),
        ));
    }

    let header = DnsHeader {
        id: u16::from_be_bytes([data[0], data[1]]),
        flags: u16::from_be_bytes([data[2], data[3]]),
        question_count: u16::from_be_bytes([data[4], data[5]]),
        answer_count: u16::from_be_bytes([data[6], data[7]]),
        authority_count: u16::from_be_bytes([data[8], data[9]]),
        additional_count: u16::from_be_bytes([data[10], data[11]]),
    };

    // Check RCODE for errors
    match header.rcode() {
        0 => {} // No error
        2 => return Err(DnsError::ServerFailure),
        3 => return Err(DnsError::NameNotFound),
        5 => return Err(DnsError::Refused),
        code => {
            return Err(DnsError::InvalidResponse(format!(
                "unexpected RCODE: {code}"
            )))
        }
    }

    let mut offset = 12;

    // Parse questions
    let mut questions = Vec::new();
    for _ in 0..header.question_count {
        let name = decode_name(data, &mut offset)?;
        if offset + 4 > data.len() {
            return Err(DnsError::InvalidResponse(
                "truncated question section".to_string(),
            ));
        }
        let qtype_raw = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let qclass_raw = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        offset += 4;

        let qtype = DnsType::from_u16(qtype_raw).unwrap_or(DnsType::A);
        let qclass = DnsClass::from_u16(qclass_raw).unwrap_or(DnsClass::IN);

        questions.push(DnsQuestion {
            name,
            qtype,
            qclass,
        });
    }

    // Parse resource record sections
    let answers = parse_records(data, &mut offset, header.answer_count)?;
    let authority = parse_records(data, &mut offset, header.authority_count)?;
    let additional = parse_records(data, &mut offset, header.additional_count)?;

    Ok(DnsMessage {
        header,
        questions,
        answers,
        authority,
        additional,
    })
}

/// Parses a sequence of resource records from the wire format.
fn parse_records(
    data: &[u8],
    offset: &mut usize,
    count: u16,
) -> Result<Vec<DnsRecord>, DnsError> {
    let mut records = Vec::new();

    for _ in 0..count {
        let name = decode_name(data, offset)?;

        if *offset + 10 > data.len() {
            return Err(DnsError::InvalidResponse(
                "truncated resource record".to_string(),
            ));
        }

        let rtype_raw = u16::from_be_bytes([data[*offset], data[*offset + 1]]);
        let class_raw = u16::from_be_bytes([data[*offset + 2], data[*offset + 3]]);
        let ttl = u32::from_be_bytes([
            data[*offset + 4],
            data[*offset + 5],
            data[*offset + 6],
            data[*offset + 7],
        ]);
        let rdlength = u16::from_be_bytes([data[*offset + 8], data[*offset + 9]]) as usize;
        *offset += 10;

        if *offset + rdlength > data.len() {
            return Err(DnsError::InvalidResponse(
                "RDATA extends past end of message".to_string(),
            ));
        }

        let rtype = DnsType::from_u16(rtype_raw).unwrap_or(DnsType::A);
        let class = DnsClass::from_u16(class_raw).unwrap_or(DnsClass::IN);

        let rdata = parse_rdata(data, offset, rtype_raw, rdlength)?;

        records.push(DnsRecord {
            name,
            rtype,
            class,
            ttl,
            rdata,
        });
    }

    Ok(records)
}

/// Parses the RDATA portion of a resource record based on its type.
fn parse_rdata(
    data: &[u8],
    offset: &mut usize,
    rtype: u16,
    rdlength: usize,
) -> Result<RData, DnsError> {
    let rdata_start = *offset;

    let result = match rtype {
        1 => {
            // A record: 4-byte IPv4 address
            if rdlength != 4 {
                return Err(DnsError::InvalidResponse(
                    "A record RDATA must be 4 bytes".to_string(),
                ));
            }
            let addr = Ipv4Addr([
                data[*offset],
                data[*offset + 1],
                data[*offset + 2],
                data[*offset + 3],
            ]);
            *offset += 4;
            RData::A(addr)
        }
        28 => {
            // AAAA record: 16-byte IPv6 address
            if rdlength != 16 {
                return Err(DnsError::InvalidResponse(
                    "AAAA record RDATA must be 16 bytes".to_string(),
                ));
            }
            let mut bytes = [0u8; 16];
            bytes.copy_from_slice(&data[*offset..*offset + 16]);
            *offset += 16;
            RData::AAAA(Ipv6Addr(bytes))
        }
        5 => {
            // CNAME record
            let cname = decode_name(data, offset)?;
            RData::CName(cname)
        }
        2 => {
            // NS record
            let ns = decode_name(data, offset)?;
            RData::Ns(ns)
        }
        12 => {
            // PTR record
            let ptr = decode_name(data, offset)?;
            RData::Ptr(ptr)
        }
        15 => {
            // MX record: 2-byte priority + domain name
            if rdlength < 3 {
                return Err(DnsError::InvalidResponse(
                    "MX record RDATA too short".to_string(),
                ));
            }
            let priority = u16::from_be_bytes([data[*offset], data[*offset + 1]]);
            *offset += 2;
            let exchange = decode_name(data, offset)?;
            RData::Mx { priority, exchange }
        }
        16 => {
            // TXT record: one or more length-prefixed strings
            let end = rdata_start + rdlength;
            let mut txt = String::new();
            while *offset < end {
                let txt_len = data[*offset] as usize;
                *offset += 1;
                if *offset + txt_len > end {
                    return Err(DnsError::InvalidResponse(
                        "TXT string extends past RDATA".to_string(),
                    ));
                }
                let segment = String::from_utf8_lossy(&data[*offset..*offset + txt_len]);
                txt.push_str(&segment);
                *offset += txt_len;
            }
            RData::Txt(txt)
        }
        33 => {
            // SRV record: priority(2) + weight(2) + port(2) + target
            if rdlength < 7 {
                return Err(DnsError::InvalidResponse(
                    "SRV record RDATA too short".to_string(),
                ));
            }
            let priority = u16::from_be_bytes([data[*offset], data[*offset + 1]]);
            let weight = u16::from_be_bytes([data[*offset + 2], data[*offset + 3]]);
            let port = u16::from_be_bytes([data[*offset + 4], data[*offset + 5]]);
            *offset += 6;
            let target = decode_name(data, offset)?;
            RData::Srv {
                priority,
                weight,
                port,
                target,
            }
        }
        _ => {
            // Unknown record type — store raw bytes
            let raw = data[*offset..*offset + rdlength].to_vec();
            *offset += rdlength;
            RData::Unknown(raw)
        }
    };

    // Ensure offset advances by exactly rdlength regardless of parsing
    let consumed = *offset - rdata_start;
    if consumed < rdlength {
        *offset = rdata_start + rdlength;
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Hosts File Parsing
// ---------------------------------------------------------------------------

/// An entry from a hosts file mapping an IP address to hostnames.
#[derive(Debug, Clone)]
pub struct HostEntry {
    /// The IP address for this entry.
    pub addr: IpAddr,
    /// The primary hostname.
    pub hostname: String,
    /// Optional alias hostnames.
    pub aliases: Vec<String>,
}

/// Parses hosts file content (e.g., `/etc/hosts` format).
///
/// Each line has the format: `IP_ADDRESS HOSTNAME [ALIAS...]`
/// Lines starting with `#` are comments. Empty lines are skipped.
pub fn parse_hosts(content: &str) -> Vec<HostEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Strip inline comments
        let line = if let Some(pos) = line.find('#') {
            &line[..pos]
        } else {
            line
        };

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        // Skip lines with invalid IPs.
        let Ok(addr) = IpAddr::from_str(parts[0]) else {
            continue;
        };

        let hostname = parts[1].to_string();
        let aliases: Vec<String> = parts[2..].iter().map(|s| (*s).to_string()).collect();

        entries.push(HostEntry {
            addr,
            hostname,
            aliases,
        });
    }

    entries
}

/// Looks up a hostname in parsed host entries, returning matching IP addresses.
pub fn lookup_hosts(entries: &[HostEntry], hostname: &str) -> Vec<IpAddr> {
    let hostname_lower = hostname.to_lowercase();
    let mut results = Vec::new();

    for entry in entries {
        let matches_primary = entry.hostname.to_lowercase() == hostname_lower;
        let matches_alias = entry
            .aliases
            .iter()
            .any(|a| a.to_lowercase() == hostname_lower);
        if matches_primary || matches_alias {
            results.push(entry.addr);
        }
    }

    results
}

// ---------------------------------------------------------------------------
// DNS Cache
// ---------------------------------------------------------------------------

/// A cached DNS response entry with TTL-based expiry.
#[derive(Clone, Debug)]
struct CacheEntry {
    name: String,
    rtype: DnsType,
    records: Vec<DnsRecord>,
    /// Timestamp (seconds) when this entry was inserted.
    inserted_at: u64,
    /// Timestamp (seconds) when this entry expires.
    expires_at: u64,
}

/// A simple TTL-aware DNS cache.
///
/// Entries are evicted when their TTL expires or when the cache reaches
/// its maximum size (oldest entries evicted first).
pub struct DnsCache {
    entries: Vec<CacheEntry>,
    max_entries: usize,
}

impl DnsCache {
    /// Creates a new cache with the specified maximum entry count.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    /// Looks up cached records for the given name and type.
    ///
    /// Returns `None` if no matching unexpired entry exists.
    pub fn lookup(&self, name: &str, rtype: DnsType, now: u64) -> Option<&[DnsRecord]> {
        let name_lower = name.to_lowercase();
        for entry in &self.entries {
            if entry.name.to_lowercase() == name_lower
                && entry.rtype == rtype
                && entry.expires_at > now
            {
                return Some(&entry.records);
            }
        }
        None
    }

    /// Inserts records into the cache with the given TTL.
    pub fn insert(&mut self, name: &str, rtype: DnsType, records: Vec<DnsRecord>, now: u64) {
        // Determine TTL from records (use minimum TTL, default to 300s)
        let ttl = records
            .iter()
            .map(|r| r.ttl)
            .min()
            .unwrap_or(300);

        let expires_at = now + u64::from(ttl);

        // Remove existing entry for same name+type
        self.entries
            .retain(|e| !(e.name.to_lowercase() == name.to_lowercase() && e.rtype == rtype));

        // Evict expired entries
        self.entries.retain(|e| e.expires_at > now);

        // Evict oldest if at capacity
        if self.entries.len() >= self.max_entries {
            // Remove the entry with the earliest insertion time
            if let Some(oldest_idx) = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.inserted_at)
                .map(|(i, _)| i)
            {
                self.entries.remove(oldest_idx);
            }
        }

        self.entries.push(CacheEntry {
            name: name.to_lowercase(),
            rtype,
            records,
            inserted_at: now,
            expires_at,
        });
    }

    /// Removes all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns the number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Resolver Configuration
// ---------------------------------------------------------------------------

/// Configuration for the DNS resolver.
#[derive(Clone, Debug)]
pub struct ResolverConfig {
    /// Nameserver IP addresses to query.
    pub nameservers: Vec<IpAddr>,
    /// Timeout in milliseconds for each query attempt.
    pub timeout_ms: u32,
    /// Number of retry attempts before giving up.
    pub attempts: u32,
    /// Search domains appended to unqualified hostnames.
    pub search_domains: Vec<String>,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            nameservers: vec![
                IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
                IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4)),
            ],
            timeout_ms: 5000,
            attempts: 3,
            search_domains: Vec::new(),
        }
    }
}

/// Parses a resolv.conf-format string into a `ResolverConfig`.
///
/// Recognized directives:
/// - `nameserver <IP>` — adds a nameserver
/// - `search <domain> ...` — sets search domains
/// - `options timeout:<N>` — sets timeout in seconds
/// - `options attempts:<N>` — sets retry count
pub fn parse_resolv_conf(content: &str) -> ResolverConfig {
    let mut config = ResolverConfig {
        nameservers: Vec::new(),
        timeout_ms: 5000,
        attempts: 3,
        search_domains: Vec::new(),
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "nameserver" => {
                if let Some(&addr_str) = parts.get(1)
                    && let Ok(addr) = IpAddr::from_str(addr_str)
                {
                    config.nameservers.push(addr);
                }
            }
            "search" | "domain" => {
                config.search_domains = parts[1..].iter().map(|s| (*s).to_string()).collect();
            }
            "options" => {
                for &opt in &parts[1..] {
                    if let Some(val) = opt.strip_prefix("timeout:")
                        && let Ok(secs) = val.parse::<u32>()
                    {
                        config.timeout_ms = secs.saturating_mul(1000);
                    } else if let Some(val) = opt.strip_prefix("attempts:")
                        && let Ok(n) = val.parse::<u32>()
                    {
                        config.attempts = n;
                    }
                }
            }
            _ => {} // Ignore unrecognized directives
        }
    }

    // Fall back to default nameservers if none specified
    if config.nameservers.is_empty() {
        config.nameservers = vec![
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4)),
        ];
    }

    config
}

// ---------------------------------------------------------------------------
// Resolver
// ---------------------------------------------------------------------------

/// A caching DNS resolver.
///
/// Resolves hostnames by first checking the hosts file, then the cache,
/// and finally querying configured nameservers. Responses are cached
/// according to their TTL values.
pub struct Resolver {
    config: ResolverConfig,
    cache: DnsCache,
    hosts: Vec<HostEntry>,
}

impl Resolver {
    /// Creates a new resolver with the given configuration and an empty hosts file.
    pub fn new(config: ResolverConfig) -> Self {
        Self {
            config,
            cache: DnsCache::new(256),
            hosts: Vec::new(),
        }
    }

    /// Creates a resolver using system configuration.
    ///
    /// Reads `/etc/resolv.conf` for nameserver configuration and
    /// `/etc/hosts` for static hostname mappings. Falls back to
    /// sensible defaults if files cannot be read.
    pub fn system() -> Self {
        let config = match std::fs::read_to_string("/etc/resolv.conf") {
            Ok(content) => parse_resolv_conf(&content),
            Err(_) => ResolverConfig::default(),
        };

        let hosts = match std::fs::read_to_string("/etc/hosts") {
            Ok(content) => parse_hosts(&content),
            Err(_) => Vec::new(),
        };

        Self {
            config,
            cache: DnsCache::new(256),
            hosts,
        }
    }

    /// Creates a resolver with specific hosts entries (useful for testing).
    pub fn with_hosts(config: ResolverConfig, hosts: Vec<HostEntry>) -> Self {
        Self {
            config,
            cache: DnsCache::new(256),
            hosts,
        }
    }

    /// Resolves a hostname to a list of IP addresses.
    ///
    /// Checks sources in order: hosts file, cache, then DNS query.
    /// Both A (IPv4) and AAAA (IPv6) records are returned.
    ///
    /// # Errors
    ///
    /// Returns [`DnsError::InvalidName`] if `hostname` is empty.
    /// Returns [`DnsError::NetworkError`] if no hosts/cache entry matches
    /// and the UDP transport is not yet wired up.
    pub fn resolve(&mut self, hostname: &str) -> Result<Vec<IpAddr>, DnsError> {
        // Validate hostname
        if hostname.is_empty() {
            return Err(DnsError::InvalidName("empty hostname".to_string()));
        }

        // Check if it's already an IP address
        if let Ok(addr) = IpAddr::from_str(hostname) {
            return Ok(vec![addr]);
        }

        // Check hosts file
        let host_results = lookup_hosts(&self.hosts, hostname);
        if !host_results.is_empty() {
            return Ok(host_results);
        }

        // Check cache (using timestamp 0 as placeholder — real implementation
        // would use system time)
        let now = self.current_time();
        if let Some(records) = self.cache.lookup(hostname, DnsType::A, now) {
            let addrs: Vec<IpAddr> = records
                .iter()
                .filter_map(|r| match &r.rdata {
                    RData::A(addr) => Some(IpAddr::V4(*addr)),
                    _ => None,
                })
                .collect();
            if !addrs.is_empty() {
                return Ok(addrs);
            }
        }

        // Query DNS (stub: returns NetworkError since we lack actual socket I/O)
        // In a full implementation, this would send a UDP packet to the nameserver
        // and parse the response.
        Err(DnsError::NetworkError(
            "DNS UDP transport not yet implemented".to_string(),
        ))
    }

    /// Queries for specific DNS record types.
    ///
    /// Returns matching records from cache or by querying nameservers.
    ///
    /// # Errors
    ///
    /// Returns [`DnsError::InvalidName`] if `name` is empty.
    /// Returns [`DnsError::NetworkError`] if no cache entry matches and
    /// the UDP transport is not yet wired up.
    pub fn query(&mut self, name: &str, qtype: DnsType) -> Result<Vec<DnsRecord>, DnsError> {
        if name.is_empty() {
            return Err(DnsError::InvalidName("empty name".to_string()));
        }

        // Check cache
        let now = self.current_time();
        if let Some(records) = self.cache.lookup(name, qtype, now) {
            return Ok(records.to_vec());
        }

        // Query DNS (stub)
        Err(DnsError::NetworkError(
            "DNS UDP transport not yet implemented".to_string(),
        ))
    }

    /// Clears the DNS cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Returns the resolver's configuration.
    pub fn config(&self) -> &ResolverConfig {
        &self.config
    }

    /// Manually inserts records into the cache (useful for testing and preloading).
    pub fn cache_insert(&mut self, name: &str, rtype: DnsType, records: Vec<DnsRecord>) {
        let now = self.current_time();
        self.cache.insert(name, rtype, records, now);
    }

    /// Returns a monotonic timestamp in seconds.
    ///
    /// Stub implementation returns 0; a full implementation would use the
    /// kernel's monotonic clock.  Kept as a method (not an associated
    /// function) so that wiring up `&self.config`/per-resolver state later
    /// is non-breaking.
    #[allow(clippy::unused_self)]
    fn current_time(&self) -> u64 {
        // TODO: integrate with OurOS monotonic clock
        0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- IPv4 Address Tests ---

    #[test]
    fn test_ipv4_display() {
        let addr = Ipv4Addr::new(192, 168, 1, 100);
        assert_eq!(addr.to_string(), "192.168.1.100");
    }

    #[test]
    fn test_ipv4_parse_valid() {
        let addr: Ipv4Addr = "10.0.0.1".parse().unwrap();
        assert_eq!(addr.octets(), [10, 0, 0, 1]);
    }

    #[test]
    fn test_ipv4_parse_zeros() {
        let addr: Ipv4Addr = "0.0.0.0".parse().unwrap();
        assert_eq!(addr.octets(), [0, 0, 0, 0]);
    }

    #[test]
    fn test_ipv4_parse_max() {
        let addr: Ipv4Addr = "255.255.255.255".parse().unwrap();
        assert_eq!(addr.octets(), [255, 255, 255, 255]);
    }

    #[test]
    fn test_ipv4_parse_invalid_octet() {
        assert!(Ipv4Addr::from_str("256.0.0.1").is_err());
    }

    #[test]
    fn test_ipv4_parse_too_few_parts() {
        assert!(Ipv4Addr::from_str("10.0.1").is_err());
    }

    #[test]
    fn test_ipv4_parse_too_many_parts() {
        assert!(Ipv4Addr::from_str("10.0.0.1.2").is_err());
    }

    // --- IPv6 Address Tests ---

    #[test]
    fn test_ipv6_display() {
        let addr = Ipv6Addr::from_segments([0x2001, 0x0db8, 0, 0, 0, 0, 0, 1]);
        assert_eq!(addr.to_string(), "2001:db8:0:0:0:0:0:1");
    }

    #[test]
    fn test_ipv6_parse_full() {
        let addr: Ipv6Addr = "2001:db8:0:0:0:0:0:1".parse().unwrap();
        assert_eq!(addr.segments(), [0x2001, 0x0db8, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn test_ipv6_parse_compressed() {
        let addr: Ipv6Addr = "2001:db8::1".parse().unwrap();
        assert_eq!(addr.segments(), [0x2001, 0x0db8, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn test_ipv6_parse_loopback() {
        let addr: Ipv6Addr = "::1".parse().unwrap();
        assert_eq!(addr.segments(), [0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn test_ipv6_parse_all_zeros() {
        let addr: Ipv6Addr = "::".parse().unwrap();
        assert_eq!(addr.segments(), [0, 0, 0, 0, 0, 0, 0, 0]);
    }

    // --- IpAddr Tests ---

    #[test]
    fn test_ipaddr_parse_v4() {
        let addr: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn test_ipaddr_parse_v6() {
        let addr: IpAddr = "::1".parse().unwrap();
        assert_eq!(
            addr,
            IpAddr::V6(Ipv6Addr::from_segments([0, 0, 0, 0, 0, 0, 0, 1]))
        );
    }

    // --- DNS Name Encoding/Decoding Tests ---

    #[test]
    fn test_encode_name_simple() {
        let encoded = encode_name("example.com").unwrap();
        assert_eq!(
            encoded,
            vec![7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]
        );
    }

    #[test]
    fn test_encode_name_trailing_dot() {
        let encoded = encode_name("example.com.").unwrap();
        assert_eq!(
            encoded,
            vec![7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0]
        );
    }

    #[test]
    fn test_encode_name_empty() {
        let encoded = encode_name("").unwrap();
        assert_eq!(encoded, vec![0]);
    }

    #[test]
    fn test_encode_name_label_too_long() {
        let long_label = "a".repeat(64);
        let name = format!("{long_label}.com");
        assert!(encode_name(&name).is_err());
    }

    #[test]
    fn test_decode_name_simple() {
        let data = vec![7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0];
        let mut offset = 0;
        let name = decode_name(&data, &mut offset).unwrap();
        assert_eq!(name, "example.com");
        assert_eq!(offset, 13);
    }

    #[test]
    fn test_decode_name_with_compression() {
        // Build a message where a second name uses a compression pointer to the first
        let mut data = vec![
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
        ];
        // Pointer at offset 13 pointing to offset 0
        data.push(0xC0);
        data.push(0x00);

        let mut offset = 13;
        let name = decode_name(&data, &mut offset).unwrap();
        assert_eq!(name, "example.com");
        assert_eq!(offset, 15);
    }

    #[test]
    fn test_decode_name_compression_loop_protection() {
        // Create a self-referencing pointer (infinite loop)
        let data = vec![0xC0, 0x00];
        let mut offset = 0;
        assert!(decode_name(&data, &mut offset).is_err());
    }

    // --- DNS Message Serialization Tests ---

    #[test]
    fn test_serialize_query_a_record() {
        let query = serialize_query("example.com", DnsType::A).unwrap();

        // Check header
        assert_eq!(query[0..2], [0x12, 0x34]); // ID
        assert_eq!(query[2..4], [0x01, 0x00]); // Flags: RD=1
        assert_eq!(query[4..6], [0x00, 0x01]); // QDCOUNT=1
        assert_eq!(query[6..8], [0x00, 0x00]); // ANCOUNT=0
        assert_eq!(query[8..10], [0x00, 0x00]); // NSCOUNT=0
        assert_eq!(query[10..12], [0x00, 0x00]); // ARCOUNT=0

        // Check question section has encoded name
        assert_eq!(query[12], 7); // "example" label length
        assert_eq!(&query[13..20], b"example");
        assert_eq!(query[20], 3); // "com" label length
        assert_eq!(&query[21..24], b"com");
        assert_eq!(query[24], 0); // Root terminator
        assert_eq!(query[25..27], [0x00, 0x01]); // QTYPE = A
        assert_eq!(query[27..29], [0x00, 0x01]); // QCLASS = IN
    }

    #[test]
    fn test_parse_response_basic() {
        // Manually construct a minimal DNS response with one A record
        let mut response: Vec<u8> = Vec::new();

        // Header
        response.extend_from_slice(&[0x12, 0x34]); // ID
        response.extend_from_slice(&[0x81, 0x80]); // Flags: QR=1, RD=1, RA=1
        response.extend_from_slice(&[0x00, 0x01]); // QDCOUNT=1
        response.extend_from_slice(&[0x00, 0x01]); // ANCOUNT=1
        response.extend_from_slice(&[0x00, 0x00]); // NSCOUNT=0
        response.extend_from_slice(&[0x00, 0x00]); // ARCOUNT=0

        // Question: example.com A IN
        response.extend_from_slice(&[7, b'e', b'x', b'a', b'm', b'p', b'l', b'e']);
        response.extend_from_slice(&[3, b'c', b'o', b'm', 0]);
        response.extend_from_slice(&[0x00, 0x01]); // QTYPE = A
        response.extend_from_slice(&[0x00, 0x01]); // QCLASS = IN

        // Answer: compression pointer to question name, then A record
        response.extend_from_slice(&[0xC0, 0x0C]); // Pointer to offset 12 (name)
        response.extend_from_slice(&[0x00, 0x01]); // TYPE = A
        response.extend_from_slice(&[0x00, 0x01]); // CLASS = IN
        response.extend_from_slice(&[0x00, 0x00, 0x01, 0x2C]); // TTL = 300
        response.extend_from_slice(&[0x00, 0x04]); // RDLENGTH = 4
        response.extend_from_slice(&[93, 184, 216, 34]); // RDATA = 93.184.216.34

        let msg = parse_response(&response).unwrap();
        assert!(msg.header.is_response());
        assert_eq!(msg.header.rcode(), 0);
        assert_eq!(msg.questions.len(), 1);
        assert_eq!(msg.questions[0].name, "example.com");
        assert_eq!(msg.answers.len(), 1);
        assert_eq!(msg.answers[0].name, "example.com");
        assert_eq!(msg.answers[0].ttl, 300);
        match &msg.answers[0].rdata {
            RData::A(addr) => assert_eq!(addr.octets(), [93, 184, 216, 34]),
            _ => panic!("expected A record"),
        }
    }

    #[test]
    fn test_parse_response_nxdomain() {
        let mut response: Vec<u8> = Vec::new();
        // Header with RCODE=3 (NXDOMAIN)
        response.extend_from_slice(&[0x12, 0x34]); // ID
        response.extend_from_slice(&[0x81, 0x83]); // Flags: QR=1, RD=1, RA=1, RCODE=3
        response.extend_from_slice(&[0x00, 0x00]); // QDCOUNT=0
        response.extend_from_slice(&[0x00, 0x00]); // ANCOUNT=0
        response.extend_from_slice(&[0x00, 0x00]); // NSCOUNT=0
        response.extend_from_slice(&[0x00, 0x00]); // ARCOUNT=0

        let result = parse_response(&response);
        // `DnsMessage` does not implement `Debug`/`PartialEq` (deliberate —
        // these are large structures and we don't want the cost in production
        // code), so we can't use `assert_eq!` on `Result<DnsMessage, _>`.
        // `matches!` checks the error variant without requiring those traits.
        assert!(matches!(result, Err(DnsError::NameNotFound)));
    }

    #[test]
    fn test_parse_response_too_short() {
        let data = vec![0x12, 0x34, 0x00];
        assert!(parse_response(&data).is_err());
    }

    // --- Hosts File Tests ---

    #[test]
    fn test_parse_hosts_basic() {
        let content = "127.0.0.1 localhost\n::1 localhost ip6-localhost\n192.168.1.1 router\n";
        let entries = parse_hosts(content);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].hostname, "localhost");
        assert_eq!(entries[0].addr, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        assert_eq!(entries[1].aliases, vec!["ip6-localhost"]);
        assert_eq!(entries[2].hostname, "router");
    }

    #[test]
    fn test_parse_hosts_comments() {
        let content = "# This is a comment\n127.0.0.1 localhost # inline comment\n";
        let entries = parse_hosts(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hostname, "localhost");
    }

    #[test]
    fn test_lookup_hosts_found() {
        let entries = vec![HostEntry {
            addr: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            hostname: "myhost".to_string(),
            aliases: vec!["myhost.local".to_string()],
        }];
        let results = lookup_hosts(&entries, "myhost");
        assert_eq!(results, vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))]);
    }

    #[test]
    fn test_lookup_hosts_alias() {
        let entries = vec![HostEntry {
            addr: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            hostname: "myhost".to_string(),
            aliases: vec!["myhost.local".to_string()],
        }];
        let results = lookup_hosts(&entries, "myhost.local");
        assert_eq!(results, vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))]);
    }

    #[test]
    fn test_lookup_hosts_case_insensitive() {
        let entries = vec![HostEntry {
            addr: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            hostname: "MyHost".to_string(),
            aliases: Vec::new(),
        }];
        let results = lookup_hosts(&entries, "myhost");
        assert_eq!(results, vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))]);
    }

    #[test]
    fn test_lookup_hosts_not_found() {
        let entries = vec![HostEntry {
            addr: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            hostname: "myhost".to_string(),
            aliases: Vec::new(),
        }];
        let results = lookup_hosts(&entries, "unknown");
        assert!(results.is_empty());
    }

    // --- Cache Tests ---

    #[test]
    fn test_cache_insert_and_lookup() {
        let mut cache = DnsCache::new(10);
        let records = vec![DnsRecord {
            name: "example.com".to_string(),
            rtype: DnsType::A,
            class: DnsClass::IN,
            ttl: 300,
            rdata: RData::A(Ipv4Addr::new(1, 2, 3, 4)),
        }];
        cache.insert("example.com", DnsType::A, records.clone(), 1000);

        let result = cache.lookup("example.com", DnsType::A, 1000);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_cache_expired_entry() {
        let mut cache = DnsCache::new(10);
        let records = vec![DnsRecord {
            name: "example.com".to_string(),
            rtype: DnsType::A,
            class: DnsClass::IN,
            ttl: 60,
            rdata: RData::A(Ipv4Addr::new(1, 2, 3, 4)),
        }];
        cache.insert("example.com", DnsType::A, records, 1000);

        // After TTL expires
        let result = cache.lookup("example.com", DnsType::A, 1100);
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = DnsCache::new(2);
        let make_record = |name: &str| {
            vec![DnsRecord {
                name: name.to_string(),
                rtype: DnsType::A,
                class: DnsClass::IN,
                ttl: 300,
                rdata: RData::A(Ipv4Addr::new(1, 2, 3, 4)),
            }]
        };

        cache.insert("a.com", DnsType::A, make_record("a.com"), 100);
        cache.insert("b.com", DnsType::A, make_record("b.com"), 200);
        cache.insert("c.com", DnsType::A, make_record("c.com"), 300);

        // Oldest entry (a.com) should have been evicted
        assert!(cache.lookup("a.com", DnsType::A, 300).is_none());
        assert!(cache.lookup("b.com", DnsType::A, 300).is_some());
        assert!(cache.lookup("c.com", DnsType::A, 300).is_some());
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = DnsCache::new(10);
        let records = vec![DnsRecord {
            name: "example.com".to_string(),
            rtype: DnsType::A,
            class: DnsClass::IN,
            ttl: 300,
            rdata: RData::A(Ipv4Addr::new(1, 2, 3, 4)),
        }];
        cache.insert("example.com", DnsType::A, records, 0);
        assert!(!cache.is_empty());

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    // --- Resolver Tests ---

    #[test]
    fn test_resolver_resolve_ip_literal() {
        let mut resolver = Resolver::new(ResolverConfig::default());
        let result = resolver.resolve("192.168.1.1").unwrap();
        assert_eq!(result, vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))]);
    }

    #[test]
    fn test_resolver_resolve_from_hosts() {
        let hosts = vec![HostEntry {
            addr: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            hostname: "localhost".to_string(),
            aliases: Vec::new(),
        }];
        let mut resolver = Resolver::with_hosts(ResolverConfig::default(), hosts);
        let result = resolver.resolve("localhost").unwrap();
        assert_eq!(result, vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))]);
    }

    #[test]
    fn test_resolver_empty_hostname() {
        let mut resolver = Resolver::new(ResolverConfig::default());
        assert!(resolver.resolve("").is_err());
    }

    // --- resolv.conf Parsing Tests ---

    #[test]
    fn test_parse_resolv_conf() {
        let content = "\
# Generated by NetworkManager
nameserver 192.168.1.1
nameserver 8.8.8.8
search example.com local.net
options timeout:2 attempts:5
";
        let config = parse_resolv_conf(content);
        assert_eq!(config.nameservers.len(), 2);
        assert_eq!(
            config.nameservers[0],
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))
        );
        assert_eq!(
            config.nameservers[1],
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))
        );
        assert_eq!(config.search_domains, vec!["example.com", "local.net"]);
        assert_eq!(config.timeout_ms, 2000);
        assert_eq!(config.attempts, 5);
    }

    #[test]
    fn test_parse_resolv_conf_empty() {
        let config = parse_resolv_conf("");
        // Should fall back to default nameservers
        assert_eq!(config.nameservers.len(), 2);
        assert_eq!(
            config.nameservers[0],
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))
        );
    }
}
