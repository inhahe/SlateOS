//! BSD/POSIX socket API.
//!
//! Translates the POSIX socket interface (`socket`, `connect`, `bind`,
//! `listen`, `accept`, `send`, `recv`, `sendto`, `recvfrom`) to our
//! native TCP/UDP/DNS syscalls.
//!
//! ## Design
//!
//! Our kernel uses separate syscall families for TCP and UDP rather than
//! a unified socket abstraction.  This module bridges the gap:
//!
//! - `socket(AF_INET, SOCK_STREAM, 0)` → creates an unconnected TCP fd
//! - `connect(fd, addr, len)` → `SYS_TCP_CONNECT(ip, port)`, stores handle
//! - `bind(fd, addr, len)` + `listen(fd, backlog)` → `SYS_TCP_BIND(port)`
//! - `accept(fd, addr, len)` → `SYS_TCP_ACCEPT(listener_handle)`
//! - `socket(AF_INET, SOCK_DGRAM, 0)` → creates an unbound UDP fd
//! - `connect(fd, addr, len)` on UDP → stores default peer (userspace only)
//! - `bind(fd, addr, len)` on UDP → `SYS_UDP_BIND(port)`
//! - `send(fd, ...)` on connected UDP → uses stored peer address
//! - `sendto(fd, ...)` on UDP → `SYS_UDP_SEND(handle, ip, port, buf, len)`
//! - `sendto(fd, NULL, ...)` on connected UDP → uses stored peer
//! - `recv(fd, ...)` on UDP → `SYS_UDP_RECV` (discards source address)
//! - `recvfrom(fd, ...)` on UDP → `SYS_UDP_RECV(handle, buf, len, addr_out)`
//!
//! ## Socket State
//!
//! Because `socket()` creates a fd before any kernel handle exists, we
//! track per-fd socket metadata (type, bound port) in a static side
//! table.  The kernel handle is created lazily on `connect()` or
//! `bind()`/`listen()`.
//!
//! ## Byte Order
//!
//! Network byte order (big-endian) is used for `sockaddr_in` fields
//! (`sin_port`, `sin_addr`) per the BSD convention.  Our kernel syscalls
//! also expect network byte order for IP addresses.

use crate::errno;
use crate::fdtable::{self, HandleKind};
use crate::syscall::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// IPv4 Internet protocols.
pub const AF_INET: i32 = 2;
/// IPv6 Internet protocols.
pub const AF_INET6: i32 = 10;
/// IPv4 (alias).
pub const PF_INET: i32 = AF_INET;

/// Sequenced, reliable, connection-based byte streams (TCP).
pub const SOCK_STREAM: i32 = 1;
/// Connectionless, unreliable datagrams (UDP).
pub const SOCK_DGRAM: i32 = 2;

/// TCP protocol number.
pub const IPPROTO_TCP: i32 = 6;
/// UDP protocol number.
pub const IPPROTO_UDP: i32 = 17;
/// Default protocol (auto-select based on socket type).
pub const IPPROTO_IP: i32 = 0;

/// Address to bind to all interfaces.
pub const INADDR_ANY: u32 = 0;
/// Loopback address (127.0.0.1).
pub const INADDR_LOOPBACK: u32 = 0x7F00_0001;
/// Broadcast address (255.255.255.255).
pub const INADDR_BROADCAST: u32 = 0xFFFF_FFFF;
/// Invalid/sentinel address.
pub const INADDR_NONE: u32 = 0xFFFF_FFFF;

/// Max string length for an IPv4 address ("255.255.255.255\0").
pub const INET_ADDRSTRLEN: usize = 16;
/// Max string length for an IPv6 address.
pub const INET6_ADDRSTRLEN: usize = 46;

/// Shut down reading.
pub const SHUT_RD: i32 = 0;
/// Shut down writing.
pub const SHUT_WR: i32 = 1;
/// Shut down both reading and writing.
pub const SHUT_RDWR: i32 = 2;

// Socket option levels.
/// Socket-level options.
pub const SOL_SOCKET: i32 = 1;
/// TCP-level options.
pub const SOL_TCP: i32 = 6;

// Socket options (SOL_SOCKET level).
/// Reuse local address.
pub const SO_REUSEADDR: i32 = 2;
/// Keep connections alive.
pub const SO_KEEPALIVE: i32 = 9;
/// Type of socket.
pub const SO_TYPE: i32 = 3;
/// Socket error.
pub const SO_ERROR: i32 = 4;
/// Receive buffer size.
pub const SO_RCVBUF: i32 = 8;
/// Send buffer size.
pub const SO_SNDBUF: i32 = 7;
/// Permit broadcast datagrams.
pub const SO_BROADCAST: i32 = 6;
/// Linger on close if unsent data.
pub const SO_LINGER: i32 = 13;
/// Reuse local port.
pub const SO_REUSEPORT: i32 = 15;
/// Receive timeout.
pub const SO_RCVTIMEO: i32 = 20;
/// Send timeout.
pub const SO_SNDTIMEO: i32 = 21;
/// Socket is accepting connections (listening).
pub const SO_ACCEPTCONN: i32 = 30;
/// Domain of the socket.
pub const SO_DOMAIN: i32 = 39;
/// Protocol of the socket.
pub const SO_PROTOCOL: i32 = 38;

// TCP-level socket options (SOL_TCP).
/// Disable Nagle's algorithm.
pub const TCP_NODELAY: i32 = 1;
/// Idle time before keepalive probes (seconds).
pub const TCP_KEEPIDLE: i32 = 4;
/// Interval between keepalive probes (seconds).
pub const TCP_KEEPINTVL: i32 = 5;
/// Number of keepalive probes before dropping.
pub const TCP_KEEPCNT: i32 = 6;
/// Peer's advertised MSS.
pub const TCP_MAXSEG: i32 = 2;
/// Enable TCP cork (coalesce small writes).
pub const TCP_CORK: i32 = 3;
/// User timeout (milliseconds) — time to wait for data ACK.
pub const TCP_USER_TIMEOUT: i32 = 18;
/// Detailed TCP connection information (Linux TCP_INFO).
pub const TCP_INFO: i32 = 11;

// IP-level socket options (IPPROTO_IP / SOL_IP).
/// IP protocol level for setsockopt/getsockopt.
pub const SOL_IP: i32 = 0;
/// Join a multicast group (RFC 1112).
/// Value: `IpMreq` struct.
pub const IP_ADD_MEMBERSHIP: i32 = 35;
/// Leave a multicast group.
/// Value: `IpMreq` struct.
pub const IP_DROP_MEMBERSHIP: i32 = 36;
/// Set the TTL for multicast packets.
pub const IP_MULTICAST_TTL: i32 = 33;
/// Set the loopback mode for multicast packets.
pub const IP_MULTICAST_LOOP: i32 = 34;

// MSG flags for send/recv.
/// Out-of-band data.
pub const MSG_OOB: i32 = 1;
/// Peek at incoming data without consuming.
pub const MSG_PEEK: i32 = 2;
/// Send without routing (ignored — we always route).
pub const MSG_DONTROUTE: i32 = 4;
/// Data was truncated (returned by recvmsg).
pub const MSG_TRUNC: i32 = 0x20;
/// Non-blocking operation.
pub const MSG_DONTWAIT: i32 = 0x40;
/// Terminate a record (ignored — TCP is byte-stream).
pub const MSG_EOR: i32 = 0x80;
/// Wait for full request or error.
pub const MSG_WAITALL: i32 = 0x100;
/// More data coming (cork the send).
pub const MSG_MORE: i32 = 0x8000;
/// Don't send SIGPIPE (ignored — no signals).
pub const MSG_NOSIGNAL: i32 = 0x4000;
/// Set close-on-exec for received fds (recvmsg).
pub const MSG_CMSG_CLOEXEC: i32 = 0x40000000;

/// Socket type flag: set `O_NONBLOCK` on the new socket (Linux extension).
pub const SOCK_NONBLOCK: i32 = 0o4000;
/// Socket type flag: set close-on-exec on the new socket (Linux extension).
pub const SOCK_CLOEXEC: i32 = 0o2_000_000;

// ---------------------------------------------------------------------------
// Multicast group request
// ---------------------------------------------------------------------------

/// IPv4 multicast group membership request (for `setsockopt`).
///
/// Used with `IP_ADD_MEMBERSHIP` / `IP_DROP_MEMBERSHIP` to join or
/// leave a multicast group.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IpMreq {
    /// Multicast group address.
    pub imr_multiaddr: InAddr,
    /// Local interface address (usually `INADDR_ANY`).
    pub imr_interface: InAddr,
}

// ---------------------------------------------------------------------------
// sockaddr structures
// ---------------------------------------------------------------------------

/// Generic socket address.
///
/// Programs cast this to/from `SockaddrIn` for IPv4.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Sockaddr {
    /// Address family (e.g., `AF_INET`).
    pub sa_family: u16,
    /// Protocol-specific address data.
    pub sa_data: [u8; 14],
}

/// IPv4 socket address.
///
/// All multi-byte fields are in network byte order (big-endian).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrIn {
    /// Address family — always `AF_INET`.
    pub sin_family: u16,
    /// Port number in network byte order.
    pub sin_port: u16,
    /// IPv4 address in network byte order.
    pub sin_addr: InAddr,
    /// Padding to reach `sizeof(struct sockaddr)`.
    pub sin_zero: [u8; 8],
}

/// IPv4 address (network byte order).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InAddr {
    /// IPv4 address as a 32-bit value in network byte order.
    pub s_addr: u32,
}

/// Size type used for address lengths.
pub type SocklenT = u32;

// ---------------------------------------------------------------------------
// Per-fd socket metadata
// ---------------------------------------------------------------------------

/// Maximum sockets tracked in the metadata table.
const MAX_SOCKETS: usize = 64;

/// Per-fd socket state that the kernel doesn't track for us.
///
/// Stored in a side table indexed by fd number (up to `MAX_SOCKETS`).
/// Contains the socket type and any state needed between `socket()`
/// and `connect()`/`bind()`.
#[derive(Clone, Copy)]
pub(crate) struct SocketMeta {
    /// Socket type (`SOCK_STREAM` or `SOCK_DGRAM`).
    pub(crate) sock_type: i32,
    /// Port bound via `bind()` (deferred until `listen()` for TCP).
    /// Network byte order.  0 if not yet bound.
    bound_port: u16,
    /// Remote peer IP address (network byte order).  Set on `connect()`.
    peer_addr: u32,
    /// Remote peer port (network byte order).  Set on `connect()`.
    peer_port: u16,
    /// Local IP address (network byte order).  Set on `bind()`.
    local_addr: u32,
    /// SO_KEEPALIVE setting (stored; kernel applies when syscall exists).
    keepalive: bool,
    /// TCP_NODELAY setting (stored; kernel applies when syscall exists).
    nodelay: bool,
    /// SO_REUSEADDR setting.
    reuseaddr: bool,
    /// SO_RCVBUF: receive buffer size (advisory, in bytes).
    rcvbuf: i32,
    /// SO_SNDBUF: send buffer size (advisory, in bytes).
    sndbuf: i32,
    /// SO_BROADCAST: permit sending to broadcast addresses.
    broadcast: bool,
    /// SO_LINGER: whether linger is enabled.
    pub(crate) linger_onoff: bool,
    /// SO_LINGER: linger time in seconds (meaningful only when linger_onoff is true).
    pub(crate) linger_secs: i32,
    /// TCP_KEEPIDLE: idle time before first keepalive probe (seconds).
    keepidle: i32,
    /// TCP_KEEPINTVL: interval between keepalive probes (seconds).
    keepintvl: i32,
    /// TCP_KEEPCNT: max keepalive probes before declaring connection dead.
    keepcnt: i32,
    /// UDP shutdown state: SHUT_RD called (disables recv).
    pub(crate) udp_shut_rd: bool,
    /// UDP shutdown state: SHUT_WR called (disables send).
    pub(crate) udp_shut_wr: bool,
    /// SO_RCVTIMEO: receive timeout in milliseconds (0 = no timeout).
    rcvtimeo_ms: u64,
    /// SO_SNDTIMEO: send timeout in milliseconds (0 = no timeout).
    sndtimeo_ms: u64,
}

/// Per-fd socket metadata table.
///
/// Indexed by fd number.  Only valid for fds with socket handle kinds.
static mut SOCKET_META: [Option<SocketMeta>; MAX_SOCKETS] = [None; MAX_SOCKETS];

/// Get a mutable pointer to the metadata table.
#[inline]
fn meta_ptr() -> *mut [Option<SocketMeta>; MAX_SOCKETS] {
    core::ptr::addr_of_mut!(SOCKET_META)
}

/// Store metadata for a socket fd.
fn set_meta(fd: i32, meta: SocketMeta) {
    if fd >= 0 && (fd as usize) < MAX_SOCKETS {
        // SAFETY: Single-threaded access.
        unsafe {
            let table = &mut *meta_ptr();
            if let Some(slot) = table.get_mut(fd as usize) {
                *slot = Some(meta);
            }
        }
    }
}

/// Get metadata for a socket fd.
pub(crate) fn get_meta(fd: i32) -> Option<SocketMeta> {
    if fd < 0 || (fd as usize) >= MAX_SOCKETS {
        return None;
    }
    // SAFETY: Single-threaded access.
    unsafe {
        let table = &*meta_ptr();
        table.get(fd as usize).copied().flatten()
    }
}

/// Remove metadata for a socket fd.
///
/// Called from `file.rs` when a socket fd is closed.
pub(crate) fn clear_meta(fd: i32) {
    if fd >= 0 && (fd as usize) < MAX_SOCKETS {
        // SAFETY: Single-threaded access.
        unsafe {
            let table = &mut *meta_ptr();
            if let Some(slot) = table.get_mut(fd as usize) {
                *slot = None;
            }
        }
    }
}

/// Copy socket metadata from one fd to another.
///
/// Called from `file.rs` when duplicating a socket fd via `dup()`
/// or `dup2()`.  Both fds share the same kernel handle, and the
/// metadata (peer address, bound port, etc.) must be available
/// from either fd for `getpeername()`/`getsockname()` to work.
pub(crate) fn copy_meta(src_fd: i32, dst_fd: i32) {
    if let Some(meta) = get_meta(src_fd) {
        set_meta(dst_fd, meta);
    }
}

// ---------------------------------------------------------------------------
// Byte-order functions
// ---------------------------------------------------------------------------

/// Convert a 16-bit value from host to network byte order.
#[unsafe(no_mangle)]
pub extern "C" fn htons(hostshort: u16) -> u16 {
    hostshort.to_be()
}

/// Convert a 32-bit value from host to network byte order.
#[unsafe(no_mangle)]
pub extern "C" fn htonl(hostlong: u32) -> u32 {
    hostlong.to_be()
}

/// Convert a 16-bit value from network to host byte order.
#[unsafe(no_mangle)]
pub extern "C" fn ntohs(netshort: u16) -> u16 {
    u16::from_be(netshort)
}

/// Convert a 32-bit value from network to host byte order.
#[unsafe(no_mangle)]
pub extern "C" fn ntohl(netlong: u32) -> u32 {
    u32::from_be(netlong)
}

// ---------------------------------------------------------------------------
// inet_addr / inet_ntoa
// ---------------------------------------------------------------------------

/// Convert an IPv4 dotted-decimal string to a 32-bit network-order address.
///
/// Returns `INADDR_NONE` (0xFFFFFFFF) on parse error.
///
/// # Safety
///
/// `cp` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inet_addr(cp: *const u8) -> u32 {
    /// Sentinel for parse failure.
    const INADDR_NONE: u32 = 0xFFFF_FFFF;

    if cp.is_null() {
        return INADDR_NONE;
    }

    let mut octets = [0u8; 4];
    let mut octet_idx: usize = 0;
    let mut cur_val: u32 = 0;
    let mut digits: u32 = 0;
    let mut i: usize = 0;

    loop {
        // SAFETY: cp is a valid null-terminated string.
        let c = unsafe { *cp.add(i) };
        if c == 0 {
            break;
        }

        if c == b'.' {
            if digits == 0 || cur_val > 255 || octet_idx >= 3 {
                return INADDR_NONE;
            }
            if let Some(slot) = octets.get_mut(octet_idx) {
                *slot = cur_val as u8;
            }
            octet_idx = octet_idx.wrapping_add(1);
            cur_val = 0;
            digits = 0;
        } else if c.is_ascii_digit() {
            // SAFETY: c is '0'..='9', so c - b'0' is 0..=9.
            #[allow(clippy::arithmetic_side_effects)]
            {
                cur_val = cur_val.wrapping_mul(10).wrapping_add(u32::from(c - b'0'));
            }
            digits = digits.wrapping_add(1);
            if cur_val > 255 {
                return INADDR_NONE;
            }
        } else {
            return INADDR_NONE;
        }

        i = i.wrapping_add(1);
    }

    // Final octet.
    if digits == 0 || cur_val > 255 || octet_idx != 3 {
        return INADDR_NONE;
    }
    octets[3] = cur_val as u8;

    // Return in network byte order (big-endian): octets[0] is MSB.
    u32::from_be_bytes(octets)
}

/// Convert a network-order IPv4 address to a dotted-decimal string.
///
/// Returns a pointer to a static buffer (not thread-safe, per POSIX).
///
/// # Safety
///
/// The returned pointer is valid until the next call to `inet_ntoa`.
#[unsafe(no_mangle)]
pub extern "C" fn inet_ntoa(addr: InAddr) -> *const u8 {
    static mut NTOA_BUF: [u8; 16] = [0u8; 16];

    let octets = addr.s_addr.to_be_bytes();
    let mut pos: usize = 0;

    // SAFETY: Single-threaded access; buffer is 16 bytes, max output
    // is "255.255.255.255\0" = 16 bytes.
    unsafe {
        let buf = core::ptr::addr_of_mut!(NTOA_BUF);
        for (idx, &octet) in octets.iter().enumerate() {
            if idx > 0 {
                write_byte(&mut *buf, &mut pos, b'.');
            }
            write_u8_decimal(&mut *buf, &mut pos, octet);
        }
        write_byte(&mut *buf, &mut pos, 0); // null terminate
        (*buf).as_ptr()
    }
}

/// Write a single byte to a buffer.
fn write_byte(buf: &mut [u8; 16], pos: &mut usize, b: u8) {
    if let Some(slot) = buf.get_mut(*pos) {
        *slot = b;
        *pos = pos.wrapping_add(1);
    }
}

/// Write a u8 as decimal digits to a buffer.
fn write_u8_decimal(buf: &mut [u8; 16], pos: &mut usize, val: u8) {
    if val >= 100 {
        #[allow(clippy::arithmetic_side_effects)]
        write_byte(buf, pos, b'0' + val / 100);
    }
    if val >= 10 {
        #[allow(clippy::arithmetic_side_effects)]
        write_byte(buf, pos, b'0' + (val / 10) % 10);
    }
    #[allow(clippy::arithmetic_side_effects)]
    write_byte(buf, pos, b'0' + val % 10);
}

// ---------------------------------------------------------------------------
// inet_pton
// ---------------------------------------------------------------------------

/// Convert an address from presentation (text) to network format.
///
/// `af` must be `AF_INET`.  `src` is a null-terminated dotted-decimal
/// string.  `dst` must point to a `struct in_addr` (4 bytes for IPv4).
///
/// Returns 1 on success, 0 if `src` is not a valid address for `af`,
/// or -1 with errno set to `EAFNOSUPPORT` if `af` is unsupported.
///
/// `AF_INET6` is recognised but not supported (returns -1).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inet_pton(af: i32, src: *const u8, dst: *mut u8) -> i32 {
    if src.is_null() || dst.is_null() {
        return 0;
    }

    match af {
        AF_INET => {
            // Parse four decimal octets separated by '.'.
            // SAFETY: src is a valid null-terminated string (caller contract).
            let addr = unsafe { inet_addr(src) };

            // inet_addr returns INADDR_NONE (0xFFFFFFFF) on failure.
            // 255.255.255.255 is also 0xFFFFFFFF — verify by re-scanning.
            if addr == 0xFFFF_FFFF && !unsafe { is_valid_ipv4(src) } {
                return 0;
            }

            // Store in network byte order.
            let bytes = addr.to_ne_bytes();
            // SAFETY: dst points to at least 4 bytes (caller contract).
            unsafe {
                *dst = bytes[0];
                *dst.add(1) = bytes[1];
                *dst.add(2) = bytes[2];
                *dst.add(3) = bytes[3];
            }
            1
        }
        AF_INET6 => {
            // Parse IPv6 address: up to 8 groups of hex separated by ':'.
            // Supports :: compression (at most one per address).
            // SAFETY: src is null-terminated (caller contract).
            unsafe { inet_pton6(src, dst) }
        }
        _ => {
            errno::set_errno(errno::EAFNOSUPPORT);
            -1
        }
    }
}

/// Parse an IPv6 address string into 16 bytes in network byte order.
///
/// Supports:
/// - Full form: `2001:0db8:85a3:0000:0000:8a2e:0370:7334`
/// - Compressed: `::1`, `fe80::1`, `2001:db8::1`
/// - IPv4-mapped: `::ffff:192.168.1.1`
///
/// # Safety
///
/// `src` must be a valid null-terminated string.
/// `dst` must point to at least 16 writable bytes.
#[allow(clippy::arithmetic_side_effects)]
unsafe fn inet_pton6(src: *const u8, dst: *mut u8) -> i32 {
    let mut groups = [0u16; 8]; // Parsed 16-bit groups.
    let mut group_count: usize = 0;
    let mut coloncolon_pos: Option<usize> = None; // Position of :: expansion.
    let mut cur_val: u32 = 0;
    let mut cur_digits: usize = 0;
    let mut i: usize = 0;

    // Handle leading :: .
    // SAFETY: src is null-terminated.
    if unsafe { *src } == b':' && unsafe { *src.add(1) } == b':' {
        coloncolon_pos = Some(0);
        i = 2;
    }

    loop {
        let c = unsafe { *src.add(i) };
        if c == 0 {
            break;
        }

        if c == b':' {
            // End of current group.
            if cur_digits == 0 && coloncolon_pos.is_some() {
                return 0; // Multiple :: or empty non-:: group.
            }
            if group_count >= 8 {
                return 0;
            }
            if cur_digits > 0 {
                groups[group_count] = cur_val as u16;
                group_count = group_count.wrapping_add(1);
                cur_val = 0;
                cur_digits = 0;
            }
            // Check for ::
            if unsafe { *src.add(i.wrapping_add(1)) } == b':' {
                if coloncolon_pos.is_some() {
                    return 0; // Only one :: allowed.
                }
                coloncolon_pos = Some(group_count);
                i = i.wrapping_add(2);
                continue;
            }
            i = i.wrapping_add(1);
            continue;
        }

        // Parse hex digit.
        let digit = match c {
            b'0'..=b'9' => u32::from(c.wrapping_sub(b'0')),
            b'a'..=b'f' => u32::from(c.wrapping_sub(b'a')).wrapping_add(10),
            b'A'..=b'F' => u32::from(c.wrapping_sub(b'A')).wrapping_add(10),
            _ => return 0, // Invalid character.
        };

        cur_val = cur_val.wrapping_shl(4) | digit;
        cur_digits = cur_digits.wrapping_add(1);

        if cur_digits > 4 || cur_val > 0xFFFF {
            return 0; // Too many digits or value overflow.
        }

        i = i.wrapping_add(1);
    }

    // Save last group.
    if cur_digits > 0 {
        if group_count >= 8 {
            return 0;
        }
        groups[group_count] = cur_val as u16;
        group_count = group_count.wrapping_add(1);
    }

    // Expand :: to fill missing groups.
    let mut result = [0u8; 16];
    if let Some(cc_pos) = coloncolon_pos {
        if group_count > 8 {
            return 0;
        }
        let fill = 8usize.wrapping_sub(group_count);
        // Groups before ::
        for g in 0..cc_pos {
            let be = groups[g].to_be_bytes();
            result[g.wrapping_mul(2)] = be[0];
            result[g.wrapping_mul(2).wrapping_add(1)] = be[1];
        }
        // Zeroed groups for ::
        // (result is already zeroed)
        // Groups after ::
        let after_count = group_count.wrapping_sub(cc_pos);
        for g in 0..after_count {
            let dst_idx = cc_pos.wrapping_add(fill).wrapping_add(g);
            let be = groups[cc_pos.wrapping_add(g)].to_be_bytes();
            result[dst_idx.wrapping_mul(2)] = be[0];
            result[dst_idx.wrapping_mul(2).wrapping_add(1)] = be[1];
        }
    } else {
        // No :: — must have exactly 8 groups.
        if group_count != 8 {
            return 0;
        }
        for g in 0..8 {
            let be = groups[g].to_be_bytes();
            result[g.wrapping_mul(2)] = be[0];
            result[g.wrapping_mul(2).wrapping_add(1)] = be[1];
        }
    }

    // Copy to output.
    // SAFETY: dst points to at least 16 bytes (caller contract).
    unsafe {
        core::ptr::copy_nonoverlapping(result.as_ptr(), dst, 16);
    }
    1
}

/// Check if a C string is a syntactically valid IPv4 dotted-decimal.
///
/// # Safety
///
/// `s` must be a valid null-terminated string.
unsafe fn is_valid_ipv4(s: *const u8) -> bool {
    let mut dots: u32 = 0;
    let mut digits: u32 = 0;
    let mut val: u32 = 0;
    let mut i: usize = 0;

    loop {
        // SAFETY: s is null-terminated.
        let c = unsafe { *s.add(i) };
        if c == 0 {
            break;
        }
        if c == b'.' {
            if digits == 0 {
                return false;
            }
            dots = dots.wrapping_add(1);
            digits = 0;
            val = 0;
        } else if c.is_ascii_digit() {
            #[allow(clippy::arithmetic_side_effects)]
            {
                val = val.wrapping_mul(10).wrapping_add(u32::from(c - b'0'));
            }
            digits = digits.wrapping_add(1);
            if val > 255 {
                return false;
            }
        } else {
            return false;
        }
        i = i.wrapping_add(1);
    }

    dots == 3 && digits > 0 && val <= 255
}

// ---------------------------------------------------------------------------
// inet_ntop
// ---------------------------------------------------------------------------

/// Convert an address from network format to presentation (text).
///
/// `af` must be `AF_INET`.  `src` points to a `struct in_addr` (4
/// bytes in network byte order).  `dst` is the output buffer of at
/// least `size` bytes.
///
/// Returns `dst` on success, or null with errno set.
#[unsafe(no_mangle)]
pub extern "C" fn inet_ntop(af: i32, src: *const u8, dst: *mut u8, size: u32) -> *const u8 {
    if src.is_null() || dst.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null();
    }

    match af {
        AF_INET => inet_ntop4(src, dst, size),
        AF_INET6 => inet_ntop6(src, dst, size),
        _ => {
            errno::set_errno(errno::EAFNOSUPPORT);
            core::ptr::null()
        }
    }
}

/// Format an IPv4 address (4 bytes) to dotted-decimal string.
fn inet_ntop4(src: *const u8, dst: *mut u8, size: u32) -> *const u8 {
    // SAFETY: src points to at least 4 bytes (caller contract).
    let octets: [u8; 4] = unsafe { [*src, *src.add(1), *src.add(2), *src.add(3)] };

    let mut tmp = [0u8; 16]; // max "255.255.255.255\0"
    let mut pos: usize = 0;
    for (idx, &octet) in octets.iter().enumerate() {
        if idx > 0 {
            write_byte_ntop(&mut tmp, &mut pos, b'.');
        }
        write_u8_dec_ntop(&mut tmp, &mut pos, octet);
    }

    let needed = pos.wrapping_add(1); // +null
    if (size as usize) < needed {
        errno::set_errno(errno::ENOSPC);
        return core::ptr::null();
    }

    // SAFETY: dst is valid for at least size bytes; needed <= size.
    unsafe {
        let mut j: usize = 0;
        while j < pos {
            if let Some(&b) = tmp.get(j) {
                *dst.add(j) = b;
            }
            j = j.wrapping_add(1);
        }
        *dst.add(pos) = 0;
    }

    dst.cast_const()
}

/// Format an IPv6 address (16 bytes) to RFC 5952 canonical string.
///
/// Produces the shortest representation with :: compression for the
/// longest run of consecutive zero groups (ties broken by earliest).
#[allow(clippy::arithmetic_side_effects)]
fn inet_ntop6(src: *const u8, dst: *mut u8, size: u32) -> *const u8 {
    // SAFETY: src points to at least 16 bytes (caller contract).
    let mut groups = [0u16; 8];
    for i in 0..8 {
        groups[i] = u16::from_be_bytes(unsafe {
            [*src.add(i * 2), *src.add(i * 2 + 1)]
        });
    }

    // Find the longest run of zero groups for :: compression.
    let mut best_start: usize = 8;
    let mut best_len: usize = 0;
    let mut cur_start: usize = 0;
    let mut cur_len: usize = 0;
    for i in 0..8 {
        if groups[i] == 0 {
            if cur_len == 0 {
                cur_start = i;
            }
            cur_len = cur_len.wrapping_add(1);
        } else {
            if cur_len > best_len {
                best_start = cur_start;
                best_len = cur_len;
            }
            cur_len = 0;
        }
    }
    if cur_len > best_len {
        best_start = cur_start;
        best_len = cur_len;
    }
    // Per RFC 5952 §4.2.2: :: must save at least one group (len >= 2,
    // or a single 0 if it's alone is fine for the full zero address).
    if best_len < 2 {
        best_start = 8; // Disable compression.
        best_len = 0;
    }

    // Format into stack buffer.  Max IPv6 string: 39 chars + null.
    let mut tmp = [0u8; 46]; // INET6_ADDRSTRLEN
    let mut pos: usize = 0;

    let mut i: usize = 0;
    while i < 8 {
        if i == best_start {
            // Emit ::
            if pos < 45 { tmp[pos] = b':'; pos = pos.wrapping_add(1); }
            if i == 0 {
                if pos < 45 { tmp[pos] = b':'; pos = pos.wrapping_add(1); }
            }
            i = i.wrapping_add(best_len);
            if i >= 8 {
                // Trailing :: — already emitted one ':'
                if pos < 45 { tmp[pos] = b':'; pos = pos.wrapping_add(1); }
            }
            continue;
        }
        if i > 0 && !(i == best_start.wrapping_add(best_len) && best_start < 8) {
            if pos < 45 { tmp[pos] = b':'; pos = pos.wrapping_add(1); }
        }
        // Write hex group without leading zeros.
        write_hex16_ntop(&mut tmp, &mut pos, groups[i]);
        i = i.wrapping_add(1);
    }

    let needed = pos.wrapping_add(1);
    if (size as usize) < needed {
        errno::set_errno(errno::ENOSPC);
        return core::ptr::null();
    }

    // SAFETY: dst is valid for at least size bytes.
    unsafe {
        let mut j: usize = 0;
        while j < pos {
            if let Some(&b) = tmp.get(j) {
                *dst.add(j) = b;
            }
            j = j.wrapping_add(1);
        }
        *dst.add(pos) = 0;
    }

    dst.cast_const()
}

/// Write a 16-bit value as lowercase hex without leading zeros.
#[allow(clippy::arithmetic_side_effects)]
fn write_hex16_ntop(buf: &mut [u8; 46], pos: &mut usize, val: u16) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if val == 0 {
        if let Some(slot) = buf.get_mut(*pos) {
            *slot = b'0';
            *pos = pos.wrapping_add(1);
        }
        return;
    }
    // Find the first non-zero nibble.
    let mut started = false;
    for shift in [12u32, 8, 4, 0] {
        let nibble = ((val as u32) >> shift) & 0xF;
        if nibble != 0 || started {
            if let Some(slot) = buf.get_mut(*pos) {
                *slot = HEX[nibble as usize];
                *pos = pos.wrapping_add(1);
            }
            started = true;
        }
    }
}

/// Write a single byte to a 16-byte buffer (inet_ntop helper).
fn write_byte_ntop(buf: &mut [u8; 16], pos: &mut usize, b: u8) {
    if let Some(slot) = buf.get_mut(*pos) {
        *slot = b;
        *pos = pos.wrapping_add(1);
    }
}

/// Write a u8 as decimal digits (inet_ntop helper).
fn write_u8_dec_ntop(buf: &mut [u8; 16], pos: &mut usize, val: u8) {
    if val >= 100 {
        #[allow(clippy::arithmetic_side_effects)]
        write_byte_ntop(buf, pos, b'0' + val / 100);
    }
    if val >= 10 {
        #[allow(clippy::arithmetic_side_effects)]
        write_byte_ntop(buf, pos, b'0' + (val / 10) % 10);
    }
    #[allow(clippy::arithmetic_side_effects)]
    write_byte_ntop(buf, pos, b'0' + val % 10);
}

// ---------------------------------------------------------------------------
// inet_aton — BSD-style address parsing
// ---------------------------------------------------------------------------

/// Convert an IPv4 address from dotted-decimal string to binary form.
///
/// Like `inet_addr` but stores the result in a `struct in_addr`
/// pointed to by `inp`.  Returns 1 on success, 0 on parse error.
///
/// This is a BSD extension commonly used by older programs and some
/// configuration parsers.
///
/// # Safety
///
/// `cp` must be a valid null-terminated string.
/// `inp` must point to at least 4 bytes (a `struct in_addr`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inet_aton(cp: *const u8, inp: *mut u32) -> i32 {
    if cp.is_null() || inp.is_null() {
        return 0;
    }

    // SAFETY: inet_addr parses the same dotted-decimal format.
    let addr = unsafe { inet_addr(cp) };

    // inet_addr returns INADDR_NONE (0xFFFFFFFF) on failure.
    // 255.255.255.255 is valid, so we double-check with the validator.
    if addr == 0xFFFF_FFFF && !unsafe { is_valid_ipv4(cp) } {
        return 0;
    }

    // SAFETY: inp is valid for 4 bytes (caller contract).
    unsafe { *inp = addr; }
    1
}

// ---------------------------------------------------------------------------
// socket()
// ---------------------------------------------------------------------------

/// Create a socket.
///
/// Only `AF_INET` with `SOCK_STREAM` (TCP) or `SOCK_DGRAM` (UDP) is
/// supported.  The kernel handle is not created until `connect()` or
/// `bind()`/`listen()`.
///
/// Returns a file descriptor on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn socket(domain: i32, sock_type: i32, protocol: i32) -> i32 {
    // Validate domain.
    if domain != AF_INET {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }

    // Validate type + protocol combination.
    match sock_type {
        SOCK_STREAM => {
            if protocol != 0 && protocol != IPPROTO_TCP {
                errno::set_errno(errno::EPROTONOSUPPORT);
                return -1;
            }
        }
        SOCK_DGRAM => {
            if protocol != 0 && protocol != IPPROTO_UDP {
                errno::set_errno(errno::EPROTONOSUPPORT);
                return -1;
            }
        }
        _ => {
            errno::set_errno(errno::EPROTONOSUPPORT);
            return -1;
        }
    }

    // Determine the initial handle kind.  The handle is 0 (no kernel
    // handle yet) — it will be set on connect/bind.
    let kind = match sock_type {
        SOCK_STREAM => HandleKind::TcpStream,
        _ => HandleKind::UdpSocket,
    };

    // Sockets are bidirectional (O_RDWR).
    let Some(fd) = fdtable::alloc_fd_with_flags(
        kind, 0, crate::fcntl::O_RDWR,
    ) else {
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    // Store socket metadata for later use by connect/bind/listen.
    set_meta(fd, SocketMeta {
        sock_type,
        bound_port: 0,
        peer_addr: 0,
        peer_port: 0,
        local_addr: 0,
        keepalive: false,
        nodelay: false,
        reuseaddr: false,
        rcvbuf: 65536,
        sndbuf: 65536,
        broadcast: false,
        linger_onoff: false,
        linger_secs: 0,
        keepidle: 75,
        keepintvl: 10,
        keepcnt: 9,
        udp_shut_rd: false,
        udp_shut_wr: false,
        rcvtimeo_ms: 0,
        sndtimeo_ms: 0,
    });

    fd
}

// ---------------------------------------------------------------------------
// connect()
// ---------------------------------------------------------------------------

/// Connect a socket to a remote address.
///
/// For TCP (`SOCK_STREAM`): performs the 3-way handshake via
/// `SYS_TCP_CONNECT`.  On success, the fd becomes a connected
/// `TcpStream` with a valid kernel handle.
///
/// For UDP: records the default destination (not yet supported by
/// the kernel — returns ENOSYS).
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `addr` must point to a valid `SockaddrIn` of at least `addrlen` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn connect(fd: i32, addr: *const Sockaddr, addrlen: SocklenT) -> i32 {
    if addr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    if (addrlen as usize) < core::mem::size_of::<SockaddrIn>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    let Some(mut meta) = get_meta(fd) else {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    };

    // SAFETY: addr is non-null and addrlen >= sizeof(SockaddrIn).
    // Use read_unaligned because Sockaddr has weaker alignment than SockaddrIn.
    let sin = unsafe { core::ptr::read_unaligned(addr.cast::<SockaddrIn>()) };

    if sin.sin_family != AF_INET as u16 && meta.sock_type != SOCK_DGRAM {
        // DGRAM connect with AF_UNSPEC (sin_family==0) is valid for disconnect.
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }

    match meta.sock_type {
        SOCK_STREAM => {
            // TCP connect: call SYS_TCP_CONNECT(ip, port).
            // The kernel expects IP in network byte order (which sin_addr
            // already is) and port as a plain u16 value.
            let ip = sin.sin_addr.s_addr;
            let port = u16::from_be(sin.sin_port);

            // If we already have a kernel handle, the socket was already
            // connected (or is a listener).
            if entry.handle != 0 {
                errno::set_errno(errno::EISCONN);
                return -1;
            }

            // Non-blocking connect: pass flag bit 0 if O_NONBLOCK is set.
            let nb_flag: u64 = if fdtable::get_status_flags(fd).unwrap_or(0)
                & crate::fcntl::O_NONBLOCK != 0
            {
                1 // CONNECT_NONBLOCK
            } else {
                0
            };

            let ret = syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), nb_flag);
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }

            // For non-blocking connect, the connection is in progress.
            // Store the handle and return EINPROGRESS (POSIX requirement).
            let in_progress = nb_flag != 0;

            // Update the fd table with the kernel connection handle.
            // Discarding the old entry is safe: handle was 0 (no kernel resource to close).
            let _ = fdtable::install_fd(fd, HandleKind::TcpStream, ret as u64);

            // Store peer address for getpeername().
            set_meta(fd, SocketMeta {
                sock_type: SOCK_STREAM,
                bound_port: meta.bound_port,
                peer_addr: sin.sin_addr.s_addr,
                peer_port: sin.sin_port,
                local_addr: meta.local_addr,
                keepalive: meta.keepalive,
                nodelay: meta.nodelay,
                reuseaddr: meta.reuseaddr,
                rcvbuf: meta.rcvbuf,
                sndbuf: meta.sndbuf,
                broadcast: meta.broadcast,
                linger_onoff: meta.linger_onoff,
                linger_secs: meta.linger_secs,
                keepidle: meta.keepidle,
                keepintvl: meta.keepintvl,
                keepcnt: meta.keepcnt,
                udp_shut_rd: false,
                udp_shut_wr: false,
                rcvtimeo_ms: meta.rcvtimeo_ms,
                sndtimeo_ms: meta.sndtimeo_ms,
            });

            if in_progress {
                // POSIX: non-blocking connect returns -1 / EINPROGRESS
                // to signal the handshake is underway.  The caller uses
                // poll/select POLLOUT to detect completion.
                errno::set_errno(errno::EINPROGRESS);
                return -1;
            }

            0
        }
        SOCK_DGRAM => {
            // UDP "connect" stores the default peer for send() and also
            // sets the kernel-side peer filter so recv/recvfrom only
            // return datagrams from the connected peer.
            // Per POSIX, connect on DGRAM can be called multiple times
            // (to change peer) or with AF_UNSPEC to disconnect.
            let entry = fdtable::get_fd(fd).unwrap_or(fdtable::FdEntry {
                kind: HandleKind::UdpSocket,
                handle: 0,
                flags: 0,
                status_flags: 0,
            });

            if sin.sin_family == 0 {
                // AF_UNSPEC → disconnect (clear stored peer + kernel filter).
                meta.peer_addr = 0;
                meta.peer_port = 0;
                set_meta(fd, meta);
                if entry.handle != 0 {
                    let _ = syscall3(SYS_UDP_CONNECT, entry.handle, 0, 0);
                }
                return 0;
            }

            meta.peer_addr = sin.sin_addr.s_addr;
            meta.peer_port = sin.sin_port;
            set_meta(fd, meta);

            // Tell the kernel to filter incoming datagrams by peer.
            // Port is stored in network byte order in sin_port; kernel
            // expects host byte order.
            if entry.handle != 0 {
                let port_host = u16::from_be(sin.sin_port);
                let _ = syscall3(
                    SYS_UDP_CONNECT,
                    entry.handle,
                    u64::from(sin.sin_addr.s_addr),
                    u64::from(port_host),
                );
            }
            0
        }
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// bind()
// ---------------------------------------------------------------------------

/// Bind a socket to a local address.
///
/// For TCP: stores the bind port and defers the kernel call to
/// `listen()`, because our kernel's `SYS_TCP_BIND` creates a
/// listener immediately.
///
/// For UDP: calls `SYS_UDP_BIND(port)` immediately.
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `addr` must point to a valid `SockaddrIn`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bind(fd: i32, addr: *const Sockaddr, addrlen: SocklenT) -> i32 {
    if addr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    if (addrlen as usize) < core::mem::size_of::<SockaddrIn>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(_entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    let Some(mut meta) = get_meta(fd) else {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    };

    // SAFETY: addr is non-null and addrlen >= sizeof(SockaddrIn).
    // Use read_unaligned because Sockaddr has weaker alignment than SockaddrIn.
    let sin = unsafe { core::ptr::read_unaligned(addr.cast::<SockaddrIn>()) };

    if sin.sin_family != AF_INET as u16 {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }

    let port = u16::from_be(sin.sin_port);

    match meta.sock_type {
        SOCK_STREAM => {
            // For TCP, defer the kernel bind until listen().
            // Just record the port and local address.
            meta.bound_port = sin.sin_port; // store in network order
            meta.local_addr = sin.sin_addr.s_addr;
            set_meta(fd, meta);
            0
        }
        SOCK_DGRAM => {
            // For UDP, bind immediately.
            let ret = syscall1(SYS_UDP_BIND, u64::from(port));
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }

            // Update fd table with the kernel UDP handle.
            // Discarding old entry is safe: handle was 0 (no kernel resource).
            let _ = fdtable::install_fd(fd, HandleKind::UdpSocket, ret as u64);
            meta.bound_port = sin.sin_port;
            meta.local_addr = sin.sin_addr.s_addr;
            set_meta(fd, meta);

            0
        }
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// listen()
// ---------------------------------------------------------------------------

/// Mark a TCP socket as listening for connections.
///
/// Calls `SYS_TCP_BIND(port)` which both binds and starts listening.
/// The `backlog` parameter is accepted but not forwarded (our kernel
/// uses a fixed backlog).
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn listen(fd: i32, _backlog: i32) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    let Some(meta) = get_meta(fd) else {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    };

    if meta.sock_type != SOCK_STREAM {
        errno::set_errno(errno::EOPNOTSUPP);
        return -1;
    }

    // Must have been bound first.
    if meta.bound_port == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Don't re-listen on an already-listening socket.
    if entry.kind == HandleKind::TcpListener && entry.handle != 0 {
        return 0;
    }

    let port = u16::from_be(meta.bound_port);
    let ret = syscall1(SYS_TCP_BIND, u64::from(port));
    if ret < 0 {
        errno::set_errno(translate_net_error(ret));
        return -1;
    }

    // Change the fd to a TcpListener with the kernel listener handle.
    // Discarding old entry is safe: handle was 0 (unconnected socket).
    let _ = fdtable::install_fd(fd, HandleKind::TcpListener, ret as u64);

    0
}

// ---------------------------------------------------------------------------
// accept()
// ---------------------------------------------------------------------------

/// Accept a connection on a listening socket.
///
/// Calls `SYS_TCP_ACCEPT(listener_handle)` and returns a new fd for
/// the accepted connection.  If `addr` is non-null, the remote address
/// is written there (currently zeroed — the kernel doesn't return peer
/// address info from accept).
///
/// Returns the new fd on success, -1 on error.
///
/// # Safety
///
/// If `addr` is non-null, it must point to a buffer of at least
/// `*addrlen` bytes, and `addrlen` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accept(
    fd: i32,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    if entry.kind != HandleKind::TcpListener || entry.handle == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // If the listener socket has O_NONBLOCK, use non-blocking accept.
    let nb_flag: u64 = if fdtable::get_status_flags(fd).unwrap_or(0)
        & crate::fcntl::O_NONBLOCK != 0
    {
        1 // ACCEPT_NONBLOCK
    } else {
        0
    };

    let ret = syscall2(SYS_TCP_ACCEPT, entry.handle, nb_flag);
    if ret < 0 {
        errno::set_errno(translate_net_error(ret));
        return -1;
    }

    // Allocate a new fd for the accepted connection.
    // Accepted sockets are bidirectional (O_RDWR).
    let conn_handle = ret as u64;
    let Some(new_fd) = fdtable::alloc_fd_with_flags(
        HandleKind::TcpStream, conn_handle, crate::fcntl::O_RDWR,
    ) else {
        // Close the connection we just accepted — no fd available.
        let _ = syscall1(SYS_TCP_CLOSE, conn_handle);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    // Query peer address from the kernel.
    let mut peer_buf = [0u8; 6]; // 4 bytes IP + 2 bytes port (network order)
    let peer_ret = syscall2(
        SYS_TCP_PEER_ADDR,
        conn_handle,
        peer_buf.as_mut_ptr() as u64,
    );
    let (peer_ip_nbo, peer_port_nbo) = if peer_ret == 0 {
        let ip = u32::from_ne_bytes([peer_buf[0], peer_buf[1], peer_buf[2], peer_buf[3]]);
        let port = u16::from_be_bytes([peer_buf[4], peer_buf[5]]);
        (ip, port.to_be()) // Store in network byte order for sockaddr_in
    } else {
        (0u32, 0u16)
    };

    // Store metadata for the new connected socket.
    let listener_meta = get_meta(fd);
    set_meta(new_fd, SocketMeta {
        sock_type: SOCK_STREAM,
        bound_port: listener_meta.map_or(0, |m| m.bound_port),
        peer_addr: peer_ip_nbo,
        peer_port: peer_port_nbo,
        local_addr: listener_meta.map_or(0, |m| m.local_addr),
        keepalive: false,
        nodelay: false,
        reuseaddr: false,
        rcvbuf: 65536,
        sndbuf: 65536,
        broadcast: false,
        linger_onoff: false,
        linger_secs: 0,
        keepidle: 75,
        keepintvl: 10,
        keepcnt: 9,
        udp_shut_rd: false,
        udp_shut_wr: false,
        rcvtimeo_ms: 0,
        sndtimeo_ms: 0,
    });

    // Fill in the peer address if requested.
    if !addr.is_null() && !addrlen.is_null() {
        let sin = SockaddrIn {
            sin_family: AF_INET as u16,
            sin_port: peer_port_nbo,
            sin_addr: InAddr { s_addr: peer_ip_nbo },
            sin_zero: [0u8; 8],
        };
        // SAFETY: caller guarantees addr/addrlen validity.
        unsafe {
            let alen = *addrlen as usize;
            let copy_len = alen.min(core::mem::size_of::<SockaddrIn>());
            core::ptr::copy_nonoverlapping(
                (&raw const sin).cast::<u8>(),
                addr.cast::<u8>(),
                copy_len,
            );
            *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
        }
    }

    new_fd
}

/// Accept a connection with flags (Linux extension).
///
/// Like `accept`, but `flags` may include `SOCK_NONBLOCK` and/or
/// `SOCK_CLOEXEC` to set those properties on the returned fd
/// atomically.
///
/// # Safety
///
/// Same requirements as `accept`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn accept4(
    fd: i32,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
    flags: i32,
) -> i32 {
    let new_fd = unsafe { accept(fd, addr, addrlen) };
    if new_fd < 0 {
        return new_fd;
    }

    // Apply SOCK_NONBLOCK: set O_NONBLOCK in the fd's status flags.
    if flags & SOCK_NONBLOCK != 0 {
        let _ = fdtable::set_status_flags(
            new_fd,
            fdtable::get_status_flags(new_fd).unwrap_or(0) | crate::fcntl::O_NONBLOCK,
        );
    }

    // Apply SOCK_CLOEXEC: set FD_CLOEXEC in the fd's per-fd flags.
    if flags & SOCK_CLOEXEC != 0 {
        let _ = fdtable::set_fd_flags(new_fd, 1); // FD_CLOEXEC = 1
    }

    new_fd
}

// ---------------------------------------------------------------------------
// send() / recv()
// ---------------------------------------------------------------------------

/// Send data on a connected socket.
///
/// For TCP: calls `SYS_TCP_SEND(handle, buf, len)`.
///
/// Returns the number of bytes sent, or -1 on error.
///
/// # Safety
///
/// `buf` must be valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn send(
    fd: i32,
    buf: *const u8,
    len: usize,
    _flags: i32,
) -> isize {
    if buf.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    match entry.kind {
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }
            let ret = syscall3(SYS_TCP_SEND, entry.handle, buf as u64, len as u64);
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }
            ret as isize
        }
        HandleKind::UdpSocket => {
            // send() on UDP requires a prior connect() to set the peer.
            let Some(meta) = get_meta(fd) else {
                errno::set_errno(errno::ENOTSOCK);
                return -1;
            };
            // Enforce SHUT_WR: return EPIPE after shutdown(SHUT_WR).
            if meta.udp_shut_wr {
                errno::set_errno(errno::EPIPE);
                return -1;
            }
            if meta.peer_addr == 0 && meta.peer_port == 0 {
                errno::set_errno(errno::EDESTADDRREQ);
                return -1;
            }
            let port = u16::from_be(meta.peer_port);
            let ret = syscall5(
                SYS_UDP_SEND,
                entry.handle,
                u64::from(meta.peer_addr),
                u64::from(port),
                buf as u64,
                len as u64,
            );
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }
            len as isize
        }
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            -1
        }
    }
}

/// Receive data from a connected socket.
///
/// For TCP: calls `SYS_TCP_RECV(handle, buf, len)`.
///
/// Returns the number of bytes received (0 = peer closed), or -1 on error.
///
/// # Safety
///
/// `buf` must be valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recv(
    fd: i32,
    buf: *mut u8,
    len: usize,
    flags: i32,
) -> isize {
    if buf.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // Build kernel flags from POSIX MSG_* constants.
    // MSG_PEEK (0x02), MSG_TRUNC (0x20), and MSG_DONTWAIT (0x40) are
    // passed through directly since we use matching numeric values.
    let kern_flags = (flags as u32)
        & (MSG_PEEK as u32 | MSG_TRUNC as u32 | MSG_DONTWAIT as u32);

    // If the socket has O_NONBLOCK set, add MSG_DONTWAIT automatically.
    let kern_flags = if fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0 {
        kern_flags | (MSG_DONTWAIT as u32)
    } else {
        kern_flags
    };

    match entry.kind {
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }
            let ret = syscall4(
                SYS_TCP_RECV, entry.handle,
                buf as u64, len as u64, kern_flags as u64,
            );
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }
            ret as isize
        }
        HandleKind::UdpSocket => {
            if entry.handle == 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // Enforce SHUT_RD: return 0 (EOF-like) after shutdown(SHUT_RD).
            if get_meta(fd).is_some_and(|m| m.udp_shut_rd) {
                return 0;
            }
            let mut src_info = [0u8; 6];
            let ret = syscall5(
                SYS_UDP_RECV,
                entry.handle,
                buf as u64,
                len as u64,
                src_info.as_mut_ptr() as u64,
                u64::from(kern_flags),
            );
            if ret >= 0 {
                return ret as isize;
            }
            // WouldBlock: if non-blocking or MSG_DONTWAIT, return EAGAIN.
            let err = translate_net_error(ret);
            if err == errno::EAGAIN || err == errno::EWOULDBLOCK {
                let is_nb = (kern_flags & MSG_DONTWAIT as u32) != 0;
                if is_nb {
                    errno::set_errno(errno::EAGAIN);
                    return -1;
                }
                // Blocking mode: poll-wait with SO_RCVTIMEO.
                let timeout_ms = get_meta(fd).map_or(0u64, |m| m.rcvtimeo_ms);
                let waited = udp_recv_wait(
                    entry.handle, buf, len, &mut src_info, kern_flags, timeout_ms,
                );
                return waited;
            }
            errno::set_errno(err);
            -1
        }
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            -1
        }
    }
}

/// Poll-wait for a UDP datagram with timeout (SO_RCVTIMEO support).
///
/// Retries `SYS_UDP_RECV` in a loop with 10ms sleeps until data arrives
/// or the timeout expires.  `timeout_ms == 0` means wait indefinitely.
/// Returns bytes received or -1 (with errno set).
fn udp_recv_wait(
    handle: u64,
    buf: *mut u8,
    len: usize,
    src_info: &mut [u8; 6],
    kern_flags: u32,
    timeout_ms: u64,
) -> isize {
    const POLL_NS: u64 = 10_000_000; // 10ms

    let deadline = if timeout_ms > 0 {
        let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
        now.saturating_add(timeout_ms.saturating_mul(1_000_000))
    } else {
        u64::MAX // No deadline — wait indefinitely.
    };

    loop {
        let _ = syscall1(SYS_SLEEP, POLL_NS);

        let ret = syscall5(
            SYS_UDP_RECV,
            handle,
            buf as u64,
            len as u64,
            src_info.as_mut_ptr() as u64,
            u64::from(kern_flags),
        );
        if ret >= 0 {
            return ret as isize;
        }
        // Still no data — check timeout.
        if deadline != u64::MAX {
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            if now >= deadline {
                errno::set_errno(errno::EAGAIN);
                return -1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// sendto() / recvfrom()
// ---------------------------------------------------------------------------

/// Send a datagram to a specific destination.
///
/// For UDP: calls `SYS_UDP_SEND(handle, ip, port, buf, len)`.
///
/// Returns the number of bytes sent, or -1 on error.
///
/// # Safety
///
/// `buf` must be valid for `len` bytes.  `dest_addr` must point to
/// a valid `SockaddrIn` of at least `addrlen` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sendto(
    fd: i32,
    buf: *const u8,
    len: usize,
    _flags: i32,
    dest_addr: *const Sockaddr,
    addrlen: SocklenT,
) -> isize {
    if buf.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // TCP sendto: works like send() — destination addr is ignored
    // (connection-oriented protocol already knows the peer).
    if entry.kind == HandleKind::TcpStream {
        if entry.handle == 0 {
            errno::set_errno(errno::ENOTCONN);
            return -1;
        }
        let ret = syscall3(SYS_TCP_SEND, entry.handle, buf as u64, len as u64);
        if ret < 0 {
            errno::set_errno(translate_net_error(ret));
            return -1;
        }
        return ret as isize;
    }

    if entry.kind != HandleKind::UdpSocket {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    }

    // Enforce SHUT_WR: return EPIPE after shutdown(SHUT_WR).
    if get_meta(fd).is_some_and(|m| m.udp_shut_wr) {
        errno::set_errno(errno::EPIPE);
        return -1;
    }

    // Determine the destination IP and port.
    // If dest_addr is NULL, use the stored peer from connect() (POSIX:
    // sendto with NULL addr on a connected DGRAM socket sends to the
    // connected peer).
    let (ip, port): (u32, u16);
    if dest_addr.is_null() {
        let Some(meta) = get_meta(fd) else {
            errno::set_errno(errno::ENOTSOCK);
            return -1;
        };
        if meta.peer_addr == 0 && meta.peer_port == 0 {
            errno::set_errno(errno::EDESTADDRREQ);
            return -1;
        }
        ip = meta.peer_addr;
        port = u16::from_be(meta.peer_port);
    } else {
        if (addrlen as usize) < core::mem::size_of::<SockaddrIn>() {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        // SAFETY: dest_addr is non-null, addrlen checked.
        let sin = unsafe { core::ptr::read_unaligned(dest_addr.cast::<SockaddrIn>()) };
        if sin.sin_family != AF_INET as u16 {
            errno::set_errno(errno::EAFNOSUPPORT);
            return -1;
        }
        ip = sin.sin_addr.s_addr;
        port = u16::from_be(sin.sin_port);
    }

    // SYS_UDP_SEND: arg0=handle, arg1=ip, arg2=port, arg3=buf, arg4=len.
    // If handle is 0 (unbound), kernel creates an ephemeral port.
    let ret = syscall5(
        SYS_UDP_SEND,
        entry.handle,
        u64::from(ip),
        u64::from(port),
        buf as u64,
        len as u64,
    );
    if ret < 0 {
        errno::set_errno(translate_net_error(ret));
        return -1;
    }
    // UDP send returns 0 on success; we return the send length.
    len as isize
}

/// Receive a datagram and its source address.
///
/// For UDP: calls `SYS_UDP_RECV(handle, buf, len, addr_out)`.
///
/// Returns the number of bytes received, or -1 on error.
///
/// # Safety
///
/// `buf` must be valid for `len` bytes.  If `src_addr` is non-null,
/// it must point to a buffer of at least `*addrlen` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recvfrom(
    fd: i32,
    buf: *mut u8,
    len: usize,
    flags: i32,
    src_addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
) -> isize {
    if buf.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // Build kernel flags (MSG_PEEK=0x02, MSG_TRUNC=0x20, MSG_DONTWAIT=0x40).
    let kern_flags = {
        let f = (flags as u32)
            & (MSG_PEEK as u32 | MSG_TRUNC as u32 | MSG_DONTWAIT as u32);
        if fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0 {
            f | (MSG_DONTWAIT as u32)
        } else {
            f
        }
    };

    match entry.kind {
        HandleKind::TcpStream => {
            // TCP recvfrom works like recv but fills in peer address.
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }

            let ret = syscall4(
                SYS_TCP_RECV, entry.handle,
                buf as u64, len as u64, kern_flags as u64,
            );
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }

            // Fill in peer address if requested.
            if !src_addr.is_null() && !addrlen.is_null() {
                unsafe {
                    let available = *addrlen as usize;
                    if available >= core::mem::size_of::<SockaddrIn>() {
                        let meta = get_meta(fd);
                        let (ip, port) = meta.map_or((0u32, 0u16), |m| {
                            (m.peer_addr, u16::from_be(m.peer_port))
                        });
                        let sa = SockaddrIn {
                            sin_family: AF_INET as u16,
                            sin_port: port.to_be(),
                            sin_addr: InAddr { s_addr: ip },
                            sin_zero: [0u8; 8],
                        };
                        core::ptr::write_unaligned(
                            src_addr.cast::<SockaddrIn>(), sa,
                        );
                    }
                    *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
                }
            }

            ret as isize
        }

        HandleKind::UdpSocket => {
            if entry.handle == 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // Enforce SHUT_RD: return 0 (EOF-like) after shutdown(SHUT_RD).
            if get_meta(fd).is_some_and(|m| m.udp_shut_rd) {
                return 0;
            }

            // The kernel returns the source address in a 6-byte buffer:
            // bytes 0-3 = IPv4 address (network byte order)
            // bytes 4-5 = port (little-endian u16)
            let mut src_info = [0u8; 6];

            let ret = syscall5(
                SYS_UDP_RECV,
                entry.handle,
                buf as u64,
                len as u64,
                src_info.as_mut_ptr() as u64,
                u64::from(kern_flags),
            );

            let ret = if ret < 0 {
                let err = translate_net_error(ret);
                if err == errno::EAGAIN || err == errno::EWOULDBLOCK {
                    let is_nb = (kern_flags & MSG_DONTWAIT as u32) != 0;
                    if is_nb {
                        errno::set_errno(errno::EAGAIN);
                        return -1;
                    }
                    // Blocking mode: poll-wait with SO_RCVTIMEO.
                    let timeout_ms = get_meta(fd).map_or(0u64, |m| m.rcvtimeo_ms);
                    let waited = udp_recv_wait(
                        entry.handle, buf, len, &mut src_info, kern_flags, timeout_ms,
                    );
                    if waited < 0 {
                        return waited;
                    }
                    waited as i64
                } else {
                    errno::set_errno(err);
                    return -1;
                }
            } else {
                ret
            };

            // Fill in the source address if requested.
            if !src_addr.is_null() && !addrlen.is_null() {
                unsafe {
                    let available = *addrlen as usize;
                    if available >= core::mem::size_of::<SockaddrIn>() {
                        let ip = u32::from_ne_bytes([
                            src_info[0], src_info[1], src_info[2], src_info[3],
                        ]);
                        let port_le = u16::from_le_bytes([src_info[4], src_info[5]]);
                        let sa = SockaddrIn {
                            sin_family: AF_INET as u16,
                            sin_port: port_le.to_be(),
                            sin_addr: InAddr { s_addr: ip },
                            sin_zero: [0u8; 8],
                        };
                        core::ptr::write_unaligned(
                            src_addr.cast::<SockaddrIn>(), sa,
                        );
                    }
                    *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
                }
            }

            ret as isize
        }

        _ => {
            errno::set_errno(errno::ENOTSOCK);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// shutdown()
// ---------------------------------------------------------------------------

/// Shut down part or all of a full-duplex connection.
///
/// Our kernel doesn't support half-shutdown — we close the whole socket.
/// `SHUT_RDWR` calls `SYS_TCP_CLOSE`.  `SHUT_RD` and `SHUT_WR` are
/// accepted but effectively no-ops (the connection stays open until
/// fully closed).
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn shutdown(fd: i32, how: i32) -> i32 {
    if !(SHUT_RD..=SHUT_RDWR).contains(&how) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    match entry.kind {
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }

            // Delegate to the kernel for proper half-close semantics.
            // SYS_TCP_SHUTDOWN(handle, how) sends FIN for SHUT_WR,
            // discards rx data for SHUT_RD, or both for SHUT_RDWR.
            let ret = syscall2(SYS_TCP_SHUTDOWN, entry.handle, how as u64);
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }

            // For SHUT_RDWR, also mark the fd as disconnected so
            // subsequent send/recv return ENOTCONN at the POSIX layer.
            if how == SHUT_RDWR {
                let _ = fdtable::install_fd(fd, HandleKind::TcpStream, 0);
            }

            0
        }
        HandleKind::UdpSocket => {
            // UDP shutdown is a local operation — it prevents further
            // send/recv on the specified half without a kernel call.
            // We don't have kernel-side shutdown for UDP; this is
            // tracked in SocketMeta and enforced in send/recv.
            if let Some(mut meta) = get_meta(fd) {
                match how {
                    SHUT_RD => meta.udp_shut_rd = true,
                    SHUT_WR => meta.udp_shut_wr = true,
                    SHUT_RDWR => {
                        meta.udp_shut_rd = true;
                        meta.udp_shut_wr = true;
                    }
                    _ => {}
                }
                set_meta(fd, meta);
            }
            0
        }
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// setsockopt() / getsockopt() stubs
// ---------------------------------------------------------------------------

/// Set a socket option.
///
/// Supports `SO_REUSEADDR`, `SO_KEEPALIVE` (SOL_SOCKET level),
/// `TCP_NODELAY` (SOL_TCP level), and `IP_ADD_MEMBERSHIP` /
/// `IP_DROP_MEMBERSHIP` (SOL_IP / IPPROTO_IP level for multicast).
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn setsockopt(
    fd: i32,
    level: i32,
    optname: i32,
    optval: *const u8,
    optlen: SocklenT,
) -> i32 {
    // Validate the fd is a socket.
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    match entry.kind {
        HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {}
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            return -1;
        }
    }

    // Handle IP-level multicast options (require IpMreq struct, not int).
    if (level == SOL_IP || level == IPPROTO_IP)
        && (optname == IP_ADD_MEMBERSHIP || optname == IP_DROP_MEMBERSHIP)
    {
        return setsockopt_multicast(fd, &entry, optname, optval, optlen);
    }

    // Read the integer option value.
    let val = if !optval.is_null() && optlen as usize >= 4 {
        // SAFETY: optval points to at least 4 readable bytes.
        i32::from_ne_bytes(unsafe {
            [*optval, *optval.add(1), *optval.add(2), *optval.add(3)]
        })
    } else {
        0
    };

    if let Some(mut meta) = get_meta(fd) {
        match (level, optname) {
            (SOL_SOCKET, SO_REUSEADDR) => { meta.reuseaddr = val != 0; }
            (SOL_SOCKET, SO_KEEPALIVE) => {
                meta.keepalive = val != 0;
                // Wire through to kernel for TCP streams with active handles.
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let _ = syscall2(
                        SYS_TCP_SET_KEEPALIVE,
                        entry.handle,
                        u64::from(val != 0),
                    );
                }
            }
            (SOL_SOCKET, SO_RCVBUF) => { meta.rcvbuf = val.max(1); }
            (SOL_SOCKET, SO_SNDBUF) => { meta.sndbuf = val.max(1); }
            (SOL_SOCKET, SO_BROADCAST) => { meta.broadcast = val != 0; }
            (SOL_SOCKET, SO_LINGER) => {
                // BSD struct linger { int l_onoff; int l_linger; } = 8 bytes.
                if !optval.is_null() && optlen as usize >= 8 {
                    let l_onoff = i32::from_ne_bytes(unsafe {
                        [*optval, *optval.add(1), *optval.add(2), *optval.add(3)]
                    });
                    let l_linger = i32::from_ne_bytes(unsafe {
                        [*optval.add(4), *optval.add(5), *optval.add(6), *optval.add(7)]
                    });
                    meta.linger_onoff = l_onoff != 0;
                    meta.linger_secs = l_linger;
                } else {
                    // Fallback: treat as simple int (non-standard but graceful).
                    meta.linger_onoff = val != 0;
                    meta.linger_secs = val;
                }
            }
            (SOL_SOCKET, SO_REUSEPORT) => {
                // Accept silently — reuseport is not meaningful with our
                // single-socket-per-port kernel model.
            }
            (SOL_SOCKET, SO_RCVTIMEO) => {
                // Timeout is a timeval struct: {tv_sec, tv_usec}.
                // If optlen is >= 16 (sizeof timeval), parse as timeval.
                // Otherwise treat val as milliseconds for compatibility.
                let ms = if optlen as usize >= 16 {
                    let tv_sec = unsafe {
                        core::ptr::read_unaligned(optval.cast::<i64>())
                    };
                    let tv_usec = unsafe {
                        core::ptr::read_unaligned(optval.add(8).cast::<i64>())
                    };
                    (tv_sec.max(0) as u64).saturating_mul(1000)
                        .saturating_add((tv_usec.max(0) as u64) / 1000)
                } else {
                    val.max(0) as u64
                };
                meta.rcvtimeo_ms = ms;
            }
            (SOL_SOCKET, SO_SNDTIMEO) => {
                let ms = if optlen as usize >= 16 {
                    let tv_sec = unsafe {
                        core::ptr::read_unaligned(optval.cast::<i64>())
                    };
                    let tv_usec = unsafe {
                        core::ptr::read_unaligned(optval.add(8).cast::<i64>())
                    };
                    (tv_sec.max(0) as u64).saturating_mul(1000)
                        .saturating_add((tv_usec.max(0) as u64) / 1000)
                } else {
                    val.max(0) as u64
                };
                meta.sndtimeo_ms = ms;
            }
            (SOL_TCP, TCP_NODELAY) => {
                meta.nodelay = val != 0;
                // Wire through to kernel for TCP streams with active handles.
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let _ = syscall2(
                        SYS_TCP_SET_NODELAY,
                        entry.handle,
                        u64::from(val != 0),
                    );
                }
            }
            (SOL_TCP, TCP_KEEPIDLE) => {
                meta.keepidle = val.max(1);
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let _ = syscall4(
                        SYS_TCP_SET_KEEPALIVE_PARAMS,
                        entry.handle,
                        meta.keepidle as u64,
                        meta.keepintvl as u64,
                        meta.keepcnt as u64,
                    );
                }
            }
            (SOL_TCP, TCP_KEEPINTVL) => {
                meta.keepintvl = val.max(1);
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let _ = syscall4(
                        SYS_TCP_SET_KEEPALIVE_PARAMS,
                        entry.handle,
                        meta.keepidle as u64,
                        meta.keepintvl as u64,
                        meta.keepcnt as u64,
                    );
                }
            }
            (SOL_TCP, TCP_KEEPCNT) => {
                meta.keepcnt = val.max(1);
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let _ = syscall4(
                        SYS_TCP_SET_KEEPALIVE_PARAMS,
                        entry.handle,
                        meta.keepidle as u64,
                        meta.keepintvl as u64,
                        meta.keepcnt as u64,
                    );
                }
            }
            (SOL_IP, IP_MULTICAST_TTL | IP_MULTICAST_LOOP) => {
                // Accept silently — no kernel support for these yet,
                // but programs often set them alongside IP_ADD_MEMBERSHIP.
                // Note: IPPROTO_IP == SOL_IP == 0, so this arm covers both.
            }
            _ => {
                // Accept unknown options silently — many programs set
                // options we don't implement and don't check the result.
            }
        }
        set_meta(fd, meta);
    }

    0
}

/// Handle `IP_ADD_MEMBERSHIP` / `IP_DROP_MEMBERSHIP` setsockopt calls.
///
/// Translates to `SYS_UDP_MCAST_JOIN` / `SYS_UDP_MCAST_LEAVE` syscalls.
fn setsockopt_multicast(
    _fd: i32,
    entry: &fdtable::FdEntry,
    optname: i32,
    optval: *const u8,
    optlen: SocklenT,
) -> i32 {
    // Must be a UDP socket.
    if entry.kind != HandleKind::UdpSocket {
        errno::set_errno(errno::ENOPROTOOPT);
        return -1;
    }

    // Validate the option value is an IpMreq.
    if optval.is_null() || (optlen as usize) < core::mem::size_of::<IpMreq>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: optval is non-null and points to at least sizeof(IpMreq) bytes.
    let mreq = unsafe { &*(optval.cast::<IpMreq>()) };
    let group_addr = mreq.imr_multiaddr.s_addr; // Network byte order u32.

    let syscall_nr = if optname == IP_ADD_MEMBERSHIP {
        SYS_UDP_MCAST_JOIN
    } else {
        SYS_UDP_MCAST_LEAVE
    };

    let ret = syscall2(syscall_nr, entry.handle, group_addr as u64);
    if ret < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    0
}

/// Get a socket option.
///
/// Returns the current value of socket options stored in the metadata
/// table.  Supports `SO_TYPE`, `SO_ERROR`, `SO_REUSEADDR`, `SO_KEEPALIVE`
/// (SOL_SOCKET) and `TCP_NODELAY` (SOL_TCP).
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getsockopt(
    fd: i32,
    level: i32,
    optname: i32,
    optval: *mut u8,
    optlen: *mut SocklenT,
) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    match entry.kind {
        HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {}
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            return -1;
        }
    }

    if optval.is_null() || optlen.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: caller guarantees optval/optlen validity.
    unsafe {
        let available = *optlen as usize;
        if available < 4 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }

        let meta = get_meta(fd);

        // Return the stored value based on level + option.
        let val: i32 = match (level, optname) {
            (SOL_SOCKET, SO_TYPE) => meta.map_or(0, |m| m.sock_type),
            (SOL_SOCKET, SO_ERROR) => {
                // For TCP sockets, query kernel poll status to detect
                // errors (e.g., failed non-blocking connect).
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let status = crate::syscall::syscall1(
                        crate::syscall::SYS_TCP_POLL_STATUS, entry.handle,
                    ) as u16;
                    // POLL_ERROR (0x08) or POLL_HANGUP (0x10) on a socket
                    // that was never established → connection refused.
                    // Note: POLLHUP on an established connection is normal
                    // EOF, not an error.
                    if (status & 0x0008) != 0 {
                        errno::ECONNREFUSED
                    } else if (status & 0x0010) != 0 && (status & 0x0004) == 0 {
                        // Hangup without writable → connection was lost.
                        errno::ECONNRESET
                    } else {
                        0 // No pending error.
                    }
                } else {
                    0
                }
            }
            (SOL_SOCKET, SO_REUSEADDR) => meta.map_or(0, |m| i32::from(m.reuseaddr)),
            (SOL_SOCKET, SO_KEEPALIVE) => meta.map_or(0, |m| i32::from(m.keepalive)),
            (SOL_SOCKET, SO_RCVBUF) => meta.map_or(65536, |m| m.rcvbuf),
            (SOL_SOCKET, SO_SNDBUF) => meta.map_or(65536, |m| m.sndbuf),
            (SOL_SOCKET, SO_BROADCAST) => meta.map_or(0, |m| i32::from(m.broadcast)),
            (SOL_SOCKET, SO_REUSEPORT) => 0,  // Port reuse not supported.
            (SOL_SOCKET, SO_LINGER) => {
                // Return struct linger { int l_onoff; int l_linger; } = 8 bytes.
                if available >= 8 {
                    let (onoff, secs) = meta.map_or((0i32, 0i32), |m| {
                        (i32::from(m.linger_onoff), m.linger_secs)
                    });
                    core::ptr::copy_nonoverlapping(
                        (&raw const onoff).cast::<u8>(), optval, 4,
                    );
                    core::ptr::copy_nonoverlapping(
                        (&raw const secs).cast::<u8>(), optval.add(4), 4,
                    );
                    *optlen = 8;
                    return 0;
                }
                // Fall back to just returning l_onoff as int.
                meta.map_or(0, |m| i32::from(m.linger_onoff))
            }
            (SOL_SOCKET, SO_RCVTIMEO) => {
                // Return as timeval if buffer is big enough.
                let ms = meta.map_or(0u64, |m| m.rcvtimeo_ms);
                if available >= 16 {
                    let tv_sec = (ms / 1000) as i64;
                    let tv_usec = ((ms % 1000) * 1000) as i64;
                    core::ptr::copy_nonoverlapping(
                        (&raw const tv_sec).cast::<u8>(), optval, 8,
                    );
                    core::ptr::copy_nonoverlapping(
                        (&raw const tv_usec).cast::<u8>(), optval.add(8), 8,
                    );
                    *optlen = 16;
                    return 0;
                }
                // Fallback: return as seconds (integer).
                (ms / 1000) as i32
            }
            (SOL_SOCKET, SO_SNDTIMEO) => {
                let ms = meta.map_or(0u64, |m| m.sndtimeo_ms);
                if available >= 16 {
                    let tv_sec = (ms / 1000) as i64;
                    let tv_usec = ((ms % 1000) * 1000) as i64;
                    core::ptr::copy_nonoverlapping(
                        (&raw const tv_sec).cast::<u8>(), optval, 8,
                    );
                    core::ptr::copy_nonoverlapping(
                        (&raw const tv_usec).cast::<u8>(), optval.add(8), 8,
                    );
                    *optlen = 16;
                    return 0;
                }
                (ms / 1000) as i32
            }
            (SOL_SOCKET, SO_ACCEPTCONN) => i32::from(entry.kind == HandleKind::TcpListener),
            (SOL_SOCKET, SO_DOMAIN) => AF_INET,
            (SOL_SOCKET, SO_PROTOCOL) => match entry.kind {
                HandleKind::TcpStream | HandleKind::TcpListener => IPPROTO_TCP,
                HandleKind::UdpSocket => IPPROTO_UDP,
                _ => 0,
            },
            (SOL_TCP, TCP_NODELAY) => meta.map_or(0, |m| i32::from(m.nodelay)),
            (SOL_TCP, TCP_KEEPIDLE) => meta.map_or(75, |m| m.keepidle),
            (SOL_TCP, TCP_KEEPINTVL) => meta.map_or(10, |m| m.keepintvl),
            (SOL_TCP, TCP_KEEPCNT) => meta.map_or(9, |m| m.keepcnt),
            (SOL_TCP, TCP_MAXSEG) => 1460,   // Default MSS (Ethernet MTU - headers).
            (SOL_TCP, TCP_CORK) => 0,        // Not corked.
            (SOL_TCP, TCP_USER_TIMEOUT) => 0, // No user timeout.
            (SOL_TCP, TCP_INFO) => {
                // TCP_INFO returns a 48-byte struct with connection details.
                // Query the kernel directly — this is a variable-size option.
                if available < 48 {
                    errno::set_errno(errno::EINVAL);
                    return -1;
                }
                if entry.handle == 0 {
                    errno::set_errno(errno::ENOTCONN);
                    return -1;
                }
                let ret = syscall3(
                    SYS_TCP_INFO, entry.handle,
                    optval as u64, available as u64,
                );
                if ret < 0 {
                    errno::set_errno(translate_net_error(ret));
                    return -1;
                }
                *optlen = 48;
                return 0;
            }
            _ => {
                errno::set_errno(errno::ENOPROTOOPT);
                return -1;
            }
        };

        core::ptr::copy_nonoverlapping(
            (&raw const val).cast::<u8>(),
            optval,
            4,
        );
        *optlen = 4;
    }

    0
}

// ---------------------------------------------------------------------------
// getpeername() / getsockname() stubs
// ---------------------------------------------------------------------------

/// Get the name of the peer socket (remote address).
///
/// Returns the remote IP and port that this socket is connected to.
/// First checks the cached metadata (set on connect/accept).  If the
/// metadata has no peer address (e.g., dup'd fd), falls back to
/// querying the kernel via `SYS_TCP_PEER_ADDR`.
///
/// # Safety
///
/// `addr` must point to writable memory of at least `*addrlen` bytes.
/// `addrlen` must be a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpeername(
    fd: i32,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    if addr.is_null() || addrlen.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(meta) = get_meta(fd) else {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    };

    let (peer_ip, peer_port) = if meta.peer_addr != 0 || meta.peer_port != 0 {
        // Cached metadata available.
        (meta.peer_addr, meta.peer_port)
    } else if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
        // Fall back to kernel query for TCP sockets.
        let mut buf = [0u8; 6];
        let ret = syscall2(
            SYS_TCP_PEER_ADDR,
            entry.handle,
            buf.as_mut_ptr() as u64,
        );
        if ret == 0 {
            let ip = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
            let port = u16::from_be_bytes([buf[4], buf[5]]);
            (ip, port.to_be())
        } else {
            errno::set_errno(errno::ENOTCONN);
            return -1;
        }
    } else {
        errno::set_errno(errno::ENOTCONN);
        return -1;
    };

    // Build a SockaddrIn with the peer address.
    let sin = SockaddrIn {
        sin_family: AF_INET as u16,
        sin_port: peer_port,
        sin_addr: InAddr { s_addr: peer_ip },
        sin_zero: [0u8; 8],
    };

    // SAFETY: caller guarantees addr/addrlen validity.
    unsafe {
        let available = *addrlen as usize;
        let copy_len = available.min(core::mem::size_of::<SockaddrIn>());
        core::ptr::copy_nonoverlapping(
            (&raw const sin).cast::<u8>(),
            addr.cast::<u8>(),
            copy_len,
        );
        #[allow(clippy::cast_possible_truncation)]
        {
            *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
        }
    }

    0
}

/// Get the local name of a socket (bound address).
///
/// Returns the local IP and port this socket is bound to.
/// For sockets bound via `bind()`, returns the bound address.
/// For unbound sockets, returns INADDR_ANY (0.0.0.0) with port 0.
///
/// # Safety
///
/// `addr` must point to writable memory of at least `*addrlen` bytes.
/// `addrlen` must be a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getsockname(
    fd: i32,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
) -> i32 {
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    if addr.is_null() || addrlen.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(meta) = get_meta(fd) else {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    };

    let sin = SockaddrIn {
        sin_family: AF_INET as u16,
        sin_port: meta.bound_port,
        sin_addr: InAddr { s_addr: meta.local_addr },
        sin_zero: [0u8; 8],
    };

    // SAFETY: caller guarantees addr/addrlen validity.
    unsafe {
        let available = *addrlen as usize;
        let copy_len = available.min(core::mem::size_of::<SockaddrIn>());
        core::ptr::copy_nonoverlapping(
            (&raw const sin).cast::<u8>(),
            addr.cast::<u8>(),
            copy_len,
        );
        #[allow(clippy::cast_possible_truncation)]
        {
            *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
        }
    }

    0
}

// ---------------------------------------------------------------------------
// gethostbyname() — simplified DNS resolution
// ---------------------------------------------------------------------------

/// Host entry for DNS results.
#[repr(C)]
pub struct Hostent {
    /// Official hostname.
    pub h_name: *const u8,
    /// Alias list (NULL-terminated).
    pub h_aliases: *const *const u8,
    /// Address type (`AF_INET`).
    pub h_addrtype: i32,
    /// Address length (4 for IPv4).
    pub h_length: i32,
    /// Address list (NULL-terminated, each points to 4-byte IPv4 addr).
    pub h_addr_list: *const *const u8,
}

/// Static storage for gethostbyname result (not thread-safe, per POSIX).
static mut HOSTENT_NAME: [u8; 256] = [0u8; 256];
static mut HOSTENT_ADDR: [u8; 4] = [0u8; 4];
static mut HOSTENT_ADDR_PTR: [*const u8; 2] = [core::ptr::null(); 2];
static mut HOSTENT_ALIASES: [*const u8; 1] = [core::ptr::null()];
static mut HOSTENT_RESULT: Hostent = Hostent {
    h_name: core::ptr::null(),
    h_aliases: core::ptr::null(),
    h_addrtype: 0,
    h_length: 0,
    h_addr_list: core::ptr::null(),
};

/// Resolve a hostname to an IPv4 address.
///
/// Returns a pointer to a static `Hostent`, or NULL on failure.
/// The returned pointer is valid until the next call.
///
/// # Safety
///
/// `name` must be a valid null-terminated hostname string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gethostbyname(name: *const u8) -> *const Hostent {
    if name.is_null() {
        return core::ptr::null();
    }

    let name_len = unsafe { crate::string::strlen(name) };
    if name_len == 0 || name_len > 253 {
        return core::ptr::null();
    }

    // Output buffer for the resolved IPv4 address.
    let mut resolved = [0u8; 4];

    // SYS_DNS_RESOLVE: arg0=hostname_ptr, arg1=hostname_len, arg2=output_ptr.
    let ret = syscall3(
        SYS_DNS_RESOLVE,
        name as u64,
        name_len as u64,
        resolved.as_mut_ptr() as u64,
    );
    if ret < 0 {
        return core::ptr::null();
    }

    // SAFETY: Single-threaded access to static storage.
    unsafe {
        // Copy the hostname into our static buffer.
        let buf = core::ptr::addr_of_mut!(HOSTENT_NAME);
        let copy_len = core::cmp::min(name_len, 255);
        core::ptr::copy_nonoverlapping(name, (*buf).as_mut_ptr(), copy_len);
        // Null-terminate the hostname copy.
        if let Some(slot) = (*buf).get_mut(copy_len) {
            *slot = 0;
        }

        // Store the resolved address.
        let addr = core::ptr::addr_of_mut!(HOSTENT_ADDR);
        (*addr) = resolved;

        // Set up address list: [&addr, NULL].
        let addr_ptr = core::ptr::addr_of_mut!(HOSTENT_ADDR_PTR);
        (*addr_ptr)[0] = (*addr).as_ptr();
        (*addr_ptr)[1] = core::ptr::null();

        // Set up empty alias list.
        let aliases = core::ptr::addr_of_mut!(HOSTENT_ALIASES);
        (*aliases)[0] = core::ptr::null();

        // Assemble the hostent.
        let result = core::ptr::addr_of_mut!(HOSTENT_RESULT);
        (*result).h_name = (*buf).as_ptr();
        (*result).h_aliases = (*aliases).as_ptr();
        (*result).h_addrtype = AF_INET;
        (*result).h_length = 4;
        (*result).h_addr_list = (*addr_ptr).as_ptr();

        result
    }
}

// ---------------------------------------------------------------------------
// gethostbyaddr — reverse DNS lookup
// ---------------------------------------------------------------------------

/// Static storage for gethostbyaddr (separate from gethostbyname to allow
/// interleaved usage, even though POSIX doesn't guarantee it).
static mut HOSTENT_REV_NAME: [u8; 256] = [0u8; 256];
static mut HOSTENT_REV_ADDR: [u8; 4] = [0u8; 4];
static mut HOSTENT_REV_ADDR_PTR: [*const u8; 2] = [core::ptr::null(); 2];
static mut HOSTENT_REV_ALIASES: [*const u8; 1] = [core::ptr::null()];
static mut HOSTENT_REV_RESULT: Hostent = Hostent {
    h_name: core::ptr::null(),
    h_aliases: core::ptr::null(),
    h_addrtype: 0,
    h_length: 0,
    h_addr_list: core::ptr::null(),
};

/// Reverse-resolve an IPv4 address to a hostname.
///
/// Given a network-byte-order IPv4 address (4 bytes at `*addr`),
/// performs a DNS PTR lookup via `SYS_DNS_REVERSE_RESOLVE`.
///
/// Returns a pointer to a static `Hostent`, or NULL on failure.
///
/// # Safety
///
/// `addr` must point to at least `len` (4) bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gethostbyaddr(
    addr: *const u8,
    len: i32,
    addr_type: i32,
) -> *const Hostent {
    if addr.is_null() || addr_type != AF_INET || len != 4 {
        return core::ptr::null();
    }

    // Read the address bytes.
    let mut ip_bytes = [0u8; 4];
    unsafe {
        ip_bytes[0] = *addr;
        ip_bytes[1] = *addr.add(1);
        ip_bytes[2] = *addr.add(2);
        ip_bytes[3] = *addr.add(3);
    }

    // Convert to the u32 representation our kernel expects
    // (network byte order = big-endian, same as sockaddr_in).
    let ip_u32 = u32::from_ne_bytes(ip_bytes);

    // Output buffer for the reverse-resolved hostname.
    let mut name_buf = [0u8; 256];

    // SYS_DNS_REVERSE_RESOLVE: arg0=ip (network order), arg1=output_ptr, arg2=output_len.
    let ret = syscall3(
        SYS_DNS_REVERSE_RESOLVE,
        u64::from(ip_u32),
        name_buf.as_mut_ptr() as u64,
        name_buf.len() as u64,
    );
    if ret < 0 {
        return core::ptr::null();
    }

    let name_len = ret as usize;
    let copy_len = if name_len < 255 { name_len } else { 255 };

    // SAFETY: Single-threaded access to static storage.
    unsafe {
        // Copy the resolved hostname into static storage.
        let buf = core::ptr::addr_of_mut!(HOSTENT_REV_NAME);
        core::ptr::copy_nonoverlapping(name_buf.as_ptr(), (*buf).as_mut_ptr(), copy_len);
        if let Some(slot) = (*buf).get_mut(copy_len) {
            *slot = 0;
        }

        // Store the address.
        let stored_addr = core::ptr::addr_of_mut!(HOSTENT_REV_ADDR);
        (*stored_addr) = ip_bytes;

        // Set up address list: [&addr, NULL].
        let addr_ptr = core::ptr::addr_of_mut!(HOSTENT_REV_ADDR_PTR);
        (*addr_ptr)[0] = (*stored_addr).as_ptr();
        (*addr_ptr)[1] = core::ptr::null();

        // Empty alias list.
        let aliases = core::ptr::addr_of_mut!(HOSTENT_REV_ALIASES);
        (*aliases)[0] = core::ptr::null();

        // Assemble the result.
        let result = core::ptr::addr_of_mut!(HOSTENT_REV_RESULT);
        (*result).h_name = (*buf).as_ptr();
        (*result).h_aliases = (*aliases).as_ptr();
        (*result).h_addrtype = AF_INET;
        (*result).h_length = 4;
        (*result).h_addr_list = (*addr_ptr).as_ptr();

        result
    }
}

// ---------------------------------------------------------------------------
// h_errno / herror / hstrerror — legacy DNS error reporting
// ---------------------------------------------------------------------------

/// Resolver error: authoritative answer — host not found.
pub const HOST_NOT_FOUND: i32 = 1;
/// Resolver error: non-authoritative — try again later.
pub const TRY_AGAIN: i32 = 2;
/// Resolver error: non-recoverable error.
pub const NO_RECOVERY: i32 = 3;
/// Resolver error: valid name, no data record of requested type.
pub const NO_DATA: i32 = 4;

/// Thread-local (actually global — single-threaded) resolver error.
static mut H_ERRNO: i32 = 0;

/// Get a pointer to the resolver error variable.
///
/// Used by C code as `extern int h_errno;` via `*__h_errno_location()`.
#[unsafe(no_mangle)]
pub extern "C" fn __h_errno_location() -> *mut i32 {
    core::ptr::addr_of_mut!(H_ERRNO)
}

/// Return a string describing a resolver error code.
#[unsafe(no_mangle)]
pub extern "C" fn hstrerror(err: i32) -> *const u8 {
    match err {
        0 => b"Resolver Error 0 (no error)\0".as_ptr(),
        HOST_NOT_FOUND => b"Host not found\0".as_ptr(),
        TRY_AGAIN => b"Try again\0".as_ptr(),
        NO_RECOVERY => b"Non-recoverable error\0".as_ptr(),
        NO_DATA => b"No address associated with name\0".as_ptr(),
        _ => b"Unknown resolver error\0".as_ptr(),
    }
}

/// Print a resolver error message to stderr.
#[unsafe(no_mangle)]
pub extern "C" fn herror(s: *const u8) {
    // SAFETY: s is null-terminated.
    if !s.is_null() {
        let slen = unsafe { crate::string::strlen(s) };
        if slen > 0 {
            let _ = syscall2(SYS_CONSOLE_WRITE, s as u64, slen as u64);
            let _ = syscall2(SYS_CONSOLE_WRITE, b": ".as_ptr() as u64, 2);
        }
    }
    // SAFETY: single-threaded access.
    let err = unsafe { *core::ptr::addr_of!(H_ERRNO) };
    let msg = hstrerror(err);
    let msg_len = unsafe { crate::string::strlen(msg) };
    let _ = syscall2(SYS_CONSOLE_WRITE, msg as u64, msg_len as u64);
    let _ = syscall2(SYS_CONSOLE_WRITE, b"\n".as_ptr() as u64, 1);
}

// ---------------------------------------------------------------------------
// getaddrinfo() / freeaddrinfo() — modern DNS resolution
// ---------------------------------------------------------------------------

/// Hints and results for `getaddrinfo()`.
#[repr(C)]
pub struct Addrinfo {
    /// AI_PASSIVE, AI_CANONNAME, etc.
    pub ai_flags: i32,
    /// Address family (AF_INET, etc.).
    pub ai_family: i32,
    /// Socket type (SOCK_STREAM, SOCK_DGRAM).
    pub ai_socktype: i32,
    /// Protocol (IPPROTO_TCP, IPPROTO_UDP).
    pub ai_protocol: i32,
    /// Length of ai_addr.
    pub ai_addrlen: SocklenT,
    /// Canonical hostname (may be null).
    pub ai_canonname: *mut u8,
    /// Socket address.
    pub ai_addr: *mut Sockaddr,
    /// Next result in linked list.
    pub ai_next: *mut Addrinfo,
}

// getaddrinfo flag constants.
/// Socket address is intended for bind().
pub const AI_PASSIVE: i32 = 0x0001;
/// Request canonical name.
pub const AI_CANONNAME: i32 = 0x0002;
/// Numeric host address string.
pub const AI_NUMERICHOST: i32 = 0x0004;
/// Numeric service string.
pub const AI_NUMERICSERV: i32 = 0x0400;

// getaddrinfo error codes.
/// Address family not supported.
pub const EAI_ADDRFAMILY: i32 = 1;
/// Temporary failure in name resolution.
pub const EAI_AGAIN: i32 = 2;
/// Invalid flags.
pub const EAI_BADFLAGS: i32 = 3;
/// Non-recoverable failure in name resolution.
pub const EAI_FAIL: i32 = 4;
/// Address family not supported.
pub const EAI_FAMILY: i32 = 5;
/// Memory allocation failure.
pub const EAI_MEMORY: i32 = 6;
/// No address associated with hostname.
pub const EAI_NODATA: i32 = 7;
/// Name or service not known.
pub const EAI_NONAME: i32 = 8;
/// Service not supported for socket type.
pub const EAI_SERVICE: i32 = 9;
/// Socket type not supported.
pub const EAI_SOCKTYPE: i32 = 10;
/// System error.
pub const EAI_SYSTEM: i32 = 11;

/// Static storage for a single getaddrinfo result.
///
/// We only return one result (the first resolved IPv4 address).
/// This avoids heap allocation in our no_std environment.
static mut GAI_RESULT: Addrinfo = Addrinfo {
    ai_flags: 0,
    ai_family: 0,
    ai_socktype: 0,
    ai_protocol: 0,
    ai_addrlen: 0,
    ai_canonname: core::ptr::null_mut(),
    ai_addr: core::ptr::null_mut(),
    ai_next: core::ptr::null_mut(),
};

/// Static storage for the sockaddr_in in the getaddrinfo result.
static mut GAI_ADDR: SockaddrIn = SockaddrIn {
    sin_family: 0,
    sin_port: 0,
    sin_addr: InAddr { s_addr: 0 },
    sin_zero: [0u8; 8],
};

/// Resolve a hostname and/or service to a list of socket addresses.
///
/// This is the modern replacement for `gethostbyname()`.  We support
/// only IPv4 (`AF_INET`) resolution and return at most one result.
///
/// Returns 0 on success, non-zero EAI_* error code on failure.
///
/// # Safety
///
/// - `node` and `service` must be valid null-terminated strings (or null).
/// - `res` must be a valid pointer to a `*mut Addrinfo`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getaddrinfo(
    node: *const u8,
    service: *const u8,
    hints: *const Addrinfo,
    res: *mut *mut Addrinfo,
) -> i32 {
    if res.is_null() {
        return EAI_SYSTEM;
    }

    // SAFETY: res is non-null.
    unsafe { *res = core::ptr::null_mut(); }

    // Parse hints if provided.
    let (want_family, want_socktype, want_passive) = if hints.is_null() {
        (0, 0, false) // Accept any family/socktype.
    } else {
        // SAFETY: hints is non-null.
        let h = unsafe { &*hints };
        // We only support AF_INET (or AF_UNSPEC=0 which means "any").
        if h.ai_family != 0 && h.ai_family != AF_INET {
            return EAI_FAMILY;
        }
        (h.ai_family, h.ai_socktype, h.ai_flags & AI_PASSIVE != 0)
    };

    // We need at least a node or a service.
    if node.is_null() && service.is_null() {
        return EAI_NONAME;
    }

    // Resolve the IP address.
    let ip: u32 = if node.is_null() {
        // No node: if AI_PASSIVE, use INADDR_ANY; otherwise loopback.
        if want_passive {
            htonl(INADDR_ANY)
        } else {
            htonl(INADDR_LOOPBACK)
        }
    } else {
        // Try numeric parse first.
        let numeric = unsafe { inet_addr(node) };
        if numeric == 0xFFFF_FFFF {
            // Not numeric — do DNS resolution.
            let he = unsafe { gethostbyname(node) };
            if he.is_null() {
                return EAI_NONAME;
            }
            // SAFETY: gethostbyname returned a valid hostent.
            unsafe {
                let addr_list = (*he).h_addr_list;
                if addr_list.is_null() || (*addr_list).is_null() {
                    return EAI_NODATA;
                }
                // Read the 4-byte IPv4 address.
                core::ptr::read_unaligned((*addr_list).cast::<u32>())
            }
        } else {
            numeric
        }
    };

    // Parse the port from the service string.
    let port: u16 = if service.is_null() {
        0
    } else {
        parse_port_string(service)
    };

    // Determine socket type and protocol.
    let socktype = if want_socktype != 0 {
        want_socktype
    } else {
        SOCK_STREAM // Default to TCP.
    };
    let protocol = match socktype {
        SOCK_STREAM => IPPROTO_TCP,
        SOCK_DGRAM => IPPROTO_UDP,
        _ => 0,
    };

    // Fill in the static result.
    // SAFETY: Single-threaded access; getaddrinfo is not re-entrant (per POSIX).
    unsafe {
        let addr = core::ptr::addr_of_mut!(GAI_ADDR);
        (*addr).sin_family = AF_INET as u16;
        (*addr).sin_port = htons(port);
        (*addr).sin_addr.s_addr = ip;
        (*addr).sin_zero = [0u8; 8];

        let result = core::ptr::addr_of_mut!(GAI_RESULT);
        (*result).ai_flags = 0;
        (*result).ai_family = if want_family != 0 { want_family } else { AF_INET };
        (*result).ai_socktype = socktype;
        (*result).ai_protocol = protocol;
        (*result).ai_addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
        (*result).ai_canonname = core::ptr::null_mut();
        (*result).ai_addr = addr.cast::<Sockaddr>();
        (*result).ai_next = core::ptr::null_mut();

        *res = result;
    }

    0 // Success.
}

/// Free an addrinfo result list.
///
/// Since our getaddrinfo uses static storage (not heap allocation),
/// this is a no-op.  Programs must still call it for POSIX compliance.
#[unsafe(no_mangle)]
pub extern "C" fn freeaddrinfo(_res: *mut Addrinfo) {
    // No-op: we use static storage, not heap allocation.
}

/// Return a string describing a getaddrinfo error code.
#[unsafe(no_mangle)]
pub extern "C" fn gai_strerror(errcode: i32) -> *const u8 {
    match errcode {
        0 => c"Success".as_ptr().cast::<u8>(),
        EAI_ADDRFAMILY | EAI_FAMILY => c"Address family not supported".as_ptr().cast::<u8>(),
        EAI_AGAIN => c"Temporary failure in name resolution".as_ptr().cast::<u8>(),
        EAI_BADFLAGS => c"Invalid flags".as_ptr().cast::<u8>(),
        EAI_FAIL => c"Non-recoverable failure".as_ptr().cast::<u8>(),
        EAI_MEMORY => c"Memory allocation failure".as_ptr().cast::<u8>(),
        EAI_NODATA => c"No address associated with hostname".as_ptr().cast::<u8>(),
        EAI_NONAME => c"Name or service not known".as_ptr().cast::<u8>(),
        EAI_SERVICE => c"Service not supported".as_ptr().cast::<u8>(),
        EAI_SOCKTYPE => c"Socket type not supported".as_ptr().cast::<u8>(),
        EAI_SYSTEM => c"System error".as_ptr().cast::<u8>(),
        _ => c"Unknown error".as_ptr().cast::<u8>(),
    }
}

// ---------------------------------------------------------------------------
// getnameinfo — reverse DNS + service lookup
// ---------------------------------------------------------------------------

/// `NI_NUMERICHOST` — return the numeric form of the host address.
pub const NI_NUMERICHOST: i32 = 1;
/// `NI_NUMERICSERV` — return the numeric form of the service port.
pub const NI_NUMERICSERV: i32 = 2;
/// `NI_NOFQDN` — return only the hostname part of the FQDN.
pub const NI_NOFQDN: i32 = 4;
/// `NI_NAMEREQD` — return an error if the hostname cannot be determined.
pub const NI_NAMEREQD: i32 = 8;
/// `NI_DGRAM` — the service is datagram (UDP) based.
pub const NI_DGRAM: i32 = 16;

/// Translate a socket address to a host name and service string.
///
/// This is the reverse of `getaddrinfo`.  Given a `sockaddr_in`, it
/// produces human-readable host and service strings.
///
/// ## Reverse DNS
///
/// When `NI_NUMERICHOST` is *not* set, attempts a PTR lookup via
/// `SYS_DNS_REVERSE_RESOLVE`.  If the lookup fails and `NI_NAMEREQD`
/// is set, returns `EAI_NONAME`.  Otherwise falls back to the numeric
/// IP representation.
///
/// ## Limitations
///
/// - Only supports `AF_INET` (IPv4).
/// - Service is always returned as the numeric port number.
///
/// Returns 0 on success, or an EAI_* error code.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn getnameinfo(
    sa: *const SockaddrIn,
    _salen: SocklenT,
    host: *mut u8,
    hostlen: SocklenT,
    serv: *mut u8,
    servlen: SocklenT,
    flags: i32,
) -> i32 {
    if sa.is_null() {
        return EAI_FAIL;
    }

    // SAFETY: sa is non-null (checked above); caller guarantees validity.
    let addr = unsafe { &*sa };

    if i32::from(addr.sin_family) != AF_INET {
        return EAI_FAMILY;
    }

    // Format the host address.
    if !host.is_null() && hostlen > 0 {
        let ip_bytes = addr.sin_addr.s_addr.to_ne_bytes();
        let mut used_reverse = false;

        // Try reverse DNS unless NI_NUMERICHOST is set.
        if (flags & NI_NUMERICHOST) == 0 {
            // SYS_DNS_REVERSE_RESOLVE: arg0 = IP (network order u32),
            // arg1 = output buffer ptr, arg2 = output buffer size.
            // Returns hostname length on success, negative on failure.
            let ip_u32 = u32::from_be_bytes(ip_bytes);
            let ret = crate::syscall::syscall3(
                crate::syscall::SYS_DNS_REVERSE_RESOLVE,
                ip_u32 as u64,
                host as u64,
                // Reserve 1 byte for null terminator.
                (hostlen as usize).saturating_sub(1) as u64,
            );
            if ret > 0 {
                let name_len = ret as usize;
                // Null-terminate the hostname.
                // SAFETY: ret < hostlen-1 (kernel ensures copy_len <= buffer),
                // and host is valid for hostlen bytes.
                unsafe { *host.add(name_len) = 0; }
                used_reverse = true;
            }
        }

        if !used_reverse {
            // NI_NAMEREQD: error if name can't be determined.
            if (flags & NI_NAMEREQD) != 0 {
                return EAI_NONAME;
            }

            // Fall back to numeric representation.
            let mut tmp = [0u8; 16]; // max "255.255.255.255\0"
            let mut pos: usize = 0;

            for (idx, &octet) in ip_bytes.iter().enumerate() {
                if idx > 0 {
                    write_byte_ntop(&mut tmp, &mut pos, b'.');
                }
                write_u8_dec_ntop(&mut tmp, &mut pos, octet);
            }

            let needed = pos.wrapping_add(1); // +null
            if (hostlen as usize) < needed {
                return EAI_OVERFLOW;
            }

            // SAFETY: host is valid for hostlen bytes (caller contract).
            unsafe {
                let mut j: usize = 0;
                while j < pos {
                    if let Some(&b) = tmp.get(j) {
                        *host.add(j) = b;
                    }
                    j = j.wrapping_add(1);
                }
                *host.add(pos) = 0;
            }
        }
    }

    // Format the service/port.
    if !serv.is_null() && servlen > 0 {
        let port = u16::from_be(addr.sin_port);
        // Convert port to decimal string.
        let mut tmp = [0u8; 6]; // max "65535\0"
        let mut pos: usize = 0;
        write_u16_decimal(&mut tmp, &mut pos, port);

        let needed = pos.wrapping_add(1);
        if (servlen as usize) < needed {
            return EAI_OVERFLOW;
        }

        unsafe {
            let mut j: usize = 0;
            while j < pos {
                if let Some(&b) = tmp.get(j) {
                    *serv.add(j) = b;
                }
                j = j.wrapping_add(1);
            }
            *serv.add(pos) = 0;
        }
    }

    0
}

/// `EAI_OVERFLOW` — buffer too small for result.
pub const EAI_OVERFLOW: i32 = -12;

/// Write a u16 as decimal digits into a small buffer.
fn write_u16_decimal(buf: &mut [u8; 6], pos: &mut usize, val: u16) {
    if val >= 10000 {
        #[allow(clippy::arithmetic_side_effects)]
        if let Some(slot) = buf.get_mut(*pos) {
            *slot = b'0' + (val / 10000) as u8;
            *pos = pos.wrapping_add(1);
        }
    }
    if val >= 1000 {
        #[allow(clippy::arithmetic_side_effects)]
        if let Some(slot) = buf.get_mut(*pos) {
            *slot = b'0' + ((val / 1000) % 10) as u8;
            *pos = pos.wrapping_add(1);
        }
    }
    if val >= 100 {
        #[allow(clippy::arithmetic_side_effects)]
        if let Some(slot) = buf.get_mut(*pos) {
            *slot = b'0' + ((val / 100) % 10) as u8;
            *pos = pos.wrapping_add(1);
        }
    }
    if val >= 10 {
        #[allow(clippy::arithmetic_side_effects)]
        if let Some(slot) = buf.get_mut(*pos) {
            *slot = b'0' + ((val / 10) % 10) as u8;
            *pos = pos.wrapping_add(1);
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    if let Some(slot) = buf.get_mut(*pos) {
        *slot = b'0' + (val % 10) as u8;
        *pos = pos.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// socketpair
// ---------------------------------------------------------------------------

/// Create a pair of connected sockets.
///
/// Stub: returns -1/ENOSYS.  Our kernel doesn't yet support
/// connected socket pairs (used for Unix domain IPC).
#[unsafe(no_mangle)]
pub extern "C" fn socketpair(
    _domain: i32,
    _sock_type: i32,
    _protocol: i32,
    sv: *mut [i32; 2],
) -> i32 {
    let _ = sv;
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// sendmsg / recvmsg
// ---------------------------------------------------------------------------

/// Scatter/gather I/O vector.
#[repr(C)]
pub struct Iovec {
    /// Base address of the buffer.
    pub iov_base: *mut u8,
    /// Length of the buffer in bytes.
    pub iov_len: usize,
}

/// Message header for `sendmsg`/`recvmsg`.
#[repr(C)]
pub struct Msghdr {
    /// Optional address (sendto/recvfrom target).
    pub msg_name: *mut u8,
    /// Length of `msg_name`.
    pub msg_namelen: SocklenT,
    /// Scatter/gather array.
    pub msg_iov: *mut Iovec,
    /// Number of elements in `msg_iov`.
    pub msg_iovlen: usize,
    /// Ancillary data (cmsghdr chain).
    pub msg_control: *mut u8,
    /// Length of `msg_control`.
    pub msg_controllen: usize,
    /// Flags on received message.
    pub msg_flags: i32,
}

/// Control message header (ancillary data).
#[repr(C)]
pub struct Cmsghdr {
    /// Length of this control message (including header).
    pub cmsg_len: usize,
    /// Originating protocol level.
    pub cmsg_level: i32,
    /// Protocol-specific type.
    pub cmsg_type: i32,
}

/// Send a message on a socket using a message header.
///
/// Iterates over all iov elements.  For small messages (≤ 4 KiB total),
/// concatenates into a stack buffer for a single `send` call.  For
/// larger messages, sends each iov sequentially.
/// Ancillary data (`msg_control`) is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sendmsg(fd: i32, msg: *const Msghdr, flags: i32) -> isize {
    if msg.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: msg is non-null and valid (caller guarantee).
    let m = unsafe { &*msg };
    if m.msg_iov.is_null() || m.msg_iovlen == 0 {
        return 0;
    }

    // Calculate total size across all iovecs.
    let mut total: usize = 0;
    let mut i: usize = 0;
    while i < m.msg_iovlen {
        // SAFETY: msg_iov is valid for msg_iovlen entries.
        let iov = unsafe { &*m.msg_iov.add(i) };
        total = total.saturating_add(iov.iov_len);
        i = i.wrapping_add(1);
    }

    if total == 0 {
        return 0;
    }

    // Single iov — send directly without copying.
    if m.msg_iovlen == 1 {
        let iov = unsafe { &*m.msg_iov };
        return unsafe { send(fd, iov.iov_base, iov.iov_len, flags) };
    }

    // If total fits in a stack buffer, concatenate for one send call.
    // This produces a single TCP segment instead of multiple.
    const STACK_BUF: usize = 4096;
    if total <= STACK_BUF {
        let mut buf = [0u8; STACK_BUF];
        let mut pos: usize = 0;
        i = 0;
        while i < m.msg_iovlen {
            let iov = unsafe { &*m.msg_iov.add(i) };
            if iov.iov_len > 0 && !iov.iov_base.is_null() {
                // SAFETY: pos + iov_len <= total <= STACK_BUF.
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        iov.iov_base.cast::<u8>(),
                        buf.as_mut_ptr().add(pos),
                        iov.iov_len,
                    );
                }
                pos = pos.wrapping_add(iov.iov_len);
            }
            i = i.wrapping_add(1);
        }
        return unsafe { send(fd, buf.as_ptr().cast(), pos, flags) };
    }

    // Larger than stack buffer — send each iov individually.
    let mut sent: isize = 0;
    i = 0;
    while i < m.msg_iovlen {
        let iov = unsafe { &*m.msg_iov.add(i) };
        if iov.iov_len > 0 && !iov.iov_base.is_null() {
            let n = unsafe { send(fd, iov.iov_base, iov.iov_len, flags) };
            if n < 0 {
                return if sent > 0 { sent } else { n };
            }
            sent = sent.wrapping_add(n);
            // Short send — stop here.
            if (n as usize) < iov.iov_len {
                break;
            }
        }
        i = i.wrapping_add(1);
    }
    sent
}

/// Receive a message from a socket using a message header.
///
/// Distributes received data across all iov elements.  For single-iov
/// messages, receives directly into the buffer.  For multi-iov messages,
/// receives into a stack buffer and copies out to each iov.
/// Ancillary data (`msg_control`) is not populated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recvmsg(fd: i32, msg: *mut Msghdr, flags: i32) -> isize {
    if msg.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: msg is non-null and valid (caller guarantee).
    let m = unsafe { &mut *msg };
    if m.msg_iov.is_null() || m.msg_iovlen == 0 {
        m.msg_flags = 0;
        m.msg_controllen = 0;
        return 0;
    }

    // Single iov — receive directly without copying.
    if m.msg_iovlen == 1 {
        let iov = unsafe { &*m.msg_iov };
        let ret = unsafe { recv(fd, iov.iov_base, iov.iov_len, flags) };
        m.msg_flags = 0;
        m.msg_controllen = 0;
        return ret;
    }

    // Multiple iovs — calculate total capacity.
    let mut total_cap: usize = 0;
    let mut i: usize = 0;
    while i < m.msg_iovlen {
        let iov = unsafe { &*m.msg_iov.add(i) };
        total_cap = total_cap.saturating_add(iov.iov_len);
        i = i.wrapping_add(1);
    }

    if total_cap == 0 {
        m.msg_flags = 0;
        m.msg_controllen = 0;
        return 0;
    }

    // Receive into a stack buffer, then distribute across iovs.
    const STACK_BUF: usize = 4096;
    let recv_cap = if total_cap < STACK_BUF { total_cap } else { STACK_BUF };
    let mut buf = [0u8; STACK_BUF];
    let ret = unsafe {
        recv(fd, buf.as_mut_ptr().cast(), recv_cap, flags)
    };
    if ret <= 0 {
        m.msg_flags = 0;
        m.msg_controllen = 0;
        return ret;
    }

    // Distribute received bytes across iovecs.
    let received = ret as usize;
    let mut remaining = received;
    let mut src_pos: usize = 0;
    i = 0;
    while i < m.msg_iovlen && remaining > 0 {
        let iov = unsafe { &*m.msg_iov.add(i) };
        if iov.iov_len > 0 && !iov.iov_base.is_null() {
            let to_copy = if remaining < iov.iov_len { remaining } else { iov.iov_len };
            // SAFETY: src_pos + to_copy <= received <= STACK_BUF;
            //         iov_base is valid for iov_len bytes (caller guarantee).
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buf.as_ptr().add(src_pos),
                    iov.iov_base.cast::<u8>(),
                    to_copy,
                );
            }
            src_pos = src_pos.wrapping_add(to_copy);
            remaining = remaining.wrapping_sub(to_copy);
        }
        i = i.wrapping_add(1);
    }

    // Set MSG_TRUNC if we received a full buffer but total capacity
    // was larger (indicating potential data truncation for datagrams).
    m.msg_flags = if received == recv_cap && recv_cap < total_cap {
        MSG_TRUNC
    } else {
        0
    };
    m.msg_controllen = 0;

    ret
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse a numeric port string to u16.
fn parse_port_string(s: *const u8) -> u16 {
    if s.is_null() {
        return 0;
    }
    let mut val: u32 = 0;
    let mut i: usize = 0;
    loop {
        // SAFETY: s is a valid null-terminated string.
        let c = unsafe { *s.add(i) };
        if c == 0 {
            break;
        }
        if !c.is_ascii_digit() {
            return 0; // Non-numeric service name — not supported.
        }
        #[allow(clippy::arithmetic_side_effects)]
        {
            val = val.wrapping_mul(10).wrapping_add(u32::from(c - b'0'));
        }
        if val > 65535 {
            return 0;
        }
        i = i.wrapping_add(1);
    }
    val as u16
}

// ---------------------------------------------------------------------------
// if_nametoindex / if_indextoname — network interface stubs
// ---------------------------------------------------------------------------

/// Convert a network interface name to its index.
///
/// Stub: returns 0 (failure) since our OS doesn't have named network
/// interfaces yet.  Programs that enumerate interfaces will see this
/// as "interface not found".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn if_nametoindex(ifname: *const u8) -> u32 {
    if ifname.is_null() {
        return 0; // 0 means "no such interface" per POSIX.
    }

    // Our kernel has a single network interface named "eth0".
    // SAFETY: caller guarantees ifname is a valid C string.
    let len = unsafe { crate::string::strlen(ifname) };
    let name = unsafe { core::slice::from_raw_parts(ifname, len) };
    // eth0 = index 1, lo = index 1 (we only have one real interface).
    u32::from(name == b"eth0" || name == b"lo")
}

/// Convert a network interface index to its name.
///
/// Returns a pointer to `ifname` on success, null on error.
/// `ifname` must point to a buffer of at least `IF_NAMESIZE` bytes.
///
/// # Safety
///
/// `ifname` must point to writable memory of at least `IF_NAMESIZE` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn if_indextoname(ifindex: u32, ifname: *mut u8) -> *mut u8 {
    if ifname.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }

    // Index 1 = "eth0" (our only interface).
    if ifindex == 1 {
        // SAFETY: caller guarantees ifname has IF_NAMESIZE bytes.
        let name = b"eth0\0";
        unsafe {
            core::ptr::copy_nonoverlapping(name.as_ptr(), ifname, name.len());
        }
        ifname
    } else {
        errno::set_errno(errno::ENXIO);
        core::ptr::null_mut()
    }
}

/// Maximum interface name length.
pub const IF_NAMESIZE: usize = 16;

// ---------------------------------------------------------------------------
// getifaddrs / freeifaddrs — interface address enumeration
// ---------------------------------------------------------------------------

/// Linked list of network interface addresses (per POSIX/BSD).
#[repr(C)]
pub struct Ifaddrs {
    /// Next entry in the linked list (NULL for last entry).
    pub ifa_next: *mut Ifaddrs,
    /// Interface name (null-terminated).
    pub ifa_name: *const u8,
    /// Interface flags (IFF_UP, IFF_LOOPBACK, etc.).
    pub ifa_flags: u32,
    /// Interface address.
    pub ifa_addr: *const Sockaddr,
    /// Network mask.
    pub ifa_netmask: *const Sockaddr,
    /// Broadcast/destination address (union in BSD; we use broadcast).
    pub ifa_broadaddr: *const Sockaddr,
    /// Interface-specific data (unused).
    pub ifa_data: *const u8,
}

// Interface flags.
/// Interface is up.
pub const IFF_UP: u32 = 1;
/// Interface is a loopback.
pub const IFF_LOOPBACK: u32 = 8;
/// Interface supports multicast.
pub const IFF_MULTICAST: u32 = 0x1000;
/// Interface is running.
pub const IFF_RUNNING: u32 = 0x40;
/// Interface supports broadcast.
pub const IFF_BROADCAST: u32 = 2;

/// Static storage for getifaddrs (single interface).
///
/// We have one physical interface ("eth0") whose address is obtained
/// from `SYS_NET_STAT` (if configured) and a loopback ("lo").
static mut IFADDRS_ETH0: Ifaddrs = Ifaddrs {
    ifa_next: core::ptr::null_mut(),
    ifa_name: core::ptr::null(),
    ifa_flags: 0,
    ifa_addr: core::ptr::null(),
    ifa_netmask: core::ptr::null(),
    ifa_broadaddr: core::ptr::null(),
    ifa_data: core::ptr::null(),
};
static mut IFADDRS_LO: Ifaddrs = Ifaddrs {
    ifa_next: core::ptr::null_mut(),
    ifa_name: core::ptr::null(),
    ifa_flags: 0,
    ifa_addr: core::ptr::null(),
    ifa_netmask: core::ptr::null(),
    ifa_broadaddr: core::ptr::null(),
    ifa_data: core::ptr::null(),
};

static mut IFADDRS_ETH0_NAME: [u8; 8] = *b"eth0\0\0\0\0";
static mut IFADDRS_LO_NAME: [u8; 4] = *b"lo\0\0";
static mut IFADDRS_ETH0_ADDR: SockaddrIn = SockaddrIn {
    sin_family: AF_INET as u16, sin_port: 0,
    sin_addr: InAddr { s_addr: 0 }, sin_zero: [0; 8],
};
static mut IFADDRS_ETH0_MASK: SockaddrIn = SockaddrIn {
    sin_family: AF_INET as u16, sin_port: 0,
    sin_addr: InAddr { s_addr: 0 }, sin_zero: [0; 8],
};
static mut IFADDRS_LO_ADDR: SockaddrIn = SockaddrIn {
    sin_family: AF_INET as u16, sin_port: 0,
    sin_addr: InAddr { s_addr: u32::to_be(INADDR_LOOPBACK) }, sin_zero: [0; 8],
};
static mut IFADDRS_LO_MASK: SockaddrIn = SockaddrIn {
    sin_family: AF_INET as u16, sin_port: 0,
    sin_addr: InAddr { s_addr: u32::to_be(0xFF00_0000) }, sin_zero: [0; 8],
};

/// Retrieve a linked list of network interface addresses.
///
/// Populates `*ifap` with a pointer to a linked list of `Ifaddrs`
/// structures (one per interface).  Our OS exposes "eth0" (the primary
/// NIC, configured via DHCP) and "lo" (loopback, always 127.0.0.1/8).
///
/// The returned data is in static storage — `freeifaddrs()` is a no-op.
///
/// # Safety
///
/// `ifap` must be a valid pointer to write the result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getifaddrs(ifap: *mut *mut Ifaddrs) -> i32 {
    if ifap.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Query the kernel for interface configuration (IP, mask, gateway, DNS, MAC, flags).
    let mut if_info = [0u8; 24];
    let net_up = syscall2(
        SYS_NET_IF_INFO,
        if_info.as_mut_ptr() as u64,
        if_info.len() as u64,
    ) == 0 && if_info[22] != 0; // byte 22 = flags, bit 0 = up

    // SAFETY: single-threaded, static storage.
    unsafe {
        // Set up loopback entry.
        let lo = core::ptr::addr_of_mut!(IFADDRS_LO);
        let lo_addr = core::ptr::addr_of_mut!(IFADDRS_LO_ADDR);
        let lo_mask = core::ptr::addr_of_mut!(IFADDRS_LO_MASK);
        let lo_name = core::ptr::addr_of_mut!(IFADDRS_LO_NAME);
        (*lo_addr).sin_addr.s_addr = u32::to_be(INADDR_LOOPBACK);
        (*lo_mask).sin_addr.s_addr = u32::to_be(0xFF00_0000); // 255.0.0.0
        (*lo).ifa_name = (*lo_name).as_ptr();
        (*lo).ifa_flags = IFF_UP | IFF_LOOPBACK | IFF_RUNNING;
        (*lo).ifa_addr = lo_addr.cast();
        (*lo).ifa_netmask = lo_mask.cast();
        (*lo).ifa_broadaddr = core::ptr::null();
        (*lo).ifa_data = core::ptr::null();
        (*lo).ifa_next = core::ptr::null_mut();

        if net_up {
            // Set up eth0 entry with real IP and mask from kernel.
            let eth0 = core::ptr::addr_of_mut!(IFADDRS_ETH0);
            let eth0_addr = core::ptr::addr_of_mut!(IFADDRS_ETH0_ADDR);
            let eth0_mask = core::ptr::addr_of_mut!(IFADDRS_ETH0_MASK);
            let eth0_name = core::ptr::addr_of_mut!(IFADDRS_ETH0_NAME);

            // if_info[0..4] = IP, [4..8] = mask (already in network byte order).
            (*eth0_addr).sin_addr.s_addr =
                u32::from_ne_bytes([if_info[0], if_info[1], if_info[2], if_info[3]]);
            (*eth0_mask).sin_addr.s_addr =
                u32::from_ne_bytes([if_info[4], if_info[5], if_info[6], if_info[7]]);

            (*eth0).ifa_name = (*eth0_name).as_ptr();
            (*eth0).ifa_flags = IFF_UP | IFF_RUNNING | IFF_MULTICAST | IFF_BROADCAST;
            (*eth0).ifa_addr = eth0_addr.cast();
            (*eth0).ifa_netmask = eth0_mask.cast();
            (*eth0).ifa_broadaddr = core::ptr::null();
            (*eth0).ifa_data = core::ptr::null();
            (*eth0).ifa_next = lo; // eth0 → lo → NULL
            *ifap = eth0;
        } else {
            // No network — just loopback.
            *ifap = lo;
        }
    }

    0
}

/// Free memory allocated by `getifaddrs()`.
///
/// No-op: our implementation uses static storage.
#[unsafe(no_mangle)]
pub extern "C" fn freeifaddrs(_ifa: *mut Ifaddrs) {
    // Static storage — nothing to free.
}

// ---------------------------------------------------------------------------
// getservbyname / getservbyport — service database
// ---------------------------------------------------------------------------

/// Service database entry.
#[repr(C)]
pub struct Servent {
    /// Official service name.
    pub s_name: *const u8,
    /// Alias list (NULL-terminated).
    pub s_aliases: *const *const u8,
    /// Port number (network byte order).
    pub s_port: i32,
    /// Protocol name.
    pub s_proto: *const u8,
}

/// Well-known services (subset of /etc/services).
struct ServiceEntry {
    name: &'static [u8],
    port: u16,
    proto: &'static [u8],
}

/// Built-in service database — covers the most commonly needed services.
static SERVICES: &[ServiceEntry] = &[
    ServiceEntry { name: b"echo",     port: 7,     proto: b"tcp" },
    ServiceEntry { name: b"echo",     port: 7,     proto: b"udp" },
    ServiceEntry { name: b"ftp-data", port: 20,    proto: b"tcp" },
    ServiceEntry { name: b"ftp",      port: 21,    proto: b"tcp" },
    ServiceEntry { name: b"ssh",      port: 22,    proto: b"tcp" },
    ServiceEntry { name: b"telnet",   port: 23,    proto: b"tcp" },
    ServiceEntry { name: b"smtp",     port: 25,    proto: b"tcp" },
    ServiceEntry { name: b"dns",      port: 53,    proto: b"udp" },
    ServiceEntry { name: b"domain",   port: 53,    proto: b"udp" },
    ServiceEntry { name: b"domain",   port: 53,    proto: b"tcp" },
    ServiceEntry { name: b"http",     port: 80,    proto: b"tcp" },
    ServiceEntry { name: b"pop3",     port: 110,   proto: b"tcp" },
    ServiceEntry { name: b"nntp",     port: 119,   proto: b"tcp" },
    ServiceEntry { name: b"ntp",      port: 123,   proto: b"udp" },
    ServiceEntry { name: b"imap",     port: 143,   proto: b"tcp" },
    ServiceEntry { name: b"snmp",     port: 161,   proto: b"udp" },
    ServiceEntry { name: b"https",    port: 443,   proto: b"tcp" },
    ServiceEntry { name: b"smtps",    port: 465,   proto: b"tcp" },
    ServiceEntry { name: b"submission", port: 587, proto: b"tcp" },
    ServiceEntry { name: b"imaps",    port: 993,   proto: b"tcp" },
    ServiceEntry { name: b"pop3s",    port: 995,   proto: b"tcp" },
    ServiceEntry { name: b"socks",    port: 1080,  proto: b"tcp" },
    ServiceEntry { name: b"mysql",    port: 3306,  proto: b"tcp" },
    ServiceEntry { name: b"postgresql", port: 5432, proto: b"tcp" },
    ServiceEntry { name: b"redis",    port: 6379,  proto: b"tcp" },
    ServiceEntry { name: b"http-alt", port: 8080,  proto: b"tcp" },
    ServiceEntry { name: b"http-alt", port: 8443,  proto: b"tcp" },
];

/// Static storage for getservbyname/getservbyport results.
static mut SERVENT_NAME: [u8; 32] = [0u8; 32];
static mut SERVENT_PROTO: [u8; 8] = [0u8; 8];
static mut SERVENT_ALIASES: [*const u8; 1] = [core::ptr::null()];
static mut SERVENT_RESULT: Servent = Servent {
    s_name: core::ptr::null(),
    s_aliases: core::ptr::null(),
    s_port: 0,
    s_proto: core::ptr::null(),
};

/// Fill the static Servent from a ServiceEntry.
///
/// # Safety
///
/// Modifies static mutable storage.  Not thread-safe.
unsafe fn fill_servent(entry: &ServiceEntry) -> *const Servent {
    let name_ptr = core::ptr::addr_of_mut!(SERVENT_NAME) as *mut u8;
    let proto_ptr = core::ptr::addr_of_mut!(SERVENT_PROTO) as *mut u8;
    let aliases_ptr = core::ptr::addr_of_mut!(SERVENT_ALIASES);
    let result_ptr = core::ptr::addr_of_mut!(SERVENT_RESULT);

    // SAFETY: All static buffers are valid for the lifetime of the program,
    // and we only write within their bounds (name≤31, proto≤7).
    unsafe {
        // Zero and copy name.
        let nlen = entry.name.len().min(31);
        core::ptr::write_bytes(name_ptr, 0, 32);
        core::ptr::copy_nonoverlapping(entry.name.as_ptr(), name_ptr, nlen);

        // Zero and copy proto.
        let plen = entry.proto.len().min(7);
        core::ptr::write_bytes(proto_ptr, 0, 8);
        core::ptr::copy_nonoverlapping(entry.proto.as_ptr(), proto_ptr, plen);

        // Empty alias list.
        (*aliases_ptr)[0] = core::ptr::null();

        // Assemble result.
        (*result_ptr).s_name = name_ptr;
        (*result_ptr).s_aliases = (*aliases_ptr).as_ptr();
        (*result_ptr).s_port = entry.port.to_be() as i32;
        (*result_ptr).s_proto = proto_ptr;
    }

    result_ptr
}

/// Look up a service by name and protocol.
///
/// Returns a pointer to a static `Servent`, or NULL if not found.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
/// `proto` may be null (match any protocol) or a null-terminated protocol name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getservbyname(
    name: *const u8,
    proto: *const u8,
) -> *const Servent {
    if name.is_null() {
        return core::ptr::null();
    }

    let name_len = unsafe { crate::string::strlen(name) };
    let name_slice = unsafe { core::slice::from_raw_parts(name, name_len) };

    let proto_slice = if proto.is_null() {
        &[]
    } else {
        let plen = unsafe { crate::string::strlen(proto) };
        unsafe { core::slice::from_raw_parts(proto, plen) }
    };

    for entry in SERVICES {
        if entry.name == name_slice {
            if proto_slice.is_empty() || entry.proto == proto_slice {
                return unsafe { fill_servent(entry) };
            }
        }
    }
    core::ptr::null()
}

/// Look up a service by port number and protocol.
///
/// `port` is in network byte order.
///
/// # Safety
///
/// `proto` may be null (match any protocol) or a null-terminated protocol name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getservbyport(
    port: i32,
    proto: *const u8,
) -> *const Servent {
    let host_port = u16::from_be(port as u16);

    let proto_slice = if proto.is_null() {
        &[]
    } else {
        let plen = unsafe { crate::string::strlen(proto) };
        unsafe { core::slice::from_raw_parts(proto, plen) }
    };

    for entry in SERVICES {
        if entry.port == host_port {
            if proto_slice.is_empty() || entry.proto == proto_slice {
                return unsafe { fill_servent(entry) };
            }
        }
    }
    core::ptr::null()
}

// ---------------------------------------------------------------------------
// getprotobyname / getprotobynumber — protocol database
// ---------------------------------------------------------------------------

/// Protocol database entry.
#[repr(C)]
pub struct Protoent {
    /// Official protocol name.
    pub p_name: *const u8,
    /// Alias list (NULL-terminated).
    pub p_aliases: *const *const u8,
    /// Protocol number.
    pub p_proto: i32,
}

/// Well-known protocols (subset of /etc/protocols).
struct ProtoEntry {
    name: &'static [u8],
    number: i32,
    aliases: &'static [&'static [u8]],
}

static PROTOCOLS: &[ProtoEntry] = &[
    ProtoEntry { name: b"ip",       number: 0,   aliases: &[b"IP"] },
    ProtoEntry { name: b"icmp",     number: 1,   aliases: &[b"ICMP"] },
    ProtoEntry { name: b"igmp",     number: 2,   aliases: &[b"IGMP"] },
    ProtoEntry { name: b"tcp",      number: 6,   aliases: &[b"TCP"] },
    ProtoEntry { name: b"udp",      number: 17,  aliases: &[b"UDP"] },
    ProtoEntry { name: b"ipv6",     number: 41,  aliases: &[b"IPv6"] },
    ProtoEntry { name: b"gre",      number: 47,  aliases: &[b"GRE"] },
    ProtoEntry { name: b"esp",      number: 50,  aliases: &[b"ESP"] },
    ProtoEntry { name: b"ah",       number: 51,  aliases: &[b"AH"] },
    ProtoEntry { name: b"icmpv6",   number: 58,  aliases: &[b"ICMPv6"] },
    ProtoEntry { name: b"sctp",     number: 132, aliases: &[b"SCTP"] },
];

static mut PROTOENT_NAME: [u8; 16] = [0u8; 16];
static mut PROTOENT_ALIASES: [*const u8; 2] = [core::ptr::null(); 2];
static mut PROTOENT_ALIAS_BUF: [u8; 16] = [0u8; 16];
static mut PROTOENT_RESULT: Protoent = Protoent {
    p_name: core::ptr::null(),
    p_aliases: core::ptr::null(),
    p_proto: 0,
};

/// Fill the static Protoent from a ProtoEntry.
///
/// # Safety
///
/// Modifies static mutable storage.
unsafe fn fill_protoent(entry: &ProtoEntry) -> *const Protoent {
    let name_raw = core::ptr::addr_of_mut!(PROTOENT_NAME) as *mut u8;
    let aliases_ptr = core::ptr::addr_of_mut!(PROTOENT_ALIASES);
    let alias_buf_raw = core::ptr::addr_of_mut!(PROTOENT_ALIAS_BUF) as *mut u8;
    let result_ptr = core::ptr::addr_of_mut!(PROTOENT_RESULT);

    // SAFETY: All static buffers are valid for the lifetime of the program,
    // and we only write within their bounds (name≤15, alias≤15).
    unsafe {
        // Zero and copy name.
        let nlen = entry.name.len().min(15);
        core::ptr::write_bytes(name_raw, 0, 16);
        core::ptr::copy_nonoverlapping(entry.name.as_ptr(), name_raw, nlen);

        // Set up alias list (first alias if available, then NULL).
        if let Some(&alias) = entry.aliases.first() {
            let alen = alias.len().min(15);
            core::ptr::write_bytes(alias_buf_raw, 0, 16);
            core::ptr::copy_nonoverlapping(alias.as_ptr(), alias_buf_raw, alen);
            (*aliases_ptr)[0] = alias_buf_raw;
            (*aliases_ptr)[1] = core::ptr::null();
        } else {
            (*aliases_ptr)[0] = core::ptr::null();
        }

        (*result_ptr).p_name = name_raw;
        (*result_ptr).p_aliases = (*aliases_ptr).as_ptr();
        (*result_ptr).p_proto = entry.number;
    }

    result_ptr
}

/// Look up a protocol by name.
///
/// Returns a pointer to a static `Protoent`, or NULL if not found.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getprotobyname(name: *const u8) -> *const Protoent {
    if name.is_null() {
        return core::ptr::null();
    }

    let name_len = unsafe { crate::string::strlen(name) };
    let name_slice = unsafe { core::slice::from_raw_parts(name, name_len) };

    for entry in PROTOCOLS {
        // Match by name (case-insensitive).
        if entry.name.len() == name_slice.len()
            && entry.name.iter().zip(name_slice.iter())
                .all(|(&a, &b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
        {
            return unsafe { fill_protoent(entry) };
        }
        // Also match against aliases.
        for &alias in entry.aliases {
            if alias.len() == name_slice.len()
                && alias.iter().zip(name_slice.iter())
                    .all(|(&a, &b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
            {
                return unsafe { fill_protoent(entry) };
            }
        }
    }
    core::ptr::null()
}

/// Look up a protocol by number.
///
/// Returns a pointer to a static `Protoent`, or NULL if not found.
#[unsafe(no_mangle)]
pub extern "C" fn getprotobynumber(number: i32) -> *const Protoent {
    for entry in PROTOCOLS {
        if entry.number == number {
            // SAFETY: single-threaded static storage.
            return unsafe { fill_protoent(entry) };
        }
    }
    core::ptr::null()
}

// ---------------------------------------------------------------------------
// Error translation
// ---------------------------------------------------------------------------

/// Translate kernel network error codes to POSIX errno values.
///
/// The kernel returns negative error codes; this converts them to
/// the appropriate socket-specific errno.
/// Translate a kernel error code (negative i64 from syscall return) to
/// a POSIX errno value.
///
/// Kernel error codes are the discriminant values of `KernelError`:
///
/// | Code | KernelError       | POSIX errno     |
/// |------|-------------------|-----------------|
/// | -1   | InternalError     | EIO             |
/// | -2   | NotSupported      | ENOTSUP         |
/// | -3   | InvalidArgument   | EINVAL          |
/// | -4   | WouldBlock        | EAGAIN          |
/// | -5   | Cancelled         | ECANCELED       |
/// | -6   | TimedOut          | ETIMEDOUT       |
/// | -100 | OutOfMemory       | ENOMEM          |
/// | -200 | NoSuchProcess     | ESRCH           |
/// | -300 | ChannelClosed     | ECONNRESET      |
/// | -301 | ChannelFull       | EAGAIN          |
/// | -304 | ResourceExhausted | ENOMEM          |
/// | -400 | PermissionDenied  | EACCES          |
/// | -500 | NotFound          | ENOENT          |
/// | -501 | AlreadyExists     | EADDRINUSE      |
/// | -505 | InvalidHandle     | EBADF           |
/// | -600 | IoError           | EIO             |
/// | -601 | NoSuchDevice      | ENODEV          |
fn translate_net_error(code: i64) -> i32 {
    match code {
        // General.
        -1  => errno::EIO,           // InternalError
        -2  => errno::ENOTSUP,       // NotSupported
        -3  => errno::EINVAL,        // InvalidArgument
        -4  => errno::EAGAIN,        // WouldBlock
        -5  => errno::ECANCELED,     // Cancelled
        -6  => errno::ETIMEDOUT,     // TimedOut

        // Memory.
        -100 => errno::ENOMEM,       // OutOfMemory

        // Process.
        -200 => errno::ESRCH,        // NoSuchProcess

        // IPC.
        -300 => errno::ECONNRESET,   // ChannelClosed
        -301 => errno::EAGAIN,       // ChannelFull
        -304 => errno::ENOMEM,       // ResourceExhausted

        // Capability / permission.
        -400 => errno::EACCES,       // PermissionDenied
        -401 => errno::EACCES,       // InvalidCapability

        // Filesystem / not-found.
        -500 => errno::ENOENT,       // NotFound  (also ECONNREFUSED for connect)
        -501 => errno::EADDRINUSE,   // AlreadyExists
        -505 => errno::EBADF,        // InvalidHandle

        // Device / I/O.
        -600 => errno::EIO,          // IoError
        -601 => errno::ENODEV,       // NoSuchDevice

        _ => errno::EIO,             // Unknown → generic I/O error
    }
}

// ---------------------------------------------------------------------------
// Tests — pure logic functions only (no syscalls needed)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Byte-order tests --

    #[test]
    fn test_htons_ntohs_roundtrip() {
        for val in [0u16, 1, 80, 443, 8080, 0xFFFF] {
            assert_eq!(ntohs(htons(val)), val);
        }
    }

    #[test]
    fn test_htonl_ntohl_roundtrip() {
        for val in [0u32, 1, 0x7F000001, 0xC0A80001, 0xFFFFFFFF] {
            assert_eq!(ntohl(htonl(val)), val);
        }
    }

    #[test]
    fn test_htons_big_endian() {
        // On little-endian x86_64, htons should swap bytes.
        let val: u16 = 0x1234;
        let net = htons(val);
        assert_eq!(net.to_be(), val.to_be());
        // Round-trip must recover original.
        assert_eq!(ntohs(net), val);
    }

    #[test]
    fn test_htonl_big_endian() {
        let val: u32 = 0x12345678;
        let net = htonl(val);
        assert_eq!(ntohl(net), val);
    }

    // -- inet_addr tests --

    #[test]
    fn test_inet_addr_valid() {
        let addr = unsafe { inet_addr(c"127.0.0.1".as_ptr().cast::<u8>()) };
        // 127.0.0.1 in network byte order = 0x7F000001 big-endian.
        assert_eq!(addr, u32::from_be_bytes([127, 0, 0, 1]));
    }

    #[test]
    fn test_inet_addr_zeros() {
        let addr = unsafe { inet_addr(c"0.0.0.0".as_ptr().cast::<u8>()) };
        assert_eq!(addr, 0);
    }

    #[test]
    fn test_inet_addr_broadcast() {
        let addr = unsafe { inet_addr(c"255.255.255.255".as_ptr().cast::<u8>()) };
        assert_eq!(addr, u32::from_be_bytes([255, 255, 255, 255]));
    }

    #[test]
    fn test_inet_addr_typical() {
        let addr = unsafe { inet_addr(c"192.168.1.1".as_ptr().cast::<u8>()) };
        assert_eq!(addr, u32::from_be_bytes([192, 168, 1, 1]));
    }

    #[test]
    fn test_inet_addr_invalid_null() {
        let addr = unsafe { inet_addr(core::ptr::null()) };
        assert_eq!(addr, 0xFFFF_FFFF); // INADDR_NONE
    }

    #[test]
    fn test_inet_addr_invalid_format() {
        // Too few octets.
        assert_eq!(unsafe { inet_addr(c"127.0.1".as_ptr().cast::<u8>()) }, 0xFFFF_FFFF);
        // Too many octets.
        assert_eq!(unsafe { inet_addr(c"1.2.3.4.5".as_ptr().cast::<u8>()) }, 0xFFFF_FFFF);
        // Octet > 255.
        assert_eq!(unsafe { inet_addr(c"256.0.0.1".as_ptr().cast::<u8>()) }, 0xFFFF_FFFF);
        // Non-numeric.
        assert_eq!(unsafe { inet_addr(c"abc.def.ghi.jkl".as_ptr().cast::<u8>()) }, 0xFFFF_FFFF);
        // Empty.
        assert_eq!(unsafe { inet_addr(c"".as_ptr().cast::<u8>()) }, 0xFFFF_FFFF);
        // Leading dot.
        assert_eq!(unsafe { inet_addr(c".1.2.3".as_ptr().cast::<u8>()) }, 0xFFFF_FFFF);
    }

    // -- inet_ntoa tests --

    #[test]
    fn test_inet_ntoa_loopback() {
        let addr = InAddr { s_addr: u32::from_be_bytes([127, 0, 0, 1]) };
        let ptr = inet_ntoa(addr);
        let s = unsafe { c_str_to_slice(ptr) };
        assert_eq!(s, b"127.0.0.1");
    }

    #[test]
    fn test_inet_ntoa_zeros() {
        let addr = InAddr { s_addr: 0 };
        let ptr = inet_ntoa(addr);
        let s = unsafe { c_str_to_slice(ptr) };
        assert_eq!(s, b"0.0.0.0");
    }

    #[test]
    fn test_inet_ntoa_broadcast() {
        let addr = InAddr { s_addr: u32::from_be_bytes([255, 255, 255, 255]) };
        let ptr = inet_ntoa(addr);
        let s = unsafe { c_str_to_slice(ptr) };
        assert_eq!(s, b"255.255.255.255");
    }

    #[test]
    fn test_inet_addr_ntoa_roundtrip() {
        let original = c"10.20.30.40";
        let addr_val = unsafe { inet_addr(original.as_ptr().cast::<u8>()) };
        assert_ne!(addr_val, 0xFFFF_FFFF);
        let ptr = inet_ntoa(InAddr { s_addr: addr_val });
        let result = unsafe { c_str_to_slice(ptr) };
        assert_eq!(result, b"10.20.30.40");
    }

    // -- parse_port_string tests --

    #[test]
    fn test_parse_port_valid() {
        assert_eq!(parse_port_string(c"80".as_ptr().cast::<u8>()), 80);
        assert_eq!(parse_port_string(c"443".as_ptr().cast::<u8>()), 443);
        assert_eq!(parse_port_string(c"8080".as_ptr().cast::<u8>()), 8080);
        assert_eq!(parse_port_string(c"65535".as_ptr().cast::<u8>()), 65535);
        assert_eq!(parse_port_string(c"0".as_ptr().cast::<u8>()), 0);
    }

    #[test]
    fn test_parse_port_invalid() {
        // Non-numeric.
        assert_eq!(parse_port_string(c"http".as_ptr().cast::<u8>()), 0);
        // Too large.
        assert_eq!(parse_port_string(c"65536".as_ptr().cast::<u8>()), 0);
        // Null pointer.
        assert_eq!(parse_port_string(core::ptr::null()), 0);
        // Empty.
        assert_eq!(parse_port_string(c"".as_ptr().cast::<u8>()), 0);
    }

    // -- translate_net_error tests --

    #[test]
    fn test_translate_net_error_known() {
        // General errors.
        assert_eq!(translate_net_error(-1), errno::EIO);            // InternalError
        assert_eq!(translate_net_error(-2), errno::ENOTSUP);        // NotSupported
        assert_eq!(translate_net_error(-3), errno::EINVAL);         // InvalidArgument
        assert_eq!(translate_net_error(-4), errno::EAGAIN);         // WouldBlock
        assert_eq!(translate_net_error(-5), errno::ECANCELED);      // Cancelled
        assert_eq!(translate_net_error(-6), errno::ETIMEDOUT);      // TimedOut
        // Memory.
        assert_eq!(translate_net_error(-100), errno::ENOMEM);       // OutOfMemory
        // Capability / permission.
        assert_eq!(translate_net_error(-400), errno::EACCES);       // PermissionDenied
        // Filesystem.
        assert_eq!(translate_net_error(-500), errno::ENOENT);       // NotFound
        assert_eq!(translate_net_error(-501), errno::EADDRINUSE);   // AlreadyExists
        // Device.
        assert_eq!(translate_net_error(-601), errno::ENODEV);       // NoSuchDevice
    }

    #[test]
    fn test_translate_net_error_unknown() {
        assert_eq!(translate_net_error(-999), errno::EIO);
        assert_eq!(translate_net_error(-42), errno::EIO);
    }

    // -- SockaddrIn layout tests --

    #[test]
    fn test_sockaddr_in_size() {
        // sockaddr_in should be 16 bytes (like Linux).
        assert_eq!(core::mem::size_of::<SockaddrIn>(), 16);
    }

    #[test]
    fn test_sockaddr_size() {
        // sockaddr should also be 16 bytes.
        assert_eq!(core::mem::size_of::<Sockaddr>(), 16);
    }

    // -- Helper --

    /// Read a null-terminated C string into a byte slice.
    unsafe fn c_str_to_slice(ptr: *const u8) -> &'static [u8] {
        if ptr.is_null() {
            return &[];
        }
        let mut len = 0;
        while unsafe { *ptr.add(len) } != 0 {
            len += 1;
        }
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }
}
