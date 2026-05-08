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
            // Just record the port.
            meta.bound_port = sin.sin_port; // store in network order
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
    set_meta(new_fd, SocketMeta {
        sock_type: SOCK_STREAM,
        bound_port: 0,
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

/// Get the name of the peer socket.
///
/// Stub: returns ENOSYS (our kernel doesn't expose peer address).
#[unsafe(no_mangle)]
pub extern "C" fn getpeername(
    fd: i32,
    _addr: *mut Sockaddr,
    _addrlen: *mut SocklenT,
) -> i32 {
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get the local name of a socket.
///
/// Stub: returns ENOSYS (our kernel doesn't expose local binding info).
#[unsafe(no_mangle)]
pub extern "C" fn getsockname(
    fd: i32,
    _addr: *mut Sockaddr,
    _addrlen: *mut SocklenT,
) -> i32 {
    if fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
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
