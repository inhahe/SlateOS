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
//! - `bind(fd, addr, len)` on UDP → `SYS_UDP_BIND(port)`
//! - `sendto(fd, ...)` on UDP → `SYS_UDP_SEND(handle, ip, port, buf, len)`
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

// MSG flags for send/recv.
/// Peek at incoming data without consuming.
pub const MSG_PEEK: i32 = 2;
/// Non-blocking operation.
pub const MSG_DONTWAIT: i32 = 0x40;

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
struct SocketMeta {
    /// Socket type (`SOCK_STREAM` or `SOCK_DGRAM`).
    sock_type: i32,
    /// Port bound via `bind()` (deferred until `listen()` for TCP).
    /// Network byte order.  0 if not yet bound.
    bound_port: u16,
    /// Remote peer IP address (network byte order).  Set on `connect()`.
    peer_addr: u32,
    /// Remote peer port (network byte order).  Set on `connect()`.
    peer_port: u16,
    /// Local IP address (network byte order).  Set on `bind()`.
    local_addr: u32,
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
fn get_meta(fd: i32) -> Option<SocketMeta> {
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
    if af == AF_INET6 {
        // Recognised but not implemented.
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }
    if af != AF_INET {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }
    if src.is_null() || dst.is_null() {
        return 0;
    }

    // Parse four decimal octets separated by '.'.
    // SAFETY: src is a valid null-terminated string (caller contract).
    let addr = unsafe { inet_addr(src) };

    // inet_addr returns INADDR_NONE (0xFFFFFFFF) on failure.
    // Unfortunately 255.255.255.255 is also 0xFFFFFFFF, but that's a
    // broadcast address that inet_pton should accept.  We check for
    // parse failure by re-scanning for valid characters.
    if addr == 0xFFFF_FFFF && !unsafe { is_valid_ipv4(src) } {
        return 0;
    }

    // Store in network byte order (inet_addr already returns NBO).
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
    if af == AF_INET6 {
        errno::set_errno(errno::EAFNOSUPPORT);
        return core::ptr::null();
    }
    if af != AF_INET {
        errno::set_errno(errno::EAFNOSUPPORT);
        return core::ptr::null();
    }
    if src.is_null() || dst.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null();
    }

    // Read 4 octets from src (network byte order).
    // SAFETY: src points to at least 4 bytes (caller contract).
    let octets: [u8; 4] = unsafe { [*src, *src.add(1), *src.add(2), *src.add(3)] };

    // Format into a stack buffer first, then check if it fits.
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

    // Copy to destination.
    // SAFETY: dst is valid for at least size bytes; needed <= size.
    unsafe {
        let mut j: usize = 0;
        while j < pos {
            if let Some(&b) = tmp.get(j) {
                *dst.add(j) = b;
            }
            j = j.wrapping_add(1);
        }
        *dst.add(pos) = 0; // null terminate
    }

    dst.cast_const()
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

    let Some(fd) = fdtable::alloc_fd(kind, 0) else {
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

    let Some(meta) = get_meta(fd) else {
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

            let ret = syscall2(SYS_TCP_CONNECT, u64::from(ip), u64::from(port));
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }

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
            });

            0
        }
        SOCK_DGRAM => {
            // UDP doesn't have kernel-level "connected" state yet.
            errno::set_errno(errno::ENOSYS);
            -1
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

    let ret = syscall1(SYS_TCP_ACCEPT, entry.handle);
    if ret < 0 {
        errno::set_errno(translate_net_error(ret));
        return -1;
    }

    // Allocate a new fd for the accepted connection.
    let conn_handle = ret as u64;
    let Some(new_fd) = fdtable::alloc_fd(HandleKind::TcpStream, conn_handle) else {
        // Close the connection we just accepted — no fd available.
        let _ = syscall1(SYS_TCP_CLOSE, conn_handle);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    // Store metadata for the new connected socket.
    // Our kernel doesn't return peer info from accept yet, so peer
    // address fields are zeroed.  When the kernel adds connection
    // info to the accept response, update these.
    let listener_meta = get_meta(fd);
    set_meta(new_fd, SocketMeta {
        sock_type: SOCK_STREAM,
        bound_port: listener_meta.map_or(0, |m| m.bound_port),
        peer_addr: 0,
        peer_port: 0,
        local_addr: listener_meta.map_or(0, |m| m.local_addr),
    });

    // Fill in the peer address if requested.
    // Our kernel doesn't return peer info from accept yet, so zero it.
    if !addr.is_null() && !addrlen.is_null() {
        // SAFETY: caller guarantees addr/addrlen validity.
        unsafe {
            let alen = *addrlen;
            let fill = core::cmp::min(
                alen as usize,
                core::mem::size_of::<SockaddrIn>(),
            );
            core::ptr::write_bytes(addr.cast::<u8>(), 0, fill);
            // Write the family at minimum.
            if fill >= 2 {
                (*addr).sa_family = AF_INET as u16;
            }
            *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
        }
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

    if entry.kind != HandleKind::TcpStream {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    }
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

    if entry.kind != HandleKind::TcpStream {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    }
    if entry.handle == 0 {
        errno::set_errno(errno::ENOTCONN);
        return -1;
    }
    let ret = syscall3(SYS_TCP_RECV, entry.handle, buf as u64, len as u64);
    if ret < 0 {
        errno::set_errno(translate_net_error(ret));
        return -1;
    }
    ret as isize
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
    if dest_addr.is_null() {
        errno::set_errno(errno::EDESTADDRREQ);
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

    if entry.kind != HandleKind::UdpSocket {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    }

    // SAFETY: dest_addr is non-null, addrlen checked.
    // Use read_unaligned because Sockaddr has weaker alignment than SockaddrIn.
    let sin = unsafe { core::ptr::read_unaligned(dest_addr.cast::<SockaddrIn>()) };

    if sin.sin_family != AF_INET as u16 {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }

    let ip = sin.sin_addr.s_addr;
    let port = u16::from_be(sin.sin_port);

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
    _flags: i32,
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

    if entry.kind != HandleKind::UdpSocket {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    }

    if entry.handle == 0 {
        // Socket not bound yet — can't receive.
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // The kernel returns the source address in a 6-byte buffer:
    // bytes 0-3 = IPv4 address (network byte order)
    // bytes 4-5 = port (little-endian u16)
    let mut src_info = [0u8; 6];

    // SYS_UDP_RECV: arg0=handle, arg1=buf, arg2=len, arg3=addr_out.
    let ret = syscall4(
        SYS_UDP_RECV,
        entry.handle,
        buf as u64,
        len as u64,
        src_info.as_mut_ptr() as u64,
    );
    if ret < 0 {
        errno::set_errno(translate_net_error(ret));
        return -1;
    }

    // Fill in the source address if requested.
    if !src_addr.is_null() && !addrlen.is_null() {
        // SAFETY: caller guarantees validity.
        unsafe {
            let available = *addrlen as usize;
            if available >= core::mem::size_of::<SockaddrIn>() {
                // Construct the sockaddr_in from the kernel's 6-byte response.
                let ip = u32::from_ne_bytes([
                    src_info[0], src_info[1], src_info[2], src_info[3],
                ]);
                let port_le = u16::from_le_bytes([src_info[4], src_info[5]]);

                let sa = SockaddrIn {
                    sin_family: AF_INET as u16,
                    sin_port: port_le.to_be(), // convert to network byte order
                    sin_addr: InAddr { s_addr: ip },
                    sin_zero: [0u8; 8],
                };
                // Use write_unaligned: Sockaddr has weaker alignment than SockaddrIn.
                core::ptr::write_unaligned(
                    src_addr.cast::<SockaddrIn>(),
                    sa,
                );
            }
            *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
        }
    }

    ret as isize
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

    if entry.kind != HandleKind::TcpStream {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    }
    if entry.handle == 0 {
        errno::set_errno(errno::ENOTCONN);
        return -1;
    }
    // Half-close is not supported.  Full shutdown closes the
    // kernel handle and marks the fd as disconnected.
    if how == SHUT_RDWR {
        let ret = syscall1(SYS_TCP_CLOSE, entry.handle);
        if ret < 0 {
            errno::set_errno(translate_net_error(ret));
            return -1;
        }
        // Mark the handle as closed but keep the fd entry so
        // subsequent send/recv return the correct error.
        // Discarding old entry is safe: we just closed its kernel handle above.
        let _ = fdtable::install_fd(fd, HandleKind::TcpStream, 0);
    }
    0
}

// ---------------------------------------------------------------------------
// setsockopt() / getsockopt() stubs
// ---------------------------------------------------------------------------

/// Set a socket option.
///
/// Stub: accepts common options silently (programs often set
/// `SO_REUSEADDR` before bind).
///
/// Returns 0 on success (always succeeds).
#[unsafe(no_mangle)]
pub extern "C" fn setsockopt(
    fd: i32,
    _level: i32,
    _optname: i32,
    _optval: *const u8,
    _optlen: SocklenT,
) -> i32 {
    // Validate the fd is a socket.
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    match entry.kind {
        HandleKind::TcpStream | HandleKind::TcpListener | HandleKind::UdpSocket => {
            // Accept silently — our kernel doesn't have per-socket options.
            0
        }
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            -1
        }
    }
}

/// Get a socket option.
///
/// Stub: returns default values for common options.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getsockopt(
    fd: i32,
    _level: i32,
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

        // Return a reasonable default based on the option.
        let val: i32 = match optname {
            SO_TYPE => get_meta(fd).map_or(0, |m| m.sock_type),
            SO_ERROR | SO_REUSEADDR | SO_KEEPALIVE => 0, // no pending error / options disabled
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
/// For TCP sockets connected via `connect()`, returns the server's
/// address.  For accepted sockets, returns zeros (peer info is not
/// yet returned by the kernel's accept syscall).
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

    // If no peer address recorded, the socket is not connected.
    if meta.peer_addr == 0 && meta.peer_port == 0 {
        errno::set_errno(errno::ENOTCONN);
        return -1;
    }

    // Build a SockaddrIn with the peer address.
    let sin = SockaddrIn {
        sin_family: AF_INET as u16,
        sin_port: meta.peer_port,
        sin_addr: InAddr { s_addr: meta.peer_addr },
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
/// ## Limitations
///
/// - Only supports `AF_INET` (IPv4).
/// - Always returns the numeric IP address (no reverse DNS).
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
        // Always numeric for now (no reverse DNS).
        let _ = flags; // NI_NAMEREQD would fail, but we always return numeric.
        let ip_bytes = addr.sin_addr.s_addr.to_ne_bytes();
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
/// Stub: sends only the first iov element using `send`.
/// Ancillary data is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sendmsg(fd: i32, msg: *const Msghdr, flags: i32) -> isize {
    if msg.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let m = unsafe { &*msg };
    if m.msg_iov.is_null() || m.msg_iovlen == 0 {
        return 0;
    }

    // Send the first iov element.
    // SAFETY: msg_iov is non-null, msg_iovlen > 0.
    let iov = unsafe { &*m.msg_iov };
    unsafe { send(fd, iov.iov_base, iov.iov_len, flags) }
}

/// Receive a message from a socket using a message header.
///
/// Stub: receives into the first iov element using `recv`.
/// Ancillary data is not populated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn recvmsg(fd: i32, msg: *mut Msghdr, flags: i32) -> isize {
    if msg.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let m = unsafe { &mut *msg };
    if m.msg_iov.is_null() || m.msg_iovlen == 0 {
        return 0;
    }

    // Receive into the first iov element.
    let iov = unsafe { &*m.msg_iov };
    let ret = unsafe { recv(fd, iov.iov_base, iov.iov_len, flags) };

    m.msg_flags = 0;
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
    if name == b"eth0" || name == b"lo" {
        // eth0 = index 1, lo = index 1 (we only have one real interface).
        1
    } else {
        0
    }
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
// Error translation
// ---------------------------------------------------------------------------

/// Translate kernel network error codes to POSIX errno values.
///
/// The kernel returns negative error codes; this converts them to
/// the appropriate socket-specific errno.
fn translate_net_error(code: i64) -> i32 {
    match code {
        -100 => errno::ECONNREFUSED, // NOT_FOUND → connection refused / host not found
        -101 => errno::EADDRINUSE,   // ALREADY_EXISTS → address in use
        -102 => errno::EINVAL,       // INVALID_ARGUMENT
        -103 => errno::EBADF,        // BAD_HANDLE
        -200 => errno::ENOMEM,       // OUT_OF_MEMORY
        -202 => errno::EAGAIN,       // WOULD_BLOCK
        -300 => errno::ENOTSUP,      // NOT_SUPPORTED
        -400 => errno::EACCES,       // PERMISSION_DENIED
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
        assert_eq!(translate_net_error(-100), errno::ECONNREFUSED);
        assert_eq!(translate_net_error(-101), errno::EADDRINUSE);
        assert_eq!(translate_net_error(-102), errno::EINVAL);
        assert_eq!(translate_net_error(-200), errno::ENOMEM);
        assert_eq!(translate_net_error(-202), errno::EAGAIN);
        assert_eq!(translate_net_error(-400), errno::EACCES);
    }

    #[test]
    fn test_translate_net_error_unknown() {
        assert_eq!(translate_net_error(-999), errno::EIO);
        assert_eq!(translate_net_error(-1), errno::EIO);
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
