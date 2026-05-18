//! OurOS DNS Lookup Utility
//!
//! Queries DNS servers for domain name records using raw UDP packets
//! constructed per RFC 1035. Supports common record types (A, AAAA,
//! MX, TXT, NS, CNAME, PTR) and reverse lookups.
//!
//! # Usage
//!
//! ```text
//! nslookup <hostname>                   A record lookup (default)
//! nslookup -type=AAAA <hostname>        AAAA (IPv6) record lookup
//! nslookup -type=MX <hostname>          MX record lookup
//! nslookup -type=TXT <hostname>         TXT record lookup
//! nslookup -type=NS <hostname>          NS record lookup
//! nslookup -type=CNAME <hostname>       CNAME record lookup
//! nslookup <hostname> <server>          Use specific DNS server
//! nslookup -reverse <ip>                Reverse DNS lookup (PTR)
//! ```

use std::env;
use std::fs;
use std::net::UdpSocket;
use std::process;
use std::time::Duration;

// ============================================================================
// DNS constants (RFC 1035)
// ============================================================================

/// DNS header is always 12 bytes.
const DNS_HEADER_LEN: usize = 12;

/// Maximum size for a DNS UDP response.
const DNS_MAX_UDP: usize = 512;

/// Default DNS server port.
const DNS_PORT: u16 = 53;

/// Default query timeout in seconds.
const TIMEOUT_SECS: u64 = 2;

/// Default DNS server when /etc/resolv.conf is unavailable.
const FALLBACK_SERVER: &str = "8.8.8.8";

// DNS record type codes (RFC 1035 and RFC 3596).
const TYPE_A: u16 = 1;
const TYPE_NS: u16 = 2;
const TYPE_CNAME: u16 = 5;
const TYPE_PTR: u16 = 12;
const TYPE_MX: u16 = 15;
const TYPE_TXT: u16 = 16;
const TYPE_AAAA: u16 = 28;

// DNS class code.
const CLASS_IN: u16 = 1;

// DNS header flag bits.
const FLAG_QR: u16 = 0x8000; // Response flag
const FLAG_RD: u16 = 0x0100; // Recursion Desired
const RCODE_MASK: u16 = 0x000F;

// ============================================================================
// Record type parsing
// ============================================================================

/// Maps a user-facing record type string to its numeric code.
fn parse_record_type(s: &str) -> Option<u16> {
    match s.to_ascii_uppercase().as_str() {
        "A" => Some(TYPE_A),
        "AAAA" => Some(TYPE_AAAA),
        "MX" => Some(TYPE_MX),
        "TXT" => Some(TYPE_TXT),
        "NS" => Some(TYPE_NS),
        "CNAME" => Some(TYPE_CNAME),
        "PTR" => Some(TYPE_PTR),
        _ => None,
    }
}

/// Returns the human-readable name for a record type code.
fn type_name(code: u16) -> &'static str {
    match code {
        TYPE_A => "A",
        TYPE_NS => "NS",
        TYPE_CNAME => "CNAME",
        TYPE_PTR => "PTR",
        TYPE_MX => "MX",
        TYPE_TXT => "TXT",
        TYPE_AAAA => "AAAA",
        _ => "UNKNOWN",
    }
}

/// Returns a human-readable RCODE description.
fn rcode_message(rcode: u16) -> &'static str {
    match rcode {
        0 => "No error",
        1 => "Format error",
        2 => "Server failure",
        3 => "Name does not exist (NXDOMAIN)",
        4 => "Not implemented",
        5 => "Query refused",
        _ => "Unknown error",
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
            if trimmed.starts_with("nameserver") {
                if let Some(addr) = trimmed.split_whitespace().nth(1) {
                    return addr.to_string();
                }
            }
        }
    }
    FALLBACK_SERVER.to_string()
}

// ============================================================================
// Reverse-lookup helper
// ============================================================================

/// Converts an IP address string into its PTR query name.
///
/// - IPv4 `1.2.3.4` becomes `4.3.2.1.in-addr.arpa`
/// - IPv6 `2001:db8::1` is expanded and reversed into `*.ip6.arpa`
fn reverse_name(ip: &str) -> Result<String, String> {
    // Try IPv4 first.
    if let Ok(v4) = ip.parse::<std::net::Ipv4Addr>() {
        let octets = v4.octets();
        return Ok(format!(
            "{}.{}.{}.{}.in-addr.arpa",
            octets[3], octets[2], octets[1], octets[0]
        ));
    }

    // Try IPv6.
    if let Ok(v6) = ip.parse::<std::net::Ipv6Addr>() {
        let segments = v6.segments();
        let mut nibbles = Vec::with_capacity(32);
        for seg in &segments {
            nibbles.push((seg >> 12) & 0xF);
            nibbles.push((seg >> 8) & 0xF);
            nibbles.push((seg >> 4) & 0xF);
            nibbles.push(seg & 0xF);
        }
        nibbles.reverse();
        let parts: Vec<String> = nibbles.iter().map(|n| format!("{n:x}")).collect();
        let mut name = parts.join(".");
        name.push_str(".ip6.arpa");
        return Ok(name);
    }

    Err(format!("'{ip}' is not a valid IPv4 or IPv6 address"))
}

// ============================================================================
// DNS packet construction
// ============================================================================

/// Builds a DNS query packet for the given domain name and record type.
///
/// Packet layout (RFC 1035 section 4):
///   - 12-byte header (ID, flags, question count)
///   - Question section: encoded QNAME + QTYPE + QCLASS
fn build_query(name: &str, qtype: u16) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // --- Header (12 bytes) ---
    // Transaction ID: derive from name hash for easy matching.
    let id = simple_hash(name);
    pkt.push((id >> 8) as u8);
    pkt.push(id as u8);

    // Flags: standard query, recursion desired.
    let flags: u16 = FLAG_RD;
    pkt.push((flags >> 8) as u8);
    pkt.push(flags as u8);

    // QDCOUNT = 1.
    pkt.push(0);
    pkt.push(1);
    // ANCOUNT, NSCOUNT, ARCOUNT = 0.
    pkt.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

    // --- Question section ---
    encode_domain_name(name, &mut pkt);

    // QTYPE.
    pkt.push((qtype >> 8) as u8);
    pkt.push(qtype as u8);

    // QCLASS = IN.
    pkt.push((CLASS_IN >> 8) as u8);
    pkt.push(CLASS_IN as u8);

    pkt
}

/// Encodes a domain name into DNS wire format (label-length-prefixed).
///
/// `www.example.com` becomes `[3]www[7]example[3]com[0]`.
fn encode_domain_name(name: &str, buf: &mut Vec<u8>) {
    for label in name.split('.') {
        let len = label.len();
        // RFC 1035: label length must fit in 6 bits (max 63).
        let clamped = if len > 63 { 63 } else { len };
        buf.push(clamped as u8);
        buf.extend_from_slice(&label.as_bytes()[..clamped]);
    }
    // Terminating zero-length label.
    buf.push(0);
}

/// Simple non-cryptographic hash for generating transaction IDs.
fn simple_hash(s: &str) -> u16 {
    let mut h: u32 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(u32::from(b));
    }
    (h & 0xFFFF) as u16
}

// ============================================================================
// DNS packet parsing
// ============================================================================

/// Represents a single parsed DNS resource record.
struct DnsRecord {
    name: String,
    rtype: u16,
    #[allow(dead_code)] // Class is always IN for our queries but kept for completeness.
    rclass: u16,
    ttl: u32,
    rdata: String,
}

/// Parsed DNS response.
struct DnsResponse {
    id: u16,
    flags: u16,
    answers: Vec<DnsRecord>,
    authorities: Vec<DnsRecord>,
    additionals: Vec<DnsRecord>,
}

/// Reads a big-endian u16 from `data` at `offset`.
/// Returns `None` if out of bounds.
fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    let hi = *data.get(offset)? as u16;
    let lo = *data.get(offset + 1)? as u16;
    Some((hi << 8) | lo)
}

/// Reads a big-endian u32 from `data` at `offset`.
fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let a = *data.get(offset)? as u32;
    let b = *data.get(offset + 1)? as u32;
    let c = *data.get(offset + 2)? as u32;
    let d = *data.get(offset + 3)? as u32;
    Some((a << 24) | (b << 16) | (c << 8) | d)
}

/// Decodes a DNS domain name from wire format, following compression pointers.
///
/// Returns the decoded name and the number of bytes consumed from `start`
/// (not counting bytes reached via pointer indirection).
fn decode_domain_name(data: &[u8], start: usize) -> Result<(String, usize), String> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = start;
    let mut jumped = false;
    let mut consumed = 0;
    // Guard against infinite pointer loops.
    let mut hops = 0;
    const MAX_HOPS: usize = 128;

    loop {
        if hops > MAX_HOPS {
            return Err("too many compression pointer hops".to_string());
        }
        let byte = *data
            .get(pos)
            .ok_or_else(|| "unexpected end of packet in domain name".to_string())?;

        if byte == 0 {
            // End of name.
            if !jumped {
                consumed = pos - start + 1;
            }
            break;
        }

        // Compression pointer: top two bits are 11.
        if byte & 0xC0 == 0xC0 {
            let ptr_hi = u16::from(byte & 0x3F);
            let ptr_lo = u16::from(
                *data
                    .get(pos + 1)
                    .ok_or_else(|| "truncated compression pointer".to_string())?,
            );
            let ptr = ((ptr_hi << 8) | ptr_lo) as usize;

            if !jumped {
                consumed = pos - start + 2;
            }
            pos = ptr;
            jumped = true;
            hops += 1;
            continue;
        }

        // Regular label.
        let len = byte as usize;
        if pos + 1 + len > data.len() {
            return Err("label extends past end of packet".to_string());
        }
        let label = String::from_utf8_lossy(&data[pos + 1..pos + 1 + len]).to_string();
        labels.push(label);
        pos += 1 + len;
        hops += 1;
    }

    if consumed == 0 && !jumped {
        consumed = 1; // Just the zero byte.
    }

    Ok((labels.join("."), consumed))
}

/// Parses the RDATA section of a resource record into a human-readable string.
fn parse_rdata(
    rtype: u16,
    data: &[u8],
    rdata_offset: usize,
    rdlen: usize,
) -> Result<String, String> {
    match rtype {
        TYPE_A => {
            if rdlen != 4 {
                return Err(format!("A record has unexpected RDLENGTH {rdlen}"));
            }
            Ok(format!(
                "{}.{}.{}.{}",
                data.get(rdata_offset).copied().unwrap_or(0),
                data.get(rdata_offset + 1).copied().unwrap_or(0),
                data.get(rdata_offset + 2).copied().unwrap_or(0),
                data.get(rdata_offset + 3).copied().unwrap_or(0),
            ))
        }

        TYPE_AAAA => {
            if rdlen != 16 {
                return Err(format!("AAAA record has unexpected RDLENGTH {rdlen}"));
            }
            let mut segments = [0u16; 8];
            for (i, seg) in segments.iter_mut().enumerate() {
                *seg = read_u16(data, rdata_offset + i * 2).unwrap_or(0);
            }
            // Use Rust's Ipv6Addr for canonical formatting (collapses zeroes).
            let addr = std::net::Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                segments[3],
                segments[4],
                segments[5],
                segments[6],
                segments[7],
            );
            Ok(addr.to_string())
        }

        TYPE_NS | TYPE_CNAME | TYPE_PTR => {
            let (name, _) = decode_domain_name(data, rdata_offset)?;
            Ok(name)
        }

        TYPE_MX => {
            if rdlen < 3 {
                return Err(format!("MX record has unexpected RDLENGTH {rdlen}"));
            }
            let preference = read_u16(data, rdata_offset).unwrap_or(0);
            let (exchange, _) = decode_domain_name(data, rdata_offset + 2)?;
            Ok(format!("{preference} {exchange}"))
        }

        TYPE_TXT => {
            let mut result = String::new();
            let end = rdata_offset + rdlen;
            let mut pos = rdata_offset;
            while pos < end {
                let txt_len = *data
                    .get(pos)
                    .ok_or_else(|| "truncated TXT record".to_string())?
                    as usize;
                pos += 1;
                if pos + txt_len > end {
                    return Err("TXT chunk extends past RDATA".to_string());
                }
                let chunk = String::from_utf8_lossy(&data[pos..pos + txt_len]);
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push('"');
                result.push_str(&chunk);
                result.push('"');
                pos += txt_len;
            }
            Ok(result)
        }

        _ => {
            // For unknown types, display hex dump of RDATA.
            let bytes: Vec<String> = data
                .get(rdata_offset..rdata_offset + rdlen)
                .unwrap_or(&[])
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            Ok(bytes.join(" "))
        }
    }
}

/// Parses a full DNS response packet.
fn parse_response(data: &[u8]) -> Result<DnsResponse, String> {
    if data.len() < DNS_HEADER_LEN {
        return Err("response too short for DNS header".to_string());
    }

    let id = read_u16(data, 0).unwrap_or(0);
    let flags = read_u16(data, 2).unwrap_or(0);
    let qdcount = read_u16(data, 4).unwrap_or(0) as usize;
    let ancount = read_u16(data, 6).unwrap_or(0) as usize;
    let nscount = read_u16(data, 8).unwrap_or(0) as usize;
    let arcount = read_u16(data, 10).unwrap_or(0) as usize;

    // Verify this is a response.
    if flags & FLAG_QR == 0 {
        return Err("packet is not a DNS response".to_string());
    }

    // Skip past the question section.
    let mut offset = DNS_HEADER_LEN;
    for _ in 0..qdcount {
        let (_, name_len) = decode_domain_name(data, offset)?;
        offset += name_len;
        offset += 4; // QTYPE (2) + QCLASS (2).
        if offset > data.len() {
            return Err("question section extends past packet".to_string());
        }
    }

    // Parse resource record sections.
    let mut answers = Vec::with_capacity(ancount);
    let mut authorities = Vec::with_capacity(nscount);
    let mut additionals = Vec::with_capacity(arcount);

    for section_idx in 0..3 {
        let count = match section_idx {
            0 => ancount,
            1 => nscount,
            _ => arcount,
        };

        for _ in 0..count {
            if offset >= data.len() {
                break;
            }

            let (name, name_len) = decode_domain_name(data, offset)?;
            offset += name_len;

            if offset + 10 > data.len() {
                return Err("resource record header extends past packet".to_string());
            }

            let rtype = read_u16(data, offset).unwrap_or(0);
            let rclass = read_u16(data, offset + 2).unwrap_or(0);
            let ttl = read_u32(data, offset + 4).unwrap_or(0);
            let rdlen = read_u16(data, offset + 8).unwrap_or(0) as usize;
            offset += 10;

            if offset + rdlen > data.len() {
                return Err("RDATA extends past packet".to_string());
            }

            let rdata = parse_rdata(rtype, data, offset, rdlen)?;
            offset += rdlen;

            let record = DnsRecord {
                name,
                rtype,
                rclass,
                ttl,
                rdata,
            };

            match section_idx {
                0 => answers.push(record),
                1 => authorities.push(record),
                _ => additionals.push(record),
            }
        }
    }

    Ok(DnsResponse {
        id,
        flags,
        answers,
        authorities,
        additionals,
    })
}

// ============================================================================
// Network I/O
// ============================================================================

/// Sends a DNS query to the specified server and returns the raw response.
fn send_query(server: &str, query: &[u8]) -> Result<Vec<u8>, String> {
    let dest = format!("{server}:{DNS_PORT}");

    let socket =
        UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("failed to bind UDP socket: {e}"))?;

    socket
        .set_read_timeout(Some(Duration::from_secs(TIMEOUT_SECS)))
        .map_err(|e| format!("failed to set read timeout: {e}"))?;

    socket
        .send_to(query, &dest)
        .map_err(|e| format!("failed to send query to {dest}: {e}"))?;

    let mut buf = vec![0u8; DNS_MAX_UDP];
    let (len, _src) = socket
        .recv_from(&mut buf)
        .map_err(|e| format!("failed to receive response (timeout or error): {e}"))?;

    buf.truncate(len);
    Ok(buf)
}

// ============================================================================
// Output formatting
// ============================================================================

/// Prints a set of DNS records in nslookup-style format.
fn print_records(section: &str, records: &[DnsRecord]) {
    if records.is_empty() {
        return;
    }
    println!("\n{section}:");
    for rec in records {
        println!(
            "  {}\ttype={}\tttl={}\t{}",
            rec.name,
            type_name(rec.rtype),
            rec.ttl,
            rec.rdata
        );
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

struct Args {
    hostname: String,
    server: Option<String>,
    qtype: u16,
    reverse: bool,
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  nslookup <hostname>                 Look up A record");
    eprintln!("  nslookup -type=<TYPE> <hostname>     Look up specific record type");
    eprintln!("  nslookup <hostname> <server>         Use specific DNS server");
    eprintln!("  nslookup -reverse <ip>               Reverse DNS lookup (PTR)");
    eprintln!();
    eprintln!("Record types: A, AAAA, MX, TXT, NS, CNAME, PTR");
}

fn parse_args() -> Result<Args, String> {
    let argv: Vec<String> = env::args().collect();

    if argv.len() < 2 {
        return Err("no arguments provided".to_string());
    }

    let hostname: Option<String>;
    let mut server: Option<String> = None;
    let mut qtype: u16 = TYPE_A;
    let mut reverse = false;

    let mut positionals: Vec<String> = Vec::new();

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];

        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-reverse" || arg == "--reverse" {
            reverse = true;
        } else if let Some(stripped) = arg.strip_prefix("-type=") {
            qtype = parse_record_type(stripped)
                .ok_or_else(|| format!("unknown record type: '{stripped}'"))?;
        } else if arg == "-type" {
            // Handle `-type AAAA` (space-separated) form.
            i += 1;
            let val = argv
                .get(i)
                .ok_or_else(|| "-type requires a value".to_string())?;
            qtype =
                parse_record_type(val).ok_or_else(|| format!("unknown record type: '{val}'"))?;
        } else if arg.starts_with('-') {
            return Err(format!("unknown option: '{arg}'"));
        } else {
            positionals.push(arg.clone());
        }

        i += 1;
    }

    // Assign positional arguments.
    match positionals.len() {
        0 => return Err("hostname is required".to_string()),
        1 => hostname = Some(positionals[0].clone()),
        2 => {
            hostname = Some(positionals[0].clone());
            server = Some(positionals[1].clone());
        }
        _ => return Err("too many positional arguments".to_string()),
    }

    // For reverse lookups, force PTR type.
    if reverse {
        qtype = TYPE_PTR;
    }

    Ok(Args {
        hostname: hostname.unwrap_or_default(),
        server,
        qtype,
        reverse,
    })
}

// ============================================================================
// Main
// ============================================================================

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let server = args.server.unwrap_or_else(default_dns_server);

    // Build the query name. For reverse lookups, convert the IP address into
    // the appropriate .arpa name.
    let query_name = if args.reverse {
        reverse_name(&args.hostname)?
    } else {
        args.hostname.clone()
    };

    println!("Server:  {server}");
    println!("Query:   {} (type {})", query_name, type_name(args.qtype));
    println!();

    let query = build_query(&query_name, args.qtype);
    let response_bytes = send_query(&server, &query)?;
    let response = parse_response(&response_bytes)?;

    // Verify response ID matches query ID.
    let expected_id = simple_hash(&query_name);
    if response.id != expected_id {
        eprintln!(
            "warning: response ID {:#06x} does not match query ID {:#06x}",
            response.id, expected_id
        );
    }

    // Check RCODE.
    let rcode = response.flags & RCODE_MASK;
    if rcode != 0 {
        return Err(format!(
            "DNS server returned error: {} (RCODE {})",
            rcode_message(rcode),
            rcode
        ));
    }

    // Print results.
    if response.answers.is_empty() {
        println!("No answer records returned.");
    } else {
        print_records("Answer", &response.answers);
    }

    print_records("Authority", &response.authorities);
    print_records("Additional", &response.additionals);

    println!();

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("nslookup: {e}");
        print_usage();
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
        assert_eq!(buf, &[9, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0]);
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
        // Place "example.com" at offset 0, then a pointer at offset 13.
        let mut data = vec![
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', // "example"
            3, b'c', b'o', b'm', // "com"
            0, // terminator
            // At offset 13: a pointer to offset 0.
            0xC0, 0x00,
        ];
        // Pad to avoid indexing issues.
        data.push(0);

        let (name, consumed) = decode_domain_name(&data, 13).unwrap();
        assert_eq!(name, "example.com");
        assert_eq!(consumed, 2); // Pointer is 2 bytes.
    }

    #[test]
    fn decode_partial_compression() {
        // "www" label then pointer to "example.com" at offset 0.
        let mut data = vec![
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
        ];
        // At offset 13: "www" + pointer to 0.
        let www_offset = data.len();
        data.extend_from_slice(&[3, b'w', b'w', b'w', 0xC0, 0x00]);

        let (name, consumed) = decode_domain_name(&data, www_offset).unwrap();
        assert_eq!(name, "www.example.com");
        assert_eq!(consumed, 6); // 1+3 (label) + 2 (pointer)
    }

    // --- Reverse lookup name construction ---

    #[test]
    fn reverse_ipv4() {
        let result = reverse_name("192.168.1.1").unwrap();
        assert_eq!(result, "1.1.168.192.in-addr.arpa");
    }

    #[test]
    fn reverse_ipv6() {
        let result = reverse_name("::1").unwrap();
        assert!(result.ends_with(".ip6.arpa"));
        assert!(result.starts_with("1.0.0.0.0.0.0.0"));
    }

    #[test]
    fn reverse_invalid() {
        assert!(reverse_name("not-an-ip").is_err());
    }

    // --- Query packet structure ---

    #[test]
    fn query_packet_structure() {
        let pkt = build_query("test.com", TYPE_A);
        // Must be at least header (12) + encoded "test.com" (10) + qtype (2) + qclass (2).
        assert!(pkt.len() >= 26);

        // Verify QDCOUNT = 1.
        assert_eq!(pkt[4], 0);
        assert_eq!(pkt[5], 1);

        // Verify flags: RD=1, everything else 0.
        let flags = u16::from(pkt[2]) << 8 | u16::from(pkt[3]);
        assert_eq!(flags, FLAG_RD);

        // The question section should end with QTYPE=A and QCLASS=IN.
        let qclass_pos = pkt.len() - 2;
        let qtype_pos = pkt.len() - 4;
        let qtype_val = u16::from(pkt[qtype_pos]) << 8 | u16::from(pkt[qtype_pos + 1]);
        let qclass_val = u16::from(pkt[qclass_pos]) << 8 | u16::from(pkt[qclass_pos + 1]);
        assert_eq!(qtype_val, TYPE_A);
        assert_eq!(qclass_val, CLASS_IN);
    }

    #[test]
    fn query_aaaa_type() {
        let pkt = build_query("v6.example.com", TYPE_AAAA);
        let qtype_pos = pkt.len() - 4;
        let qtype_val = u16::from(pkt[qtype_pos]) << 8 | u16::from(pkt[qtype_pos + 1]);
        assert_eq!(qtype_val, TYPE_AAAA);
    }

    // --- Response parsing ---

    /// Build a minimal valid DNS response with one A record.
    fn make_a_response(name: &str, ip: [u8; 4]) -> Vec<u8> {
        let mut pkt = Vec::new();
        let id = simple_hash(name);
        pkt.push((id >> 8) as u8);
        pkt.push(id as u8);

        // Flags: QR=1, RD=1, RA=1, RCODE=0.
        let flags: u16 = FLAG_QR | FLAG_RD | 0x0080;
        pkt.push((flags >> 8) as u8);
        pkt.push(flags as u8);

        // QDCOUNT=1, ANCOUNT=1, NSCOUNT=0, ARCOUNT=0.
        pkt.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 0]);

        // Question section.
        encode_domain_name(name, &mut pkt);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);

        // Answer section: same name (via pointer to offset 12), TYPE A, CLASS IN, TTL 300, RDLEN 4.
        pkt.extend_from_slice(&[0xC0, 12]); // Pointer to name at offset 12.
        pkt.extend_from_slice(&[0, TYPE_A as u8]); // TYPE.
        pkt.extend_from_slice(&[0, CLASS_IN as u8]); // CLASS.
        pkt.extend_from_slice(&[0, 0, 1, 44]); // TTL = 300.
        pkt.extend_from_slice(&[0, 4]); // RDLENGTH = 4.
        pkt.extend_from_slice(&ip);

        pkt
    }

    #[test]
    fn parse_a_response() {
        let pkt = make_a_response("example.com", [93, 184, 216, 34]);
        let resp = parse_response(&pkt).unwrap();

        assert_eq!(resp.answers.len(), 1);
        assert_eq!(resp.answers[0].rtype, TYPE_A);
        assert_eq!(resp.answers[0].rdata, "93.184.216.34");
        assert_eq!(resp.answers[0].ttl, 300);
    }

    #[test]
    fn parse_nxdomain() {
        let mut pkt = make_a_response("nope.invalid", [0, 0, 0, 0]);
        // Set RCODE to 3 (NXDOMAIN).
        pkt[3] = (pkt[3] & 0xF0) | 3;
        // Remove the answer (set ANCOUNT=0, truncate RDATA).
        pkt[7] = 0;

        let resp = parse_response(&pkt).unwrap();
        let rcode = resp.flags & RCODE_MASK;
        assert_eq!(rcode, 3);
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
        assert_eq!(parse_record_type("BOGUS"), None);
    }

    #[test]
    fn type_name_round_trip() {
        for &code in &[TYPE_A, TYPE_NS, TYPE_CNAME, TYPE_PTR, TYPE_MX, TYPE_TXT, TYPE_AAAA] {
            let name = type_name(code);
            assert_eq!(parse_record_type(name), Some(code));
        }
    }

    #[test]
    fn rcode_messages_coverage() {
        assert_eq!(rcode_message(0), "No error");
        assert_eq!(rcode_message(3), "Name does not exist (NXDOMAIN)");
        assert_eq!(rcode_message(99), "Unknown error");
    }

    // --- RDATA parsing ---

    #[test]
    fn parse_rdata_a_record() {
        let data = [10, 0, 0, 1];
        let result = parse_rdata(TYPE_A, &data, 0, 4).unwrap();
        assert_eq!(result, "10.0.0.1");
    }

    #[test]
    fn parse_rdata_aaaa_record() {
        // 2001:0db8::0001
        let mut data = vec![0u8; 16];
        data[0] = 0x20;
        data[1] = 0x01;
        data[2] = 0x0d;
        data[3] = 0xb8;
        // bytes 4..14 are zero.
        data[15] = 0x01;
        let result = parse_rdata(TYPE_AAAA, &data, 0, 16).unwrap();
        assert_eq!(result, "2001:db8::1");
    }

    #[test]
    fn parse_rdata_txt_record() {
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
    fn parse_rdata_cname() {
        // Wire-format "mail.example.com"
        let data = [
            4, b'm', b'a', b'i', b'l', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c',
            b'o', b'm', 0,
        ];
        let result = parse_rdata(TYPE_CNAME, &data, 0, data.len()).unwrap();
        assert_eq!(result, "mail.example.com");
    }

    #[test]
    fn parse_rdata_mx_record() {
        // MX: preference=10, then wire-format domain "mx.test.com".
        let mut data = vec![0u8, 10]; // preference = 10
        let mut name_buf = Vec::new();
        encode_domain_name("mx.test.com", &mut name_buf);
        data.extend_from_slice(&name_buf);

        let result = parse_rdata(TYPE_MX, &data, 0, data.len()).unwrap();
        assert_eq!(result, "10 mx.test.com");
    }

    #[test]
    fn parse_rdata_unknown_type() {
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        let result = parse_rdata(999, &data, 0, 4).unwrap();
        assert_eq!(result, "de ad be ef");
    }

    // --- Edge cases ---

    #[test]
    fn parse_response_too_short() {
        let data = [0u8; 5];
        assert!(parse_response(&data).is_err());
    }

    #[test]
    fn decode_empty_name() {
        let data = [0u8];
        let (name, consumed) = decode_domain_name(&data, 0).unwrap();
        assert!(name.is_empty());
        assert_eq!(consumed, 1);
    }

    #[test]
    fn simple_hash_deterministic() {
        let h1 = simple_hash("example.com");
        let h2 = simple_hash("example.com");
        assert_eq!(h1, h2);
    }

    #[test]
    fn simple_hash_different_inputs() {
        let h1 = simple_hash("foo.com");
        let h2 = simple_hash("bar.com");
        assert_ne!(h1, h2);
    }

    #[test]
    fn default_server_returns_something() {
        // Should not panic regardless of whether resolv.conf exists.
        let server = default_dns_server();
        assert!(!server.is_empty());
    }

    // --- Argument parsing ---

    #[test]
    fn parse_rdata_a_wrong_length() {
        let data = [10, 0, 0];
        assert!(parse_rdata(TYPE_A, &data, 0, 3).is_err());
    }

    #[test]
    fn parse_rdata_aaaa_wrong_length() {
        let data = [0u8; 8];
        assert!(parse_rdata(TYPE_AAAA, &data, 0, 8).is_err());
    }

    #[test]
    fn reverse_ipv4_loopback() {
        let result = reverse_name("127.0.0.1").unwrap();
        assert_eq!(result, "1.0.0.127.in-addr.arpa");
    }

    #[test]
    fn reverse_ipv6_full() {
        let result = reverse_name("2001:db8::1").unwrap();
        // The last nibble should be '1', the rest '0' (for the trailing ::1).
        assert!(result.contains("1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0"));
        assert!(result.ends_with("ip6.arpa"));
    }

    // --- Full response with multiple answers ---

    #[test]
    fn parse_multi_answer_response() {
        // Build a response with 2 A records for "multi.test".
        let name = "multi.test";
        let mut pkt = Vec::new();
        let id = simple_hash(name);
        pkt.push((id >> 8) as u8);
        pkt.push(id as u8);

        let flags: u16 = FLAG_QR | FLAG_RD | 0x0080;
        pkt.push((flags >> 8) as u8);
        pkt.push(flags as u8);

        // QDCOUNT=1, ANCOUNT=2.
        pkt.extend_from_slice(&[0, 1, 0, 2, 0, 0, 0, 0]);

        // Question section.
        let q_offset = pkt.len();
        encode_domain_name(name, &mut pkt);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);

        // Answer 1: pointer to question name.
        pkt.extend_from_slice(&[0xC0, q_offset as u8]);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);
        pkt.extend_from_slice(&[0, 0, 0, 60]); // TTL=60
        pkt.extend_from_slice(&[0, 4, 1, 2, 3, 4]); // RDLEN=4, IP=1.2.3.4

        // Answer 2: same pointer.
        pkt.extend_from_slice(&[0xC0, q_offset as u8]);
        pkt.extend_from_slice(&[0, TYPE_A as u8, 0, CLASS_IN as u8]);
        pkt.extend_from_slice(&[0, 0, 0, 60]);
        pkt.extend_from_slice(&[0, 4, 5, 6, 7, 8]); // IP=5.6.7.8

        let resp = parse_response(&pkt).unwrap();
        assert_eq!(resp.answers.len(), 2);
        assert_eq!(resp.answers[0].rdata, "1.2.3.4");
        assert_eq!(resp.answers[1].rdata, "5.6.7.8");
    }
}
