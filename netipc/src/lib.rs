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

pub mod ring;

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

/// Request: shared-memory handshake — map the shared-memory region identified
/// by `handle` (of `size` bytes) into the daemon's address space, verify the
/// [`SHM_PING_REQUEST_MAGIC`] the kernel wrote at offset 0, overwrite offset 8
/// with [`SHM_PING_RESPONSE_MAGIC`], unmap, and reply. Operands:
/// `[handle_le:8][size_le:4]`. Reply: [`ST_OK`] (magic verified + response
/// written) or [`ST_FAIL`]. This is the bootstrap that proves cross-address-
/// space `SYS_SHM_MAP` sharing — the exact mechanism the Phase-5 data ring
/// uses to hand the daemon its SQ/CQ/data region.
pub const OP_SHM_PING: u8 = 0x05;

/// Magic the kernel writes at byte offset 0 of an [`OP_SHM_PING`] region; the
/// daemon reads it back to confirm it mapped the *same* physical frames.
pub const SHM_PING_REQUEST_MAGIC: u64 = 0x5348_4D50_494E_4751; // "SHMPINGQ"
/// Magic the daemon writes at byte offset 8; the kernel reads it back to
/// confirm the daemon's writes are visible in the kernel's view of the region.
pub const SHM_PING_RESPONSE_MAGIC: u64 = 0x5348_4D50_4F4E_4752; // "SHMPONGR"

/// Request: shared-memory **ring** handshake — the first end-to-end exercise of
/// the [`ring`] SQ/CQ driver across the address-space boundary. The kernel has
/// laid out the region identified by `handle` (of `size` bytes) as a ring
/// (`Ring::init`), written a payload into the data area, and submitted one
/// `OP_SEND` SQE. The daemon maps the region, `Ring::attach`es, pops the SQE,
/// reads the payload, ASCII-upper-cases it in place, pushes a completion
/// ([`ring::Cqe`]) carrying the echoed `user_data` and the byte count as
/// `result`, unmaps, and replies. Operands: `[handle_le:8][size_le:4]` (same
/// layout as [`OP_SHM_PING`]). Reply: [`ST_OK`] (SQE consumed + completion
/// posted) or [`ST_FAIL`]. This validates the whole zero-copy data path the
/// Phase-5 socket API rides on: kernel produces → daemon consumes/transforms →
/// kernel reaps, with no bytes copied through the control channel.
pub const OP_RING_ECHO: u8 = 0x06;

/// `user_data` the kernel stamps on the [`OP_RING_ECHO`] SQE; the daemon echoes
/// it back in the completion so the kernel can confirm it reaped the right one.
pub const RING_ECHO_USER_DATA: u64 = 0x5249_4E47_4543_484F; // "RINGECHO"

/// Request: shared-memory **ring TCP** — the ring-native equivalent of the
/// one-shot [`OP_TCP_FETCH`] control op, but with the whole TCP transaction
/// driven through the [`ring`] data path instead of the control channel. The
/// kernel lays out the region identified by `handle` (of `size` bytes) as a ring
/// (`Ring::init`), writes the request payload into the data area, and submits a
/// batch of socket SQEs: [`ring::OP_CONNECT`] (destination endpoint packed into
/// `aux` via [`ring::Sqe::pack_endpoint`]), [`ring::OP_SEND`] (payload window),
/// [`ring::OP_RECV`] (empty window for the daemon to fill), and
/// [`ring::OP_CLOSE`]. The daemon maps the region, `Ring::attach`es, then drains
/// the SQ driving a single live [`ring::Ring`]-backed TCP connection: connect →
/// send → recv (writing the response bytes back into the recv window and posting
/// the byte count as the completion `result`) → close. It posts one completion
/// per SQE, unmaps, and replies. Operands: `[handle_le:8][size_le:4]` (same
/// layout as [`OP_SHM_PING`]/[`OP_RING_ECHO`]). Reply: [`ST_OK`] (batch drained,
/// completions posted) or [`ST_FAIL`]. This is the Phase-4 capstone: a real
/// TCP fetch flowing entirely over the zero-copy ring, the shape the Phase-5
/// streaming socket API is built on.
pub const OP_RING_TCP: u8 = 0x07;

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
    /// Shared-memory handshake: map region `handle` (`size` bytes), verify the
    /// request magic, write the response magic, unmap.
    ShmPing {
        /// Shared-memory region handle to map (a `ShmHandle` raw u64).
        handle: u64,
        /// Region size in bytes (as reported by `SYS_SHM_SIZE`).
        size: u32,
    },
    /// Shared-memory ring echo: map region `handle` (`size` bytes), attach as a
    /// [`ring::Ring`], pop the kernel's `OP_SEND` SQE, upper-case its payload in
    /// place, push a completion, unmap.
    RingEcho {
        /// Shared-memory region handle to map (a `ShmHandle` raw u64).
        handle: u64,
        /// Region size in bytes (as reported by `SYS_SHM_SIZE`).
        size: u32,
    },
    /// Shared-memory ring TCP: map region `handle` (`size` bytes), attach as a
    /// [`ring::Ring`], and drain the SQ driving a single live TCP connection
    /// (connect → send → recv → close), posting one completion per SQE.
    RingTcp {
        /// Shared-memory region handle to map (a `ShmHandle` raw u64).
        handle: u64,
        /// Region size in bytes (as reported by `SYS_SHM_SIZE`).
        size: u32,
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
            OP_SHM_PING => {
                let (handle, size) = parse_handle_size(rest)?;
                Some(Request::ShmPing { handle, size })
            }
            OP_RING_ECHO => {
                let (handle, size) = parse_handle_size(rest)?;
                Some(Request::RingEcho { handle, size })
            }
            OP_RING_TCP => {
                let (handle, size) = parse_handle_size(rest)?;
                Some(Request::RingTcp { handle, size })
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

/// Encode an [`OP_SHM_PING`] request (`[op][handle_le:8][size_le:4]`) into
/// `out`. Returns bytes written (13), or `None` if `out` is too small.
#[must_use]
pub fn encode_shm_ping(out: &mut [u8], handle: u64, size: u32) -> Option<usize> {
    encode_handle_size(out, OP_SHM_PING, handle, size)
}

/// Encode an [`OP_RING_ECHO`] request (`[op][handle_le:8][size_le:4]`) into
/// `out`. Returns bytes written (13), or `None` if `out` is too small.
#[must_use]
pub fn encode_ring_echo(out: &mut [u8], handle: u64, size: u32) -> Option<usize> {
    encode_handle_size(out, OP_RING_ECHO, handle, size)
}

/// Encode an [`OP_RING_TCP`] request (`[op][handle_le:8][size_le:4]`) into
/// `out`. Returns bytes written (13), or `None` if `out` is too small.
#[must_use]
pub fn encode_ring_tcp(out: &mut [u8], handle: u64, size: u32) -> Option<usize> {
    encode_handle_size(out, OP_RING_TCP, handle, size)
}

/// Shared encoder for the `[op][handle_le:8][size_le:4]` request layout used by
/// [`OP_SHM_PING`], [`OP_RING_ECHO`], and [`OP_RING_TCP`].
#[must_use]
fn encode_handle_size(out: &mut [u8], op: u8, handle: u64, size: u32) -> Option<usize> {
    let dst = out.get_mut(..13)?;
    dst[0] = op;
    dst[1..9].copy_from_slice(&handle.to_le_bytes());
    dst[9..13].copy_from_slice(&size.to_le_bytes());
    Some(13)
}

/// Shared decoder for the `[handle_le:8][size_le:4]` operand layout (the `rest`
/// after the opcode) used by [`OP_SHM_PING`], [`OP_RING_ECHO`], and
/// [`OP_RING_TCP`].
#[must_use]
fn parse_handle_size(rest: &[u8]) -> Option<(u64, u32)> {
    let head = rest.get(..12)?;
    let handle = u64::from_le_bytes([
        head[0], head[1], head[2], head[3], head[4], head[5], head[6], head[7],
    ]);
    let size = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    Some((handle, size))
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
    fn shm_ping_round_trip() {
        let mut buf = [0u8; 32];
        let n = encode_shm_ping(&mut buf, 0x1234_5678_9abc_def0, 16384).unwrap();
        assert_eq!(n, 13);
        assert_eq!(buf[0], OP_SHM_PING);
        match Request::parse(&buf[..n]).unwrap() {
            Request::ShmPing { handle, size } => {
                assert_eq!(handle, 0x1234_5678_9abc_def0);
                assert_eq!(size, 16384);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn short_shm_ping_request_is_none() {
        // op + handle but truncated size field.
        assert!(Request::parse(&[OP_SHM_PING, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0]).is_none());
    }

    #[test]
    fn ring_echo_round_trip() {
        let mut buf = [0u8; 32];
        let n = encode_ring_echo(&mut buf, 0x0fed_cba9_8765_4321, 65536).unwrap();
        assert_eq!(n, 13);
        assert_eq!(buf[0], OP_RING_ECHO);
        match Request::parse(&buf[..n]).unwrap() {
            Request::RingEcho { handle, size } => {
                assert_eq!(handle, 0x0fed_cba9_8765_4321);
                assert_eq!(size, 65536);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn short_ring_echo_request_is_none() {
        // op + handle but truncated size field.
        assert!(Request::parse(&[OP_RING_ECHO, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0]).is_none());
    }

    #[test]
    fn ring_tcp_round_trip() {
        let mut buf = [0u8; 32];
        let n = encode_ring_tcp(&mut buf, 0x1122_3344_5566_7788, 4096).unwrap();
        assert_eq!(n, 13);
        assert_eq!(buf[0], OP_RING_TCP);
        match Request::parse(&buf[..n]).unwrap() {
            Request::RingTcp { handle, size } => {
                assert_eq!(handle, 0x1122_3344_5566_7788);
                assert_eq!(size, 4096);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn short_ring_tcp_request_is_none() {
        // op + handle but truncated size field.
        assert!(Request::parse(&[OP_RING_TCP, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0]).is_none());
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
