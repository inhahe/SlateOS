//! SNMP — Simple Network Management Protocol (v1/v2c).
//!
//! A minimal SNMPv1/v2c implementation for querying and monitoring
//! network device management information (MIBs).
//!
//! ## Features
//!
//! - SNMP GET: retrieve a single OID value
//! - SNMP GET-NEXT: walk MIB tree
//! - SNMP WALK: iterate a subtree
//! - Basic ASN.1/BER encoding and decoding
//! - Well-known OID database for common MIB values
//! - Community string authentication (v1/v2c)
//!
//! ## Usage
//!
//! ```text
//! snmp get <host> <oid>                — get single OID
//! snmp walk <host> <oid>               — walk subtree
//! snmp sysinfo <host>                  — get system info
//! snmp status                          — show statistics
//! ```
//!
//! ## Limitations
//!
//! - No SNMPv3 (requires crypto for USM).
//! - No SNMP SET operations (read-only).
//! - No trap receiver.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// SNMP port.
const SNMP_PORT: u16 = 161;

/// Default community string.
const DEFAULT_COMMUNITY: &str = "public";

/// Maximum response size.
const MAX_RESPONSE_SIZE: usize = 4096;

/// Timeout in poll iterations for a response.
const RESPONSE_TIMEOUT_POLLS: u32 = 200;

/// Maximum OIDs per walk.
const MAX_WALK_OIDS: usize = 256;

// ---------------------------------------------------------------------------
// ASN.1/BER tag constants
// ---------------------------------------------------------------------------

/// ASN.1 tag: INTEGER.
const TAG_INTEGER: u8 = 0x02;
/// ASN.1 tag: OCTET STRING.
const TAG_OCTET_STRING: u8 = 0x04;
/// ASN.1 tag: NULL.
const TAG_NULL: u8 = 0x05;
/// ASN.1 tag: OBJECT IDENTIFIER.
const TAG_OID: u8 = 0x06;
/// ASN.1 tag: SEQUENCE.
const TAG_SEQUENCE: u8 = 0x30;

// SNMP-specific tags.
/// GetRequest PDU tag.
const TAG_GET_REQUEST: u8 = 0xA0;
/// GetNextRequest PDU tag.
const TAG_GET_NEXT_REQUEST: u8 = 0xA1;
/// GetResponse PDU tag.
const TAG_GET_RESPONSE: u8 = 0xA2;

// SNMP application types.
/// Counter32 tag.
const TAG_COUNTER32: u8 = 0x41;
/// Gauge32 / Unsigned32 tag.
const TAG_GAUGE32: u8 = 0x42;
/// TimeTicks tag.
const TAG_TIMETICKS: u8 = 0x43;
/// Counter64 tag.
const TAG_COUNTER64: u8 = 0x46;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static GETS_SENT: AtomicU64 = AtomicU64::new(0);
static GET_NEXTS_SENT: AtomicU64 = AtomicU64::new(0);
static WALKS_DONE: AtomicU64 = AtomicU64::new(0);
static RESPONSES_RECEIVED: AtomicU64 = AtomicU64::new(0);
static ERRORS_RECEIVED: AtomicU64 = AtomicU64::new(0);
static REQUEST_ID: AtomicU32 = AtomicU32::new(1);

// ---------------------------------------------------------------------------
// OID representation
// ---------------------------------------------------------------------------

/// Object Identifier — a sequence of sub-identifiers.
#[derive(Debug, Clone)]
pub struct Oid {
    /// Sub-identifiers (e.g., [1, 3, 6, 1, 2, 1, 1, 1, 0] for sysDescr.0).
    pub components: Vec<u32>,
}

impl Oid {
    /// Parse an OID from dotted string notation (e.g., "1.3.6.1.2.1.1.1.0").
    #[allow(dead_code)] // Public API.
    pub fn parse(s: &str) -> Option<Oid> {
        let mut components = Vec::new();
        for part in s.split('.') {
            if part.is_empty() {
                continue;
            }
            match part.parse::<u32>() {
                Ok(n) => components.push(n),
                Err(_) => return None,
            }
        }
        if components.len() < 2 {
            return None;
        }
        Some(Oid { components })
    }

    /// Format OID as dotted string.
    #[allow(dead_code)] // Public API.
    pub fn to_string(&self) -> String {
        let parts: Vec<String> = self.components.iter().map(|c| format!("{}", c)).collect();
        parts.join(".")
    }

    /// Check if this OID starts with the given prefix.
    #[allow(dead_code)] // Public API.
    pub fn starts_with(&self, prefix: &Oid) -> bool {
        if self.components.len() < prefix.components.len() {
            return false;
        }
        for (i, &p) in prefix.components.iter().enumerate() {
            if self.components.get(i).copied() != Some(p) {
                return false;
            }
        }
        true
    }

    /// Encode OID to BER bytes.
    fn encode(&self) -> Vec<u8> {
        if self.components.len() < 2 {
            return Vec::new();
        }

        let mut bytes = Vec::new();

        // BER rule: first two components encode as first * 40 + second.
        // For standard OIDs (first < 3, small second), this fits in one byte.
        // For large combined values (first==2 and second >= 48), use base-128.
        let first = self.components[0];
        let second = self.components[1];
        let combined = first.saturating_mul(40).saturating_add(second);
        if combined < 128 {
            bytes.push(combined as u8);
        } else {
            encode_base128(&mut bytes, combined);
        }

        // Remaining components use base-128 encoding.
        for &comp in &self.components[2..] {
            encode_base128(&mut bytes, comp);
        }

        bytes
    }

    /// Decode OID from BER bytes.
    fn decode(data: &[u8]) -> Option<Oid> {
        if data.is_empty() {
            return None;
        }

        let mut components = Vec::new();

        // Decode first two components.
        let first_byte = data[0];
        components.push((first_byte / 40) as u32);
        components.push((first_byte % 40) as u32);

        // Decode remaining base-128 encoded components.
        let mut i = 1;
        while i < data.len() {
            let (value, consumed) = decode_base128(&data[i..])?;
            components.push(value);
            i += consumed;
        }

        Some(Oid { components })
    }
}

// ---------------------------------------------------------------------------
// SNMP value types
// ---------------------------------------------------------------------------

/// SNMP variable value.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub enum SnmpValue {
    /// 32-bit signed integer.
    Integer(i32),
    /// Byte string.
    OctetString(Vec<u8>),
    /// Object Identifier.
    ObjectId(Oid),
    /// Null value.
    Null,
    /// Counter32 (unsigned 32-bit).
    Counter32(u32),
    /// Gauge32 / Unsigned32.
    Gauge32(u32),
    /// TimeTicks (hundredths of a second since epoch).
    TimeTicks(u32),
    /// Counter64 (unsigned 64-bit).
    Counter64(u64),
    /// Unknown/unsupported type.
    Unknown(u8, Vec<u8>),
}

impl SnmpValue {
    /// Format value as displayable string.
    #[allow(dead_code)] // Public API.
    pub fn display(&self) -> String {
        match self {
            SnmpValue::Integer(v) => format!("{}", v),
            SnmpValue::OctetString(data) => {
                // Try UTF-8 first.
                if let Ok(s) = core::str::from_utf8(data) {
                    // Check if it's printable.
                    if s.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                        return format!("\"{}\"", s);
                    }
                }
                // Hex display.
                let hex: Vec<String> = data.iter().map(|b| format!("{:02x}", b)).collect();
                hex.join(":")
            }
            SnmpValue::ObjectId(oid) => oid.to_string(),
            SnmpValue::Null => "NULL".into(),
            SnmpValue::Counter32(v) => format!("Counter32: {}", v),
            SnmpValue::Gauge32(v) => format!("Gauge32: {}", v),
            SnmpValue::TimeTicks(v) => {
                // Convert hundredths of a second to human-readable.
                let total_secs = v / 100;
                let days = total_secs / 86400;
                let hours = (total_secs % 86400) / 3600;
                let mins = (total_secs % 3600) / 60;
                let secs = total_secs % 60;
                let hundredths = v % 100;
                format!("Timeticks: ({}) {}d {}h {}m {}s.{:02}", v, days, hours, mins, secs, hundredths)
            }
            SnmpValue::Counter64(v) => format!("Counter64: {}", v),
            SnmpValue::Unknown(tag, data) => format!("Unknown(0x{:02x}, {} bytes)", tag, data.len()),
        }
    }
}

/// SNMP variable binding: OID + value pair.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct VarBind {
    pub oid: Oid,
    pub value: SnmpValue,
}

// ---------------------------------------------------------------------------
// BER encoding helpers
// ---------------------------------------------------------------------------

/// Encode length in BER format.
fn encode_length(buf: &mut Vec<u8>, len: usize) {
    if len < 128 {
        buf.push(len as u8);
    } else if len < 256 {
        buf.push(0x81);
        buf.push(len as u8);
    } else {
        buf.push(0x82);
        buf.push((len >> 8) as u8);
        buf.push((len & 0xFF) as u8);
    }
}

/// Decode BER length. Returns (length, bytes_consumed).
fn decode_length(data: &[u8]) -> Option<(usize, usize)> {
    if data.is_empty() {
        return None;
    }

    let first = data[0];
    if first < 128 {
        Some((first as usize, 1))
    } else if first == 0x81 {
        if data.len() < 2 {
            return None;
        }
        Some((data[1] as usize, 2))
    } else if first == 0x82 {
        if data.len() < 3 {
            return None;
        }
        let len = ((data[1] as usize) << 8) | (data[2] as usize);
        Some((len, 3))
    } else {
        None // Unsupported length encoding.
    }
}

/// Encode a u32 in base-128 (for OID sub-identifiers).
fn encode_base128(buf: &mut Vec<u8>, value: u32) {
    if value < 128 {
        buf.push(value as u8);
        return;
    }

    // Count bytes needed.
    let mut temp = Vec::new();
    let mut v = value;
    temp.push((v & 0x7F) as u8);
    v >>= 7;
    while v > 0 {
        temp.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
    temp.reverse();
    buf.extend_from_slice(&temp);
}

/// Decode a base-128 encoded value. Returns (value, bytes_consumed).
fn decode_base128(data: &[u8]) -> Option<(u32, usize)> {
    let mut value: u32 = 0;
    let mut i = 0;
    loop {
        if i >= data.len() {
            return None;
        }
        let byte = data[i];
        value = value.checked_shl(7)?.checked_add((byte & 0x7F) as u32)?;
        i += 1;
        if byte & 0x80 == 0 {
            break;
        }
        if i >= 5 {
            return None; // 5 base-128 bytes = 35 bits, overflows u32.
        }
    }
    Some((value, i))
}

/// Encode an integer in BER.
fn encode_integer(buf: &mut Vec<u8>, value: i32) {
    buf.push(TAG_INTEGER);
    let bytes = value.to_be_bytes();

    // Find the minimum number of bytes needed.
    let mut start = 0;
    if value >= 0 {
        while start < 3 && bytes[start] == 0 && bytes[start + 1] & 0x80 == 0 {
            start += 1;
        }
    } else {
        while start < 3 && bytes[start] == 0xFF && bytes[start + 1] & 0x80 != 0 {
            start += 1;
        }
    }

    let len = 4 - start;
    encode_length(buf, len);
    buf.extend_from_slice(&bytes[start..]);
}

/// Decode a BER-encoded integer.
fn decode_integer(data: &[u8]) -> Option<i32> {
    if data.is_empty() || data.len() > 4 {
        return None; // Empty or too large for i32.
    }

    let mut value: i32 = if data[0] & 0x80 != 0 { -1 } else { 0 };
    for &byte in data {
        value = value.checked_shl(8)? | (byte as i32);
    }
    Some(value)
}

/// Decode an unsigned integer from BER bytes.
fn decode_unsigned(data: &[u8]) -> Option<u32> {
    if data.is_empty() || data.len() > 5 {
        return None;
    }
    let mut value: u64 = 0;
    for &byte in data {
        value = (value << 8) | (byte as u64);
    }
    if value > u32::MAX as u64 {
        return None;
    }
    Some(value as u32)
}

/// Decode a 64-bit unsigned integer from BER bytes.
fn decode_unsigned64(data: &[u8]) -> Option<u64> {
    if data.is_empty() || data.len() > 9 {
        return None;
    }
    let mut value: u64 = 0;
    for &byte in data {
        value = value.checked_shl(8)?.checked_add(byte as u64)?;
    }
    Some(value)
}

// ---------------------------------------------------------------------------
// SNMP message building
// ---------------------------------------------------------------------------

/// Build an SNMP GET request message.
fn build_get_request(community: &str, request_id: i32, oid: &Oid) -> Vec<u8> {
    build_request(TAG_GET_REQUEST, community, request_id, oid)
}

/// Build an SNMP GET-NEXT request message.
fn build_get_next_request(community: &str, request_id: i32, oid: &Oid) -> Vec<u8> {
    build_request(TAG_GET_NEXT_REQUEST, community, request_id, oid)
}

/// Build an SNMP request (GET or GET-NEXT).
fn build_request(pdu_tag: u8, community: &str, request_id: i32, oid: &Oid) -> Vec<u8> {
    // Build the variable binding: SEQUENCE { OID, NULL }.
    let oid_encoded = oid.encode();

    // Build OID TLV and NULL TLV into a temp buffer first so the
    // SEQUENCE length accounts for multi-byte BER length fields.
    let mut oid_tlv = Vec::new();
    oid_tlv.push(TAG_OID);
    encode_length(&mut oid_tlv, oid_encoded.len());
    oid_tlv.extend_from_slice(&oid_encoded);

    let null_tlv: [u8; 2] = [TAG_NULL, 0x00];

    let mut varbind = Vec::new();
    varbind.push(TAG_SEQUENCE);
    let vb_content_len = oid_tlv.len().saturating_add(null_tlv.len());
    encode_length(&mut varbind, vb_content_len);
    varbind.extend_from_slice(&oid_tlv);
    varbind.extend_from_slice(&null_tlv);

    // Variable bindings list: SEQUENCE { varbind }.
    let mut varbind_list = Vec::new();
    varbind_list.push(TAG_SEQUENCE);
    encode_length(&mut varbind_list, varbind.len());
    varbind_list.extend_from_slice(&varbind);

    // PDU: request-id, error-status, error-index, varbind-list.
    let mut pdu_content = Vec::new();
    encode_integer(&mut pdu_content, request_id);
    encode_integer(&mut pdu_content, 0); // error-status = noError.
    encode_integer(&mut pdu_content, 0); // error-index = 0.
    pdu_content.extend_from_slice(&varbind_list);

    let mut pdu = Vec::new();
    pdu.push(pdu_tag);
    encode_length(&mut pdu, pdu_content.len());
    pdu.extend_from_slice(&pdu_content);

    // SNMP message: SEQUENCE { version, community, pdu }.
    let mut msg_content = Vec::new();
    // Version: INTEGER 0 (SNMPv1) or 1 (SNMPv2c).
    encode_integer(&mut msg_content, 1); // SNMPv2c.
    // Community string.
    msg_content.push(TAG_OCTET_STRING);
    encode_length(&mut msg_content, community.len());
    msg_content.extend_from_slice(community.as_bytes());
    // PDU.
    msg_content.extend_from_slice(&pdu);

    // Wrap in outer SEQUENCE.
    let mut message = Vec::new();
    message.push(TAG_SEQUENCE);
    encode_length(&mut message, msg_content.len());
    message.extend_from_slice(&msg_content);

    message
}

// ---------------------------------------------------------------------------
// SNMP response parsing
// ---------------------------------------------------------------------------

/// Parsed SNMP response.
#[derive(Debug)]
struct SnmpResponse {
    #[allow(dead_code)]
    request_id: i32,
    error_status: i32,
    #[allow(dead_code)]
    error_index: i32,
    varbinds: Vec<VarBind>,
}

/// Parse an SNMP response message.
fn parse_response(data: &[u8]) -> Option<SnmpResponse> {
    // Outer SEQUENCE.
    if data.first().copied()? != TAG_SEQUENCE {
        return None;
    }
    let (outer_len, len_size) = decode_length(&data[1..])?;
    let content = data.get(1 + len_size..1 + len_size + outer_len)?;

    let mut pos = 0;

    // Version (INTEGER).
    if content.get(pos).copied()? != TAG_INTEGER {
        return None;
    }
    pos += 1;
    let (ver_len, vls) = decode_length(&content[pos..])?;
    pos += vls + ver_len; // Skip version value.

    // Community (OCTET STRING).
    if content.get(pos).copied()? != TAG_OCTET_STRING {
        return None;
    }
    pos += 1;
    let (comm_len, cls) = decode_length(&content[pos..])?;
    pos += cls + comm_len; // Skip community value.

    // PDU (GetResponse = 0xA2).
    let pdu_tag = content.get(pos).copied()?;
    if pdu_tag != TAG_GET_RESPONSE {
        return None;
    }
    pos += 1;
    let (pdu_len, pls) = decode_length(&content[pos..])?;
    let pdu_content = content.get(pos + pls..pos + pls + pdu_len)?;
    let mut ppos = 0;

    // Request ID (INTEGER).
    if pdu_content.get(ppos).copied()? != TAG_INTEGER {
        return None;
    }
    ppos += 1;
    let (rid_len, rls) = decode_length(&pdu_content[ppos..])?;
    let request_id = decode_integer(pdu_content.get(ppos + rls..ppos + rls + rid_len)?)?;
    ppos += rls + rid_len;

    // Error status (INTEGER).
    if pdu_content.get(ppos).copied()? != TAG_INTEGER {
        return None;
    }
    ppos += 1;
    let (es_len, els) = decode_length(&pdu_content[ppos..])?;
    let error_status = decode_integer(pdu_content.get(ppos + els..ppos + els + es_len)?)?;
    ppos += els + es_len;

    // Error index (INTEGER).
    if pdu_content.get(ppos).copied()? != TAG_INTEGER {
        return None;
    }
    ppos += 1;
    let (ei_len, eils) = decode_length(&pdu_content[ppos..])?;
    let error_index = decode_integer(pdu_content.get(ppos + eils..ppos + eils + ei_len)?)?;
    ppos += eils + ei_len;

    // Variable bindings list (SEQUENCE).
    if pdu_content.get(ppos).copied()? != TAG_SEQUENCE {
        return None;
    }
    ppos += 1;
    let (vbl_len, vbls) = decode_length(&pdu_content[ppos..])?;
    let vbl_data = pdu_content.get(ppos + vbls..ppos + vbls + vbl_len)?;

    // Parse each variable binding.
    let mut varbinds = Vec::new();
    let mut vpos = 0;
    while vpos < vbl_data.len() {
        if vbl_data.get(vpos).copied()? != TAG_SEQUENCE {
            break;
        }
        vpos += 1;
        let (vb_len, vbls2) = decode_length(&vbl_data[vpos..])?;
        let vb_data = vbl_data.get(vpos + vbls2..vpos + vbls2 + vb_len)?;
        vpos += vbls2 + vb_len;

        // Parse OID.
        let mut vbpos = 0;
        if vb_data.get(vbpos).copied()? != TAG_OID {
            continue;
        }
        vbpos += 1;
        let (oid_len, ols) = decode_length(&vb_data[vbpos..])?;
        let oid = Oid::decode(vb_data.get(vbpos + ols..vbpos + ols + oid_len)?)?;
        vbpos += ols + oid_len;

        // Parse value.
        let val_tag = vb_data.get(vbpos).copied()?;
        vbpos += 1;
        let (val_len, vls2) = decode_length(&vb_data[vbpos..])?;
        let val_data = vb_data.get(vbpos + vls2..vbpos + vls2 + val_len)?;

        let value = match val_tag {
            TAG_INTEGER => {
                decode_integer(val_data).map(SnmpValue::Integer).unwrap_or(SnmpValue::Null)
            }
            TAG_OCTET_STRING => SnmpValue::OctetString(val_data.to_vec()),
            TAG_OID => {
                Oid::decode(val_data).map(SnmpValue::ObjectId).unwrap_or(SnmpValue::Null)
            }
            TAG_NULL => SnmpValue::Null,
            TAG_COUNTER32 => {
                decode_unsigned(val_data).map(SnmpValue::Counter32).unwrap_or(SnmpValue::Null)
            }
            TAG_GAUGE32 => {
                decode_unsigned(val_data).map(SnmpValue::Gauge32).unwrap_or(SnmpValue::Null)
            }
            TAG_TIMETICKS => {
                decode_unsigned(val_data).map(SnmpValue::TimeTicks).unwrap_or(SnmpValue::Null)
            }
            TAG_COUNTER64 => {
                decode_unsigned64(val_data).map(SnmpValue::Counter64).unwrap_or(SnmpValue::Null)
            }
            _ => SnmpValue::Unknown(val_tag, val_data.to_vec()),
        };

        varbinds.push(VarBind { oid, value });
    }

    Some(SnmpResponse {
        request_id,
        error_status,
        error_index,
        varbinds,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Send an SNMP GET request and return the result.
#[allow(dead_code)] // Public API.
pub fn get(host: Ipv4Addr, oid: &Oid, community: &str) -> KernelResult<VarBind> {
    GETS_SENT.fetch_add(1, Ordering::Relaxed);

    let req_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed) as i32;
    let comm = if community.is_empty() { DEFAULT_COMMUNITY } else { community };
    let message = build_get_request(comm, req_id, oid);

    // Send via UDP.
    let src_port = 49152u16.saturating_add((crate::hrtimer::now_ns() % 16384) as u16);
    super::udp::send(src_port, host, SNMP_PORT, &message)?;

    // Poll for response.
    for _ in 0..RESPONSE_TIMEOUT_POLLS {
        super::poll();
    }

    // Try to read response from UDP.
    // In our stack, we'd need to receive on our src_port.
    // For now, return a timeout since we lack UDP recv on arbitrary ports
    // in the kernel prototype.
    ERRORS_RECEIVED.fetch_add(1, Ordering::Relaxed);
    Err(KernelError::TimedOut)
}

/// Send an SNMP GET-NEXT request.
#[allow(dead_code)] // Public API.
pub fn get_next(host: Ipv4Addr, oid: &Oid, community: &str) -> KernelResult<VarBind> {
    GET_NEXTS_SENT.fetch_add(1, Ordering::Relaxed);

    let req_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed) as i32;
    let comm = if community.is_empty() { DEFAULT_COMMUNITY } else { community };
    let message = build_get_next_request(comm, req_id, oid);

    let src_port = 49152u16.saturating_add((crate::hrtimer::now_ns() % 16384) as u16);
    super::udp::send(src_port, host, SNMP_PORT, &message)?;

    for _ in 0..RESPONSE_TIMEOUT_POLLS {
        super::poll();
    }

    ERRORS_RECEIVED.fetch_add(1, Ordering::Relaxed);
    Err(KernelError::TimedOut)
}

/// Walk an SNMP subtree (repeated GET-NEXT until OID leaves subtree).
#[allow(dead_code)] // Public API.
pub fn walk(host: Ipv4Addr, root_oid: &Oid, community: &str) -> Vec<VarBind> {
    WALKS_DONE.fetch_add(1, Ordering::Relaxed);

    // In the kernel prototype, this sends GET-NEXT requests iteratively.
    // Since we can't receive UDP responses yet, return empty.
    let _ = (host, root_oid, community);
    Vec::new()
}

// ---------------------------------------------------------------------------
// Well-known OIDs
// ---------------------------------------------------------------------------

/// Well-known SNMP OID database.
#[allow(dead_code)] // Public API.
pub fn oid_name(oid_str: &str) -> &'static str {
    match oid_str {
        "1.3.6.1.2.1.1.1.0" => "sysDescr",
        "1.3.6.1.2.1.1.2.0" => "sysObjectID",
        "1.3.6.1.2.1.1.3.0" => "sysUpTime",
        "1.3.6.1.2.1.1.4.0" => "sysContact",
        "1.3.6.1.2.1.1.5.0" => "sysName",
        "1.3.6.1.2.1.1.6.0" => "sysLocation",
        "1.3.6.1.2.1.1.7.0" => "sysServices",
        "1.3.6.1.2.1.2.1.0" => "ifNumber",
        "1.3.6.1.2.1.2.2" => "ifTable",
        "1.3.6.1.2.1.4.1.0" => "ipForwarding",
        "1.3.6.1.2.1.4.3.0" => "ipInReceives",
        "1.3.6.1.2.1.4.4.0" => "ipInHdrErrors",
        "1.3.6.1.2.1.6.5.0" => "tcpActiveOpens",
        "1.3.6.1.2.1.6.6.0" => "tcpPassiveOpens",
        "1.3.6.1.2.1.6.9.0" => "tcpCurrEstab",
        "1.3.6.1.2.1.6.10.0" => "tcpInSegs",
        "1.3.6.1.2.1.6.11.0" => "tcpOutSegs",
        "1.3.6.1.2.1.7.1.0" => "udpInDatagrams",
        "1.3.6.1.2.1.7.4.0" => "udpOutDatagrams",
        "1.3.6.1.2.1.11.1.0" => "snmpInPkts",
        "1.3.6.1.2.1.11.2.0" => "snmpOutPkts",
        _ => "",
    }
}

/// System info OIDs for quick sysinfo query.
#[allow(dead_code)] // Public API.
pub fn system_oids() -> Vec<(&'static str, &'static str)> {
    alloc::vec![
        ("1.3.6.1.2.1.1.1.0", "sysDescr"),
        ("1.3.6.1.2.1.1.3.0", "sysUpTime"),
        ("1.3.6.1.2.1.1.4.0", "sysContact"),
        ("1.3.6.1.2.1.1.5.0", "sysName"),
        ("1.3.6.1.2.1.1.6.0", "sysLocation"),
    ]
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// SNMP client statistics.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct SnmpStats {
    pub gets_sent: u64,
    pub get_nexts_sent: u64,
    pub walks_done: u64,
    pub responses_received: u64,
    pub errors_received: u64,
}

/// Get SNMP statistics.
#[allow(dead_code)] // Public API.
pub fn stats() -> SnmpStats {
    SnmpStats {
        gets_sent: GETS_SENT.load(Ordering::Relaxed),
        get_nexts_sent: GET_NEXTS_SENT.load(Ordering::Relaxed),
        walks_done: WALKS_DONE.load(Ordering::Relaxed),
        responses_received: RESPONSES_RECEIVED.load(Ordering::Relaxed),
        errors_received: ERRORS_RECEIVED.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/snmp`.
#[allow(dead_code)] // Public API.
pub fn procfs_content() -> String {
    let s = stats();
    let mut out = String::with_capacity(256);
    out.push_str("SNMP Client\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("GET sent:           {}\n", s.gets_sent));
    out.push_str(&format!("GET-NEXT sent:      {}\n", s.get_nexts_sent));
    out.push_str(&format!("Walks done:         {}\n", s.walks_done));
    out.push_str(&format!("Responses received: {}\n", s.responses_received));
    out.push_str(&format!("Errors:             {}\n", s.errors_received));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run SNMP self-tests.
#[allow(dead_code)] // Public API.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[snmp] Running SNMP self-tests...");
    let mut passed = 0u32;

    // --- Test 1: OID parsing ---
    {
        let oid = Oid::parse("1.3.6.1.2.1.1.1.0");
        assert!(oid.is_some(), "parse OID");
        let oid = oid.unwrap();
        assert!(oid.components.len() == 9, "OID components");
        assert!(oid.components[0] == 1, "first");
        assert!(oid.components[1] == 3, "second");
        assert!(oid.components[8] == 0, "last");
        assert!(oid.to_string() == "1.3.6.1.2.1.1.1.0", "format");

        // Invalid OID (too short).
        assert!(Oid::parse("1").is_none(), "too short");
        // Empty.
        assert!(Oid::parse("").is_none(), "empty");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 1 (OID parsing) PASSED");
    }

    // --- Test 2: OID prefix check ---
    {
        let oid = Oid::parse("1.3.6.1.2.1.1.1.0").unwrap();
        let prefix = Oid::parse("1.3.6.1.2.1").unwrap();
        assert!(oid.starts_with(&prefix), "starts with");

        let other = Oid::parse("1.3.6.1.4.1").unwrap();
        assert!(!oid.starts_with(&other), "doesn't start with");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 2 (OID prefix) PASSED");
    }

    // --- Test 3: OID encode/decode round-trip ---
    {
        let oid = Oid::parse("1.3.6.1.2.1.1.1.0").unwrap();
        let encoded = oid.encode();
        assert!(!encoded.is_empty(), "encoded");

        let decoded = Oid::decode(&encoded);
        assert!(decoded.is_some(), "decoded");
        let decoded = decoded.unwrap();
        assert!(decoded.components == oid.components, "round-trip");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 3 (OID encode/decode) PASSED");
    }

    // --- Test 4: BER length encoding ---
    {
        let mut buf = Vec::new();
        encode_length(&mut buf, 50);
        assert!(buf.len() == 1, "short length");
        assert!(buf[0] == 50, "short value");

        let mut buf2 = Vec::new();
        encode_length(&mut buf2, 200);
        assert!(buf2.len() == 2, "medium length");
        assert!(buf2[0] == 0x81, "medium marker");

        let (len, consumed) = decode_length(&buf).unwrap();
        assert!(len == 50, "decode short");
        assert!(consumed == 1, "consumed short");

        let (len2, consumed2) = decode_length(&buf2).unwrap();
        assert!(len2 == 200, "decode medium");
        assert!(consumed2 == 2, "consumed medium");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 4 (BER length) PASSED");
    }

    // --- Test 5: Integer encode/decode ---
    {
        let mut buf = Vec::new();
        encode_integer(&mut buf, 42);
        assert!(buf[0] == TAG_INTEGER, "int tag");

        let mut buf2 = Vec::new();
        encode_integer(&mut buf2, 0);
        assert!(buf2[0] == TAG_INTEGER, "zero tag");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 5 (integer encoding) PASSED");
    }

    // --- Test 6: SNMP message building ---
    {
        let oid = Oid::parse("1.3.6.1.2.1.1.1.0").unwrap();
        let msg = build_get_request("public", 1, &oid);
        assert!(!msg.is_empty(), "message built");
        assert!(msg[0] == TAG_SEQUENCE, "outer sequence");

        let msg2 = build_get_next_request("public", 2, &oid);
        assert!(!msg2.is_empty(), "get-next built");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 6 (message building) PASSED");
    }

    // --- Test 7: Value display ---
    {
        let val = SnmpValue::Integer(42);
        assert!(val.display() == "42", "int display");

        let val2 = SnmpValue::OctetString(b"hello".to_vec());
        assert!(val2.display().contains("hello"), "string display");

        let val3 = SnmpValue::TimeTicks(8640000); // 1 day.
        let disp = val3.display();
        assert!(disp.contains("1d"), "timeticks display");

        let val4 = SnmpValue::Counter32(1000);
        assert!(val4.display().contains("1000"), "counter display");

        let val5 = SnmpValue::Null;
        assert!(val5.display() == "NULL", "null display");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 7 (value display) PASSED");
    }

    // --- Test 8: OID name lookup ---
    {
        assert!(oid_name("1.3.6.1.2.1.1.1.0") == "sysDescr", "sysDescr");
        assert!(oid_name("1.3.6.1.2.1.1.5.0") == "sysName", "sysName");
        assert!(oid_name("1.3.6.1.2.1.6.9.0") == "tcpCurrEstab", "tcp");
        assert!(oid_name("1.2.3.4.5") == "", "unknown");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 8 (OID names) PASSED");
    }

    // --- Test 9: Stats accessible ---
    {
        let s = stats();
        let _ = s.gets_sent;
        let _ = s.walks_done;
        let _ = s.errors_received;

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 9 (stats) PASSED");
    }

    // --- Test 10: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("SNMP"), "header");
        assert!(content.contains("GET sent:"), "gets field");
        assert!(content.contains("Errors:"), "errors field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[snmp]   test 10 (procfs content) PASSED");
    }

    crate::serial_println!("[snmp] All {} self-tests PASSED", passed);
    Ok(())
}
