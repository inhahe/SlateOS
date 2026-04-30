//! DNS resolver (RFC 1035).
//!
//! Simple recursive DNS stub resolver that sends queries to a
//! configured DNS server and parses A record responses.
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
//! - No caching (queries are always sent fresh).
//! - No CNAME chasing (returns first A record found).
//! - No EDNS0 or DNSSEC.

use alloc::string::String;
use alloc::vec::Vec;

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

/// Parse a DNS response and extract the first A record IP.
#[allow(clippy::arithmetic_side_effects)]
fn parse_response(data: &[u8]) -> KernelResult<Ipv4Addr> {
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
        // TTL at offset+4..offset+8.
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]);
        offset += 10;

        let rd_end = offset + rdlength as usize;
        if rd_end > data.len() {
            break;
        }

        if rtype == TYPE_A && rclass == CLASS_IN && rdlength == 4 {
            let mut ip = [0u8; 4];
            ip.copy_from_slice(&data[offset..offset + 4]);
            return Ok(Ipv4Addr(ip));
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
/// Sends a DNS A record query to the configured DNS server and
/// waits for a response (blocking with polling, up to ~2 seconds).
#[allow(clippy::arithmetic_side_effects)]
pub fn resolve(name: &str) -> KernelResult<Ipv4Addr> {
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
                Ok(ip) => {
                    crate::serial_println!("[dns] Resolved '{}' → {}", name, ip);
                    return Ok(ip);
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

/// Resolve a domain name and return it as a formatted string.
pub fn resolve_str(name: &str) -> KernelResult<String> {
    let ip = resolve(name)?;
    Ok(alloc::format!("{}", ip))
}
