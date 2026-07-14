//! Request/reply wire schema for the SlateOS `net.stack` IPC service.
//!
//! This crate is the **single source of truth** for the control-message format
//! exchanged over the Service-Registry channel between the kernel socket-syscall
//! forwarders and the userspace `netstack` daemon (see
//! `net-userspace-migration.md` Phase 4 and `design-decisions.md` §64). Both
//! sides link it so the opcodes, status codes, and byte layouts can never drift
//! apart — the exact class of bug that duplicated magic constants invite.
//!
//! Wire format (all messages ride inside a `channel::Message` byte payload):
//!
//! ```text
//! Request  = [op:u8][operands…]
//! Reply    = [status:u8][result…]
//! ```
//!
//! The control path is one request/one reply per connection (one-shot). Bulk
//! TCP/UDP streaming will later add a shared-memory data ring; only the
//! notification/control envelope lives here.
//!
//! Everything is allocation-free: requests/replies are decoded by borrowing the
//! caller's buffer, and encoded by writing into a caller buffer.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

// ---------------------------------------------------------------------------
// Opcodes (first request byte) and status codes (first reply byte).
// ---------------------------------------------------------------------------

/// Request: resolve an `A` record. Operands: hostname bytes (ASCII, no NUL).
/// Reply: [`ST_OK`] + 4 IPv4 bytes, or [`ST_FAIL`].
pub const OP_RESOLVE_A: u8 = 0x01;

/// Request: reverse-resolve (`PTR`) an IPv4 address. Operands: 4 IPv4 bytes.
/// Reply: [`ST_OK`] + dotted-ASCII hostname (no trailing dot/NUL), or [`ST_FAIL`].
pub const OP_RESOLVE_PTR: u8 = 0x02;

/// Request: one-shot TCP fetch — connect to `ip:port`, send `payload`, read the
/// response, and close. Operands: `[ip:4][port_be:2][payload…]`. Reply:
/// [`ST_OK`] + the received response bytes (possibly empty), or [`ST_FAIL`].
/// This collapses a whole TCP transaction into a single control round-trip; a
/// streaming socket API arrives with the Phase-5 shared-memory data ring.
pub const OP_TCP_FETCH: u8 = 0x03;

/// Request: one-shot UDP exchange — send `payload` as a single datagram to
/// `ip:port`, wait for one response datagram, and return it. Operands:
/// `[ip:4][port_be:2][payload…]` (same layout as [`OP_TCP_FETCH`]). Reply:
/// [`ST_OK`] + the response datagram payload (possibly empty), or [`ST_FAIL`].
/// Suits request/response UDP protocols (DNS, NTP, STUN); a streaming socket
/// API arrives with the Phase-5 shared-memory data ring.
pub const OP_UDP_EXCHANGE: u8 = 0x04;

/// Reply status: success. Any op-specific result bytes follow.
pub const ST_OK: u8 = 0x00;
/// Reply status: failure. No result bytes follow.
pub const ST_FAIL: u8 = 0x01;

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// A decoded request, as seen by the daemon. Borrows the request buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'a> {
    /// Forward-resolve the borrowed hostname's first `A` record.
    ResolveA(&'a [u8]),
    /// Reverse-resolve the given IPv4 address (`PTR`).
    ResolvePtr([u8; 4]),
    /// One-shot TCP fetch to `ip:port`, sending the borrowed `payload`.
    TcpFetch {
        /// Destination IPv4 address.
        ip: [u8; 4],
        /// Destination TCP port.
        port: u16,
        /// Bytes to send once the connection is established (may be empty).
        payload: &'a [u8],
    },
    /// One-shot UDP exchange with `ip:port`, sending the borrowed `payload` as a
    /// single datagram and returning the first response datagram.
    UdpExchange {
        /// Destination IPv4 address.
        ip: [u8; 4],
        /// Destination UDP port.
        port: u16,
        /// Datagram payload to send (may be empty).
        payload: &'a [u8],
    },
    /// An opcode this build does not recognise (carries the raw byte).
    Unknown(u8),
}

impl<'a> Request<'a> {
    /// Decode a request message. Returns `None` only for a structurally invalid
    /// message (empty, or a known op with too few operand bytes). An unknown
    /// opcode is *not* an error — it decodes to [`Request::Unknown`] so the
    /// daemon can reply [`ST_FAIL`] uniformly.
    #[must_use]
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let (op, rest) = bytes.split_first()?;
        match *op {
            OP_RESOLVE_A => Some(Request::ResolveA(rest)),
            OP_RESOLVE_PTR => {
                let ip = rest.get(..4)?;
                Some(Request::ResolvePtr([ip[0], ip[1], ip[2], ip[3]]))
            }
            OP_TCP_FETCH => {
                let head = rest.get(..6)?;
                let ip = [head[0], head[1], head[2], head[3]];
                let port = u16::from_be_bytes([head[4], head[5]]);
                let payload = rest.get(6..)?;
                Some(Request::TcpFetch { ip, port, payload })
            }
            OP_UDP_EXCHANGE => {
                let head = rest.get(..6)?;
                let ip = [head[0], head[1], head[2], head[3]];
                let port = u16::from_be_bytes([head[4], head[5]]);
                let payload = rest.get(6..)?;
                Some(Request::UdpExchange { ip, port, payload })
            }
            other => Some(Request::Unknown(other)),
        }
    }
}

/// Encode an [`OP_RESOLVE_A`] request for `hostname` into `out`. Returns the
/// number of bytes written, or `None` if `out` is too small.
#[must_use]
pub fn encode_resolve_a(out: &mut [u8], hostname: &[u8]) -> Option<usize> {
    let total = hostname.len().checked_add(1)?;
    let dst = out.get_mut(..total)?;
    dst[0] = OP_RESOLVE_A;
    dst[1..].copy_from_slice(hostname);
    Some(total)
}

/// Encode an [`OP_RESOLVE_PTR`] request for IPv4 `ip` into `out`. Returns the
/// number of bytes written (5), or `None` if `out` is too small.
#[must_use]
pub fn encode_resolve_ptr(out: &mut [u8], ip: &[u8; 4]) -> Option<usize> {
    let dst = out.get_mut(..5)?;
    dst[0] = OP_RESOLVE_PTR;
    dst[1..5].copy_from_slice(ip);
    Some(5)
}

/// Encode an [`OP_TCP_FETCH`] request (`[op][ip:4][port_be:2][payload]`) into
/// `out`. Returns bytes written, or `None` if `out` is too small.
#[must_use]
pub fn encode_tcp_fetch(out: &mut [u8], ip: &[u8; 4], port: u16, payload: &[u8]) -> Option<usize> {
    encode_ip_port_payload(out, OP_TCP_FETCH, ip, port, payload)
}

/// Encode an [`OP_UDP_EXCHANGE`] request (`[op][ip:4][port_be:2][payload]`) into
/// `out`. Returns bytes written, or `None` if `out` is too small.
#[must_use]
pub fn encode_udp_exchange(out: &mut [u8], ip: &[u8; 4], port: u16, payload: &[u8]) -> Option<usize> {
    encode_ip_port_payload(out, OP_UDP_EXCHANGE, ip, port, payload)
}

/// Shared encoder for the `[op][ip:4][port_be:2][payload]` request layout used
/// by both [`OP_TCP_FETCH`] and [`OP_UDP_EXCHANGE`].
#[must_use]
fn encode_ip_port_payload(
    out: &mut [u8],
    op: u8,
    ip: &[u8; 4],
    port: u16,
    payload: &[u8],
) -> Option<usize> {
    let total = payload.len().checked_add(7)?;
    let dst = out.get_mut(..total)?;
    dst[0] = op;
    dst[1..5].copy_from_slice(ip);
    dst[5..7].copy_from_slice(&port.to_be_bytes());
    dst[7..].copy_from_slice(payload);
    Some(total)
}

// ---------------------------------------------------------------------------
// Replies
// ---------------------------------------------------------------------------

/// Encode a success reply carrying an IPv4 address (the [`OP_RESOLVE_A`] result)
/// into `out`. Returns bytes written (5), or `None` if `out` is too small.
#[must_use]
pub fn encode_ok_ipv4(out: &mut [u8], ip: &[u8; 4]) -> Option<usize> {
    let dst = out.get_mut(..5)?;
    dst[0] = ST_OK;
    dst[1..5].copy_from_slice(ip);
    Some(5)
}

/// Encode a success reply carrying an arbitrary byte payload (`[ST_OK][bytes]`)
/// into `out`. Returns bytes written, or `None` if `out` is too small.
#[must_use]
pub fn encode_ok_bytes(out: &mut [u8], bytes: &[u8]) -> Option<usize> {
    let total = bytes.len().checked_add(1)?;
    let dst = out.get_mut(..total)?;
    dst[0] = ST_OK;
    dst[1..].copy_from_slice(bytes);
    Some(total)
}

/// Encode a success reply carrying a name (the [`OP_RESOLVE_PTR`] result) into
/// `out`. Alias for [`encode_ok_bytes`] with name-specific intent. Returns bytes
/// written, or `None` if `out` is too small.
#[must_use]
pub fn encode_ok_name(out: &mut [u8], name: &[u8]) -> Option<usize> {
    encode_ok_bytes(out, name)
}

/// Encode a failure reply into `out`. Returns bytes written (1), or `None` if
/// `out` is empty.
#[must_use]
pub fn encode_fail(out: &mut [u8]) -> Option<usize> {
    let dst = out.get_mut(..1)?;
    dst[0] = ST_FAIL;
    Some(1)
}

/// Decoded outcome of a reply whose OK payload is an IPv4 address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ipv4Reply {
    /// Success: the resolved address.
    Ok([u8; 4]),
    /// The daemon reported failure ([`ST_FAIL`]).
    Fail,
    /// The reply was malformed (unknown status byte or short OK payload).
    Malformed,
}

/// Decode a reply to an [`OP_RESOLVE_A`] request.
#[must_use]
pub fn parse_ipv4_reply(bytes: &[u8]) -> Ipv4Reply {
    match bytes.first().copied() {
        Some(ST_OK) => match bytes.get(1..5) {
            Some(ip) => Ipv4Reply::Ok([ip[0], ip[1], ip[2], ip[3]]),
            None => Ipv4Reply::Malformed,
        },
        Some(ST_FAIL) => Ipv4Reply::Fail,
        _ => Ipv4Reply::Malformed,
    }
}

/// Decoded outcome of a reply whose OK payload is a name. Borrows the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameReply<'a> {
    /// Success: the decoded dotted-ASCII name (may be empty).
    Ok(&'a [u8]),
    /// The daemon reported failure ([`ST_FAIL`]).
    Fail,
    /// The reply was malformed (unknown status byte).
    Malformed,
}

/// Decode a reply to an [`OP_RESOLVE_PTR`] request.
#[must_use]
pub fn parse_name_reply(bytes: &[u8]) -> NameReply<'_> {
    match bytes.first().copied() {
        Some(ST_OK) => NameReply::Ok(bytes.get(1..).unwrap_or(&[])),
        Some(ST_FAIL) => NameReply::Fail,
        _ => NameReply::Malformed,
    }
}

/// Decoded outcome of a reply whose OK payload is arbitrary bytes (e.g. the
/// [`OP_TCP_FETCH`] response). Borrows the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BytesReply<'a> {
    /// Success: the payload bytes (may be empty).
    Ok(&'a [u8]),
    /// The daemon reported failure ([`ST_FAIL`]).
    Fail,
    /// The reply was malformed (unknown status byte).
    Malformed,
}

/// Decode a reply carrying an arbitrary byte payload (e.g. [`OP_TCP_FETCH`]).
#[must_use]
pub fn parse_bytes_reply(bytes: &[u8]) -> BytesReply<'_> {
    match bytes.first().copied() {
        Some(ST_OK) => BytesReply::Ok(bytes.get(1..).unwrap_or(&[])),
        Some(ST_FAIL) => BytesReply::Fail,
        _ => BytesReply::Malformed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_a_round_trip() {
        let mut buf = [0u8; 64];
        let n = encode_resolve_a(&mut buf, b"example.com").unwrap();
        assert_eq!(buf[0], OP_RESOLVE_A);
        match Request::parse(&buf[..n]).unwrap() {
            Request::ResolveA(h) => assert_eq!(h, b"example.com"),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn resolve_ptr_round_trip() {
        let mut buf = [0u8; 8];
        let n = encode_resolve_ptr(&mut buf, &[8, 8, 4, 4]).unwrap();
        assert_eq!(n, 5);
        assert_eq!(buf[0], OP_RESOLVE_PTR);
        match Request::parse(&buf[..n]).unwrap() {
            Request::ResolvePtr(ip) => assert_eq!(ip, [8, 8, 4, 4]),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn empty_request_is_none() {
        assert!(Request::parse(&[]).is_none());
    }

    #[test]
    fn short_ptr_request_is_none() {
        // op byte present but fewer than 4 operand bytes.
        assert!(Request::parse(&[OP_RESOLVE_PTR, 1, 2]).is_none());
    }

    #[test]
    fn unknown_opcode_decodes_to_unknown() {
        match Request::parse(&[0x7f, 0xaa]).unwrap() {
            Request::Unknown(op) => assert_eq!(op, 0x7f),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn resolve_a_empty_hostname_ok() {
        // A zero-length hostname is structurally valid (the daemon will fail it).
        let mut buf = [0u8; 4];
        let n = encode_resolve_a(&mut buf, b"").unwrap();
        assert_eq!(n, 1);
        match Request::parse(&buf[..n]).unwrap() {
            Request::ResolveA(h) => assert!(h.is_empty()),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn encoders_reject_small_buffers() {
        let mut tiny = [0u8; 0];
        assert!(encode_resolve_a(&mut tiny, b"x").is_none());
        assert!(encode_resolve_ptr(&mut [0u8; 4], &[1, 2, 3, 4]).is_none());
        assert!(encode_ok_ipv4(&mut [0u8; 4], &[1, 2, 3, 4]).is_none());
        assert!(encode_fail(&mut []).is_none());
    }

    #[test]
    fn ipv4_reply_round_trip() {
        let mut buf = [0u8; 8];
        let n = encode_ok_ipv4(&mut buf, &[93, 184, 216, 34]).unwrap();
        assert_eq!(parse_ipv4_reply(&buf[..n]), Ipv4Reply::Ok([93, 184, 216, 34]));
        let n = encode_fail(&mut buf).unwrap();
        assert_eq!(parse_ipv4_reply(&buf[..n]), Ipv4Reply::Fail);
        assert_eq!(parse_ipv4_reply(&[]), Ipv4Reply::Malformed);
        // OK status but truncated address.
        assert_eq!(parse_ipv4_reply(&[ST_OK, 1, 2]), Ipv4Reply::Malformed);
    }

    #[test]
    fn tcp_fetch_round_trip() {
        let mut buf = [0u8; 64];
        let n = encode_tcp_fetch(&mut buf, &[93, 184, 216, 34], 80, b"GET /\r\n").unwrap();
        assert_eq!(buf[0], OP_TCP_FETCH);
        match Request::parse(&buf[..n]).unwrap() {
            Request::TcpFetch { ip, port, payload } => {
                assert_eq!(ip, [93, 184, 216, 34]);
                assert_eq!(port, 80);
                assert_eq!(payload, b"GET /\r\n");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn tcp_fetch_empty_payload_ok() {
        let mut buf = [0u8; 16];
        let n = encode_tcp_fetch(&mut buf, &[1, 2, 3, 4], 443, b"").unwrap();
        assert_eq!(n, 7);
        match Request::parse(&buf[..n]).unwrap() {
            Request::TcpFetch { ip, port, payload } => {
                assert_eq!(ip, [1, 2, 3, 4]);
                assert_eq!(port, 443);
                assert!(payload.is_empty());
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn short_tcp_fetch_request_is_none() {
        // op + ip but no port bytes.
        assert!(Request::parse(&[OP_TCP_FETCH, 1, 2, 3, 4, 0]).is_none());
    }

    #[test]
    fn udp_exchange_round_trip() {
        let mut buf = [0u8; 64];
        let n = encode_udp_exchange(&mut buf, &[8, 8, 8, 8], 53, b"query").unwrap();
        assert_eq!(buf[0], OP_UDP_EXCHANGE);
        match Request::parse(&buf[..n]).unwrap() {
            Request::UdpExchange { ip, port, payload } => {
                assert_eq!(ip, [8, 8, 8, 8]);
                assert_eq!(port, 53);
                assert_eq!(payload, b"query");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn short_udp_exchange_request_is_none() {
        // op + ip but no port bytes.
        assert!(Request::parse(&[OP_UDP_EXCHANGE, 9, 9, 9, 9, 0]).is_none());
    }

    #[test]
    fn bytes_reply_round_trip() {
        let mut buf = [0u8; 32];
        let n = encode_ok_bytes(&mut buf, b"HTTP/1.1 200").unwrap();
        assert_eq!(parse_bytes_reply(&buf[..n]), BytesReply::Ok(b"HTTP/1.1 200"));
        let n = encode_fail(&mut buf).unwrap();
        assert_eq!(parse_bytes_reply(&buf[..n]), BytesReply::Fail);
        assert_eq!(parse_bytes_reply(&[ST_OK]), BytesReply::Ok(b""));
        assert_eq!(parse_bytes_reply(&[0x42]), BytesReply::Malformed);
    }

    #[test]
    fn name_reply_round_trip() {
        let mut buf = [0u8; 32];
        let n = encode_ok_name(&mut buf, b"dns.google").unwrap();
        assert_eq!(parse_name_reply(&buf[..n]), NameReply::Ok(b"dns.google"));
        let n = encode_fail(&mut buf).unwrap();
        assert_eq!(parse_name_reply(&buf[..n]), NameReply::Fail);
        // OK with an empty name is valid (no PTR text but success framing).
        assert_eq!(parse_name_reply(&[ST_OK]), NameReply::Ok(b""));
        assert_eq!(parse_name_reply(&[0x55]), NameReply::Malformed);
    }
}
