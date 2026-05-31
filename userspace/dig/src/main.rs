//! OurOS DNS Lookup Utility (`dig`)
//!
//! A feature-rich DNS lookup tool modelled after ISC's `dig`. Builds DNS query
//! packets from scratch per RFC 1035 and parses wire-format responses including
//! compression pointers. Communicates with DNS servers via OurOS kernel syscalls
//! (UDP and TCP).
//!
//! # Usage
//!
//! ```text
//! dig example.com                     A record lookup (default)
//! dig example.com AAAA                Query specific record type
//! dig @8.8.8.8 example.com            Use specific DNS server
//! dig -x 93.184.216.34                Reverse (PTR) lookup
//! dig example.com +short              Short output (data only)
//! dig example.com +tcp                Query over TCP
//! dig example.com +norecurse          Disable recursion desired
//! dig example.com +trace              Iterative trace from root
//! dig example.com +timeout=5          Set timeout to 5 seconds
//! dig example.com +retry=3            Set retry count to 3
//! ```

#![deny(clippy::all)]
// Tests are allowed to panic on bad data.
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![cfg_attr(not(test), warn(clippy::panic))]
#![cfg_attr(not(test), warn(clippy::indexing_slicing))]

use std::env;
use std::fs;
use std::process;
use std::time::Instant;

// ============================================================================
// Syscall numbers
// ============================================================================

// Native OurOS syscall numbers (kernel/src/syscall/number.rs). These were
// previously wrong: the UDP block used 820/821/822 (820 is DNS_RESOLVE, not
// UDP_BIND), and the TCP block was off by one (802/803/804 collided with
// TCP_RECV/CLOSE and an unassigned slot). Corrected to the real ABI.
const SYS_UDP_BIND: u64 = 810;
const SYS_UDP_SEND: u64 = 811;
const SYS_UDP_RECV: u64 = 812;
const SYS_TCP_CONNECT: u64 = 800;
const SYS_TCP_SEND: u64 = 801;
const SYS_TCP_RECV: u64 = 802;
const SYS_TCP_CLOSE: u64 = 803;
// Native OurOS monotonic clock (kernel syscall/number.rs); no-arg, returns
// boot-relative nanoseconds in rax.  (Syscall 30 is SYS_IRQ_REGISTER.)
const SYS_CLOCK_MONOTONIC: u64 = 10;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 3-argument syscall via the x86_64 `syscall` instruction.
///
/// # Safety
///
/// The caller must ensure:
/// - `nr` is a valid syscall number.
/// - Arguments are valid for the specific syscall (e.g., pointers must be
///   readable/writable as required, sizes must be accurate).
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid for the given syscall.
    // The `syscall` instruction clobbers rcx and r11 per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issue a 1-argument syscall (e.g. close).
///
/// # Safety
///
/// Same requirements as [`syscall3`].
#[cfg(target_arch = "x86_64")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees the argument is valid. rcx and r11 clobbered.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Bind a UDP socket to `0.0.0.0:0` (ephemeral port). Returns a handle.
fn udp_bind() -> Result<u64, DigError> {
    // SAFETY: SYS_UDP_BIND with 0,0,0 binds an ephemeral port. No pointers.
    let ret = unsafe { syscall3(SYS_UDP_BIND, 0, 0, 0) };
    if ret < 0 {
        return Err(DigError::Network(format!("udp_bind failed: {ret}")));
    }
    Ok(ret as u64)
}

/// Send `data` via UDP to `ip`:`port`.
/// `handle` is the socket handle from [`udp_bind`].
/// `ip` is a u32 in network byte order. `port` is in host byte order.
fn udp_send(handle: u64, ip: u32, port: u16, data: &[u8]) -> Result<usize, DigError> {
    // Pack destination address: ip (32 bits) | port (16 bits) in a u64.
    let dest = (u64::from(ip) << 16) | u64::from(port);
    // SAFETY: We pass a valid handle and pointer/length for the data buffer.
    let ret = unsafe {
        syscall3(SYS_UDP_SEND, handle, data.as_ptr() as u64, (data.len() as u64) | (dest << 32))
    };
    if ret < 0 {
        return Err(DigError::Network(format!("udp_send failed: {ret}")));
    }
    Ok(ret as usize)
}

/// Receive UDP data into `buf`. Returns number of bytes received.
fn udp_recv(handle: u64, buf: &mut [u8], _timeout_ms: u64) -> Result<usize, DigError> {
    // SAFETY: We pass a valid handle and a mutable buffer with its correct length.
    let ret = unsafe {
        syscall3(
            SYS_UDP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(DigError::Timeout);
    }
    Ok(ret as usize)
}

/// Open a TCP connection to `ip`:`port`. Returns a handle.
fn tcp_connect(ip: u32, port: u16) -> Result<u64, DigError> {
    // SAFETY: Scalar arguments only, no pointers.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 0) };
    if ret < 0 {
        return Err(DigError::Network(format!("tcp_connect failed: {ret}")));
    }
    Ok(ret as u64)
}

/// Send all bytes over a TCP connection.
fn tcp_send_all(handle: u64, data: &[u8]) -> Result<(), DigError> {
    let mut offset = 0;
    while offset < data.len() {
        // offset < data.len() is guaranteed by the loop condition.
        let remaining = data.get(offset..).unwrap_or(&[]);
        // SAFETY: Valid handle and pointer/length.
        let ret = unsafe {
            syscall3(
                SYS_TCP_SEND,
                handle,
                remaining.as_ptr() as u64,
                remaining.len() as u64,
            )
        };
        if ret <= 0 {
            return Err(DigError::Network(format!("tcp_send failed: {ret}")));
        }
        offset = offset.saturating_add(ret as usize);
    }
    Ok(())
}

/// Receive up to `buf.len()` bytes from a TCP connection.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> Result<usize, DigError> {
    // SAFETY: Valid handle and mutable buffer pointer/length.
    let ret = unsafe {
        syscall3(
            SYS_TCP_RECV,
            handle,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if ret < 0 {
        return Err(DigError::Network(format!("tcp_recv failed: {ret}")));
    }
    Ok(ret as usize)
}

/// Close a TCP connection handle.
fn tcp_close(handle: u64) {
    // SAFETY: Valid handle. Ignoring the return: handle becomes invalid regardless.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Read the monotonic clock in microseconds.
fn clock_monotonic_us() -> u64 {
    // SAFETY: SYS_CLOCK_MONOTONIC returns the time; no pointer arguments needed.
    let ret = unsafe { syscall3(SYS_CLOCK_MONOTONIC, 0, 0, 0) };
    if ret < 0 { 0 } else { ret as u64 }
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug)]
enum DigError {
    Usage(String),
    Network(String),
    Timeout,
    DnsProtocol(String),
    #[allow(dead_code)] // Kept for future use when file-based config is read.
    Io(String),
}

impl std::fmt::Display for DigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(msg) => write!(f, "{msg}"),
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::Timeout => write!(f, "connection timed out; no servers could be reached"),
            Self::DnsProtocol(msg) => write!(f, "DNS protocol error: {msg}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

// ============================================================================
// DNS constants (RFC 1035)
// ============================================================================

const DNS_HEADER_LEN: usize = 12;
const DNS_MAX_UDP: usize = 4096; // EDNS allows larger, but 4096 is a safe default.
const DNS_PORT: u16 = 53;
const DEFAULT_TIMEOUT_SECS: u32 = 5;
const DEFAULT_RETRIES: u32 = 2;
const FALLBACK_SERVER: &str = "8.8.8.8";

// DNS record type codes.
const TYPE_A: u16 = 1;
const TYPE_NS: u16 = 2;
const TYPE_CNAME: u16 = 5;
const TYPE_SOA: u16 = 6;
const TYPE_PTR: u16 = 12;
const TYPE_MX: u16 = 15;
const TYPE_TXT: u16 = 16;
const TYPE_AAAA: u16 = 28;
const TYPE_SRV: u16 = 33;
const TYPE_ANY: u16 = 255;

// DNS class code.
const CLASS_IN: u16 = 1;

// DNS header flags.
const FLAG_QR: u16 = 0x8000;
const FLAG_AA: u16 = 0x0400;
const FLAG_TC: u16 = 0x0200;
const FLAG_RD: u16 = 0x0100;
const FLAG_RA: u16 = 0x0080;
const FLAG_AD: u16 = 0x0020;
const FLAG_CD: u16 = 0x0010;

const OPCODE_SHIFT: u16 = 11;
const OPCODE_MASK: u16 = 0xF;
const RCODE_MASK: u16 = 0x000F;

// ============================================================================
// Record type mapping
// ============================================================================

/// Maps a user-facing record type string to its numeric code.
fn parse_record_type(s: &str) -> Option<u16> {
    match s.to_ascii_uppercase().as_str() {
        "A" => Some(TYPE_A),
        "AAAA" => Some(TYPE_AAAA),
        "MX" => Some(TYPE_MX),
        "NS" => Some(TYPE_NS),
        "CNAME" => Some(TYPE_CNAME),
        "TXT" => Some(TYPE_TXT),
        "SOA" => Some(TYPE_SOA),
        "PTR" => Some(TYPE_PTR),
        "SRV" => Some(TYPE_SRV),
        "ANY" => Some(TYPE_ANY),
        _ => None,
    }
}

/// Returns the human-readable name for a record type code.
fn type_name(code: u16) -> &'static str {
    match code {
        TYPE_A => "A",
        TYPE_NS => "NS",
        TYPE_CNAME => "CNAME",
        TYPE_SOA => "SOA",
        TYPE_PTR => "PTR",
        TYPE_MX => "MX",
        TYPE_TXT => "TXT",
        TYPE_AAAA => "AAAA",
        TYPE_SRV => "SRV",
        TYPE_ANY => "ANY",
        other => {
            // Return "TYPE<N>" for unknown types. We leak a small string here
            // which is acceptable for a short-lived CLI tool.
            let s = format!("TYPE{other}");
            // Leak to get a 'static lifetime -- dig runs once then exits.
            Box::leak(s.into_boxed_str())
        }
    }
}

/// Returns a human-readable RCODE description.
fn rcode_str(rcode: u16) -> &'static str {
    match rcode {
        0 => "NOERROR",
        1 => "FORMERR",
        2 => "SERVFAIL",
        3 => "NXDOMAIN",
        4 => "NOTIMP",
        5 => "REFUSED",
        6 => "YXDOMAIN",
        7 => "YXRRSET",
        8 => "NXRRSET",
        9 => "NOTAUTH",
        10 => "NOTZONE",
        _ => "UNKNOWN",
    }
}

/// Returns the opcode name.
fn opcode_str(opcode: u16) -> &'static str {
    match opcode {
        0 => "QUERY",
        1 => "IQUERY",
        2 => "STATUS",
        4 => "NOTIFY",
        5 => "UPDATE",
        _ => "UNKNOWN",
    }
}

/// Returns the class name.
fn class_name(class: u16) -> &'static str {
    match class {
        1 => "IN",
        3 => "CH",
        4 => "HS",
        255 => "ANY",
        _ => "UNKNOWN",
    }
}

// ============================================================================
// DNS server discovery
// ============================================================================

/// Reads the first `nameserver` entry from /etc/resolv.conf.
/// Falls back to a well-known public resolver if the file is unreadable.
fn default_dns_server() -> String {
    if let Ok(contents) = fs::read_to_string("/etc/resolv.conf") {
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("nameserver")
                && let Some(addr) = trimmed.split_whitespace().nth(1)
            {
                return addr.to_string();
            }
        }
    }
    FALLBACK_SERVER.to_string()
}

// ============================================================================
// IP address helpers
// ============================================================================

/// Parse a dotted-decimal IPv4 address into a u32 in network byte order.
fn parse_ipv4(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        let val: u16 = part.parse().ok()?;
        if val > 255 {
            return None;
        }
        if let Some(slot) = octets.get_mut(i) {
            *slot = val as u8;
        }
    }
    Some(u32::from_be_bytes(octets))
}

/// Format a u32 IP address (network byte order) as dotted-decimal.
fn format_ipv4(ip: u32) -> String {
    let o = ip.to_be_bytes();
    format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
}

// ============================================================================
// Reverse-lookup helper
// ============================================================================

/// Converts an IPv4 address string into its PTR query name.
/// `1.2.3.4` becomes `4.3.2.1.in-addr.arpa`.
fn reverse_name_v4(ip_str: &str) -> Result<String, DigError> {
    let parts: Vec<&str> = ip_str.split('.').collect();
    if parts.len() != 4 {
        return Err(DigError::Usage(format!("'{ip_str}' is not a valid IPv4 address")));
    }
    // Validate each octet.
    for part in &parts {
        let _: u8 = part
            .parse()
            .map_err(|_| DigError::Usage(format!("'{ip_str}' is not a valid IPv4 address")))?;
    }
    Ok(format!(
        "{}.{}.{}.{}.in-addr.arpa",
        parts.get(3).copied().unwrap_or("0"),
        parts.get(2).copied().unwrap_or("0"),
        parts.get(1).copied().unwrap_or("0"),
        parts.first().copied().unwrap_or("0"),
    ))
}

// ============================================================================
// DNS packet construction (RFC 1035)
// ============================================================================

/// Encodes a domain name into DNS wire format (label-length-prefixed).
/// `www.example.com` becomes `[3]www[7]example[3]com[0]`.
fn encode_domain_name(name: &str, buf: &mut Vec<u8>) {
    let name = name.trim_end_matches('.');
    if name.is_empty() {
        buf.push(0);
        return;
    }
    for label in name.split('.') {
        let len = label.len().min(63);
        buf.push(len as u8);
        // len <= label.len(), so the slice always exists.
        buf.extend_from_slice(label.as_bytes().get(..len).unwrap_or(&[]));
    }
    buf.push(0);
}

/// Simple non-cryptographic hash for generating transaction IDs.
fn make_txn_id() -> u16 {
    // Use the monotonic clock as a source of entropy.
    let t = clock_monotonic_us();
    let mut h: u32 = 5381;
    for b in t.to_le_bytes() {
        h = h.wrapping_mul(33).wrapping_add(u32::from(b));
    }
    (h & 0xFFFF) as u16
}

/// Builds a DNS query packet.
fn build_query(name: &str, qtype: u16, recursion: bool, txn_id: u16) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // Header (12 bytes).
    pkt.push((txn_id >> 8) as u8);
    pkt.push(txn_id as u8);

    let mut flags: u16 = 0;
    if recursion {
        flags |= FLAG_RD;
    }
    pkt.push((flags >> 8) as u8);
    pkt.push(flags as u8);

    // QDCOUNT = 1, ANCOUNT = 0, NSCOUNT = 0, ARCOUNT = 0.
    pkt.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);

    // Question section.
    encode_domain_name(name, &mut pkt);
    pkt.push((qtype >> 8) as u8);
    pkt.push(qtype as u8);
    pkt.push((CLASS_IN >> 8) as u8);
    pkt.push(CLASS_IN as u8);

    pkt
}

// ============================================================================
// DNS packet parsing
// ============================================================================

/// Read a big-endian u16 from `data` at `offset`.
fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    let hi = *data.get(offset)? as u16;
    let lo = *data.get(offset.checked_add(1)?)? as u16;
    Some((hi << 8) | lo)
}

/// Read a big-endian u32 from `data` at `offset`.
fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let a = *data.get(offset)? as u32;
    let b = *data.get(offset.checked_add(1)?)? as u32;
    let c = *data.get(offset.checked_add(2)?)? as u32;
    let d = *data.get(offset.checked_add(3)?)? as u32;
    Some((a << 24) | (b << 16) | (c << 8) | d)
}

/// Decode a DNS domain name from wire format, following compression pointers.
/// Returns the decoded name and the number of bytes consumed starting at `start`.
fn decode_domain_name(data: &[u8], start: usize) -> Result<(String, usize), DigError> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = start;
    let mut jumped = false;
    let mut consumed = 0;
    let mut hops = 0;
    const MAX_HOPS: usize = 128;

    loop {
        if hops > MAX_HOPS {
            return Err(DigError::DnsProtocol(
                "too many compression pointer hops".to_string(),
            ));
        }
        let byte = *data.get(pos).ok_or_else(|| {
            DigError::DnsProtocol("unexpected end of packet in domain name".to_string())
        })?;

        if byte == 0 {
            if !jumped {
                consumed = pos.saturating_sub(start).saturating_add(1);
            }
            break;
        }

        // Compression pointer: top two bits are 11.
        if byte & 0xC0 == 0xC0 {
            let ptr_lo = *data.get(pos.saturating_add(1)).ok_or_else(|| {
                DigError::DnsProtocol("truncated compression pointer".to_string())
            })?;
            let ptr = (u16::from(byte & 0x3F) << 8 | u16::from(ptr_lo)) as usize;
            if !jumped {
                consumed = pos.saturating_sub(start).saturating_add(2);
            }
            pos = ptr;
            jumped = true;
            hops += 1;
            continue;
        }

        // Regular label.
        let len = byte as usize;
        let label_end = pos.saturating_add(1).saturating_add(len);
        if label_end > data.len() {
            return Err(DigError::DnsProtocol(
                "label extends past end of packet".to_string(),
            ));
        }
        let label =
            String::from_utf8_lossy(data.get(pos + 1..label_end).unwrap_or_default()).to_string();
        labels.push(label);
        pos = label_end;
        hops += 1;
    }

    if consumed == 0 && !jumped {
        consumed = 1;
    }

    Ok((labels.join("."), consumed))
}

/// A parsed DNS resource record.
#[derive(Clone)]
struct DnsRecord {
    name: String,
    rtype: u16,
    rclass: u16,
    ttl: u32,
    #[allow(dead_code)] // Retained for callers that need raw RDATA bytes (e.g. DNSSEC validation).
    rdata_raw: Vec<u8>,
    rdata: String,
}

/// Parsed DNS header fields.
struct DnsHeader {
    id: u16,
    flags: u16,
    qdcount: u16,
    ancount: u16,
    nscount: u16,
    arcount: u16,
}

/// Parsed DNS response.
struct DnsResponse {
    header: DnsHeader,
    questions: Vec<DnsQuestion>,
    answers: Vec<DnsRecord>,
    authorities: Vec<DnsRecord>,
    additionals: Vec<DnsRecord>,
    raw_size: usize,
}

struct DnsQuestion {
    name: String,
    qtype: u16,
    qclass: u16,
}

impl DnsHeader {
    fn is_response(&self) -> bool {
        self.flags & FLAG_QR != 0
    }
    fn opcode(&self) -> u16 {
        (self.flags >> OPCODE_SHIFT) & OPCODE_MASK
    }
    fn rcode(&self) -> u16 {
        self.flags & RCODE_MASK
    }
    fn is_authoritative(&self) -> bool {
        self.flags & FLAG_AA != 0
    }
    fn is_truncated(&self) -> bool {
        self.flags & FLAG_TC != 0
    }
    fn recursion_desired(&self) -> bool {
        self.flags & FLAG_RD != 0
    }
    fn recursion_available(&self) -> bool {
        self.flags & FLAG_RA != 0
    }
    fn authenticated_data(&self) -> bool {
        self.flags & FLAG_AD != 0
    }
    fn checking_disabled(&self) -> bool {
        self.flags & FLAG_CD != 0
    }

    /// Build the flags line like `qr rd ra`.
    fn flags_string(&self) -> String {
        let mut flags = Vec::new();
        if self.is_response() {
            flags.push("qr");
        }
        if self.is_authoritative() {
            flags.push("aa");
        }
        if self.is_truncated() {
            flags.push("tc");
        }
        if self.recursion_desired() {
            flags.push("rd");
        }
        if self.recursion_available() {
            flags.push("ra");
        }
        if self.authenticated_data() {
            flags.push("ad");
        }
        if self.checking_disabled() {
            flags.push("cd");
        }
        flags.join(" ")
    }
}

/// Parse RDATA into a human-readable string.
fn parse_rdata(
    rtype: u16,
    data: &[u8],
    rdata_offset: usize,
    rdlen: usize,
) -> Result<String, DigError> {
    match rtype {
        TYPE_A => {
            if rdlen != 4 {
                return Err(DigError::DnsProtocol(format!(
                    "A record has unexpected RDLENGTH {rdlen}"
                )));
            }
            Ok(format!(
                "{}.{}.{}.{}",
                data.get(rdata_offset).copied().unwrap_or(0),
                data.get(rdata_offset.saturating_add(1)).copied().unwrap_or(0),
                data.get(rdata_offset.saturating_add(2)).copied().unwrap_or(0),
                data.get(rdata_offset.saturating_add(3)).copied().unwrap_or(0),
            ))
        }

        TYPE_AAAA => {
            if rdlen != 16 {
                return Err(DigError::DnsProtocol(format!(
                    "AAAA record has unexpected RDLENGTH {rdlen}"
                )));
            }
            let mut segments = [0u16; 8];
            for (i, seg) in segments.iter_mut().enumerate() {
                *seg = read_u16(data, rdata_offset.saturating_add(i.saturating_mul(2)))
                    .unwrap_or(0);
            }
            let addr = std::net::Ipv6Addr::new(
                segments[0], segments[1], segments[2], segments[3],
                segments[4], segments[5], segments[6], segments[7],
            );
            Ok(addr.to_string())
        }

        TYPE_NS | TYPE_CNAME | TYPE_PTR => {
            let (name, _) = decode_domain_name(data, rdata_offset)?;
            Ok(format!("{name}."))
        }

        TYPE_MX => {
            if rdlen < 3 {
                return Err(DigError::DnsProtocol(format!(
                    "MX record has unexpected RDLENGTH {rdlen}"
                )));
            }
            let preference = read_u16(data, rdata_offset).unwrap_or(0);
            let (exchange, _) = decode_domain_name(data, rdata_offset.saturating_add(2))?;
            Ok(format!("{preference} {exchange}."))
        }

        TYPE_SOA => {
            let (mname, mname_len) = decode_domain_name(data, rdata_offset)?;
            let rname_start = rdata_offset.saturating_add(mname_len);
            let (rname, rname_len) = decode_domain_name(data, rname_start)?;
            let numbers_start = rname_start.saturating_add(rname_len);
            let serial = read_u32(data, numbers_start).unwrap_or(0);
            let refresh = read_u32(data, numbers_start.saturating_add(4)).unwrap_or(0);
            let retry = read_u32(data, numbers_start.saturating_add(8)).unwrap_or(0);
            let expire = read_u32(data, numbers_start.saturating_add(12)).unwrap_or(0);
            let minimum = read_u32(data, numbers_start.saturating_add(16)).unwrap_or(0);
            Ok(format!(
                "{mname}. {rname}. {serial} {refresh} {retry} {expire} {minimum}"
            ))
        }

        TYPE_SRV => {
            if rdlen < 7 {
                return Err(DigError::DnsProtocol(format!(
                    "SRV record has unexpected RDLENGTH {rdlen}"
                )));
            }
            let priority = read_u16(data, rdata_offset).unwrap_or(0);
            let weight = read_u16(data, rdata_offset.saturating_add(2)).unwrap_or(0);
            let port = read_u16(data, rdata_offset.saturating_add(4)).unwrap_or(0);
            let (target, _) = decode_domain_name(data, rdata_offset.saturating_add(6))?;
            Ok(format!("{priority} {weight} {port} {target}."))
        }

        TYPE_TXT => {
            let mut result = String::new();
            let end = rdata_offset.saturating_add(rdlen);
            let mut pos = rdata_offset;
            while pos < end {
                let txt_len = *data.get(pos).ok_or_else(|| {
                    DigError::DnsProtocol("truncated TXT record".to_string())
                })? as usize;
                pos = pos.saturating_add(1);
                let chunk_end = pos.saturating_add(txt_len);
                if chunk_end > end {
                    return Err(DigError::DnsProtocol(
                        "TXT chunk extends past RDATA".to_string(),
                    ));
                }
                let chunk = String::from_utf8_lossy(data.get(pos..chunk_end).unwrap_or_default());
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push('"');
                result.push_str(&chunk);
                result.push('"');
                pos = chunk_end;
            }
            Ok(result)
        }

        _ => {
            // Unknown type: hex dump.
            let end = rdata_offset.saturating_add(rdlen);
            let bytes = data.get(rdata_offset..end).unwrap_or_default();
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
            Ok(format!("\\# {rdlen} {}", hex.join(" ")))
        }
    }
}

/// Parse a full DNS response packet.
fn parse_response(data: &[u8]) -> Result<DnsResponse, DigError> {
    if data.len() < DNS_HEADER_LEN {
        return Err(DigError::DnsProtocol(
            "response too short for DNS header".to_string(),
        ));
    }

    let header = DnsHeader {
        id: read_u16(data, 0).unwrap_or(0),
        flags: read_u16(data, 2).unwrap_or(0),
        qdcount: read_u16(data, 4).unwrap_or(0),
        ancount: read_u16(data, 6).unwrap_or(0),
        nscount: read_u16(data, 8).unwrap_or(0),
        arcount: read_u16(data, 10).unwrap_or(0),
    };

    if !header.is_response() {
        return Err(DigError::DnsProtocol(
            "packet is not a DNS response".to_string(),
        ));
    }

    // Parse question section.
    let mut offset = DNS_HEADER_LEN;
    let mut questions = Vec::with_capacity(header.qdcount as usize);
    for _ in 0..header.qdcount {
        let (name, name_len) = decode_domain_name(data, offset)?;
        offset = offset.saturating_add(name_len);
        let qtype = read_u16(data, offset).ok_or_else(|| {
            DigError::DnsProtocol("question section truncated".to_string())
        })?;
        let qclass = read_u16(data, offset.saturating_add(2)).ok_or_else(|| {
            DigError::DnsProtocol("question section truncated".to_string())
        })?;
        offset = offset.saturating_add(4);
        questions.push(DnsQuestion {
            name,
            qtype,
            qclass,
        });
    }

    // Parse resource record sections.
    let section_counts = [header.ancount, header.nscount, header.arcount];
    let mut sections: [Vec<DnsRecord>; 3] = [Vec::new(), Vec::new(), Vec::new()];

    for (sec_idx, &count) in section_counts.iter().enumerate() {
        for _ in 0..count {
            if offset >= data.len() {
                break;
            }

            let (name, name_len) = decode_domain_name(data, offset)?;
            offset = offset.saturating_add(name_len);

            if offset.saturating_add(10) > data.len() {
                return Err(DigError::DnsProtocol(
                    "resource record header extends past packet".to_string(),
                ));
            }

            let rtype = read_u16(data, offset).unwrap_or(0);
            let rclass = read_u16(data, offset.saturating_add(2)).unwrap_or(0);
            let ttl = read_u32(data, offset.saturating_add(4)).unwrap_or(0);
            let rdlen = read_u16(data, offset.saturating_add(8)).unwrap_or(0) as usize;
            offset = offset.saturating_add(10);

            let rdata_end = offset.saturating_add(rdlen);
            if rdata_end > data.len() {
                return Err(DigError::DnsProtocol(
                    "RDATA extends past packet".to_string(),
                ));
            }

            let rdata_raw = data.get(offset..rdata_end).unwrap_or_default().to_vec();
            let rdata = parse_rdata(rtype, data, offset, rdlen)?;
            offset = rdata_end;

            if let Some(section) = sections.get_mut(sec_idx) {
                section.push(DnsRecord {
                    name,
                    rtype,
                    rclass,
                    ttl,
                    rdata_raw,
                    rdata,
                });
            }
        }
    }

    let [answers, authorities, additionals] = sections;

    Ok(DnsResponse {
        header,
        questions,
        answers,
        authorities,
        additionals,
        raw_size: data.len(),
    })
}

// ============================================================================
// Network transport
// ============================================================================

/// Send a DNS query via UDP and return the raw response.
fn query_udp(
    server_ip: u32,
    query_pkt: &[u8],
    timeout_ms: u64,
    retries: u32,
) -> Result<Vec<u8>, DigError> {
    let handle = udp_bind()?;
    let mut last_err = DigError::Timeout;

    for _ in 0..=retries {
        if let Err(e) = udp_send(handle, server_ip, DNS_PORT, query_pkt) {
            last_err = e;
            continue;
        }
        let mut buf = vec![0u8; DNS_MAX_UDP];
        match udp_recv(handle, &mut buf, timeout_ms) {
            Ok(n) => {
                buf.truncate(n);
                return Ok(buf);
            }
            Err(e) => {
                last_err = e;
            }
        }
    }

    Err(last_err)
}

/// Send a DNS query via TCP (RFC 1035 section 4.2.2: 2-byte length prefix).
fn query_tcp(server_ip: u32, query_pkt: &[u8]) -> Result<Vec<u8>, DigError> {
    let handle = tcp_connect(server_ip, DNS_PORT)?;

    // TCP DNS: 2-byte big-endian length prefix.
    let len = query_pkt.len() as u16;
    let mut tcp_pkt = Vec::with_capacity(2 + query_pkt.len());
    tcp_pkt.push((len >> 8) as u8);
    tcp_pkt.push(len as u8);
    tcp_pkt.extend_from_slice(query_pkt);

    if let Err(e) = tcp_send_all(handle, &tcp_pkt) {
        tcp_close(handle);
        return Err(e);
    }

    // Read the 2-byte length prefix of the response.
    let mut len_buf = [0u8; 2];
    let mut len_read = 0;
    while len_read < 2 {
        // len_read < 2 is guaranteed by the loop condition.
        match tcp_recv(handle, len_buf.get_mut(len_read..).unwrap_or(&mut [])) {
            Ok(0) => {
                tcp_close(handle);
                return Err(DigError::Network(
                    "connection closed before response length".to_string(),
                ));
            }
            Ok(n) => len_read += n,
            Err(e) => {
                tcp_close(handle);
                return Err(e);
            }
        }
    }

    let resp_len = u16::from_be_bytes(len_buf) as usize;
    let mut resp_buf = vec![0u8; resp_len];
    let mut total_read = 0;

    while total_read < resp_len {
        // total_read < resp_len is guaranteed by the loop condition.
        match tcp_recv(handle, resp_buf.get_mut(total_read..).unwrap_or(&mut [])) {
            Ok(0) => break,
            Ok(n) => total_read += n,
            Err(e) => {
                tcp_close(handle);
                return Err(e);
            }
        }
    }

    tcp_close(handle);
    resp_buf.truncate(total_read);
    Ok(resp_buf)
}

/// Send a query and get a parsed response, using UDP or TCP as specified.
fn perform_query(
    server_ip: u32,
    name: &str,
    qtype: u16,
    opts: &DigOptions,
) -> Result<(DnsResponse, u64), DigError> {
    let txn_id = make_txn_id();
    let query_pkt = build_query(name, qtype, opts.recursion, txn_id);

    let start = Instant::now();

    let raw_response = if opts.tcp {
        query_tcp(server_ip, &query_pkt)?
    } else {
        let timeout_ms = u64::from(opts.timeout_secs).saturating_mul(1000);
        let resp = query_udp(server_ip, &query_pkt, timeout_ms, opts.retries)?;

        // If response is truncated, retry with TCP.
        if resp.len() >= DNS_HEADER_LEN {
            let flags = read_u16(&resp, 2).unwrap_or(0);
            if flags & FLAG_TC != 0 {
                query_tcp(server_ip, &query_pkt)?
            } else {
                resp
            }
        } else {
            resp
        }
    };

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let response = parse_response(&raw_response)?;

    // Verify transaction ID.
    if response.header.id != txn_id {
        return Err(DigError::DnsProtocol(format!(
            "response ID {:#06x} does not match query ID {:#06x}",
            response.header.id, txn_id
        )));
    }

    Ok((response, elapsed_ms))
}

// ============================================================================
// Output formatting
// ============================================================================

/// Print a dig-style banner.
fn print_banner(args: &DigArgs) {
    let type_str = type_name(args.qtype);
    if args.reverse {
        println!(
            "\n; <<>> DiG 0.1.0 <<>> -x {}",
            args.query_name
        );
    } else {
        println!(
            "\n; <<>> DiG 0.1.0 <<>> {} {}",
            args.query_name, type_str
        );
    }
}

/// Print the HEADER section of a dig response.
fn print_header(resp: &DnsResponse) {
    let h = &resp.header;
    println!(
        ";; ->>HEADER<<- opcode: {}, status: {}, id: {}",
        opcode_str(h.opcode()),
        rcode_str(h.rcode()),
        h.id
    );
    println!(
        ";; flags: {}; QUERY: {}, ANSWER: {}, AUTHORITY: {}, ADDITIONAL: {}",
        h.flags_string(),
        h.qdcount,
        h.ancount,
        h.nscount,
        h.arcount
    );
}

/// Print the QUESTION section.
fn print_question_section(questions: &[DnsQuestion]) {
    if questions.is_empty() {
        return;
    }
    println!("\n;; QUESTION SECTION:");
    for q in questions {
        println!(
            ";{:<23}  {:<7} {}",
            format!("{}.", q.name),
            class_name(q.qclass),
            type_name(q.qtype)
        );
    }
}

/// Print a resource record section (ANSWER, AUTHORITY, or ADDITIONAL).
fn print_rr_section(label: &str, records: &[DnsRecord]) {
    if records.is_empty() {
        return;
    }
    println!("\n;; {} SECTION:", label);
    for rec in records {
        println!(
            "{:<24}{:<8}{:<8}{:<8}{}",
            format!("{}.", rec.name),
            rec.ttl,
            class_name(rec.rclass),
            type_name(rec.rtype),
            rec.rdata
        );
    }
}

/// Print the query statistics footer.
fn print_footer(server: &str, elapsed_ms: u64, msg_size: usize) {
    println!();
    println!(";; Query time: {} msec", elapsed_ms);
    println!(";; SERVER: {}#{}", server, DNS_PORT);
    println!(";; MSG SIZE  rcvd: {}", msg_size);
    println!();
}

/// Print short output (just the RDATA of answer records).
fn print_short(resp: &DnsResponse) {
    for rec in &resp.answers {
        println!("{}", rec.rdata);
    }
}

/// Print full dig-style output.
fn print_full(
    args: &DigArgs,
    resp: &DnsResponse,
    elapsed_ms: u64,
) {
    print_banner(args);
    println!(";; global options: +cmd");
    print_header(resp);
    print_question_section(&resp.questions);
    print_rr_section("ANSWER", &resp.answers);
    print_rr_section("AUTHORITY", &resp.authorities);
    print_rr_section("ADDITIONAL", &resp.additionals);
    print_footer(&args.server, elapsed_ms, resp.raw_size);
}

// ============================================================================
// Trace mode (+trace)
// ============================================================================

/// Root server hints (a subset). We start iterative resolution from these.
const ROOT_SERVERS: &[(&str, &str)] = &[
    ("a.root-servers.net", "198.41.0.4"),
    ("b.root-servers.net", "170.247.170.2"),
    ("c.root-servers.net", "192.33.4.12"),
    ("d.root-servers.net", "199.7.91.13"),
    ("e.root-servers.net", "192.203.230.10"),
];

/// Perform an iterative trace from root servers to the final answer.
fn trace_query(args: &DigArgs) -> Result<(), DigError> {
    let name = &args.query_name;
    let qtype = args.qtype;

    println!("\n; <<>> DiG 0.1.0 <<>> +trace {} {}", name, type_name(qtype));
    println!(";; global options: +cmd");

    // Start with root servers.
    let mut current_servers: Vec<(String, u32)> = ROOT_SERVERS
        .iter()
        .filter_map(|(rname, ip_str)| {
            parse_ipv4(ip_str).map(|ip| (rname.to_string(), ip))
        })
        .collect();

    let mut depth = 0;
    const MAX_DEPTH: usize = 20;

    // Non-recursive query options for trace.
    let trace_opts = DigOptions {
        recursion: false,
        tcp: args.options.tcp,
        timeout_secs: args.options.timeout_secs,
        retries: args.options.retries,
        short: false,
    };

    // Query "." NS at root first.
    if let Some(&(_, root_ip)) = current_servers.first()
        && let Ok((resp, elapsed)) = perform_query(root_ip, ".", TYPE_NS, &trace_opts)
    {
        println!();
        print_rr_section("", &resp.answers);
        let server_str = format_ipv4(root_ip);
        print_footer(&server_str, elapsed, resp.raw_size);
    }

    loop {
        if depth > MAX_DEPTH {
            println!(";; Maximum trace depth reached.");
            break;
        }

        if current_servers.is_empty() {
            println!(";; No more servers to query.");
            break;
        }

        // Pick the first available server.
        let (server_name, server_ip) = current_servers.first().cloned().unwrap_or_default();

        match perform_query(server_ip, name, qtype, &trace_opts) {
            Ok((resp, elapsed)) => {
                let rcode = resp.header.rcode();
                let server_str = format_ipv4(server_ip);

                // Print what we got.
                if !resp.answers.is_empty() {
                    print_rr_section("", &resp.answers);
                    println!(";; Received {} answer(s) from {} ({})", resp.answers.len(), server_name, server_str);
                    print_footer(&server_str, elapsed, resp.raw_size);
                    break; // We have our answer.
                }

                if !resp.authorities.is_empty() {
                    print_rr_section("", &resp.authorities);
                    println!(";; Received referral from {} ({})", server_name, server_str);
                    print_footer(&server_str, elapsed, resp.raw_size);

                    // Gather the next set of nameservers from the authority section.
                    let mut next_servers = Vec::new();

                    for auth_rec in &resp.authorities {
                        if auth_rec.rtype == TYPE_NS {
                            let ns_name = auth_rec.rdata.trim_end_matches('.');
                            // Look in the additional section for an A record for this NS.
                            let mut found_ip = None;
                            for add_rec in &resp.additionals {
                                if add_rec.rtype == TYPE_A
                                    && add_rec.name == ns_name
                                    && let Some(ip) = parse_ipv4(&add_rec.rdata)
                                {
                                    found_ip = Some(ip);
                                    break;
                                }
                            }
                            if let Some(ip) = found_ip {
                                next_servers.push((ns_name.to_string(), ip));
                            }
                            // If no glue record, we skip this NS (could resolve it, but
                            // that adds complexity beyond what trace typically does).
                        }
                    }

                    if next_servers.is_empty() {
                        println!(";; No glue records for referred nameservers; cannot continue trace.");
                        break;
                    }

                    current_servers = next_servers;
                } else if rcode != 0 {
                    println!(";; Server {} returned {}", server_str, rcode_str(rcode));
                    break;
                } else {
                    println!(";; Empty response from {} ({})", server_name, server_str);
                    break;
                }
            }
            Err(e) => {
                println!(";; Query to {} ({}) failed: {}", server_name, format_ipv4(server_ip), e);
                // Try next server.
                if current_servers.len() > 1 {
                    current_servers.remove(0);
                } else {
                    break;
                }
            }
        }

        depth += 1;
    }

    Ok(())
}

// ============================================================================
// Argument parsing
// ============================================================================

struct DigOptions {
    recursion: bool,
    tcp: bool,
    timeout_secs: u32,
    retries: u32,
    short: bool,
}

impl Default for DigOptions {
    fn default() -> Self {
        Self {
            recursion: true,
            tcp: false,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            retries: DEFAULT_RETRIES,
            short: false,
        }
    }
}

struct DigArgs {
    query_name: String,
    server: String,
    qtype: u16,
    reverse: bool,
    trace: bool,
    options: DigOptions,
}

fn print_usage() {
    eprintln!("Usage: dig [@server] [-x addr] [name] [type] [+options]");
    eprintln!();
    eprintln!("Query types: A, AAAA, MX, NS, CNAME, TXT, SOA, PTR, SRV, ANY");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  +short          Short output (answer data only)");
    eprintln!("  +tcp            Use TCP instead of UDP");
    eprintln!("  +norecurse      Do not set the RD (recursion desired) flag");
    eprintln!("  +recurse        Set the RD flag (default)");
    eprintln!("  +trace          Iterative trace from root servers");
    eprintln!("  +timeout=N      Set query timeout to N seconds (default: 5)");
    eprintln!("  +retry=N        Set number of retries (default: 2)");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  dig example.com");
    eprintln!("  dig example.com AAAA");
    eprintln!("  dig @8.8.8.8 example.com MX");
    eprintln!("  dig -x 93.184.216.34");
    eprintln!("  dig example.com +short +tcp");
}

fn parse_args() -> Result<DigArgs, DigError> {
    let argv: Vec<String> = env::args().collect();

    let mut server: Option<String> = None;
    let mut name: Option<String> = None;
    let mut qtype: Option<u16> = None;
    let mut reverse = false;
    let mut trace = false;
    let mut opts = DigOptions::default();
    let mut reverse_ip: Option<String> = None;

    let mut i = 1;
    while i < argv.len() {
        let arg = argv.get(i).cloned().unwrap_or_default();

        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-x" {
            reverse = true;
            i += 1;
            reverse_ip = argv.get(i).cloned();
            if reverse_ip.is_none() {
                return Err(DigError::Usage("-x requires an IP address".to_string()));
            }
        } else if let Some(srv) = arg.strip_prefix('@') {
            server = Some(srv.to_string());
        } else if let Some(option) = arg.strip_prefix('+') {
            // Parse +option.
            if option == "short" {
                opts.short = true;
            } else if option == "tcp" {
                opts.tcp = true;
            } else if option == "norecurse" || option == "norec" {
                opts.recursion = false;
            } else if option == "recurse" || option == "rec" {
                opts.recursion = true;
            } else if option == "trace" {
                trace = true;
            } else if let Some(val) = option.strip_prefix("timeout=") {
                opts.timeout_secs = val.parse().unwrap_or(DEFAULT_TIMEOUT_SECS);
            } else if let Some(val) = option.strip_prefix("retry=") {
                opts.retries = val.parse().unwrap_or(DEFAULT_RETRIES);
            } else {
                // Silently ignore unknown options (dig behavior).
            }
        } else if arg.starts_with('-') {
            // Unknown dash option.
            return Err(DigError::Usage(format!("unknown option: '{arg}'")));
        } else {
            // Positional: could be name or type.
            if parse_record_type(&arg).is_some() && name.is_some() {
                // It is a record type and we already have a name.
                qtype = parse_record_type(&arg);
            } else if name.is_none() {
                name = Some(arg.clone());
            } else if qtype.is_none() {
                // Try as a type; if not, treat as error.
                if let Some(t) = parse_record_type(&arg) {
                    qtype = Some(t);
                } else {
                    return Err(DigError::Usage(format!(
                        "unexpected argument: '{arg}'"
                    )));
                }
            } else {
                return Err(DigError::Usage(format!(
                    "unexpected argument: '{arg}'"
                )));
            }
        }

        i += 1;
    }

    // Handle reverse lookup.
    let query_name;
    if reverse {
        let ip_str = reverse_ip.ok_or_else(|| {
            DigError::Usage("-x requires an IP address".to_string())
        })?;
        query_name = reverse_name_v4(&ip_str)?;
        if qtype.is_none() {
            qtype = Some(TYPE_PTR);
        }
    } else if let Some(n) = name {
        query_name = n;
    } else {
        // No name given: default to querying "." for NS (like real dig).
        query_name = ".".to_string();
        if qtype.is_none() {
            qtype = Some(TYPE_NS);
        }
    }

    let resolved_server = server.unwrap_or_else(default_dns_server);
    let resolved_qtype = qtype.unwrap_or(TYPE_A);

    Ok(DigArgs {
        query_name,
        server: resolved_server,
        qtype: resolved_qtype,
        reverse,
        trace,
        options: opts,
    })
}

// ============================================================================
// Main
// ============================================================================

fn run() -> Result<(), DigError> {
    let args = parse_args()?;

    // Trace mode handles its own output.
    if args.trace {
        return trace_query(&args);
    }

    // Resolve server name to IP.
    let server_ip = parse_ipv4(&args.server).ok_or_else(|| {
        DigError::Usage(format!(
            "cannot parse server address '{}' (hostname resolution for servers not yet supported; use an IP)",
            args.server
        ))
    })?;

    // Perform the query.
    let (response, elapsed_ms) = perform_query(
        server_ip,
        &args.query_name,
        args.qtype,
        &args.options,
    )?;

    // Handle CNAME chains: if we asked for a non-CNAME type and got a CNAME,
    // follow it (up to a reasonable depth).
    let mut final_response = response;
    let mut final_elapsed = elapsed_ms;
    let mut cname_depth = 0;
    const MAX_CNAME_DEPTH: usize = 10;

    if args.qtype != TYPE_CNAME {
        while cname_depth < MAX_CNAME_DEPTH {
            // Check if we only got CNAME records and no records of the requested type.
            let has_target_type = final_response
                .answers
                .iter()
                .any(|r| r.rtype == args.qtype);
            let cname_rec = final_response
                .answers
                .iter()
                .find(|r| r.rtype == TYPE_CNAME);

            if has_target_type || cname_rec.is_none() {
                break;
            }

            // Follow the CNAME.
            let cname_target = cname_rec
                .map(|r| r.rdata.trim_end_matches('.').to_string())
                .unwrap_or_default();

            if cname_target.is_empty() {
                break;
            }

            match perform_query(server_ip, &cname_target, args.qtype, &args.options) {
                Ok((resp, ms)) => {
                    // Merge: keep original CNAME answers, add new answers.
                    let mut merged_answers = final_response.answers.clone();
                    merged_answers.extend(resp.answers.iter().cloned());
                    final_response = DnsResponse {
                        header: resp.header,
                        questions: final_response.questions,
                        answers: merged_answers,
                        authorities: resp.authorities,
                        additionals: resp.additionals,
                        raw_size: resp.raw_size,
                    };
                    final_elapsed = final_elapsed.saturating_add(ms);
                }
                Err(_) => break,
            }

            cname_depth += 1;
        }
    }

    // Output.
    if args.options.short {
        print_short(&final_response);
    } else {
        print_full(&args, &final_response, final_elapsed);
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!(";; dig: {e}");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Domain name encoding / decoding ---

    #[test]
    fn encode_simple_domain() {
        let mut buf = Vec::new();
        encode_domain_name("www.example.com", &mut buf);
        assert_eq!(
            buf,
            &[
                3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c',
                b'o', b'm', 0
            ]
        );
    }

    #[test]
    fn encode_single_label() {
        let mut buf = Vec::new();
        encode_domain_name("localhost", &mut buf);
        assert_eq!(
            buf,
            &[9, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0]
        );
    }

    #[test]
    fn encode_trailing_dot() {
        let mut buf = Vec::new();
        encode_domain_name("example.com.", &mut buf);
        // Trailing dot should be stripped; result same as without.
        let mut expected = Vec::new();
        encode_domain_name("example.com", &mut expected);
        assert_eq!(buf, expected);
    }

    #[test]
    fn encode_root() {
        let mut buf = Vec::new();
        encode_domain_name(".", &mut buf);
        assert_eq!(buf, &[0]);
    }

    #[test]
    fn encode_empty() {
        let mut buf = Vec::new();
        encode_domain_name("", &mut buf);
        assert_eq!(buf, &[0]);
    }

    #[test]
    fn decode_simple_domain() {
        let data = [
            3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o',
            b'm', 0,
        ];
        let (name, consumed) = decode_domain_name(&data, 0).unwrap();
        assert_eq!(name, "www.example.com");
        assert_eq!(consumed, 17);
    }

    #[test]
    fn decode_compressed_pointer() {
        let mut data = vec![
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
            // At offset 13: pointer to offset 0.
            0xC0, 0x00,
        ];
        data.push(0); // padding

        let (name, consumed) = decode_domain_name(&data, 13).unwrap();
        assert_eq!(name, "example.com");
        assert_eq!(consumed, 2);
    }

    #[test]
    fn decode_partial_compression() {
        let mut data = vec![
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
        ];
        let www_offset = data.len();
        data.extend_from_slice(&[3, b'w', b'w', b'w', 0xC0, 0x00]);

        let (name, consumed) = decode_domain_name(&data, www_offset).unwrap();
        assert_eq!(name, "www.example.com");
        assert_eq!(consumed, 6);
    }

    #[test]
    fn decode_empty_name() {
        let data = [0u8];
        let (name, consumed) = decode_domain_name(&data, 0).unwrap();
        assert!(name.is_empty());
        assert_eq!(consumed, 1);
    }

    #[test]
    fn decode_truncated_errors() {
        let data = [3, b'a', b'b']; // label says 3 bytes but only 2 follow
        assert!(decode_domain_name(&data, 0).is_err());
    }

    #[test]
    fn decode_truncated_pointer() {
        let data = [0xC0]; // pointer but no second byte
        assert!(decode_domain_name(&data, 0).is_err());
    }

    // --- Reverse lookup ---

    #[test]
    fn reverse_ipv4_basic() {
        let result = reverse_name_v4("192.168.1.1").unwrap();
        assert_eq!(result, "1.1.168.192.in-addr.arpa");
    }

    #[test]
    fn reverse_ipv4_loopback() {
        let result = reverse_name_v4("127.0.0.1").unwrap();
        assert_eq!(result, "1.0.0.127.in-addr.arpa");
    }

    #[test]
    fn reverse_invalid() {
        assert!(reverse_name_v4("not-an-ip").is_err());
        assert!(reverse_name_v4("256.0.0.1").is_err());
        assert!(reverse_name_v4("1.2.3").is_err());
    }

    // --- IPv4 parsing ---

    #[test]
    fn parse_ipv4_basic() {
        let ip = parse_ipv4("8.8.8.8").unwrap();
        assert_eq!(ip, u32::from_be_bytes([8, 8, 8, 8]));
    }

    #[test]
    fn parse_ipv4_invalid() {
        assert!(parse_ipv4("not.an.ip.addr").is_none());
        assert!(parse_ipv4("256.0.0.1").is_none());
        assert!(parse_ipv4("1.2.3").is_none());
        assert!(parse_ipv4("").is_none());
    }

    #[test]
    fn format_ipv4_roundtrip() {
        let ip = parse_ipv4("10.20.30.40").unwrap();
        assert_eq!(format_ipv4(ip), "10.20.30.40");
    }

    // --- Query packet structure ---

    #[test]
    fn query_packet_has_correct_header() {
        let pkt = build_query("test.com", TYPE_A, true, 0x1234);
        assert!(pkt.len() >= DNS_HEADER_LEN);

        // Transaction ID.
        assert_eq!(pkt[0], 0x12);
        assert_eq!(pkt[1], 0x34);

        // Flags: RD=1, everything else 0.
        let flags = u16::from(pkt[2]) << 8 | u16::from(pkt[3]);
        assert_eq!(flags, FLAG_RD);

        // QDCOUNT = 1.
        assert_eq!(pkt[4], 0);
        assert_eq!(pkt[5], 1);

        // ANCOUNT, NSCOUNT, ARCOUNT = 0.
        assert_eq!(pkt[6], 0);
        assert_eq!(pkt[7], 0);
        assert_eq!(pkt[8], 0);
        assert_eq!(pkt[9], 0);
        assert_eq!(pkt[10], 0);
        assert_eq!(pkt[11], 0);
    }

    #[test]
    fn query_packet_no_recursion() {
        let pkt = build_query("test.com", TYPE_A, false, 0xABCD);
        let flags = u16::from(pkt[2]) << 8 | u16::from(pkt[3]);
        assert_eq!(flags & FLAG_RD, 0);
    }

    #[test]
    fn query_packet_question_section() {
        let pkt = build_query("test.com", TYPE_AAAA, true, 0x5678);
        // QTYPE and QCLASS are the last 4 bytes.
        let len = pkt.len();
        let qtype_val = u16::from(pkt[len - 4]) << 8 | u16::from(pkt[len - 3]);
        let qclass_val = u16::from(pkt[len - 2]) << 8 | u16::from(pkt[len - 1]);
        assert_eq!(qtype_val, TYPE_AAAA);
        assert_eq!(qclass_val, CLASS_IN);
    }

    // --- Record type helpers ---

    #[test]
    fn parse_known_types() {
        assert_eq!(parse_record_type("A"), Some(TYPE_A));
        assert_eq!(parse_record_type("aaaa"), Some(TYPE_AAAA));
        assert_eq!(parse_record_type("mx"), Some(TYPE_MX));
        assert_eq!(parse_record_type("txt"), Some(TYPE_TXT));
        assert_eq!(parse_record_type("ns"), Some(TYPE_NS));
        assert_eq!(parse_record_type("Cname"), Some(TYPE_CNAME));
        assert_eq!(parse_record_type("PTR"), Some(TYPE_PTR));
        assert_eq!(parse_record_type("soa"), Some(TYPE_SOA));
        assert_eq!(parse_record_type("SRV"), Some(TYPE_SRV));
        assert_eq!(parse_record_type("ANY"), Some(TYPE_ANY));
        assert_eq!(parse_record_type("BOGUS"), None);
    }

    #[test]
    fn type_name_round_trip() {
        for &code in &[TYPE_A, TYPE_NS, TYPE_CNAME, TYPE_SOA, TYPE_PTR, TYPE_MX, TYPE_TXT, TYPE_AAAA, TYPE_SRV] {
            let name = type_name(code);
            assert_eq!(parse_record_type(name), Some(code));
        }
    }

    #[test]
    fn type_name_unknown() {
        let name = type_name(9999);
        assert!(name.starts_with("TYPE"));
    }

    #[test]
    fn rcode_strings() {
        assert_eq!(rcode_str(0), "NOERROR");
        assert_eq!(rcode_str(3), "NXDOMAIN");
        assert_eq!(rcode_str(5), "REFUSED");
        assert_eq!(rcode_str(99), "UNKNOWN");
    }

    #[test]
    fn opcode_strings() {
        assert_eq!(opcode_str(0), "QUERY");
        assert_eq!(opcode_str(2), "STATUS");
        assert_eq!(opcode_str(99), "UNKNOWN");
    }

    #[test]
    fn class_names() {
        assert_eq!(class_name(1), "IN");
        assert_eq!(class_name(3), "CH");
        assert_eq!(class_name(255), "ANY");
        assert_eq!(class_name(999), "UNKNOWN");
    }

    // --- RDATA parsing ---

    #[test]
    fn parse_rdata_a_record() {
        let data = [10, 0, 0, 1];
        let result = parse_rdata(TYPE_A, &data, 0, 4).unwrap();
        assert_eq!(result, "10.0.0.1");
    }

    #[test]
    fn parse_rdata_a_wrong_length() {
        let data = [10, 0, 0];
        assert!(parse_rdata(TYPE_A, &data, 0, 3).is_err());
    }

    #[test]
    fn parse_rdata_aaaa_record() {
        let mut data = vec![0u8; 16];
        data[0] = 0x20;
        data[1] = 0x01;
        data[2] = 0x0d;
        data[3] = 0xb8;
        data[15] = 0x01;
        let result = parse_rdata(TYPE_AAAA, &data, 0, 16).unwrap();
        assert_eq!(result, "2001:db8::1");
    }

    #[test]
    fn parse_rdata_aaaa_wrong_length() {
        let data = [0u8; 8];
        assert!(parse_rdata(TYPE_AAAA, &data, 0, 8).is_err());
    }

    #[test]
    fn parse_rdata_cname() {
        let data = [
            4, b'm', b'a', b'i', b'l', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c',
            b'o', b'm', 0,
        ];
        let result = parse_rdata(TYPE_CNAME, &data, 0, data.len()).unwrap();
        assert_eq!(result, "mail.example.com.");
    }

    #[test]
    fn parse_rdata_ns() {
        let data = [
            3, b'n', b's', b'1', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o',
            b'm', 0,
        ];
        let result = parse_rdata(TYPE_NS, &data, 0, data.len()).unwrap();
        assert_eq!(result, "ns1.example.com.");
    }

    #[test]
    fn parse_rdata_ptr() {
        let data = [
            3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o',
            b'm', 0,
        ];
        let result = parse_rdata(TYPE_PTR, &data, 0, data.len()).unwrap();
        assert_eq!(result, "www.example.com.");
    }

    #[test]
    fn parse_rdata_mx_record() {
        let mut data = vec![0u8, 10]; // preference = 10
        let mut name_buf = Vec::new();
        encode_domain_name("mx.test.com", &mut name_buf);
        data.extend_from_slice(&name_buf);

        let result = parse_rdata(TYPE_MX, &data, 0, data.len()).unwrap();
        assert_eq!(result, "10 mx.test.com.");
    }

    #[test]
    fn parse_rdata_mx_wrong_length() {
        let data = [0u8, 10]; // too short - no exchange name
        assert!(parse_rdata(TYPE_MX, &data, 0, 2).is_err());
    }

    #[test]
    fn parse_rdata_txt_single() {
        let data = [5, b'h', b'e', b'l', b'l', b'o'];
        let result = parse_rdata(TYPE_TXT, &data, 0, 6).unwrap();
        assert_eq!(result, "\"hello\"");
    }

    #[test]
    fn parse_rdata_txt_multiple_chunks() {
        let data = [2, b'h', b'i', 3, b'b', b'y', b'e'];
        let result = parse_rdata(TYPE_TXT, &data, 0, 7).unwrap();
        assert_eq!(result, "\"hi\" \"bye\"");
    }

    #[test]
    fn parse_rdata_soa() {
        let mut data = Vec::new();
        encode_domain_name("ns1.example.com", &mut data);
        encode_domain_name("admin.example.com", &mut data);
        // serial, refresh, retry, expire, minimum
        data.extend_from_slice(&2024010100u32.to_be_bytes());
        data.extend_from_slice(&3600u32.to_be_bytes());
        data.extend_from_slice(&900u32.to_be_bytes());
        data.extend_from_slice(&604800u32.to_be_bytes());
        data.extend_from_slice(&86400u32.to_be_bytes());

        let result = parse_rdata(TYPE_SOA, &data, 0, data.len()).unwrap();
        assert!(result.contains("ns1.example.com."));
        assert!(result.contains("admin.example.com."));
        assert!(result.contains("2024010100"));
        assert!(result.contains("3600"));
        assert!(result.contains("900"));
        assert!(result.contains("604800"));
        assert!(result.contains("86400"));
    }

    #[test]
    fn parse_rdata_srv() {
        let mut data = Vec::new();
        data.extend_from_slice(&10u16.to_be_bytes()); // priority
        data.extend_from_slice(&20u16.to_be_bytes()); // weight
        data.extend_from_slice(&443u16.to_be_bytes()); // port
        encode_domain_name("sip.example.com", &mut data);

        let result = parse_rdata(TYPE_SRV, &data, 0, data.len()).unwrap();
        assert_eq!(result, "10 20 443 sip.example.com.");
    }

    #[test]
    fn parse_rdata_srv_wrong_length() {
        let data = [0u8; 5]; // too short
        assert!(parse_rdata(TYPE_SRV, &data, 0, 5).is_err());
    }

    #[test]
    fn parse_rdata_unknown_type() {
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        let result = parse_rdata(999, &data, 0, 4).unwrap();
        assert!(result.contains("de ad be ef"));
    }

    // --- Response parsing ---

    /// Build a minimal valid DNS response with one A record.
    fn make_a_response(name: &str, ip: [u8; 4], txn_id: u16) -> Vec<u8> {
        let mut pkt = Vec::new();
        pkt.push((txn_id >> 8) as u8);
        pkt.push(txn_id as u8);

        // Flags: QR=1, RD=1, RA=1, RCODE=0.
        let flags: u16 = FLAG_QR | FLAG_RD | FLAG_RA;
        pkt.push((flags >> 8) as u8);
        pkt.push(flags as u8);

        // QDCOUNT=1, ANCOUNT=1, NSCOUNT=0, ARCOUNT=0.
        pkt.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 0]);

        // Question section.
        let name_offset = pkt.len();
        encode_domain_name(name, &mut pkt);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);

        // Answer section: pointer to name, TYPE A, CLASS IN, TTL 300, RDLEN 4.
        pkt.extend_from_slice(&[0xC0, name_offset as u8]);
        pkt.extend_from_slice(&[0, TYPE_A as u8]);
        pkt.extend_from_slice(&[0, CLASS_IN as u8]);
        pkt.extend_from_slice(&[0, 0, 1, 44]); // TTL = 300
        pkt.extend_from_slice(&[0, 4]);
        pkt.extend_from_slice(&ip);

        pkt
    }

    #[test]
    fn parse_a_response() {
        let pkt = make_a_response("example.com", [93, 184, 216, 34], 0x1234);
        let resp = parse_response(&pkt).unwrap();

        assert!(resp.header.is_response());
        assert_eq!(resp.header.rcode(), 0);
        assert_eq!(resp.header.id, 0x1234);
        assert_eq!(resp.answers.len(), 1);
        assert_eq!(resp.answers[0].rtype, TYPE_A);
        assert_eq!(resp.answers[0].rdata, "93.184.216.34");
        assert_eq!(resp.answers[0].ttl, 300);
    }

    #[test]
    fn parse_response_flags() {
        let pkt = make_a_response("test.com", [1, 2, 3, 4], 0xAAAA);
        let resp = parse_response(&pkt).unwrap();

        assert!(resp.header.is_response());
        assert!(resp.header.recursion_desired());
        assert!(resp.header.recursion_available());
        assert!(!resp.header.is_authoritative());
        assert!(!resp.header.is_truncated());
        assert_eq!(resp.header.opcode(), 0);
        assert!(resp.header.flags_string().contains("qr"));
        assert!(resp.header.flags_string().contains("rd"));
        assert!(resp.header.flags_string().contains("ra"));
    }

    #[test]
    fn parse_response_questions() {
        let pkt = make_a_response("test.com", [1, 2, 3, 4], 0xBBBB);
        let resp = parse_response(&pkt).unwrap();

        assert_eq!(resp.questions.len(), 1);
        assert_eq!(resp.questions[0].name, "test.com");
        assert_eq!(resp.questions[0].qtype, TYPE_A);
        assert_eq!(resp.questions[0].qclass, CLASS_IN);
    }

    #[test]
    fn parse_nxdomain() {
        let mut pkt = make_a_response("nope.invalid", [0, 0, 0, 0], 0xCCCC);
        // Set RCODE to 3 (NXDOMAIN).
        pkt[3] = (pkt[3] & 0xF0) | 3;
        // Set ANCOUNT=0.
        pkt[7] = 0;

        let resp = parse_response(&pkt).unwrap();
        assert_eq!(resp.header.rcode(), 3);
        assert_eq!(rcode_str(resp.header.rcode()), "NXDOMAIN");
    }

    #[test]
    fn parse_response_too_short() {
        let data = [0u8; 5];
        assert!(parse_response(&data).is_err());
    }

    #[test]
    fn parse_response_not_a_response() {
        // Build a packet with QR=0 (query, not response).
        let mut pkt = make_a_response("test.com", [1, 2, 3, 4], 0x1111);
        pkt[2] &= 0x7F; // Clear QR bit.
        assert!(parse_response(&pkt).is_err());
    }

    #[test]
    fn parse_multi_answer_response() {
        let name = "multi.test";
        let mut pkt = Vec::new();
        let txn_id: u16 = 0xDDDD;
        pkt.push((txn_id >> 8) as u8);
        pkt.push(txn_id as u8);

        let flags: u16 = FLAG_QR | FLAG_RD | FLAG_RA;
        pkt.push((flags >> 8) as u8);
        pkt.push(flags as u8);

        // QDCOUNT=1, ANCOUNT=2.
        pkt.extend_from_slice(&[0, 1, 0, 2, 0, 0, 0, 0]);

        // Question section.
        let q_offset = pkt.len();
        encode_domain_name(name, &mut pkt);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);

        // Answer 1.
        pkt.extend_from_slice(&[0xC0, q_offset as u8]);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);
        pkt.extend_from_slice(&[0, 0, 0, 60]); // TTL=60
        pkt.extend_from_slice(&[0, 4, 1, 2, 3, 4]);

        // Answer 2.
        pkt.extend_from_slice(&[0xC0, q_offset as u8]);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);
        pkt.extend_from_slice(&[0, 0, 0, 60]);
        pkt.extend_from_slice(&[0, 4, 5, 6, 7, 8]);

        let resp = parse_response(&pkt).unwrap();
        assert_eq!(resp.answers.len(), 2);
        assert_eq!(resp.answers[0].rdata, "1.2.3.4");
        assert_eq!(resp.answers[1].rdata, "5.6.7.8");
    }

    #[test]
    fn parse_response_with_authority_and_additional() {
        let name = "example.com";
        let mut pkt = Vec::new();
        let txn_id: u16 = 0xEEEE;
        pkt.push((txn_id >> 8) as u8);
        pkt.push(txn_id as u8);

        let flags: u16 = FLAG_QR | FLAG_RD | FLAG_RA;
        pkt.push((flags >> 8) as u8);
        pkt.push(flags as u8);

        // QDCOUNT=1, ANCOUNT=1, NSCOUNT=1, ARCOUNT=1.
        pkt.extend_from_slice(&[0, 1, 0, 1, 0, 1, 0, 1]);

        // Question.
        let q_offset = pkt.len();
        encode_domain_name(name, &mut pkt);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);

        // Answer: A record.
        pkt.extend_from_slice(&[0xC0, q_offset as u8]);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);
        pkt.extend_from_slice(&[0, 0, 1, 44]); // TTL=300
        pkt.extend_from_slice(&[0, 4, 93, 184, 216, 34]);

        // Authority: NS record.
        pkt.extend_from_slice(&[0xC0, q_offset as u8]); // compressed name pointer
        pkt.extend_from_slice(&[0, TYPE_NS as u8, 0, CLASS_IN as u8]);
        pkt.extend_from_slice(&[0, 0, 14, 16]); // TTL=3600 (full 4 bytes)
        // NS rdata: wire-format "ns1.example.com", preceded by a 2-byte RDLENGTH.
        let mut ns_name_buf = Vec::new();
        encode_domain_name("ns1.example.com", &mut ns_name_buf);
        pkt.extend_from_slice(&[0, ns_name_buf.len() as u8]); // RDLENGTH
        pkt.extend_from_slice(&ns_name_buf);

        // Additional: A record for ns1.example.com.
        encode_domain_name("ns1.example.com", &mut pkt);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);
        pkt.extend_from_slice(&[0, 0, 14, 16]); // TTL=3600
        pkt.extend_from_slice(&[0, 4, 198, 51, 100, 1]);

        let resp = parse_response(&pkt).unwrap();
        assert_eq!(resp.answers.len(), 1);
        assert_eq!(resp.authorities.len(), 1);
        assert_eq!(resp.additionals.len(), 1);
        assert_eq!(resp.authorities[0].rtype, TYPE_NS);
        assert_eq!(resp.additionals[0].rtype, TYPE_A);
    }

    // --- Header flag analysis ---

    #[test]
    fn header_authoritative() {
        let mut pkt = make_a_response("auth.test", [1, 1, 1, 1], 0x1000);
        // Set AA flag.
        pkt[2] |= (FLAG_AA >> 8) as u8;
        let resp = parse_response(&pkt).unwrap();
        assert!(resp.header.is_authoritative());
        assert!(resp.header.flags_string().contains("aa"));
    }

    #[test]
    fn header_truncated() {
        let mut pkt = make_a_response("trunc.test", [1, 1, 1, 1], 0x2000);
        // Set TC flag.
        pkt[2] |= (FLAG_TC >> 8) as u8;
        let resp = parse_response(&pkt).unwrap();
        assert!(resp.header.is_truncated());
        assert!(resp.header.flags_string().contains("tc"));
    }

    // --- Read helpers ---

    #[test]
    fn read_u16_basic() {
        let data = [0x12, 0x34, 0x56];
        assert_eq!(read_u16(&data, 0), Some(0x1234));
        assert_eq!(read_u16(&data, 1), Some(0x3456));
        assert_eq!(read_u16(&data, 2), None); // out of bounds
    }

    #[test]
    fn read_u32_basic() {
        let data = [0x12, 0x34, 0x56, 0x78, 0x9A];
        assert_eq!(read_u32(&data, 0), Some(0x12345678));
        assert_eq!(read_u32(&data, 1), Some(0x3456789A));
        assert_eq!(read_u32(&data, 2), None); // out of bounds
    }

    #[test]
    fn read_u16_empty() {
        let data: [u8; 0] = [];
        assert_eq!(read_u16(&data, 0), None);
    }

    #[test]
    fn read_u32_empty() {
        let data: [u8; 0] = [];
        assert_eq!(read_u32(&data, 0), None);
    }

    // --- Default server discovery ---

    #[test]
    fn default_server_returns_something() {
        let server = default_dns_server();
        assert!(!server.is_empty());
    }

    // --- Error display ---

    #[test]
    fn error_display() {
        assert!(format!("{}", DigError::Timeout).contains("timed out"));
        assert!(format!("{}", DigError::Network("test".into())).contains("test"));
        assert!(format!("{}", DigError::DnsProtocol("bad".into())).contains("bad"));
        assert!(format!("{}", DigError::Usage("oops".into())).contains("oops"));
        assert!(format!("{}", DigError::Io("fail".into())).contains("fail"));
    }

    // --- Build query with various types ---

    #[test]
    fn build_query_for_all_types() {
        for &qtype in &[TYPE_A, TYPE_AAAA, TYPE_MX, TYPE_NS, TYPE_CNAME, TYPE_TXT, TYPE_SOA, TYPE_PTR, TYPE_SRV] {
            let pkt = build_query("test.example.com", qtype, true, 0x9999);
            let len = pkt.len();
            let encoded_type = u16::from(pkt[len - 4]) << 8 | u16::from(pkt[len - 3]);
            assert_eq!(encoded_type, qtype, "failed for type {}", type_name(qtype));
        }
    }

    // --- Full round-trip: build query, parse response ---

    #[test]
    fn full_roundtrip_a_record() {
        let name = "roundtrip.test";
        let txn_id = 0x4242;
        let query_pkt = build_query(name, TYPE_A, true, txn_id);

        // Verify query is well-formed.
        assert!(query_pkt.len() > DNS_HEADER_LEN);
        let q_flags = u16::from(query_pkt[2]) << 8 | u16::from(query_pkt[3]);
        assert_eq!(q_flags & FLAG_QR, 0); // It's a query, not a response.

        // Build a response to match.
        let resp_pkt = make_a_response(name, [10, 20, 30, 40], txn_id);
        let resp = parse_response(&resp_pkt).unwrap();

        assert_eq!(resp.header.id, txn_id);
        assert_eq!(resp.answers.len(), 1);
        assert_eq!(resp.answers[0].rdata, "10.20.30.40");
    }

    // --- Raw_size tracking ---

    #[test]
    fn response_raw_size() {
        let pkt = make_a_response("size.test", [1, 2, 3, 4], 0xFFFF);
        let resp = parse_response(&pkt).unwrap();
        assert_eq!(resp.raw_size, pkt.len());
    }
}
