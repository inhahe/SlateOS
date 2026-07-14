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

/// Encode a success reply carrying a name (the [`OP_RESOLVE_PTR`] result) into
/// `out`. Returns bytes written, or `None` if `out` is too small.
#[must_use]
pub fn encode_ok_name(out: &mut [u8], name: &[u8]) -> Option<usize> {
    let total = name.len().checked_add(1)?;
    let dst = out.get_mut(..total)?;
    dst[0] = ST_OK;
    dst[1..].copy_from_slice(name);
    Some(total)
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
