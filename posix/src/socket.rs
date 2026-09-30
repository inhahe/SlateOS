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
use crate::interrupt::{Mark, Restart};
use crate::syscall::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Unspecified address family (used to disconnect UDP, or as wildcard).
pub const AF_UNSPEC: i32 = 0;
/// Unix domain sockets (local IPC).
pub const AF_UNIX: i32 = 1;
/// Synonym for `AF_UNIX`.
pub const AF_LOCAL: i32 = AF_UNIX;
/// IPv4 Internet protocols.
pub const AF_INET: i32 = 2;
/// IPv6 Internet protocols.
pub const AF_INET6: i32 = 10;
/// Link-level addresses (`sockaddr_ll`), as `getifaddrs` gives a link's.
pub const AF_PACKET: i32 = 17;
/// Protocol family aliases.
pub const PF_UNIX: i32 = AF_UNIX;
/// IPv4 (alias).
pub const PF_INET: i32 = AF_INET;
/// IPv6 (alias).
pub const PF_INET6: i32 = AF_INET6;

/// Sequenced, reliable, connection-based byte streams (TCP).
pub const SOCK_STREAM: i32 = 1;
/// Connectionless, unreliable datagrams (UDP).
pub const SOCK_DGRAM: i32 = 2;
/// Raw network protocol access.
pub const SOCK_RAW: i32 = 3;
/// Sequenced, reliable, connection-based, fixed-length datagrams.
pub const SOCK_SEQPACKET: i32 = 5;

/// Default protocol (auto-select based on socket type).
pub const IPPROTO_IP: i32 = 0;
/// ICMP protocol number.
pub const IPPROTO_ICMP: i32 = 1;
/// TCP protocol number.
pub const IPPROTO_TCP: i32 = 6;
/// UDP protocol number.
pub const IPPROTO_UDP: i32 = 17;
/// IPv6 protocol number (for `setsockopt` level).
pub const IPPROTO_IPV6: i32 = 41;
/// ICMPv6 protocol number.
pub const IPPROTO_ICMPV6: i32 = 58;
/// Raw IP protocol number.
pub const IPPROTO_RAW: i32 = 255;

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
/// Receive low watermark (minimum bytes for readability).
pub const SO_RCVLOWAT: i32 = 18;
/// Send low watermark (minimum space for writability).
pub const SO_SNDLOWAT: i32 = 19;

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
/// Set the IP Time-To-Live for unicast packets.
pub const IP_TTL: i32 = 2;
/// Set the IP Type-of-Service / DSCP field.
pub const IP_TOS: i32 = 1;
/// Set the TTL for multicast packets.
pub const IP_MULTICAST_TTL: i32 = 33;
/// Set the loopback mode for multicast packets.
pub const IP_MULTICAST_LOOP: i32 = 34;

// IPv6-level socket options (IPPROTO_IPV6).
/// Restrict socket to IPv6-only (no IPv4-mapped addresses).
pub const IPV6_V6ONLY: i32 = 26;
/// Unicast hop limit.
pub const IPV6_UNICAST_HOPS: i32 = 16;
/// Multicast hop limit.
pub const IPV6_MULTICAST_HOPS: i32 = 18;
/// Multicast loopback.
pub const IPV6_MULTICAST_LOOP: i32 = 19;
/// Join a multicast group (IPv6).
pub const IPV6_JOIN_GROUP: i32 = 20;
/// Leave a multicast group (IPv6).
pub const IPV6_LEAVE_GROUP: i32 = 21;

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
pub const MSG_CMSG_CLOEXEC: i32 = 0x4000_0000;

/// Socket type flag: set `O_NONBLOCK` on the new socket (Linux extension).
pub const SOCK_NONBLOCK: i32 = 0o4000;
/// Socket type flag: set close-on-exec on the new socket (Linux extension).
pub const SOCK_CLOEXEC: i32 = 0o2_000_000;

/// Mask of bits used for the socket type identifier itself (the low 4
/// bits).  Linux `include/linux/net.h` defines `SOCK_TYPE_MASK` as 0xf;
/// any bit above 0xf in `sock_type` must be a recognised type-flag
/// (`SOCK_NONBLOCK` or `SOCK_CLOEXEC`) or `socket()` returns EINVAL.
pub const SOCK_TYPE_MASK: i32 = 0xf;

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

/// IPv6 address (128 bits, network byte order).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct In6Addr {
    /// 16 bytes of IPv6 address in network byte order.
    pub s6_addr: [u8; 16],
}

/// `in6addr_any` — the IPv6 wildcard address, all sixteen bytes zero.
///
/// This is a VARIABLE and not a macro, which is why its absence is a link
/// error rather than a compile error: `<netinet/in.h>` declares it `extern`
/// and every program that binds an IPv6 listening socket references it.
/// `IN6ADDR_ANY_INIT` is the macro form and needs nothing from us.
///
/// Measured need: one of the twenty undefined symbols in upstream CMake's link
/// against our libc — see `scripts/cmake-spike/README.md`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static in6addr_any: In6Addr = In6Addr { s6_addr: [0; 16] };

/// `in6addr_loopback` — `::1`.
///
/// Added beside `in6addr_any` although only that one was in the measured set.
/// The two are declared together in `<netinet/in.h>`, defined together in every
/// libc, and used together; shipping one of a pair because one of a pair was
/// the symbol that happened to be reported is how the next link fails on the
/// other.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static in6addr_loopback: In6Addr = In6Addr {
    s6_addr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
};

/// IPv6 socket address.
///
/// Layout matches Linux `struct sockaddr_in6` (28 bytes):
///   sin6_family(2) + sin6_port(2) + sin6_flowinfo(4) +
///   sin6_addr(16) + sin6_scope_id(4) = 28.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrIn6 {
    /// Address family — always `AF_INET6`.
    pub sin6_family: u16,
    /// Port number in network byte order.
    pub sin6_port: u16,
    /// IPv6 flow label and traffic class.
    pub sin6_flowinfo: u32,
    /// IPv6 address.
    pub sin6_addr: In6Addr,
    /// Scope ID (e.g., link-local interface index).
    pub sin6_scope_id: u32,
}

/// Unix domain socket address.
///
/// Layout matches Linux `struct sockaddr_un` (110 bytes):
///   sun_family(2) + sun_path(108) = 110.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrUn {
    /// Address family — always `AF_UNIX`.
    pub sun_family: u16,
    /// Pathname (null-terminated, or abstract with leading NUL).
    pub sun_path: [u8; 108],
}

/// Generic socket address storage, large enough for any address family.
///
/// Layout matches Linux `struct sockaddr_storage` (128 bytes):
///   ss_family(2) + __ss_padding(126) = 128, aligned to 8 bytes.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct SockaddrStorage {
    /// Address family.
    pub ss_family: u16,
    /// Padding (implementation detail — do not access directly).
    __ss_padding: [u8; 126],
}

/// IPv6 "any" address (all zeros).
pub const IN6ADDR_ANY_INIT: In6Addr = In6Addr { s6_addr: [0; 16] };
/// IPv6 loopback address (::1).
pub const IN6ADDR_LOOPBACK_INIT: In6Addr = In6Addr {
    s6_addr: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
};

/// Size type used for address lengths.
pub type SocklenT = u32;

// ---------------------------------------------------------------------------
// Per-fd socket metadata
// ---------------------------------------------------------------------------

/// Maximum sockets tracked in the metadata table.
///
/// Derived from `fdtable::MAX_FDS` so that socket metadata can be
/// stored for any valid fd index.  If a socket gets fd >= MAX_SOCKETS,
/// all metadata operations (setsockopt, timeouts, keepalive, etc.)
/// silently fail — which is why this must match the fd table capacity.
const MAX_SOCKETS: usize = crate::fdtable::MAX_FDS;

/// Per-fd socket state that the kernel doesn't track for us.
///
/// Stored in a side table indexed by fd number (up to `MAX_SOCKETS`).
/// Contains the socket type and any state needed between `socket()`
/// and `connect()`/`bind()`.
#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // C-compatible struct mirrors BSD socket state; booleans are the natural representation.
pub(crate) struct SocketMeta {
    /// Socket type (`SOCK_STREAM` or `SOCK_DGRAM`).
    pub(crate) sock_type: i32,
    /// Port bound via `bind()` (deferred until `listen()` for TCP).
    /// Network byte order.  0 if not yet bound.
    bound_port: u16,
    /// Remote peer IP address (network byte order).  Set on `connect()`.
    pub(crate) peer_addr: u32,
    /// Remote peer port (network byte order).  Set on `connect()`.
    pub(crate) peer_port: u16,
    /// Local IP address (network byte order).  Set on `bind()`, or by
    /// `connect()` and `accept()` to the address the route goes out from.
    local_addr: u32,
    /// `local_addr` was chosen by `connect()`, not `bind()`: a datagram
    /// disconnect forgets it.
    local_from_connect: bool,
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
    pub(crate) rcvtimeo_ms: u64,
    /// SO_SNDTIMEO: send timeout in milliseconds (0 = no timeout).
    pub(crate) sndtimeo_ms: u64,
    /// TCP_CORK: buffer small sends until uncorked (advisory; kernel
    /// doesn't support explicit corking yet, so this only affects
    /// getsockopt reporting).
    cork: bool,
}

/// Backing storage for the per-fd socket metadata table.
///
/// Indexed by fd number; only meaningful for fds with socket handle kinds.
///
/// **This table must have exactly the same scope as the fd table it is keyed
/// by.**  On the target that is process-global, because a posix process has
/// one fd table.  On host builds the fd table is per-*thread* (see
/// `fdtable::fd_store` and design-decisions.md §110, which made that split so
/// libtest's parallel threads stop handing each other fd numbers) — so this
/// table has to be per-thread too.
///
/// It was not, and the mismatch was observable: two tests on different threads
/// each allocate a socket and, drawing from *separate* per-thread fd tables,
/// both get the same fd number `N`.  They then shared one `SOCKET_META[N]`.
/// When the first finished and `close()`d, `clear_meta(N)` wiped the entry the
/// second was still using, and its next call saw a live fd with no metadata —
/// reporting `ENOTSOCK` for a perfectly good socket.  That is what made
/// `test_phase201_bind_port443_no_cap_eacces` fail intermittently (observed
/// once in roughly four full runs of the suite).
mod meta_store {
    use super::{MAX_SOCKETS, SocketMeta};

    /// The table type, named once so the two implementations agree.
    pub(super) type MetaTable = [Option<SocketMeta>; MAX_SOCKETS];

    #[cfg(target_os = "none")]
    mod imp {
        use super::{MAX_SOCKETS, MetaTable};

        static mut SOCKET_META: MetaTable = [None; MAX_SOCKETS];

        pub(super) fn table() -> *mut MetaTable {
            &raw mut SOCKET_META
        }
    }

    #[cfg(not(target_os = "none"))]
    mod imp {
        use super::{MAX_SOCKETS, MetaTable};
        use core::cell::UnsafeCell;

        std::thread_local! {
            static SOCKET_META: UnsafeCell<MetaTable> =
                const { UnsafeCell::new([None; MAX_SOCKETS]) };
        }

        /// Used only during thread-local teardown, when the real table has
        /// already been dropped — mirrors `fdtable`'s `FD_FALLBACK`, so a late
        /// `close()` from a destructor scribbles somewhere harmless rather
        /// than panicking.
        static mut META_FALLBACK: MetaTable = [None; MAX_SOCKETS];

        pub(super) fn table() -> *mut MetaTable {
            SOCKET_META
                .try_with(UnsafeCell::get)
                .unwrap_or(&raw mut META_FALLBACK)
        }
    }

    /// Raw pointer to this thread's (host) or the process's (target) table.
    pub(super) fn table() -> *mut MetaTable {
        imp::table()
    }
}

/// Get a mutable pointer to the metadata table.
#[inline]
fn meta_ptr() -> *mut meta_store::MetaTable {
    meta_store::table()
}

/// Resolve `fd` the way Linux's `sockfd_lookup_light` (net/socket.c:553) does:
/// `fdget` first, so an unopened descriptor is `EBADF`, and only then
/// `sock_from_file`, so an open non-socket is `ENOTSOCK`.
///
/// Returns `false` with `errno` already set when the descriptor is unusable.
/// Entry points whose upstream counterpart calls `sockfd_lookup_light` *before*
/// reading their user arguments (`bind`, `sendmsg`, `recvmsg`, `setsockopt`,
/// `getsockopt`, …) must call this first; the ones that import their buffer
/// first (`sendto`, `recvfrom`) must not.
fn socket_fd_is_valid(fd: i32) -> bool {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return false;
    };
    match entry.kind {
        HandleKind::TcpStream
        | HandleKind::TcpListener
        | HandleKind::UdpSocket
        | HandleKind::UnixStream => true,
        _ => {
            errno::set_errno(errno::ENOTSOCK);
            false
        }
    }
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn htons(hostshort: u16) -> u16 {
    hostshort.to_be()
}

/// Convert a 32-bit value from host to network byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn htonl(hostlong: u32) -> u32 {
    hostlong.to_be()
}

/// Convert a 16-bit value from network to host byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ntohs(netshort: u16) -> u16 {
    u16::from_be(netshort)
}

/// Convert a 32-bit value from network to host byte order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ntohl(netlong: u32) -> u32 {
    u32::from_be(netlong)
}

// ---------------------------------------------------------------------------
// socket()
// ---------------------------------------------------------------------------

/// Create a socket.
///
/// `AF_INET` with `SOCK_STREAM` (TCP) or `SOCK_DGRAM` (UDP) is
/// supported.  `SOCK_RAW` is recognized but not yet implemented
/// (returns `ENOSYS` with `CAP_NET_RAW`, `EACCES` without).
/// The kernel handle is not created until `connect()` or
/// `bind()`/`listen()`.
///
/// # Capability gate (Phase 205)
///
/// `SOCK_RAW` requires `CAP_NET_RAW`, matching Linux's check inside
/// `inet_create()`.  Without the cap, `EACCES`; with the cap, `ENOSYS`
/// (raw socket backend not implemented).
///
/// Returns a file descriptor on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn socket(domain: i32, sock_type: i32, protocol: i32) -> i32 {
    // Linux semantics (net/socket.c::__sys_socket):
    //   flags = type & ~SOCK_TYPE_MASK;
    //   if (flags & ~(SOCK_CLOEXEC | SOCK_NONBLOCK))
    //           return -EINVAL;
    //   type &= SOCK_TYPE_MASK;
    // The flag check is the very first thing the syscall does,
    // before family/protocol/type bounds checks.  A buggy caller
    // passing stray flag bits is told about it regardless of
    // whether the rest of the call is well-formed.
    //
    // Additionally, `type < 0` is rejected with EINVAL — the upper
    // sign bit cannot be a valid flag (SOCK_CLOEXEC is bit 19,
    // SOCK_NONBLOCK is bit 11, both well below bit 31).
    if sock_type < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let flags = sock_type & !SOCK_TYPE_MASK;
    if flags & !(SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Validate domain.
    if domain != AF_INET {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }

    // After flag validation, the bits we keep are the type identifier
    // (low SOCK_TYPE_MASK bits) plus the two known type-flags.
    let base_type = sock_type & SOCK_TYPE_MASK;

    // Validate type + protocol combination.
    match base_type {
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
        SOCK_RAW => {
            // Phase 205: Linux gates raw socket creation on
            // CAP_NET_RAW inside inet_create() (net/ipv4/af_inet.c).
            // The cap check runs after domain and type validation
            // but before the actual socket allocation.  Without the
            // cap, return EACCES (matching Linux's errno for
            // unprivileged raw socket attempts).  With the cap,
            // return ENOSYS: raw sockets aren't implemented yet.
            if !crate::sys_capability::has_capability(crate::sys_capability::CAP_NET_RAW) {
                errno::set_errno(errno::EACCES);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            return -1;
        }
        _ => {
            errno::set_errno(errno::EPROTONOSUPPORT);
            return -1;
        }
    }

    // Determine the initial handle kind.  The handle is 0 (no kernel
    // handle yet) — it will be set on connect/bind.
    let kind = match base_type {
        SOCK_STREAM => HandleKind::TcpStream,
        _ => HandleKind::UdpSocket,
    };

    // Sockets are bidirectional (O_RDWR).  Apply O_NONBLOCK if requested.
    let initial_flags = crate::fcntl::O_RDWR
        | if (flags & SOCK_NONBLOCK) != 0 {
            crate::fcntl::O_NONBLOCK
        } else {
            0
        };
    let Some(fd) = fdtable::alloc_fd_with_flags(kind, 0, initial_flags) else {
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    // Apply FD_CLOEXEC if requested.
    if (flags & SOCK_CLOEXEC) != 0 {
        let _ = fdtable::set_fd_flags(fd, 1); // FD_CLOEXEC = 1
    }

    // Store socket metadata for later use by connect/bind/listen.
    set_meta(
        fd,
        SocketMeta {
            sock_type: base_type,
            bound_port: 0,
            peer_addr: 0,
            peer_port: 0,
            local_addr: 0,
            local_from_connect: false,
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
            cork: false,
        },
    );

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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::too_many_lines)] // Splitting this function would fragment the connect() logic across TCP/UDP handling.
pub unsafe extern "C" fn connect(fd: i32, addr: *const Sockaddr, addrlen: SocklenT) -> i32 {
    // Linux's order, from `__sys_connect` (net/socket.c:2056): `fdget(fd)`
    // decides EBADF *first*, so a bad descriptor beats every argument error.
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // Then `move_addr_to_kernel` (net/socket.c:247).  It returns 0 early when
    // `ulen == 0` — the copy never runs — so a zero addrlen does not fault even
    // on a NULL pointer; it falls through to the protocol's length check below.
    if addrlen != 0 && addr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // `__sys_connect` uses a bare `fdget`, not `sockfd_lookup_light`, so
    // `sock_from_file` runs *after* the address copy: here ENOTSOCK ranks below
    // EFAULT.  `bind` is the other way round — see the comment there.
    let Some(mut meta) = get_meta(fd) else {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    };

    // Last, the protocol handler's own length test (`inet_stream_connect` →
    // `__inet_stream_connect`, net/ipv4/af_inet.c).
    if (addrlen as usize) < core::mem::size_of::<SockaddrIn>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: addr is non-null and addrlen >= sizeof(SockaddrIn).
    // Use read_unaligned because Sockaddr has weaker alignment than SockaddrIn.
    let sin = unsafe { core::ptr::read_unaligned(addr.cast::<SockaddrIn>()) };

    if sin.sin_family != AF_INET as u16 && meta.sock_type != SOCK_DGRAM {
        // DGRAM connect with AF_UNSPEC (sin_family==0) is valid for disconnect.
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }

    // Guard: connect() on a listening socket is invalid.  After listen()
    // the entry kind becomes TcpListener — the socket is consumed for
    // accepting connections and cannot be used for outgoing connections.
    if entry.kind == HandleKind::TcpListener {
        errno::set_errno(errno::EISCONN);
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
            // connected or has a connect in progress.
            if entry.handle != 0 {
                // POSIX: EISCONN if established, EALREADY if still connecting.
                let status =
                    crate::syscall::syscall1(crate::syscall::SYS_TCP_POLL_STATUS, entry.handle)
                        as u16;
                // POLL_WRITABLE(0x04) without POLL_ERROR(0x08) = established.
                if (status & 0x0004) != 0 && (status & 0x0008) == 0 {
                    errno::set_errno(errno::EISCONN);
                } else if (status & 0x0008) != 0 {
                    // Connection failed — allow re-use is not POSIX, but some
                    // implementations return ECONNREFUSED here.  We follow Linux
                    // which returns ECONNABORTED for a failed non-blocking connect
                    // on a second connect() call.  However the simplest correct
                    // behavior is EISCONN (socket is consumed).
                    errno::set_errno(errno::EISCONN);
                } else {
                    // Still in SYN_SENT — handshake in progress.
                    errno::set_errno(errno::EALREADY);
                }
                return -1;
            }

            // Non-blocking connect: pass flag bit 0 if O_NONBLOCK is set.
            let nb_flag: u64 = u64::from(
                fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0,
            );

            let ret = syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), nb_flag);
            if ret < 0 {
                // The kernel's blocking connect returns specific errors:
                // -2 (NotSupported) = connection refused (RST on SYN)
                // -6 (TimedOut) = no SYN-ACK after retransmission
                // Map these to correct POSIX errno values.
                let posix_err = match ret {
                    -2 => errno::ECONNREFUSED,
                    -6 => errno::ETIMEDOUT,
                    _ => translate_net_error(ret),
                };
                errno::set_errno(posix_err);
                return -1;
            }

            // For non-blocking connect, the connection is in progress.
            // Store the handle and return EINPROGRESS (POSIX requirement).
            let in_progress = nb_flag != 0;

            // Update the fd table with the kernel connection handle.
            // Discarding the old entry is safe: handle was 0 (no kernel resource to close).
            // Preserve existing status flags (e.g. O_NONBLOCK set via fcntl).
            let prev_flags = fdtable::get_status_flags(fd).unwrap_or(0);
            let _ =
                fdtable::install_fd_with_flags(fd, HandleKind::TcpStream, ret as u64, prev_flags);

            // Store peer address for getpeername().
            set_meta(
                fd,
                SocketMeta {
                    sock_type: SOCK_STREAM,
                    bound_port: meta.bound_port,
                    peer_addr: sin.sin_addr.s_addr,
                    peer_port: sin.sin_port,
                    local_addr: if meta.local_addr != 0 {
                        meta.local_addr
                    } else {
                        route_source(sin.sin_addr.s_addr).unwrap_or(0)
                    },
                    local_from_connect: meta.local_addr == 0,
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
                    cork: meta.cork,
                },
            );

            // Apply socket options that were set before connect.
            // The kernel connection starts with defaults; we need to wire
            // through any options the program set before calling connect().
            let handle = ret as u64;
            if meta.nodelay {
                let _ = syscall2(SYS_TCP_SET_NODELAY, handle, 1);
            }
            if meta.keepalive {
                let _ = syscall2(crate::syscall::SYS_TCP_SET_KEEPALIVE, handle, 1);
                // Apply custom keepalive params if any differ from defaults.
                if meta.keepidle != 75 || meta.keepintvl != 10 || meta.keepcnt != 9 {
                    let _ = syscall4(
                        crate::syscall::SYS_TCP_SET_KEEPALIVE_PARAMS,
                        handle,
                        meta.keepidle as u64,
                        meta.keepintvl as u64,
                        meta.keepcnt as u64,
                    );
                }
            }
            // TCP_CORK: tracked in metadata only (kernel doesn't have
            // explicit cork support yet).  No kernel call needed.

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
                // Linux's `udp_disconnect` forgets a source the connect chose,
                // but not one `bind` set.
                if meta.local_from_connect {
                    meta.local_addr = 0;
                    meta.local_from_connect = false;
                }
                set_meta(fd, meta);
                if entry.handle != 0 {
                    let _ = syscall3(SYS_UDP_CONNECT, entry.handle, 0, 0);
                }
                return 0;
            }

            meta.peer_addr = sin.sin_addr.s_addr;
            meta.peer_port = sin.sin_port;
            // The source this peer is reached from, as `getsockname` will
            // report it -- unless `bind` chose one.
            if meta.local_addr == 0 || meta.local_from_connect {
                meta.local_addr = route_source(sin.sin_addr.s_addr).unwrap_or(0);
                meta.local_from_connect = true;
            }
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
/// # Capability gate (Phase 201)
///
/// Ports below 1024 ("privileged ports") require `CAP_NET_BIND_SERVICE`.
/// Linux enforces this inside `inet_bind()` → `inet_csk_get_port()` /
/// `udp_lib_get_port()` via `inet_port_requires_bind_service(port)`.
/// The check runs after all argument validation (EBADF, ENOTSOCK, EFAULT,
/// EINVAL, EAFNOSUPPORT — in that order, see the body) so callers with bad
/// arguments see the argument error, not EACCES.  Port 0 (ephemeral) and
/// ports ≥ 1024 bypass the gate.
///
/// Returns 0 on success, -1 on error.
///
/// # Safety
///
/// `addr` must point to a valid `SockaddrIn`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn bind(fd: i32, addr: *const Sockaddr, addrlen: SocklenT) -> i32 {
    // Linux's order, from `__sys_bind` (net/socket.c:1835): `sockfd_lookup_light`
    // (net/socket.c:553) decides EBADF and then ENOTSOCK *before*
    // `move_addr_to_kernel` ever looks at the address, so both descriptor errors
    // outrank every argument error.  (`connect` differs: it uses a bare `fdget`,
    // so its ENOTSOCK ranks *below* EFAULT.  The asymmetry is upstream's.)
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    let Some(mut meta) = get_meta(fd) else {
        errno::set_errno(errno::ENOTSOCK);
        return -1;
    };

    // `move_addr_to_kernel` (net/socket.c:247) returns 0 early when `ulen == 0`,
    // so a zero addrlen never dereferences the pointer — it falls through to the
    // protocol's length check below and yields EINVAL, not EFAULT.
    if addrlen != 0 && addr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Last, the protocol handler's own length test (`inet_bind` →
    // `__inet_bind`, net/ipv4/af_inet.c).
    if (addrlen as usize) < core::mem::size_of::<SockaddrIn>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: addr is non-null and addrlen >= sizeof(SockaddrIn).
    // Use read_unaligned because Sockaddr has weaker alignment than SockaddrIn.
    let sin = unsafe { core::ptr::read_unaligned(addr.cast::<SockaddrIn>()) };

    if sin.sin_family != AF_INET as u16 {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }

    let port = u16::from_be(sin.sin_port);

    // Phase 201: privileged port gate.  Ports 1..1023 require
    // CAP_NET_BIND_SERVICE.  Port 0 (let the kernel pick an
    // ephemeral port) and ports >= 1024 are unrestricted.
    // Linux returns EACCES (not EPERM) for this — it's a
    // resource-access denial, not a missing-capability signal.
    if port != 0
        && port < 1024
        && !crate::sys_capability::has_capability(crate::sys_capability::CAP_NET_BIND_SERVICE)
    {
        errno::set_errno(errno::EACCES);
        return -1;
    }

    match meta.sock_type {
        SOCK_STREAM => {
            // POSIX: bind on a connected socket is EINVAL.
            if entry.handle != 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // POSIX: bind on an already-bound socket is EINVAL.
            if meta.bound_port != 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // For TCP, defer the kernel bind until listen().
            // Just record the port and local address.
            meta.bound_port = sin.sin_port; // store in network order
            meta.local_addr = sin.sin_addr.s_addr;
            meta.local_from_connect = false;
            set_meta(fd, meta);
            0
        }
        SOCK_DGRAM => {
            // POSIX: bind on an already-bound socket is EINVAL.
            if entry.handle != 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // For UDP, bind immediately.
            let ret = syscall1(SYS_UDP_BIND, u64::from(port));
            if ret < 0 {
                errno::set_errno(translate_net_error(ret));
                return -1;
            }

            // Update fd table with the kernel UDP handle.
            // Discarding old entry is safe: handle was 0 (no kernel resource).
            // Preserve existing status flags (O_NONBLOCK, etc.).
            let prev_flags = fdtable::get_status_flags(fd).unwrap_or(0);
            let _ =
                fdtable::install_fd_with_flags(fd, HandleKind::UdpSocket, ret as u64, prev_flags);
            meta.bound_port = sin.sin_port;
            meta.local_addr = sin.sin_addr.s_addr;
            meta.local_from_connect = false;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

    // Don't re-listen on an already-listening socket.
    if entry.kind == HandleKind::TcpListener && entry.handle != 0 {
        return 0;
    }

    // POSIX: listen on a connected socket is EINVAL.
    if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // If not bound, implicitly bind to port 0 (ephemeral).
    // POSIX: listen() on an unbound socket auto-binds.
    let port = if meta.bound_port == 0 {
        0
    } else {
        u16::from_be(meta.bound_port)
    };
    let ret = syscall1(SYS_TCP_BIND, u64::from(port));
    if ret < 0 {
        errno::set_errno(translate_net_error(ret));
        return -1;
    }

    // Change the fd to a TcpListener with the kernel listener handle.
    // Discarding old entry is safe: handle was 0 (unconnected socket).
    // Preserve existing status flags (O_NONBLOCK for non-blocking accept).
    let prev_flags = fdtable::get_status_flags(fd).unwrap_or(0);
    let _ = fdtable::install_fd_with_flags(fd, HandleKind::TcpListener, ret as u64, prev_flags);

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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn accept(fd: i32, addr: *mut Sockaddr, addrlen: *mut SocklenT) -> i32 {
    // Handlers counted from here (`crate::interrupt`).
    let mark = Mark::now();
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // POSIX error hierarchy for accept():
    // - ENOTSOCK: fd is not a socket at all
    // - EOPNOTSUPP: socket type doesn't support accept (UDP)
    // - EINVAL: socket is listening but handle is 0 (shouldn't happen)
    match entry.kind {
        HandleKind::TcpListener if entry.handle != 0 => { /* valid — proceed */ }
        HandleKind::TcpListener => {
            // Listener without a kernel handle (shouldn't happen normally).
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        HandleKind::TcpStream | HandleKind::UdpSocket => {
            // Connected TCP or UDP — accept is not supported.
            errno::set_errno(errno::EOPNOTSUPP);
            return -1;
        }
        _ => {
            // File, Pipe, Console — not a socket.
            errno::set_errno(errno::ENOTSOCK);
            return -1;
        }
    }

    let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;

    // Always use non-blocking accept (ACCEPT_NONBLOCK=1) to avoid the
    // kernel's limited internal timeout.  Implement blocking semantics
    // in the POSIX layer with a proper poll-wait loop.
    let ret = syscall2(SYS_TCP_ACCEPT, entry.handle, 1); // Non-blocking
    if ret >= 0 {
        // Connection available immediately — fall through.
    } else {
        let err = translate_net_error(ret);
        if (err == errno::EAGAIN || err == errno::EWOULDBLOCK) && !is_nb {
            // Blocking mode: poll-wait for a connection.
            let conn = accept_wait(entry.handle, mark);
            if conn < 0 {
                return conn;
            }
            // Fall through with conn as the handle.
            // SAFETY: addr/addrlen validity ensured by caller of accept().
            return unsafe { finish_accept(fd, conn as u64, addr, addrlen) };
        }
        errno::set_errno(err);
        return -1;
    }

    let conn_handle = ret as u64;
    // SAFETY: addr/addrlen validity ensured by caller of accept().
    unsafe { finish_accept(fd, conn_handle, addr, addrlen) }
}

/// The rule a socket call blocked on data or room ends by
/// ([`crate::interrupt`]): a handler installed with `SA_RESTART` lets it
/// wait on, unless the socket has a timeout for the call -- signal(7):
/// "unless a timeout has been set on the socket".
fn socket_rule(timeout_ms: u64) -> Restart {
    if timeout_ms > 0 {
        Restart::Never
    } else {
        Restart::IfAsked
    }
}

/// [`socket_rule`] for descriptor `fd`, receiving or sending.
pub(crate) fn wait_rule(fd: i32, receiving: bool) -> Restart {
    socket_rule(get_meta(fd).map_or(0, |m| {
        if receiving {
            m.rcvtimeo_ms
        } else {
            m.sndtimeo_ms
        }
    }))
}

/// Poll-wait for a connection on a blocking listener.
///
/// Retries `SYS_TCP_ACCEPT` (non-blocking) in a loop with 10ms sleeps
/// until a connection arrives.  Returns the connection handle (≥0) or
/// -1 with errno set.
fn accept_wait(listener_handle: u64, mark: Mark) -> i32 {
    const POLL_NS: u64 = 10_000_000; // 10ms

    loop {
        let ret = syscall2(SYS_TCP_ACCEPT, listener_handle, 1);
        if ret >= 0 {
            return ret as i32;
        }
        let err = translate_net_error(ret);
        if err != errno::EAGAIN && err != errno::EWOULDBLOCK {
            // Real error (listener closed, etc.)
            errno::set_errno(err);
            return -1;
        }
        // Still no connection — sleep then retry.
        // A handler ends the wait by the call's rule, and only now,
        // with nothing come (`crate::interrupt`).
        if mark.interrupted(Restart::IfAsked) {
            errno::set_errno(errno::EINTR);
            return -1;
        }
        crate::lowlevellock::nap(POLL_NS, Restart::IfAsked, mark);
    }
}

/// Finish the accept operation: allocate fd, query peer address, store metadata.
///
/// # Safety
///
/// `addr` and `addrlen` must be valid as per the `accept()` contract.
unsafe fn finish_accept(
    listener_fd: i32,
    conn_handle: u64,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
) -> i32 {
    // Allocate a new fd for the accepted connection.
    // Accepted sockets are bidirectional (O_RDWR).
    let Some(new_fd) =
        fdtable::alloc_fd_with_flags(HandleKind::TcpStream, conn_handle, crate::fcntl::O_RDWR)
    else {
        // Close the connection we just accepted — no fd available.
        let _ = syscall1(SYS_TCP_CLOSE, conn_handle);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    // Query peer address from the kernel.
    let mut peer_buf = [0u8; 6]; // 4 bytes IP + 2 bytes port (network order)
    let peer_ret = syscall2(SYS_TCP_PEER_ADDR, conn_handle, peer_buf.as_mut_ptr() as u64);
    let (peer_ip_nbo, peer_port_nbo) = if peer_ret == 0 {
        let ip = u32::from_ne_bytes([peer_buf[0], peer_buf[1], peer_buf[2], peer_buf[3]]);
        let port = u16::from_be_bytes([peer_buf[4], peer_buf[5]]);
        (ip, port.to_be()) // Store in network byte order for sockaddr_in
    } else {
        (0u32, 0u16)
    };

    // Store metadata for the new connected socket.
    let listener_meta = get_meta(listener_fd);
    set_meta(
        new_fd,
        SocketMeta {
            sock_type: SOCK_STREAM,
            bound_port: listener_meta.map_or(0, |m| m.bound_port),
            peer_addr: peer_ip_nbo,
            peer_port: peer_port_nbo,
            local_addr: match listener_meta.map_or(0, |m| m.local_addr) {
                0 => route_source(peer_ip_nbo).unwrap_or(0),
                a => a,
            },
            local_from_connect: false,
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
            cork: false,
        },
    );

    // Fill in the peer address if requested.
    if !addr.is_null() && !addrlen.is_null() {
        let sin = SockaddrIn {
            sin_family: AF_INET as u16,
            sin_port: peer_port_nbo,
            sin_addr: InAddr {
                s_addr: peer_ip_nbo,
            },
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
/// atomically.  Any other bits in `flags` are rejected with EINVAL,
/// matching Linux's `net/socket.c::__sys_accept4_file` prologue:
///
/// ```c
/// if (flags & ~(SOCK_CLOEXEC | SOCK_NONBLOCK))
///         return -EINVAL;
/// ```
///
/// The flag-mask check precedes the underlying accept call, so a
/// buggy caller passing garbage in `flags` sees EINVAL even when the
/// listening fd would otherwise produce EBADF / EINVAL itself.
///
/// # Safety
///
/// Same requirements as `accept`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn accept4(
    fd: i32,
    addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
    flags: i32,
) -> i32 {
    // Linux validates flags FIRST, before any fd or pointer
    // inspection.  Only SOCK_NONBLOCK and SOCK_CLOEXEC are valid.
    if flags & !(SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::too_many_lines)] // Large match over socket handle kinds (TCP / UnixStream / error cases); splitting would scatter related per-kind send handling.
pub unsafe extern "C" fn send(fd: i32, buf: *const u8, len: usize, flags: i32) -> isize {
    // Handlers counted from here (`crate::interrupt`).
    let mark = Mark::now();
    if buf.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // POSIX: send with len==0 succeeds with 0 bytes (no-op) — but only on a
    // *valid* descriptor.  `__sys_sendto` (net/socket.c:2161) always runs
    // `sockfd_lookup_light`, whatever the length, so a zero-length send is the
    // documented way to probe a socket for EBADF/ENOTSOCK and must not
    // short-circuit above the lookup.
    if len == 0 {
        return 0;
    }

    // MSG_NOSIGNAL (0x4000) is a no-op — we have no SIGPIPE.
    // MSG_DONTWAIT (0x40) — non-blocking hint (also triggered by O_NONBLOCK).
    let _ = flags; // Accepted flags: MSG_NOSIGNAL, MSG_DONTWAIT.

    match entry.kind {
        HandleKind::TcpStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }
            // MSG_DONTWAIT (0x40) or O_NONBLOCK → non-blocking mode.
            let is_nb = (flags & MSG_DONTWAIT) != 0
                || fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            if !is_nb {
                // Blocking socket: use tcp_send_wait for full-write
                // semantics.  Linux's blocking send() loops until ALL
                // bytes are accepted.  Programs depend on this.
                let timeout_ms = get_meta(fd).map_or(0u64, |m| m.sndtimeo_ms);
                return tcp_send_wait(entry.handle, buf, len, timeout_ms, mark);
            }
            // Non-blocking: try once.
            let ret = syscall3(SYS_TCP_SEND, entry.handle, buf as u64, len as u64);
            if ret >= 0 {
                return ret as isize;
            }
            // ChannelClosed from send covers two distinct POSIX errors:
            // - EPIPE: local write side shut down (SHUT_WR sent FIN), or
            //          peer cleanly closed (FIN received).
            // - ECONNRESET: peer sent RST (abortive close).
            // Distinguish by checking last_error: RST sets TCP_ERR_RESET,
            // while graceful shutdown / local SHUT_WR leaves it at NONE.
            let posix_err = translate_net_error(ret);
            if posix_err == errno::ECONNRESET {
                let last =
                    crate::syscall::syscall1(crate::syscall::SYS_TCP_LAST_ERROR, entry.handle)
                        as u8;
                if last == 2 {
                    // TCP_ERR_RESET — genuine connection reset.
                    errno::set_errno(errno::ECONNRESET);
                } else {
                    // Local shutdown or graceful close — EPIPE per POSIX.
                    errno::set_errno(errno::EPIPE);
                }
            } else {
                errno::set_errno(posix_err);
            }
            -1
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
            // Implicit bind if not yet bound (POSIX: first send on
            // unbound DGRAM socket binds to an ephemeral port).
            let handle = if entry.handle == 0 {
                let bind_ret = syscall1(SYS_UDP_BIND, 0);
                if bind_ret < 0 {
                    errno::set_errno(translate_net_error(bind_ret));
                    return -1;
                }
                let new_handle = bind_ret as u64;
                let prev_flags = fdtable::get_status_flags(fd).unwrap_or(0);
                let _ = fdtable::install_fd_with_flags(
                    fd,
                    HandleKind::UdpSocket,
                    new_handle,
                    prev_flags,
                );
                // Re-apply peer filter on the new handle.
                let peer_port_host = u16::from_be(meta.peer_port);
                let _ = syscall3(
                    SYS_UDP_CONNECT,
                    new_handle,
                    u64::from(meta.peer_addr),
                    u64::from(peer_port_host),
                );
                new_handle
            } else {
                entry.handle
            };
            let port = u16::from_be(meta.peer_port);
            let ret = syscall5(
                SYS_UDP_SEND,
                handle,
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
        HandleKind::UnixStream => {
            // Stream socket endpoint: blocking unless MSG_DONTWAIT or
            // O_NONBLOCK.  Returns partial writes like a pipe/TCP stream.
            let is_nb = (flags & MSG_DONTWAIT) != 0
                || fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
            let ret = if is_nb {
                syscall3(
                    SYS_SOCKETPAIR_TRY_SEND,
                    entry.handle,
                    buf as u64,
                    len as u64,
                )
            } else {
                syscall3(SYS_SOCKETPAIR_SEND, entry.handle, buf as u64, len as u64)
            };
            if ret == errno::native::CHANNEL_CLOSED {
                // Peer's read side gone — POSIX EPIPE (no SIGPIPE here).
                errno::set_errno(errno::EPIPE);
                return -1;
            }
            // translate() sets errno and returns -1 for negative codes,
            // or passes through the byte count for ret >= 0.
            errno::translate(ret) as isize
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::too_many_lines)] // Large match over socket handle kinds (TCP / UnixStream / error cases); splitting would scatter related per-kind recv handling.
pub unsafe extern "C" fn recv(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize {
    // Handlers counted from here (`crate::interrupt`).
    let mark = Mark::now();
    if buf.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // POSIX: recv with len==0 succeeds immediately with 0 bytes.
    // This is NOT an EOF indicator — it's a no-op.  It still requires a valid
    // descriptor: `__sys_recvfrom` (net/socket.c:2224) runs
    // `sockfd_lookup_light` regardless of `size`, so EBADF/ENOTSOCK win.
    if len == 0 {
        return 0;
    }

    // Build kernel flags from POSIX MSG_* constants.
    // MSG_PEEK (0x02), MSG_TRUNC (0x20), and MSG_DONTWAIT (0x40) are
    // passed through directly since we use matching numeric values.
    let kern_flags = (flags as u32) & (MSG_PEEK as u32 | MSG_TRUNC as u32 | MSG_DONTWAIT as u32);

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
            let is_nb = (kern_flags & MSG_DONTWAIT as u32) != 0;
            let timeout_ms = get_meta(fd).map_or(0u64, |m| m.rcvtimeo_ms);
            // MSG_WAITALL is meaningless with MSG_PEEK (peeking doesn't
            // consume data, so looping would re-read the same bytes forever).
            let waitall = (flags & MSG_WAITALL) != 0 && !is_nb && (flags & MSG_PEEK) == 0;

            // Always try non-blocking first so we can implement
            // timeout/blocking semantics in userspace consistently.
            let try_flags = kern_flags | MSG_DONTWAIT as u32;

            let ret = syscall4(
                SYS_TCP_RECV,
                entry.handle,
                buf as u64,
                len as u64,
                u64::from(try_flags),
            );
            if ret > 0 {
                // MSG_WAITALL: keep receiving until `len` bytes or EOF/error.
                if waitall && (ret as usize) < len {
                    return tcp_recv_waitall(
                        entry.handle,
                        buf,
                        len,
                        kern_flags,
                        timeout_ms,
                        ret as usize,
                        mark,
                    );
                }
                return ret as isize;
            }
            if ret == 0 {
                return 0; // EOF
            }

            let err = translate_net_error(ret);
            if (err == errno::EAGAIN || err == errno::EWOULDBLOCK) && !is_nb {
                // Blocking socket — poll-wait with SO_RCVTIMEO.
                // timeout_ms == 0 means wait indefinitely.
                if waitall {
                    return tcp_recv_waitall(
                        entry.handle,
                        buf,
                        len,
                        kern_flags,
                        timeout_ms,
                        0,
                        mark,
                    );
                }
                return tcp_recv_wait(entry.handle, buf, len, kern_flags, timeout_ms, mark);
            }
            errno::set_errno(err);
            -1
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
                    entry.handle,
                    buf,
                    len,
                    &mut src_info,
                    kern_flags,
                    timeout_ms,
                    mark,
                );
                return waited;
            }
            errno::set_errno(err);
            -1
        }
        HandleKind::UnixStream => {
            // Stream socket endpoint: blocking recv unless MSG_DONTWAIT
            // or O_NONBLOCK.  A return of 0 is EOF (peer write side gone).
            // MSG_PEEK/MSG_WAITALL are not supported by the kernel
            // stream-socket recv path; they are ignored.
            let is_nb = (kern_flags & MSG_DONTWAIT as u32) != 0;
            let ret = if is_nb {
                syscall3(
                    SYS_SOCKETPAIR_TRY_RECV,
                    entry.handle,
                    buf as u64,
                    len as u64,
                )
            } else {
                syscall3(SYS_SOCKETPAIR_RECV, entry.handle, buf as u64, len as u64)
            };
            // translate() sets errno and returns -1 for negative codes,
            // or passes through the byte count (0 = EOF) for ret >= 0.
            errno::translate(ret) as isize
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
    mark: Mark,
) -> isize {
    const POLL_NS: u64 = 10_000_000; // 10ms

    let deadline = if timeout_ms > 0 {
        let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
        now.saturating_add(timeout_ms.saturating_mul(1_000_000))
    } else {
        u64::MAX // No deadline — wait indefinitely.
    };

    loop {
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
        // Check for real errors (not just "no data yet").
        let err = translate_net_error(ret);
        if err != errno::EAGAIN && err != errno::EWOULDBLOCK {
            errno::set_errno(err);
            return -1;
        }
        // Still no data — check timeout before sleeping.
        if deadline != u64::MAX {
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            if now >= deadline {
                errno::set_errno(errno::EAGAIN);
                return -1;
            }
        }
        // A handler ends the wait by the call's rule, and only now,
        // with nothing come (`crate::interrupt`).
        if mark.interrupted(socket_rule(timeout_ms)) {
            errno::set_errno(errno::EINTR);
            return -1;
        }
        crate::lowlevellock::nap(POLL_NS, socket_rule(timeout_ms), mark);
    }
}

/// Poll-wait for TCP data with timeout (SO_RCVTIMEO support).
///
/// Uses MSG_DONTWAIT + polling loop to implement the POSIX timeout
/// semantics.  `timeout_ms == 0` means wait indefinitely (shouldn't
/// be called in that case, but handled for safety).
/// Returns bytes received or -1 (with errno set).
pub(crate) fn tcp_recv_wait(
    handle: u64,
    buf: *mut u8,
    len: usize,
    kern_flags: u32,
    timeout_ms: u64,
    mark: Mark,
) -> isize {
    const POLL_NS: u64 = 10_000_000; // 10ms

    let deadline = if timeout_ms > 0 {
        let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
        now.saturating_add(timeout_ms.saturating_mul(1_000_000))
    } else {
        u64::MAX
    };

    // Always use MSG_DONTWAIT so the kernel returns immediately.
    let flags_nb = kern_flags | MSG_DONTWAIT as u32;

    loop {
        let ret = syscall4(
            SYS_TCP_RECV,
            handle,
            buf as u64,
            len as u64,
            u64::from(flags_nb),
        );
        if ret > 0 {
            return ret as isize;
        }
        if ret == 0 {
            // 0 = EOF / connection closed — return it as-is.
            return 0;
        }
        // Negative: check if it's just WouldBlock (no data yet).
        let err = translate_net_error(ret);
        if err != errno::EAGAIN && err != errno::EWOULDBLOCK {
            // Real error (ECONNRESET, etc.)
            errno::set_errno(err);
            return -1;
        }
        // Still no data — check timeout before sleeping.
        if deadline != u64::MAX {
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            if now >= deadline {
                errno::set_errno(errno::EAGAIN);
                return -1;
            }
        }
        // A handler ends the wait by the call's rule, and only now,
        // with nothing come (`crate::interrupt`).
        if mark.interrupted(socket_rule(timeout_ms)) {
            errno::set_errno(errno::EINTR);
            return -1;
        }
        crate::lowlevellock::nap(POLL_NS, socket_rule(timeout_ms), mark);
    }
}

/// MSG_WAITALL implementation for TCP recv.
///
/// Accumulates data into `buf` until exactly `total_len` bytes have been
/// received, EOF is reached, an error occurs, or the timeout expires.
/// `already_read` is how many bytes were already received before entering
/// this function (from the initial non-blocking try).
///
/// Returns total bytes read or -1 (with errno set).
fn tcp_recv_waitall(
    handle: u64,
    buf: *mut u8,
    total_len: usize,
    kern_flags: u32,
    timeout_ms: u64,
    already_read: usize,
    mark: Mark,
) -> isize {
    const POLL_NS: u64 = 10_000_000; // 10ms

    let deadline = if timeout_ms > 0 {
        let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
        now.saturating_add(timeout_ms.saturating_mul(1_000_000))
    } else {
        u64::MAX
    };

    let flags_nb = kern_flags | MSG_DONTWAIT as u32;
    let mut got = already_read;

    while got < total_len {
        let remaining = total_len.saturating_sub(got);
        // SAFETY: buf is valid for total_len bytes; buf.add(got) is within bounds.
        let dst = unsafe { buf.add(got) };

        let ret = syscall4(
            SYS_TCP_RECV,
            handle,
            dst as u64,
            remaining as u64,
            u64::from(flags_nb),
        );
        if ret > 0 {
            got = got.saturating_add(ret as usize);
            continue;
        }
        if ret == 0 {
            // EOF — return what we have (POSIX: MSG_WAITALL returns short
            // read on EOF rather than blocking forever).
            break;
        }
        let err = translate_net_error(ret);
        if err != errno::EAGAIN && err != errno::EWOULDBLOCK {
            // Real error.  If we have partial data, return it (POSIX).
            if got > 0 {
                break;
            }
            errno::set_errno(err);
            return -1;
        }
        // WouldBlock: sleep and retry.
        if deadline != u64::MAX {
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            if now >= deadline {
                // Timeout: return what we have, or EAGAIN if nothing.
                if got > 0 {
                    break;
                }
                errno::set_errno(errno::EAGAIN);
                return -1;
            }
        }
        // A handler ends the wait by the call's rule, and only now,
        // with nothing come (`crate::interrupt`).
        if mark.interrupted(socket_rule(timeout_ms)) {
            if got > 0 {
                break;
            }
            errno::set_errno(errno::EINTR);
            return -1;
        }
        crate::lowlevellock::nap(POLL_NS, socket_rule(timeout_ms), mark);
    }

    got as isize
}

/// Poll-wait for TCP send to succeed (blocking write with SO_SNDTIMEO).
///
/// Retries `SYS_TCP_SEND` in a loop with 10ms sleeps until the kernel
/// accepts data or the timeout expires.  `timeout_ms == 0` means wait
/// indefinitely.
///
/// Returns bytes sent or -1 (with errno set).
pub(crate) fn tcp_send_wait(
    handle: u64,
    buf: *const u8,
    len: usize,
    timeout_ms: u64,
    mark: Mark,
) -> isize {
    const POLL_NS: u64 = 10_000_000; // 10ms

    let deadline = if timeout_ms > 0 {
        let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
        now.saturating_add(timeout_ms.saturating_mul(1_000_000))
    } else {
        u64::MAX
    };

    // Linux's blocking send() on TCP loops internally until ALL bytes
    // are accepted into the send buffer (or an error/signal occurs).
    // Many programs depend on this "full-write" behavior for blocking
    // sockets and don't handle short writes.  We match Linux here.
    let mut sent: usize = 0;

    while sent < len {
        let remaining = len.saturating_sub(sent);
        // SAFETY: buf is valid for `len` bytes; buf.add(sent) is in bounds.
        let ptr = unsafe { buf.add(sent) };

        let ret = syscall3(SYS_TCP_SEND, handle, ptr as u64, remaining as u64);
        if ret > 0 {
            sent = sent.saturating_add(ret as usize);
            continue;
        }
        if ret == 0 {
            // 0 bytes sent — connection may be closing.
            break;
        }
        // Negative: check if it's just WouldBlock (window still closed).
        let err = translate_net_error(ret);
        if err != errno::EAGAIN && err != errno::EWOULDBLOCK {
            // Real error — distinguish RST (ECONNRESET) from local
            // shutdown/graceful close (EPIPE).
            if sent > 0 {
                // Partial data already accepted — return that count.
                break;
            }
            if err == errno::ECONNRESET {
                let last =
                    crate::syscall::syscall1(crate::syscall::SYS_TCP_LAST_ERROR, handle) as u8;
                errno::set_errno(if last == 2 {
                    errno::ECONNRESET
                } else {
                    errno::EPIPE
                });
            } else {
                errno::set_errno(err);
            }
            return -1;
        }
        // Still can't send — check timeout before sleeping.
        if deadline != u64::MAX {
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            if now >= deadline {
                if sent > 0 {
                    break;
                }
                errno::set_errno(errno::EAGAIN);
                return -1;
            }
        }
        // A handler ends the wait by the call's rule, and only now,
        // with nothing come (`crate::interrupt`).
        if mark.interrupted(socket_rule(timeout_ms)) {
            if sent > 0 {
                break;
            }
            errno::set_errno(errno::EINTR);
            return -1;
        }
        crate::lowlevellock::nap(POLL_NS, socket_rule(timeout_ms), mark);
    }

    sent as isize
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::too_many_lines)] // Unified TCP/UDP sendto with implicit bind; splitting would scatter the logic.
pub unsafe extern "C" fn sendto(
    fd: i32,
    buf: *const u8,
    len: usize,
    flags: i32,
    dest_addr: *const Sockaddr,
    addrlen: SocklenT,
) -> isize {
    // Handlers counted from here (`crate::interrupt`).
    let mark = Mark::now();
    if buf.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // MSG_NOSIGNAL (0x4000) is a no-op — we have no SIGPIPE.
    let _ = flags;

    // TCP sendto: works like send() — destination addr is ignored
    // (connection-oriented protocol already knows the peer).
    if entry.kind == HandleKind::TcpStream {
        if entry.handle == 0 {
            errno::set_errno(errno::ENOTCONN);
            return -1;
        }
        let is_nb = (flags & MSG_DONTWAIT) != 0
            || fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
        if !is_nb {
            // Blocking socket: use tcp_send_wait for full-write
            // semantics (same as send()).
            let timeout_ms = get_meta(fd).map_or(0u64, |m| m.sndtimeo_ms);
            return tcp_send_wait(entry.handle, buf, len, timeout_ms, mark);
        }
        // Non-blocking: try once.
        let ret = syscall3(SYS_TCP_SEND, entry.handle, buf as u64, len as u64);
        if ret >= 0 {
            return ret as isize;
        }
        let posix_err = translate_net_error(ret);
        // Distinguish RST (ECONNRESET) from local shutdown (EPIPE).
        if posix_err == errno::ECONNRESET {
            let last =
                crate::syscall::syscall1(crate::syscall::SYS_TCP_LAST_ERROR, entry.handle) as u8;
            errno::set_errno(if last == 2 {
                errno::ECONNRESET
            } else {
                errno::EPIPE
            });
        } else {
            errno::set_errno(posix_err);
        }
        return -1;
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

    // Implicit bind: if the socket hasn't been bound yet (handle == 0),
    // bind to ephemeral port 0 before sending.  POSIX requires this
    // for unbound DGRAM sockets on the first sendto/send.
    let handle = if entry.handle == 0 {
        let bind_ret = syscall1(SYS_UDP_BIND, 0); // port 0 = ephemeral
        if bind_ret < 0 {
            errno::set_errno(translate_net_error(bind_ret));
            return -1;
        }
        let new_handle = bind_ret as u64;
        // Update fd table with the new kernel handle, preserving status flags.
        let prev_flags = fdtable::get_status_flags(fd).unwrap_or(0);
        let _ = fdtable::install_fd_with_flags(fd, HandleKind::UdpSocket, new_handle, prev_flags);
        // If this socket was connected (has a peer set), re-apply the
        // kernel-side peer filter so recvfrom() only returns datagrams
        // from the connected peer.
        if let Some(meta) = get_meta(fd)
            && (meta.peer_addr != 0 || meta.peer_port != 0)
        {
            let peer_port_host = u16::from_be(meta.peer_port);
            let _ = syscall3(
                SYS_UDP_CONNECT,
                new_handle,
                u64::from(meta.peer_addr),
                u64::from(peer_port_host),
            );
        }
        new_handle
    } else {
        entry.handle
    };

    // SYS_UDP_SEND: arg0=handle, arg1=ip, arg2=port, arg3=buf, arg4=len.
    let ret = syscall5(
        SYS_UDP_SEND,
        handle,
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::too_many_lines)] // TCP/UDP recvfrom with blocking, timeout, and address fill logic; splitting would fragment the flow.
pub unsafe extern "C" fn recvfrom(
    fd: i32,
    buf: *mut u8,
    len: usize,
    flags: i32,
    src_addr: *mut Sockaddr,
    addrlen: *mut SocklenT,
) -> isize {
    // Handlers counted from here (`crate::interrupt`).
    let mark = Mark::now();
    if buf.is_null() && len > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };

    // POSIX: recvfrom with len==0 succeeds immediately with 0 bytes.
    // Without this, a zero-length TCP recv returns 0 from the kernel
    // which would be incorrectly interpreted as EOF.  This matches
    // recv()'s behavior and Linux's semantics — including the ordering:
    // `__sys_recvfrom` (net/socket.c:2224) looks the descriptor up whatever
    // `size` is, so EBADF still beats the zero-length success.
    if len == 0 {
        return 0;
    }

    // Build kernel flags (MSG_PEEK=0x02, MSG_TRUNC=0x20, MSG_DONTWAIT=0x40).
    let kern_flags = {
        let f = (flags as u32) & (MSG_PEEK as u32 | MSG_TRUNC as u32 | MSG_DONTWAIT as u32);
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

            let is_nb = (kern_flags & MSG_DONTWAIT as u32) != 0;
            let timeout_ms = get_meta(fd).map_or(0u64, |m| m.rcvtimeo_ms);
            // MSG_WAITALL is meaningless with MSG_PEEK (same as recv above).
            let waitall = (flags & MSG_WAITALL) != 0 && !is_nb && (flags & MSG_PEEK) == 0;

            // Always try non-blocking first so we can implement timeout
            // semantics in userspace.  If data is available, we return
            // immediately without entering the poll loop.
            let try_flags = kern_flags | MSG_DONTWAIT as u32;

            let ret = syscall4(
                SYS_TCP_RECV,
                entry.handle,
                buf as u64,
                len as u64,
                u64::from(try_flags),
            );

            let recv_result = match ret.cmp(&0) {
                core::cmp::Ordering::Greater => {
                    // MSG_WAITALL: keep receiving until full.
                    if waitall && (ret as usize) < len {
                        tcp_recv_waitall(
                            entry.handle,
                            buf,
                            len,
                            kern_flags,
                            timeout_ms,
                            ret as usize,
                            mark,
                        )
                    } else {
                        ret as isize
                    }
                }
                core::cmp::Ordering::Equal => 0, // EOF
                core::cmp::Ordering::Less => {
                    let err = translate_net_error(ret);
                    if (err == errno::EAGAIN || err == errno::EWOULDBLOCK) && !is_nb {
                        if waitall {
                            tcp_recv_waitall(
                                entry.handle,
                                buf,
                                len,
                                kern_flags,
                                timeout_ms,
                                0,
                                mark,
                            )
                        } else {
                            tcp_recv_wait(entry.handle, buf, len, kern_flags, timeout_ms, mark)
                        }
                    } else {
                        errno::set_errno(err);
                        return -1;
                    }
                }
            };

            if recv_result < 0 {
                return recv_result;
            }

            // Fill in peer address if requested.
            if !src_addr.is_null() && !addrlen.is_null() {
                unsafe {
                    let available = *addrlen as usize;
                    if available >= core::mem::size_of::<SockaddrIn>() {
                        let meta = get_meta(fd);
                        let (ip, port) =
                            meta.map_or((0u32, 0u16), |m| (m.peer_addr, u16::from_be(m.peer_port)));
                        let sa = SockaddrIn {
                            sin_family: AF_INET as u16,
                            sin_port: port.to_be(),
                            sin_addr: InAddr { s_addr: ip },
                            sin_zero: [0u8; 8],
                        };
                        core::ptr::write_unaligned(src_addr.cast::<SockaddrIn>(), sa);
                    }
                    *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
                }
            }

            recv_result
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
                        entry.handle,
                        buf,
                        len,
                        &mut src_info,
                        kern_flags,
                        timeout_ms,
                        mark,
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
                            src_info[0],
                            src_info[1],
                            src_info[2],
                            src_info[3],
                        ]);
                        let port_le = u16::from_le_bytes([src_info[4], src_info[5]]);
                        let sa = SockaddrIn {
                            sin_family: AF_INET as u16,
                            sin_port: port_le.to_be(),
                            sin_addr: InAddr { s_addr: ip },
                            sin_zero: [0u8; 8],
                        };
                        core::ptr::write_unaligned(src_addr.cast::<SockaddrIn>(), sa);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

            // Do NOT zero the handle here.  The kernel connection still
            // needs to complete the FIN exchange and be cleaned up by
            // close().  After shutdown, the kernel enforces:
            // - SHUT_RD: recv returns 0 (EOF, via local_read_closed)
            // - SHUT_WR: send returns EPIPE (via local_write_closed)
            // - SHUT_RDWR: both of the above
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
        HandleKind::UnixStream => {
            if entry.handle == 0 {
                errno::set_errno(errno::ENOTCONN);
                return -1;
            }

            // Delegate to the kernel for proper half-close semantics.
            // The kernel's stream-socket `how` values match POSIX exactly
            // (SHUT_RD=0, SHUT_WR=1, SHUT_RDWR=2):
            // - SHUT_RD: further recv on this endpoint returns EOF.
            // - SHUT_WR: further send fails (EPIPE); the peer reading our
            //   ring drains remaining bytes then sees EOF.
            // - SHUT_RDWR: both of the above.
            let ret = syscall2(SYS_SOCKETPAIR_SHUTDOWN, entry.handle, how as u64);
            if ret < 0 {
                // Negative kernel codes (e.g. CHANNEL_CLOSED, INVALID_HANDLE)
                // map to errno and -1 via the generic translation table.
                return errno::translate(ret) as i32;
            }

            // Do NOT zero the handle here: close() still needs to release the
            // kernel endpoint refcount.  The kernel enforces post-shutdown
            // semantics on subsequent send/recv against the same handle.
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // tv_sec/tv_usec are standard POSIX field names.
#[allow(clippy::too_many_lines)] // Large match over socket option levels/names; splitting would scatter related option handling.
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
        i32::from_ne_bytes(unsafe { [*optval, *optval.add(1), *optval.add(2), *optval.add(3)] })
    } else {
        0
    };

    if let Some(mut meta) = get_meta(fd) {
        match (level, optname) {
            (SOL_SOCKET, SO_REUSEADDR) => {
                meta.reuseaddr = val != 0;
            }
            (SOL_SOCKET, SO_KEEPALIVE) => {
                meta.keepalive = val != 0;
                // Wire through to kernel for TCP streams with active handles.
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let _ = syscall2(SYS_TCP_SET_KEEPALIVE, entry.handle, u64::from(val != 0));
                }
            }
            (SOL_SOCKET, SO_RCVBUF) => {
                meta.rcvbuf = val.max(1);
            }
            (SOL_SOCKET, SO_SNDBUF) => {
                meta.sndbuf = val.max(1);
            }
            // RCVLOWAT/SNDLOWAT, REUSEPORT, multicast TTL/loop:
            // documented no-ops — intentionally same body as wildcard arm.
            #[allow(clippy::match_same_arms)]
            (SOL_SOCKET, SO_RCVLOWAT | SO_SNDLOWAT | SO_REUSEPORT)
            | (SOL_IP, IP_MULTICAST_TTL | IP_MULTICAST_LOOP) => {}
            (SOL_SOCKET, SO_BROADCAST) => {
                meta.broadcast = val != 0;
            }
            (SOL_SOCKET, SO_LINGER) => {
                // BSD struct linger { int l_onoff; int l_linger; } = 8 bytes.
                if !optval.is_null() && optlen as usize >= 8 {
                    let l_onoff = i32::from_ne_bytes(unsafe {
                        [*optval, *optval.add(1), *optval.add(2), *optval.add(3)]
                    });
                    let l_linger = i32::from_ne_bytes(unsafe {
                        [
                            *optval.add(4),
                            *optval.add(5),
                            *optval.add(6),
                            *optval.add(7),
                        ]
                    });
                    meta.linger_onoff = l_onoff != 0;
                    meta.linger_secs = l_linger;
                } else {
                    // Fallback: treat as simple int (non-standard but graceful).
                    meta.linger_onoff = val != 0;
                    meta.linger_secs = val;
                }
            }
            (SOL_SOCKET, SO_RCVTIMEO) => {
                // Timeout is a timeval struct: {tv_sec, tv_usec}.
                // If optlen is >= 16 (sizeof timeval), parse as timeval.
                // Otherwise treat val as milliseconds for compatibility.
                let ms = if optlen as usize >= 16 {
                    let tv_sec = unsafe { core::ptr::read_unaligned(optval.cast::<i64>()) };
                    let tv_usec = unsafe { core::ptr::read_unaligned(optval.add(8).cast::<i64>()) };
                    // Ceiling division: round sub-millisecond values UP so
                    // that any non-zero timeval yields at least 1ms (otherwise
                    // {0, 500us} would store as 0ms = "no timeout" = block forever).
                    (tv_sec.max(0) as u64)
                        .saturating_mul(1000)
                        .saturating_add((tv_usec.max(0) as u64).div_ceil(1000))
                } else {
                    val.max(0) as u64
                };
                meta.rcvtimeo_ms = ms;
            }
            (SOL_SOCKET, SO_SNDTIMEO) => {
                let ms = if optlen as usize >= 16 {
                    let tv_sec = unsafe { core::ptr::read_unaligned(optval.cast::<i64>()) };
                    let tv_usec = unsafe { core::ptr::read_unaligned(optval.add(8).cast::<i64>()) };
                    // Ceiling division (same rationale as SO_RCVTIMEO above).
                    (tv_sec.max(0) as u64)
                        .saturating_mul(1000)
                        .saturating_add((tv_usec.max(0) as u64).div_ceil(1000))
                } else {
                    val.max(0) as u64
                };
                meta.sndtimeo_ms = ms;
            }
            (SOL_TCP, TCP_NODELAY) => {
                meta.nodelay = val != 0;
                // Wire through to kernel for TCP streams with active handles.
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let _ = syscall2(SYS_TCP_SET_NODELAY, entry.handle, u64::from(val != 0));
                }
            }
            (SOL_TCP, TCP_CORK) => {
                // Track cork state.  Kernel doesn't have explicit cork
                // support yet, so this is advisory (getsockopt will
                // report the correct value).  TCP_CORK and TCP_NODELAY
                // are complementary: cork holds data, nodelay pushes it.
                meta.cork = val != 0;
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

    // Validate the option value is an IpMreq.  Linux's `do_ip_setsockopt`
    // (net/ipv4/ip_sockglue.c:1222-1233) tests `optlen < sizeof(struct ip_mreq)`
    // and only then runs `copy_from_sockptr`, so a short optlen is EINVAL and a
    // bad pointer at an adequate optlen is EFAULT.  They are distinct verdicts;
    // folding both into EINVAL told a caller with a valid-length buffer at a
    // bad address that its *length* was wrong.
    if (optlen as usize) < core::mem::size_of::<IpMreq>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if optval.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: optval is non-null and points to at least sizeof(IpMreq) bytes.
    // Use read_unaligned because optval is *const u8 (alignment 1) but
    // IpMreq requires alignment 4.
    let mreq = unsafe { core::ptr::read_unaligned(optval.cast::<IpMreq>()) };
    let group_addr = mreq.imr_multiaddr.s_addr; // Network byte order u32.

    let syscall_nr = if optname == IP_ADD_MEMBERSHIP {
        SYS_UDP_MCAST_JOIN
    } else {
        SYS_UDP_MCAST_LEAVE
    };

    let ret = syscall2(syscall_nr, entry.handle, u64::from(group_addr));
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::similar_names)] // tv_sec/tv_usec are standard POSIX field names.
#[allow(clippy::too_many_lines)] // Large match over socket option levels/names; splitting would scatter related option handling.
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
                // Query and clear the connection's pending error code.
                // POSIX requires SO_ERROR to be clear-on-read: after
                // getsockopt returns the error, subsequent reads return 0.
                if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
                    let err = crate::syscall::syscall2(
                        crate::syscall::SYS_TCP_LAST_ERROR,
                        entry.handle,
                        1, // clear=true
                    ) as u8;
                    match err {
                        1 => errno::ECONNREFUSED,
                        2 => errno::ECONNRESET,
                        3 => errno::ETIMEDOUT,
                        _ => 0, // No pending error (normal close or active).
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
            // These options all return 0: no port reuse, no user timeout, no DSCP/TOS.
            (SOL_SOCKET, SO_REUSEPORT) | (SOL_TCP, TCP_USER_TIMEOUT) | (SOL_IP, IP_TOS) => 0,
            (SOL_SOCKET, SO_LINGER) => {
                // Return struct linger { int l_onoff; int l_linger; } = 8 bytes.
                if available >= 8 {
                    let (onoff, secs) =
                        meta.map_or((0i32, 0i32), |m| (i32::from(m.linger_onoff), m.linger_secs));
                    core::ptr::copy_nonoverlapping((&raw const onoff).cast::<u8>(), optval, 4);
                    core::ptr::copy_nonoverlapping(
                        (&raw const secs).cast::<u8>(),
                        optval.add(4),
                        4,
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
                    #[allow(clippy::arithmetic_side_effects)]
                    // ms%1000 is 0..999; *1000 cannot overflow u64.
                    let tv_sec = (ms / 1000) as i64;
                    #[allow(clippy::arithmetic_side_effects)]
                    let tv_usec = ((ms % 1000) * 1000) as i64;
                    core::ptr::copy_nonoverlapping((&raw const tv_sec).cast::<u8>(), optval, 8);
                    core::ptr::copy_nonoverlapping(
                        (&raw const tv_usec).cast::<u8>(),
                        optval.add(8),
                        8,
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
                    #[allow(clippy::arithmetic_side_effects)]
                    // ms%1000 is 0..999; *1000 cannot overflow u64.
                    let tv_sec = (ms / 1000) as i64;
                    #[allow(clippy::arithmetic_side_effects)]
                    let tv_usec = ((ms % 1000) * 1000) as i64;
                    core::ptr::copy_nonoverlapping((&raw const tv_sec).cast::<u8>(), optval, 8);
                    core::ptr::copy_nonoverlapping(
                        (&raw const tv_usec).cast::<u8>(),
                        optval.add(8),
                        8,
                    );
                    *optlen = 16;
                    return 0;
                }
                (ms / 1000) as i32
            }
            // Default: readable/writable when >=1 byte available/of space.
            // Multicast TTL defaults to 1; multicast loopback defaults to enabled (1).
            (SOL_SOCKET, SO_RCVLOWAT | SO_SNDLOWAT)
            | (SOL_IP, IP_MULTICAST_TTL | IP_MULTICAST_LOOP) => 1,
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
            (SOL_TCP, TCP_MAXSEG) => 1460, // Default MSS (Ethernet MTU - headers).
            (SOL_TCP, TCP_CORK) => meta.map_or(0, |m| i32::from(m.cork)),
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
                let ret = syscall3(SYS_TCP_INFO, entry.handle, optval as u64, available as u64);
                if ret < 0 {
                    errno::set_errno(translate_net_error(ret));
                    return -1;
                }
                *optlen = 48;
                return 0;
            }
            // IP-level options: return sensible defaults even though we
            // don't have per-socket kernel storage for these yet.
            (SOL_IP, IP_TTL) => 64, // Default unicast TTL.
            _ => {
                errno::set_errno(errno::ENOPROTOOPT);
                return -1;
            }
        };

        core::ptr::copy_nonoverlapping((&raw const val).cast::<u8>(), optval, 4);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getpeername(fd: i32, addr: *mut Sockaddr, addrlen: *mut SocklenT) -> i32 {
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
        let ret = syscall2(SYS_TCP_PEER_ADDR, entry.handle, buf.as_mut_ptr() as u64);
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
        core::ptr::copy_nonoverlapping((&raw const sin).cast::<u8>(), addr.cast::<u8>(), copy_len);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getsockname(fd: i32, addr: *mut Sockaddr, addrlen: *mut SocklenT) -> i32 {
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

    // Determine the local port.  If the metadata has an explicit bound port
    // (from bind()), use it.  Otherwise query the kernel for the ephemeral
    // port assigned during connect() or bind(port=0).
    // get_fd(fd) was already checked at the top, so this is unreachable
    // in practice, but we handle the None case gracefully.
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    let local_port = if meta.bound_port != 0 {
        meta.bound_port
    } else if entry.kind == HandleKind::TcpStream && entry.handle != 0 {
        // Connection: query kernel for ephemeral port (arg1=0 = connection).
        let port_raw =
            crate::syscall::syscall2(crate::syscall::SYS_TCP_LOCAL_PORT, entry.handle, 0);
        if port_raw > 0 {
            (port_raw as u16).to_be()
        } else {
            0
        }
    } else if entry.kind == HandleKind::TcpListener && entry.handle != 0 {
        // Listener: query kernel for listener's bound port (arg1=1 = listener).
        let port_raw =
            crate::syscall::syscall2(crate::syscall::SYS_TCP_LOCAL_PORT, entry.handle, 1);
        if port_raw > 0 {
            (port_raw as u16).to_be()
        } else {
            0
        }
    } else if entry.kind == HandleKind::UdpSocket && entry.handle != 0 {
        let port_raw = crate::syscall::syscall1(crate::syscall::SYS_UDP_LOCAL_PORT, entry.handle);
        if port_raw > 0 {
            (port_raw as u16).to_be()
        } else {
            0
        }
    } else {
        0
    };

    // Determine the local IP address.  Use stored metadata if set,
    // otherwise fall back to 0.0.0.0 (INADDR_ANY).
    let local_ip = meta.local_addr;

    let sin = SockaddrIn {
        sin_family: AF_INET as u16,
        sin_port: local_port,
        sin_addr: InAddr { s_addr: local_ip },
        sin_zero: [0u8; 8],
    };

    // SAFETY: caller guarantees addr/addrlen validity.
    unsafe {
        let available = *addrlen as usize;
        let copy_len = available.min(core::mem::size_of::<SockaddrIn>());
        core::ptr::copy_nonoverlapping((&raw const sin).cast::<u8>(), addr.cast::<u8>(), copy_len);
        #[allow(clippy::cast_possible_truncation)]
        {
            *addrlen = core::mem::size_of::<SockaddrIn>() as SocklenT;
        }
    }

    0
}

// ---------------------------------------------------------------------------
// struct hostent
// ---------------------------------------------------------------------------

/// `struct hostent`: what the hosts database answers ([`crate::hosts`]).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Hostent {
    /// Official hostname.
    pub h_name: *const u8,
    /// Alias list (NULL-terminated).
    pub h_aliases: *const *const u8,
    /// Address family: `AF_INET` or `AF_INET6`.
    pub h_addrtype: i32,
    /// Address length: 4 or 16.
    pub h_length: i32,
    /// Address list (NULL-terminated, each `h_length` bytes).
    pub h_addr_list: *const *const u8,
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

/// Get a pointer to the calling thread's resolver error variable.
///
/// C code reaches this through `<netdb.h>`'s `#define h_errno
/// (*__h_errno_location())`, exactly as glibc defines it.  There is
/// deliberately **no** exported `h_errno` *data* symbol: a program that
/// declared `extern int h_errno;` and read it directly would see a variable
/// nobody ever writes, which is a silent wrong answer.  A link error is the
/// better failure.
///
/// The storage is per-thread (`crate::perthread`) because two threads doing
/// concurrent lookups must not overwrite each other's error code.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __h_errno_location() -> *mut i32 {
    // SAFETY: `perthread::current()` is non-null and valid for this thread,
    // and no other thread holds a pointer into this block.
    unsafe { &raw mut (*crate::perthread::current()).h_errno }
}

/// Set the calling thread's resolver error.
pub(crate) fn set_h_errno(val: i32) {
    // SAFETY: as in `__h_errno_location`.
    unsafe {
        (*crate::perthread::current()).h_errno = val;
    }
}

/// Read the calling thread's resolver error.
#[must_use]
pub(crate) fn get_h_errno() -> i32 {
    // SAFETY: as in `__h_errno_location`.
    unsafe { (*crate::perthread::current()).h_errno }
}

/// The message for a resolver error, glibc's: `h_errlist` for 0 to 4,
/// "Resolver internal error" for `NETDB_INTERNAL` and anything negative.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn hstrerror(err: i32) -> *const u8 {
    let m: &core::ffi::CStr = match err {
        e if e < 0 => c"Resolver internal error",
        0 => c"Resolver Error 0 (no error)",
        HOST_NOT_FOUND => c"Unknown host",
        TRY_AGAIN => c"Host name lookup failure",
        NO_RECOVERY => c"Unknown server error",
        NO_DATA => c"No address associated with name",
        _ => c"Unknown resolver error",
    };
    m.as_ptr().cast()
}

/// Write `s: ` (when `s` is neither NULL nor empty), `h_errno`'s message and
/// a newline to standard error, in one `writev` as glibc does -- straight
/// to the descriptor, past `stderr`'s buffer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn herror(s: *const u8) {
    let msg = hstrerror(get_h_errno());
    // SAFETY: `hstrerror` answers with a static NUL-terminated string; `s`,
    // when not NULL, is the caller's.
    let (prefix, text) = unsafe {
        let p: &[u8] = if s.is_null() {
            &[]
        } else {
            core::slice::from_raw_parts(s, crate::string::strlen(s))
        };
        (
            p,
            core::slice::from_raw_parts(msg, crate::string::strlen(msg)),
        )
    };
    let iov = |b: &[u8]| crate::file::Iovec {
        iov_base: b.as_ptr().cast_mut(),
        iov_len: b.len(),
    };
    let parts = [iov(prefix), iov(b": "), iov(text), iov(b"\n")];
    let v: &[crate::file::Iovec] = if prefix.is_empty() {
        &parts[2..]
    } else {
        &parts
    };
    // A message that cannot be written has nowhere else to go.
    let _ = crate::file::writev(2, v.as_ptr(), v.len() as i32);
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
    /// Socket address.
    ///
    /// **`ai_addr` comes before `ai_canonname`.** These two were the other way
    /// round until 2026-09-09, which is wrong against glibc and musl alike --
    /// not a divergence between them, just wrong. A caller that read `ai_addr`
    /// got the canonical-name string and handed it to `connect()` as a
    /// `struct sockaddr`. Found by `scripts/check-libc-abi.py`;
    /// `design-decisions.md` 1011.
    pub ai_addr: *mut Sockaddr,
    /// Canonical hostname (may be null).
    pub ai_canonname: *mut u8,
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

// getaddrinfo's and getnameinfo's error codes: negative, as musl's
// `<netdb.h>` and glibc's both number them.  They were 1 to 11 until
// 2026-09-27, so a caller -- whose header is musl's -- testing for
// `EAI_NONAME` (-2) or retrying on `EAI_AGAIN` (-3) never saw either; only
// `gai_strerror`, which read the same wrong numbers, agreed with them.
/// ai_flags has a bad value.
pub const EAI_BADFLAGS: i32 = -1;
/// Name or service not known.
pub const EAI_NONAME: i32 = -2;
/// Temporary failure in name resolution.
pub const EAI_AGAIN: i32 = -3;
/// Non-recoverable failure in name resolution.
pub const EAI_FAIL: i32 = -4;
/// No address associated with hostname.
pub const EAI_NODATA: i32 = -5;
/// ai_family not supported.
pub const EAI_FAMILY: i32 = -6;
/// ai_socktype not supported.
pub const EAI_SOCKTYPE: i32 = -7;
/// The service is not supported for ai_socktype.
pub const EAI_SERVICE: i32 = -8;
/// Address family for hostname not supported.
pub const EAI_ADDRFAMILY: i32 = -9;
/// Memory allocation failure.
pub const EAI_MEMORY: i32 = -10;
/// System error: `errno` says which.
pub const EAI_SYSTEM: i32 = -11;

/// `EAI_OVERFLOW` — buffer too small for result.
pub const EAI_OVERFLOW: i32 = -12;

// ---------------------------------------------------------------------------
// socketpair
// ---------------------------------------------------------------------------

/// Create a pair of connected sockets.
///
/// Backed by the kernel stream-socket object (`SYS_SOCKETPAIR_CREATE`),
/// which returns two bonded endpoint handles.  Bytes written on one fd
/// are read on the other and vice-versa.
///
/// Supported arguments:
/// - `domain`: `AF_UNIX` / `AF_LOCAL` only (other families →
///   `EAFNOSUPPORT`).
/// - `sock_type`: `SOCK_STREAM`, optionally OR-ed with `SOCK_NONBLOCK`
///   and/or `SOCK_CLOEXEC`.  `SOCK_DGRAM` / `SOCK_SEQPACKET` are not
///   implemented (→ `EOPNOTSUPP`); see the limitation note in todo.txt.
/// - `protocol`: must be 0 (`EPROTONOSUPPORT` otherwise).
///
/// On success writes the two fds into `sv[0]`/`sv[1]` and returns 0.
/// On failure returns -1 with `errno` set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn socketpair(domain: i32, sock_type: i32, protocol: i32, sv: *mut [i32; 2]) -> i32 {
    // ---- Argument validation (all paths here return before any
    // syscall, so they are exercisable by host unit tests) ----
    //
    // The order below is `__sys_socketpair`'s (net/socket.c:1729).  It tests
    // the type's flag bits at :1737 — before it has reserved a descriptor, let
    // alone looked at `usockvec` — but it reaches `put_user(fd1, &usockvec[0])`
    // at :1758 *before* `sock_create` at :1771.  So the flag EINVAL outranks
    // EFAULT, and EFAULT in turn outranks the family/type/protocol verdicts.

    // A negative type cannot carry valid flag bits (SOCK_NONBLOCK and
    // SOCK_CLOEXEC are both well below the sign bit), so upstream's single
    // `flags & ~(SOCK_CLOEXEC | SOCK_NONBLOCK)` test rejects it; we spell that
    // case out separately only because `!SOCK_TYPE_MASK` on a negative `i32`
    // is easier to reason about once the sign bit is already excluded.
    if sock_type < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let type_flags = sock_type & !SOCK_TYPE_MASK;
    if type_flags & !(SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let base_type = sock_type & SOCK_TYPE_MASK;

    if sv.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Only Unix-domain pairs are supported.
    if domain != AF_UNIX {
        errno::set_errno(errno::EAFNOSUPPORT);
        return -1;
    }
    // Only stream sockets are implemented.  SOCK_DGRAM / SOCK_SEQPACKET
    // would need datagram-framed kernel objects (not yet built).
    if base_type != SOCK_STREAM {
        errno::set_errno(errno::EOPNOTSUPP);
        return -1;
    }
    // PF_UNIX defines no protocols; only 0 is valid.
    if protocol != 0 {
        errno::set_errno(errno::EPROTONOSUPPORT);
        return -1;
    }

    // ---- Create the kernel endpoint pair ----
    let (raw0, raw1) = syscall3_2ret(SYS_SOCKETPAIR_CREATE, 0, 0, 0);
    if raw0 < 0 {
        // The kernel create path does not currently fail, but guard
        // defensively so a future failure surfaces a real errno.
        return errno::translate(raw0) as i32;
    }
    let h0 = raw0 as u64;
    let h1 = raw1 as u64;

    let initial_flags = crate::fcntl::O_RDWR
        | if (type_flags & SOCK_NONBLOCK) != 0 {
            crate::fcntl::O_NONBLOCK
        } else {
            0
        };

    // Allocate the first fd.
    let Some(fd0) = fdtable::alloc_fd_with_flags(HandleKind::UnixStream, h0, initial_flags) else {
        // Could not install fd0 — release both kernel endpoints.
        let _ = syscall1(SYS_SOCKETPAIR_CLOSE, h0);
        let _ = syscall1(SYS_SOCKETPAIR_CLOSE, h1);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    // Allocate the second fd.
    let Some(fd1) = fdtable::alloc_fd_with_flags(HandleKind::UnixStream, h1, initial_flags) else {
        // Roll back fd0 (frees its table slot) and close both endpoints.
        let _ = fdtable::close_fd(fd0);
        let _ = syscall1(SYS_SOCKETPAIR_CLOSE, h0);
        let _ = syscall1(SYS_SOCKETPAIR_CLOSE, h1);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    // Apply FD_CLOEXEC to both fds if requested.
    if (type_flags & SOCK_CLOEXEC) != 0 {
        let _ = fdtable::set_fd_flags(fd0, fdtable::FD_CLOEXEC);
        let _ = fdtable::set_fd_flags(fd1, fdtable::FD_CLOEXEC);
    }

    // Write the fd pair into the caller's array.  Use raw writes (rather
    // than indexing) to satisfy the indexing_slicing lint and to avoid
    // assuming alignment of the user pointer.
    // SAFETY: `sv` was null-checked above; the caller guarantees it
    // points to a writable `[i32; 2]`.
    unsafe {
        let out = sv.cast::<i32>();
        core::ptr::write(out, fd0);
        core::ptr::write(out.add(1), fd1);
    }
    0
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::too_many_lines)]
// Scatter/gather send with stack buffer consolidation for TCP/UDP; splitting would fragment the iov assembly logic.
#[allow(clippy::cast_ptr_alignment)] // SAFETY: msg_name is typed as *mut u8 in Msghdr but actually points to a sockaddr; callers guarantee correct alignment.
pub unsafe extern "C" fn sendmsg(fd: i32, msg: *const Msghdr, flags: i32) -> isize {
    const STACK_BUF: usize = 4096;
    const LARGE_BUF: usize = 16384;

    // `__sys_sendmsg` (net/socket.c:2634) runs `sockfd_lookup_light` before
    // `___sys_sendmsg` copies the header in, so EBADF/ENOTSOCK outrank both the
    // EFAULT on `msg` and the zero-length success below.  (`sendto` is the
    // other way round: `__sys_sendto` at :2171 imports the buffer *first*.)
    if !socket_fd_is_valid(fd) {
        return -1;
    }

    if msg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: msg is non-null and valid (caller guarantee).
    let m = unsafe { &*msg };
    if m.msg_iov.is_null() || m.msg_iovlen == 0 {
        return 0;
    }
    // POSIX: EMSGSIZE if msg_iovlen exceeds IOV_MAX.
    if m.msg_iovlen > 1024 {
        errno::set_errno(errno::EMSGSIZE);
        return -1;
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
    // Use sendto when msg_name is set (for UDP to specified destination).
    if m.msg_iovlen == 1 {
        let iov = unsafe { &*m.msg_iov };
        if !m.msg_name.is_null() && m.msg_namelen > 0 {
            return unsafe {
                sendto(
                    fd,
                    iov.iov_base.cast::<u8>(),
                    iov.iov_len,
                    flags,
                    m.msg_name.cast::<Sockaddr>().cast_const(),
                    m.msg_namelen,
                )
            };
        }
        return unsafe { send(fd, iov.iov_base, iov.iov_len, flags) };
    }

    // Determine if we have a destination address for sendto.
    let has_dest = !m.msg_name.is_null() && m.msg_namelen > 0;

    // If total fits in a stack buffer, concatenate for one send call.
    // This produces a single TCP segment / UDP datagram instead of multiple.
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
        return if has_dest {
            unsafe {
                sendto(
                    fd,
                    buf.as_ptr(),
                    pos,
                    flags,
                    m.msg_name.cast::<Sockaddr>().cast_const(),
                    m.msg_namelen,
                )
            }
        } else {
            unsafe { send(fd, buf.as_ptr().cast(), pos, flags) }
        };
    }

    // Larger than stack buffer.
    // For UDP: MUST concatenate all iovecs into a single datagram.
    // Sending each iov separately would create multiple datagrams, which
    // is wrong — sendmsg on DGRAM sends exactly one datagram.
    // For TCP: per-iov sending is correct (TCP is a byte stream).
    let is_dgram = fdtable::get_fd(fd).is_some_and(|e| e.kind == HandleKind::UdpSocket);

    if is_dgram {
        // UDP: concatenate into a larger buffer.  Max UDP payload = 65507.
        const UDP_MAX: usize = 65507;
        if total > UDP_MAX {
            errno::set_errno(errno::EMSGSIZE);
            return -1;
        }
        // Use a 16 KiB stack buffer (one page).  For the rare case of
        // UDP datagrams > 16 KiB, use a 64 KiB buffer.
        if total <= LARGE_BUF {
            let mut buf = [0u8; LARGE_BUF];
            let mut pos: usize = 0;
            i = 0;
            while i < m.msg_iovlen {
                let iov = unsafe { &*m.msg_iov.add(i) };
                if iov.iov_len > 0 && !iov.iov_base.is_null() {
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
            return if has_dest {
                unsafe {
                    sendto(
                        fd,
                        buf.as_ptr(),
                        pos,
                        flags,
                        m.msg_name.cast::<Sockaddr>().cast_const(),
                        m.msg_namelen,
                    )
                }
            } else {
                unsafe { send(fd, buf.as_ptr().cast(), pos, flags) }
            };
        }
        // > 16 KiB but <= 65507: use maximum buffer.
        let mut buf = [0u8; UDP_MAX];
        let mut pos: usize = 0;
        i = 0;
        while i < m.msg_iovlen {
            let iov = unsafe { &*m.msg_iov.add(i) };
            if iov.iov_len > 0 && !iov.iov_base.is_null() {
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
        return if has_dest {
            unsafe {
                sendto(
                    fd,
                    buf.as_ptr(),
                    pos,
                    flags,
                    m.msg_name.cast::<Sockaddr>().cast_const(),
                    m.msg_namelen,
                )
            }
        } else {
            unsafe { send(fd, buf.as_ptr().cast(), pos, flags) }
        };
    }

    // TCP: send each iov individually (correct for byte-stream sockets).
    let mut sent: isize = 0;
    i = 0;
    while i < m.msg_iovlen {
        let iov = unsafe { &*m.msg_iov.add(i) };
        if iov.iov_len > 0 && !iov.iov_base.is_null() {
            let n = if has_dest {
                unsafe {
                    sendto(
                        fd,
                        iov.iov_base.cast::<u8>(),
                        iov.iov_len,
                        flags,
                        m.msg_name.cast::<Sockaddr>().cast_const(),
                        m.msg_namelen,
                    )
                }
            } else {
                unsafe { send(fd, iov.iov_base, iov.iov_len, flags) }
            };
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::too_many_lines)]
// Scatter/gather recv with multi-iov distribution and MSG_WAITALL; splitting would fragment the logic.
#[allow(clippy::cast_ptr_alignment)] // SAFETY: msg_name is typed as *mut u8 in Msghdr but actually points to a sockaddr; callers guarantee correct alignment.
pub unsafe extern "C" fn recvmsg(fd: i32, msg: *mut Msghdr, flags: i32) -> isize {
    const STACK_BUF_SMALL: usize = 4096;
    const STACK_BUF_LARGE: usize = 16384; // One 16 KiB page.

    // `__sys_recvmsg` (net/socket.c) runs `sockfd_lookup_light` before
    // `___sys_recvmsg` copies the header in, so EBADF/ENOTSOCK outrank both the
    // EFAULT on `msg` and the zero-length success below.  (`recvfrom` is the
    // other way round: `__sys_recvfrom` at :2237 imports the buffer *first*.)
    if !socket_fd_is_valid(fd) {
        return -1;
    }

    if msg.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: msg is non-null and valid (caller guarantee).
    let m = unsafe { &mut *msg };
    if m.msg_iov.is_null() || m.msg_iovlen == 0 {
        m.msg_flags = 0;
        m.msg_controllen = 0;
        return 0;
    }
    // POSIX: EMSGSIZE if msg_iovlen exceeds IOV_MAX.
    if m.msg_iovlen > 1024 {
        errno::set_errno(errno::EMSGSIZE);
        return -1;
    }

    // Single iov — receive directly without copying.
    // Use recvfrom to populate msg_name (source address) if requested.
    if m.msg_iovlen == 1 {
        let iov = unsafe { &*m.msg_iov };
        let ret = if !m.msg_name.is_null() && m.msg_namelen > 0 {
            let mut addrlen = m.msg_namelen;
            let r = unsafe {
                recvfrom(
                    fd,
                    iov.iov_base.cast::<u8>(),
                    iov.iov_len,
                    flags,
                    m.msg_name.cast::<Sockaddr>(),
                    &raw mut addrlen,
                )
            };
            m.msg_namelen = addrlen;
            r
        } else {
            m.msg_namelen = 0;
            unsafe { recv(fd, iov.iov_base, iov.iov_len, flags) }
        };
        // Detect truncation for datagram sockets: when MSG_TRUNC was passed,
        // the kernel returns the real datagram size.  If ret > buffer size,
        // the datagram was truncated.
        let is_dgram = fdtable::get_fd(fd).is_some_and(|e| e.kind == HandleKind::UdpSocket);
        m.msg_flags = if is_dgram && ret > 0 && (ret as usize) > iov.iov_len {
            MSG_TRUNC
        } else {
            0
        };
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

    // MSG_WAITALL + multi-iov on TCP: receive directly into each iov
    // sequentially, bypassing the 4 KiB stack buffer.  The stack buffer
    // approach would cap the receive at 4096 bytes total, violating
    // MSG_WAITALL semantics when total_cap > 4096.
    if (flags & MSG_WAITALL) != 0
        && let Some(entry) = fdtable::get_fd(fd)
        && entry.kind == HandleKind::TcpStream
    {
        let mut total_recv: isize = 0;
        let want_name = !m.msg_name.is_null() && m.msg_namelen > 0;
        let mut got_name = false;
        i = 0;
        while i < m.msg_iovlen {
            // SAFETY: msg_iov is valid for msg_iovlen entries.
            let iov = unsafe { &*m.msg_iov.add(i) };
            if iov.iov_len > 0 && !iov.iov_base.is_null() {
                // Get source address from the first successful recv.
                let n = if want_name && !got_name {
                    let mut addrlen = m.msg_namelen;
                    let r = unsafe {
                        recvfrom(
                            fd,
                            iov.iov_base.cast::<u8>(),
                            iov.iov_len,
                            flags,
                            m.msg_name.cast::<Sockaddr>(),
                            &raw mut addrlen,
                        )
                    };
                    if r > 0 {
                        m.msg_namelen = addrlen;
                        got_name = true;
                    }
                    r
                } else {
                    unsafe { recv(fd, iov.iov_base, iov.iov_len, flags) }
                };
                if n < 0 {
                    // Error: return partial data if any, else propagate.
                    if total_recv > 0 {
                        break;
                    }
                    m.msg_flags = 0;
                    m.msg_controllen = 0;
                    return n;
                }
                if n == 0 {
                    // EOF: return what we have (POSIX: short read on EOF).
                    break;
                }
                total_recv = total_recv.wrapping_add(n);
                // Short recv within this iov means EOF/timeout reached.
                if (n as usize) < iov.iov_len {
                    break;
                }
            }
            i = i.wrapping_add(1);
        }
        if !got_name {
            m.msg_namelen = 0;
        }
        m.msg_flags = 0;
        m.msg_controllen = 0;
        return total_recv;
    }

    // Receive into a stack buffer, then distribute across iovs.
    // Use recvfrom to populate msg_name (source address) if requested.
    //
    // For UDP: datagrams are message-oriented — if the receive buffer is
    // smaller than the datagram, the excess is discarded (not returned on
    // the next recv).  We must provide a buffer at least as large as
    // total_cap (clamped to 16 KiB) to avoid unnecessary truncation.
    // For TCP: 4 KiB is fine — TCP is a byte stream and short reads are
    // expected; the remaining data is still in the kernel buffer.

    let is_dgram = fdtable::get_fd(fd).is_some_and(|e| e.kind == HandleKind::UdpSocket);

    // Choose receive buffer size: for UDP, use the larger buffer if needed
    // to avoid truncating datagrams the caller has room for.
    let use_large_buf = is_dgram && total_cap > STACK_BUF_SMALL;

    let (received, trunc_flag) = if use_large_buf {
        let recv_cap = if total_cap < STACK_BUF_LARGE {
            total_cap
        } else {
            STACK_BUF_LARGE
        };
        let mut buf = [0u8; STACK_BUF_LARGE];
        let ret = if !m.msg_name.is_null() && m.msg_namelen > 0 {
            let mut addrlen = m.msg_namelen;
            let r = unsafe {
                recvfrom(
                    fd,
                    buf.as_mut_ptr(),
                    recv_cap,
                    flags,
                    m.msg_name.cast::<Sockaddr>(),
                    &raw mut addrlen,
                )
            };
            m.msg_namelen = addrlen;
            r
        } else {
            m.msg_namelen = 0;
            unsafe { recv(fd, buf.as_mut_ptr().cast(), recv_cap, flags) }
        };
        if ret <= 0 {
            m.msg_flags = 0;
            m.msg_controllen = 0;
            return ret;
        }
        let received = ret as usize;
        // Distribute received bytes across iovecs.
        let mut remaining = received;
        let mut src_pos: usize = 0;
        i = 0;
        while i < m.msg_iovlen && remaining > 0 {
            let iov = unsafe { &*m.msg_iov.add(i) };
            if iov.iov_len > 0 && !iov.iov_base.is_null() {
                let to_copy = if remaining < iov.iov_len {
                    remaining
                } else {
                    iov.iov_len
                };
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
        let trunc = received == recv_cap && recv_cap < total_cap;
        (ret, trunc)
    } else {
        let recv_cap = if total_cap < STACK_BUF_SMALL {
            total_cap
        } else {
            STACK_BUF_SMALL
        };
        let mut buf = [0u8; STACK_BUF_SMALL];
        let ret = if !m.msg_name.is_null() && m.msg_namelen > 0 {
            let mut addrlen = m.msg_namelen;
            let r = unsafe {
                recvfrom(
                    fd,
                    buf.as_mut_ptr(),
                    recv_cap,
                    flags,
                    m.msg_name.cast::<Sockaddr>(),
                    &raw mut addrlen,
                )
            };
            m.msg_namelen = addrlen;
            r
        } else {
            m.msg_namelen = 0;
            unsafe { recv(fd, buf.as_mut_ptr().cast(), recv_cap, flags) }
        };
        if ret <= 0 {
            m.msg_flags = 0;
            m.msg_controllen = 0;
            return ret;
        }
        let received = ret as usize;
        // Distribute received bytes across iovecs.
        let mut remaining = received;
        let mut src_pos: usize = 0;
        i = 0;
        while i < m.msg_iovlen && remaining > 0 {
            let iov = unsafe { &*m.msg_iov.add(i) };
            if iov.iov_len > 0 && !iov.iov_base.is_null() {
                let to_copy = if remaining < iov.iov_len {
                    remaining
                } else {
                    iov.iov_len
                };
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
        let trunc = received == recv_cap && recv_cap < total_cap;
        (ret, trunc)
    };

    // Set MSG_TRUNC if the receive buffer was full and smaller than total
    // capacity (indicating potential data truncation for datagrams).
    // TCP is a byte stream — short reads are normal, not truncation.
    m.msg_flags = if trunc_flag && is_dgram { MSG_TRUNC } else { 0 };
    m.msg_controllen = 0;

    received
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Network interfaces: names, indices and addresses
// ---------------------------------------------------------------------------
//
// This system has two interfaces, numbered as Linux numbers them: the loopback
// first, then the one NIC. The kernel calls the NIC `eth0`
// (`kernel/src/fs/netdev.rs`, `NIC_IFACE`) and describes it through
// `SYS_NET_IF_INFO`, which always answers -- the interface exists whether or
// not it is up. Until 2026-09-27 `lo` and `eth0` were both index 1, index 1
// was `eth0` alone, `if_nameindex` listed only `eth0`, and `if_nameindex` and
// `getifaddrs` handed every caller the same static storage -- rewritten under
// a caller still reading it, and raced by two threads.

/// The interfaces, in index order: `(index, name)`.
const INTERFACES: [(u32, &[u8]); 2] = [(1, b"lo"), (2, b"eth0")];

/// Maximum interface name length, the terminating NUL included.
pub const IF_NAMESIZE: usize = 16;

/// The index of the interface named `name`, if there is one.
fn interface_index(name: &[u8]) -> Option<u32> {
    INTERFACES
        .iter()
        .find(|&&(_, n)| n == name)
        .map(|&(i, _)| i)
}

/// Convert a network interface name to its index.
///
/// Returns 0 for a name no interface has, with `errno` `ENODEV`, as glibc
/// does (its `SIOCGIFINDEX` answers `ENODEV`); a NULL name is 0 as well.
///
/// # Safety
///
/// `ifname` must be NULL or a valid NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn if_nametoindex(ifname: *const u8) -> u32 {
    if ifname.is_null() {
        return 0;
    }
    // SAFETY: the caller's contract: a NUL-terminated string.
    let name = unsafe { core::ffi::CStr::from_ptr(ifname.cast()) }.to_bytes();
    if let Some(index) = interface_index(name) {
        return index;
    }
    errno::set_errno(errno::ENODEV);
    0
}

/// Convert a network interface index to its name, written into `ifname`.
///
/// Returns `ifname`, or NULL with `errno` `ENXIO` for an index no interface
/// has -- glibc's mapping of the kernel's `ENODEV`.
///
/// # Safety
///
/// `ifname` must point to writable memory of at least [`IF_NAMESIZE`] bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn if_indextoname(ifindex: u32, ifname: *mut u8) -> *mut u8 {
    if ifname.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    let Some(&(_, name)) = INTERFACES.iter().find(|&&(i, _)| i == ifindex) else {
        errno::set_errno(errno::ENXIO);
        return core::ptr::null_mut();
    };
    // SAFETY: the caller's contract: `IF_NAMESIZE` writable bytes, and every
    // name here is shorter than that with its NUL.
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), ifname, name.len());
        *ifname.add(name.len()) = 0;
    }
    ifname
}

/// Entry returned by `if_nameindex`.
#[repr(C)]
pub struct IfNameindex {
    /// Interface index (1-based, 0 = end of list).
    pub if_index: u32,
    /// Interface name (null-terminated, points into the same allocation).
    pub if_name: *mut u8,
}

/// `if_nameindex`'s block: the array, its terminator, and the names the
/// entries point into -- one allocation, so that `if_freenameindex` is `free`.
#[repr(C)]
struct NameindexBlock {
    entries: [IfNameindex; INTERFACES.len() + 1],
    names: [[u8; IF_NAMESIZE]; INTERFACES.len()],
}

/// Return every network interface's index and name: an array ending in an
/// entry whose index is 0, allocated for this caller.  Free it with
/// [`if_freenameindex`].  NULL with `ENOMEM` if memory ran out.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn if_nameindex() -> *mut IfNameindex {
    let block = crate::malloc::calloc(1, size_of::<NameindexBlock>()).cast::<NameindexBlock>();
    if block.is_null() {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }
    // SAFETY: `block` is a fresh, zeroed allocation of one `NameindexBlock`
    // (malloc's alignment suits it), used by nothing else; all-zero is a valid
    // `NameindexBlock` (null names, index 0), so the terminator is already set.
    unsafe {
        let names = core::ptr::addr_of_mut!((*block).names).cast::<[u8; IF_NAMESIZE]>();
        let entries = core::ptr::addr_of_mut!((*block).entries).cast::<IfNameindex>();
        for (k, &(index, name)) in INTERFACES.iter().enumerate() {
            let slot = names.add(k).cast::<u8>();
            core::ptr::copy_nonoverlapping(name.as_ptr(), slot, name.len());
            entries.add(k).write(IfNameindex {
                if_index: index,
                if_name: slot,
            });
        }
        entries
    }
}

/// Free an array [`if_nameindex`] returned.  NULL is ignored.
///
/// # Safety
///
/// `ptr` is NULL, or an array from [`if_nameindex`] not freed before.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn if_freenameindex(ptr: *mut IfNameindex) {
    // SAFETY: the caller's contract: the start of one `if_nameindex` block,
    // which is one `malloc` allocation.
    unsafe { crate::malloc::free(ptr.cast()) };
}

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
    /// Broadcast address -- or the union's other member, the far end of a
    /// point-to-point link, which is how glibc reads the loopback's.
    pub ifa_broadaddr: *const Sockaddr,
    /// A link entry's counters, an [`RtnlLinkStats`]; NULL for an address
    /// entry, as in glibc.
    pub ifa_data: *const u8,
}

// Interface flags, as Linux numbers them.
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
/// The link has carrier.  Past the sixteen bits `SIOCGIFFLAGS` carries, but
/// in the flags `getifaddrs` reports, which glibc takes from netlink.
pub const IFF_LOWER_UP: u32 = 0x1_0000;

/// `sll_hatype` of an Ethernet link (`net/if_arp.h`).
pub const ARPHRD_ETHER: u16 = 1;
/// `sll_hatype` of the loopback (`net/if_arp.h`).
pub const ARPHRD_LOOPBACK: u16 = 772;

/// `struct sockaddr_ll` (`netpacket/packet.h`): a link-level address, as a
/// link's [`getifaddrs`] entry gives its hardware and broadcast addresses.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrLl {
    /// `AF_PACKET`.
    pub sll_family: u16,
    /// Link-level protocol, network byte order: 0 in a `getifaddrs` entry.
    pub sll_protocol: u16,
    /// The interface's index.
    pub sll_ifindex: i32,
    /// The link's type: an `ARPHRD_*` number.
    pub sll_hatype: u16,
    /// Packet type: 0 in a `getifaddrs` entry.
    pub sll_pkttype: u8,
    /// How many bytes of `sll_addr` are the address.
    pub sll_halen: u8,
    /// The address, zero-padded.
    pub sll_addr: [u8; 8],
}

/// `struct rtnl_link_stats` (`linux/if_link.h`): a link's counters, which a
/// link's [`getifaddrs`] entry points `ifa_data` at.  Thirty-two bits wide,
/// as Linux's are: a count past 2^32 shows its low half.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtnlLinkStats {
    /// Packets received.
    pub rx_packets: u32,
    /// Packets sent.
    pub tx_packets: u32,
    /// Bytes received.
    pub rx_bytes: u32,
    /// Bytes sent.
    pub tx_bytes: u32,
    /// Bad packets received.
    pub rx_errors: u32,
    /// Packets that could not be sent.
    pub tx_errors: u32,
    /// Packets received and dropped.
    pub rx_dropped: u32,
    /// Packets dropped before they were sent.
    pub tx_dropped: u32,
    /// Multicast packets received.
    pub multicast: u32,
    /// Collisions.
    pub collisions: u32,
    /// Receive errors: a bad length.
    pub rx_length_errors: u32,
    /// Receive errors: the ring overflowed.
    pub rx_over_errors: u32,
    /// Receive errors: a bad CRC.
    pub rx_crc_errors: u32,
    /// Receive errors: a misaligned frame.
    pub rx_frame_errors: u32,
    /// Receive errors: the FIFO overran.
    pub rx_fifo_errors: u32,
    /// Receive errors: missed by the receiver.
    pub rx_missed_errors: u32,
    /// Send errors: aborted.
    pub tx_aborted_errors: u32,
    /// Send errors: the carrier was lost.
    pub tx_carrier_errors: u32,
    /// Send errors: the FIFO underran.
    pub tx_fifo_errors: u32,
    /// Send errors: no heartbeat.
    pub tx_heartbeat_errors: u32,
    /// Send errors: a late collision.
    pub tx_window_errors: u32,
    /// Compressed packets received.
    pub rx_compressed: u32,
    /// Compressed packets sent.
    pub tx_compressed: u32,
    /// Packets received and dropped for want of a protocol to take them.
    pub rx_nohandler: u32,
}

/// A link's `getifaddrs` entry: the node and everything it points at.
#[repr(C)]
struct LinkEntry {
    node: Ifaddrs,
    addr: SockaddrLl,
    broadaddr: SockaddrLl,
    stats: RtnlLinkStats,
    name: [u8; IF_NAMESIZE],
}

/// An address's `getifaddrs` entry: the node and everything it points at.
#[repr(C)]
struct InetEntry {
    node: Ifaddrs,
    addr: SockaddrIn,
    netmask: SockaddrIn,
    broadaddr: SockaddrIn,
    name: [u8; IF_NAMESIZE],
}

/// One `getifaddrs` list, in one allocation so that [`freeifaddrs`] is
/// `free`: each link's entry, then each address's.  The list's head, the
/// first link's node, is at offset 0.
#[repr(C)]
struct IfaddrsBlock {
    links: [LinkEntry; INTERFACES.len()],
    inet: [InetEntry; INTERFACES.len()],
}

/// A link's `getifaddrs` entry, before it is laid out.
struct LinkRow {
    name: &'static [u8],
    index: i32,
    flags: u32,
    hatype: u16,
    /// Its hardware address, and the one that reaches every station.
    addr: [u8; 6],
    broadcast: [u8; 6],
    stats: RtnlLinkStats,
}

/// An address's `getifaddrs` entry, before it is laid out.
struct InetRow {
    name: &'static [u8],
    flags: u32,
    /// Address, mask and broadcast address, network byte order.
    addr: u32,
    mask: u32,
    broadcast: u32,
}

/// An IPv4 address or mask, in network byte order, as a `sockaddr_in`.
fn inet_sockaddr(s_addr: u32) -> SockaddrIn {
    SockaddrIn {
        sin_family: AF_INET as u16,
        sin_port: 0,
        sin_addr: InAddr { s_addr },
        sin_zero: [0; 8],
    }
}

/// One of `link`'s hardware addresses, as a `sockaddr_ll`.
fn link_sockaddr(link: &LinkRow, hw: [u8; 6]) -> SockaddrLl {
    let mut sll_addr = [0u8; 8];
    for (to, from) in sll_addr.iter_mut().zip(hw) {
        *to = from;
    }
    SockaddrLl {
        sll_family: AF_PACKET as u16,
        sll_protocol: 0,
        sll_ifindex: link.index,
        sll_hatype: link.hatype,
        sll_pkttype: 0,
        sll_halen: 6,
        sll_addr,
    }
}

/// The loopback's flags, as Linux reports them.
const LO_FLAGS: u32 = IFF_UP | IFF_LOOPBACK | IFF_RUNNING | IFF_LOWER_UP;

/// `eth0`'s flags: an Ethernet link's, and up, running and with carrier
/// while the kernel has it up -- the kernel reports no carrier of its own.
fn eth0_flags(up: bool) -> u32 {
    let link = IFF_BROADCAST | IFF_MULTICAST;
    if up {
        link | IFF_UP | IFF_RUNNING | IFF_LOWER_UP
    } else {
        link
    }
}

/// The broadcast address glibc reports for `eth0`'s address and mask
/// (network byte order).  The kernel keeps none, so this is what a Linux
/// system configured the same way reports: the subnet's all-ones address --
/// except for a /31 or /32, which Linux gives no broadcast address (RFC
/// 3021; `ip addr ... brd +` sets none), and then glibc, reading the netlink
/// answer as a point-to-point link's, reports the address itself.
fn broadcast_for(addr: u32, mask: u32) -> u32 {
    if mask.count_ones() >= 31 {
        addr
    } else {
        addr | !mask
    }
}

/// Write a link's entry at `e`, ahead of `next`; answer its node.
///
/// # Safety
///
/// `e` is valid to write a `LinkEntry`, is zeroed, and lives as long as the
/// list does.
unsafe fn write_link(e: *mut LinkEntry, row: &LinkRow, next: *mut Ifaddrs) -> *mut Ifaddrs {
    // SAFETY: the caller's contract.  Each field is written through its own
    // place; the name fits with the NUL the zeroed entry already holds
    // (`IF_NAMESIZE` counts it, and no name here is longer than four).
    unsafe {
        core::ptr::addr_of_mut!((*e).addr).write(link_sockaddr(row, row.addr));
        core::ptr::addr_of_mut!((*e).broadaddr).write(link_sockaddr(row, row.broadcast));
        core::ptr::addr_of_mut!((*e).stats).write(row.stats);
        let name = core::ptr::addr_of_mut!((*e).name).cast::<u8>();
        core::ptr::copy_nonoverlapping(row.name.as_ptr(), name, row.name.len());
        core::ptr::addr_of_mut!((*e).node).write(Ifaddrs {
            ifa_next: next,
            ifa_name: name,
            ifa_flags: row.flags,
            ifa_addr: core::ptr::addr_of!((*e).addr).cast::<Sockaddr>(),
            ifa_netmask: core::ptr::null(),
            ifa_broadaddr: core::ptr::addr_of!((*e).broadaddr).cast::<Sockaddr>(),
            ifa_data: core::ptr::addr_of!((*e).stats).cast::<u8>(),
        });
        core::ptr::addr_of_mut!((*e).node)
    }
}

/// Write an address's entry at `e`, ahead of `next`; answer its node.
///
/// # Safety
///
/// As for [`write_link`], with an `InetEntry`.
unsafe fn write_inet(e: *mut InetEntry, row: &InetRow, next: *mut Ifaddrs) -> *mut Ifaddrs {
    // SAFETY: as in `write_link`.
    unsafe {
        core::ptr::addr_of_mut!((*e).addr).write(inet_sockaddr(row.addr));
        core::ptr::addr_of_mut!((*e).netmask).write(inet_sockaddr(row.mask));
        core::ptr::addr_of_mut!((*e).broadaddr).write(inet_sockaddr(row.broadcast));
        let name = core::ptr::addr_of_mut!((*e).name).cast::<u8>();
        core::ptr::copy_nonoverlapping(row.name.as_ptr(), name, row.name.len());
        core::ptr::addr_of_mut!((*e).node).write(Ifaddrs {
            ifa_next: next,
            ifa_name: name,
            ifa_flags: row.flags,
            ifa_addr: core::ptr::addr_of!((*e).addr).cast::<Sockaddr>(),
            ifa_netmask: core::ptr::addr_of!((*e).netmask).cast::<Sockaddr>(),
            ifa_broadaddr: core::ptr::addr_of!((*e).broadaddr).cast::<Sockaddr>(),
            ifa_data: core::ptr::null(),
        });
        core::ptr::addr_of_mut!((*e).node)
    }
}

/// `getifaddrs`'s list for this `eth0`, whose counters are `stats`: `lo`'s
/// link and `eth0`'s, then `lo`'s address and `eth0`'s if it has one --
/// glibc's order, which is netlink's.  NULL if memory ran out.
fn build_ifaddrs(nic: Nic, stats: RtnlLinkStats) -> *mut Ifaddrs {
    let eth0 = eth0_flags(nic.up);
    let links: [LinkRow; INTERFACES.len()] = [
        LinkRow {
            name: b"lo",
            index: 1,
            flags: LO_FLAGS,
            hatype: ARPHRD_LOOPBACK,
            addr: [0; 6],
            broadcast: [0; 6],
            // The kernel counts no loopback traffic.
            stats: RtnlLinkStats::default(),
        },
        LinkRow {
            name: b"eth0",
            index: 2,
            flags: eth0,
            hatype: ARPHRD_ETHER,
            addr: nic.mac,
            broadcast: [0xff; 6],
            stats,
        },
    ];
    let loopback = u32::to_be(INADDR_LOOPBACK);
    let addresses: [Option<InetRow>; INTERFACES.len()] = [
        Some(InetRow {
            name: b"lo",
            flags: LO_FLAGS,
            addr: loopback,
            mask: u32::to_be(0xFF00_0000),
            // Linux gives the loopback's address no broadcast address, and
            // glibc reads the netlink answer as a point-to-point link's.
            broadcast: loopback,
        }),
        // An address stays on a link that is down, and Linux lists it.
        (nic.ip != 0).then(|| InetRow {
            name: b"eth0",
            flags: eth0,
            addr: nic.ip,
            mask: nic.mask,
            broadcast: broadcast_for(nic.ip, nic.mask),
        }),
    ];
    let block = crate::malloc::calloc(1, size_of::<IfaddrsBlock>()).cast::<IfaddrsBlock>();
    if block.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: `block` is a fresh, zeroed allocation of one `IfaddrsBlock`
    // (malloc's alignment suits it), used by nothing else.  `links` and
    // `addresses` have as many rows as the block has entries of each kind,
    // so every index is in bounds, and each entry is written once.
    unsafe {
        let link_at = core::ptr::addr_of_mut!((*block).links).cast::<LinkEntry>();
        let inet_at = core::ptr::addr_of_mut!((*block).inet).cast::<InetEntry>();
        // Last entry first, so each node's `ifa_next` is the one just written.
        let mut next: *mut Ifaddrs = core::ptr::null_mut();
        for (k, row) in addresses.iter().enumerate().rev() {
            if let Some(row) = row {
                next = write_inet(inet_at.add(k), row, next);
            }
        }
        for (k, row) in links.iter().enumerate().rev() {
            next = write_link(link_at.add(k), row, next);
        }
        next
    }
}

/// `eth0` as the kernel describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Nic {
    /// The kernel found a NIC, and nothing has set it down.
    up: bool,
    /// Its hardware address: all zero when the kernel found no NIC.
    mac: [u8; 6],
    /// Address, mask and gateway, network byte order.  The address is 0
    /// until DHCP has answered.
    ip: u32,
    mask: u32,
    gateway: u32,
}

impl Nic {
    /// What the kernel describes when it found no NIC.
    const NONE: Self = Self {
        up: false,
        mac: [0; 6],
        ip: 0,
        mask: 0,
        gateway: 0,
    };
}

/// `eth0`, from the kernel's record: [0..4] address, [4..8] mask, [8..12]
/// gateway, [12..16] DNS server -- network byte order -- [16..22] hardware
/// address, [22] bit 0 up (`sys_net_if_info`).  A kernel that does not
/// answer is taken for a machine with no NIC.
fn nic() -> Nic {
    #[cfg(test)]
    {
        // SAFETY: this thread's stand-in.
        unsafe { *test_nic() }
    }
    #[cfg(not(test))]
    {
        let mut info = [0u8; 24];
        if syscall2(SYS_NET_IF_INFO, info.as_mut_ptr() as u64, info.len() as u64) != 0 {
            return Nic::NONE;
        }
        let word = |range: core::ops::Range<usize>| {
            info.get(range)
                .and_then(|b| <[u8; 4]>::try_from(b).ok())
                .map_or(0, u32::from_ne_bytes)
        };
        Nic {
            up: info.get(22).is_some_and(|&f| f & 1 != 0),
            mac: info
                .get(16..22)
                .and_then(|b| <[u8; 6]>::try_from(b).ok())
                .unwrap_or_default(),
            ip: word(0..4),
            mask: word(4..8),
            gateway: word(8..12),
        }
    }
}

/// The kernel's six counters for the NIC (`SYS_NET_STAT`, source 1): bytes,
/// packets and errors sent, bytes and packets received, and received
/// packets dropped.  `None` if it does not answer.
fn nic_counters() -> Option<[u64; 6]> {
    #[cfg(test)]
    {
        // SAFETY: this thread's stand-in.
        unsafe { *test_nic_counters() }
    }
    #[cfg(not(test))]
    {
        let mut raw = [0u8; 48];
        if syscall2(SYS_NET_STAT, raw.as_mut_ptr() as u64, 1) != 0 {
            return None;
        }
        let mut counters = [0u64; 6];
        for (counter, bytes) in counters.iter_mut().zip(raw.chunks_exact(8)) {
            *counter = <[u8; 8]>::try_from(bytes).map_or(0, u64::from_le_bytes);
        }
        Some(counters)
    }
}

/// A 64-bit counter's low half, as Linux's `copy_rtnl_link_stats` keeps it
/// when it fills the 32-bit `rtnl_link_stats`.
#[allow(clippy::cast_possible_truncation)] // the truncation is the point
fn low32(count: u64) -> u32 {
    count as u32
}

/// `eth0`'s counters as `rtnl_link_stats` carries them: the kernel's own
/// for the NIC, each cut to 32 bits; zero where the kernel keeps no count,
/// and all zero if it does not answer.
fn nic_stats() -> RtnlLinkStats {
    let Some(
        [
            tx_bytes,
            tx_packets,
            tx_errors,
            rx_bytes,
            rx_packets,
            rx_drops,
        ],
    ) = nic_counters()
    else {
        return RtnlLinkStats::default();
    };
    RtnlLinkStats {
        rx_packets: low32(rx_packets),
        tx_packets: low32(tx_packets),
        rx_bytes: low32(rx_bytes),
        tx_bytes: low32(tx_bytes),
        tx_errors: low32(tx_errors),
        rx_dropped: low32(rx_drops),
        ..RtnlLinkStats::default()
    }
}

#[cfg(test)]
crate::perprocess::process_global! {
    /// The host tests' NIC: none unless a test gives one.
    fn test_nic() -> Nic = Nic::NONE;
    /// The host tests' NIC counters: none unless a test gives them.
    fn test_nic_counters() -> Option<[u64; 6]> = None;
}

/// Give this host test thread a NIC, and counters for it.
#[cfg(test)]
fn set_test_nic(nic: Nic, counters: Option<[u64; 6]>) {
    // SAFETY: this thread's stand-ins.
    unsafe {
        *test_nic() = nic;
        *test_nic_counters() = counters;
    }
}

/// Give this host test thread an `eth0` that is up, with QEMU's MAC and
/// this address, mask and gateway (network byte order) -- or, for `None`,
/// no address yet.
#[cfg(test)]
pub(crate) fn set_test_eth0(eth0: Option<(u32, u32, u32)>) {
    let (ip, mask, gateway) = eth0.unwrap_or((0, 0, 0));
    set_test_nic(
        Nic {
            up: true,
            mac: [0x52, 0x54, 0, 0x12, 0x34, 0x56],
            ip,
            mask,
            gateway,
        },
        None,
    );
}

/// `eth0`'s address, mask and gateway, network byte order, when it can
/// carry traffic: up, and with an address.
fn eth0_info() -> Option<(u32, u32, u32)> {
    let nic = nic();
    (nic.up && nic.ip != 0).then_some((nic.ip, nic.mask, nic.gateway))
}

/// The address a connection to `peer` (network byte order) goes out from,
/// as this system routes it: the loopback's for 127/8 -- and for 0.0.0.0,
/// which Linux takes as the loopback -- and `eth0`'s for anything its
/// subnet, a broadcast or multicast, or its gateway reaches.  `None`: no
/// route.
pub(crate) fn route_source(peer: u32) -> Option<u32> {
    let loopback = u32::to_be(INADDR_LOOPBACK);
    let host = u32::from_be(peer);
    if host >> 24 == 127 || host == 0 {
        return Some(loopback);
    }
    let (ip, mask, gw) = eth0_info()?;
    let multicast = host >> 28 == 0xe;
    if peer & mask == ip & mask || host == u32::MAX || multicast || gw != 0 {
        Some(ip)
    } else {
        None
    }
}

/// Each interface's IPv4 address and mask, network byte order, as
/// `getifaddrs` lists them: the loopback's, then `eth0`'s if it has one --
/// up or not, as Linux keeps an address on a link that is down and lists it
/// (glibc's `check_pf`, and `SIOCGIFCONF`, count it).
pub(crate) fn for_each_ipv4_interface(mut f: impl FnMut(u32, u32)) {
    f(u32::to_be(INADDR_LOOPBACK), u32::to_be(0xFF00_0000));
    let nic = nic();
    if nic.ip != 0 {
        f(nic.ip, nic.mask);
    }
}

/// Retrieve a linked list of network interface addresses.
///
/// Each call allocates its own list, freed with [`freeifaddrs`], in glibc's
/// order -- which is netlink's: each link's `AF_PACKET` entry, then each
/// address's `AF_INET` entry.
///
/// | Entry | `ifa_addr` | `ifa_broadaddr` | `ifa_data` |
/// |---|---|---|---|
/// | `lo`, `AF_PACKET` | `ARPHRD_LOOPBACK`, a zero address | the same | its [`RtnlLinkStats`]: zero, as the kernel counts no loopback traffic |
/// | `eth0`, `AF_PACKET` | `ARPHRD_ETHER`, the NIC's MAC | `ff:ff:ff:ff:ff:ff` | the NIC's counters |
/// | `lo`, `AF_INET` | 127.0.0.1, mask 255.0.0.0 | 127.0.0.1: glibc reads the loopback as a point-to-point link | NULL |
/// | `eth0`, `AF_INET`, while it has an address, up or not | its address and mask | the subnet's broadcast address ([`broadcast_for`]) | NULL |
///
/// A link entry's `ifa_addr` and `ifa_broadaddr` are `sockaddr_ll`s
/// ([`SockaddrLl`]) and its `ifa_netmask` is NULL.  `eth0`'s flags are an
/// Ethernet link's, `IFF_BROADCAST | IFF_MULTICAST`, with `IFF_UP |
/// IFF_RUNNING | IFF_LOWER_UP` while the kernel has it up; `lo`'s are
/// `IFF_UP | IFF_LOOPBACK | IFF_RUNNING | IFF_LOWER_UP`.  `eth0` is listed
/// even when the kernel found no NIC -- down, with a zero MAC -- as
/// [`if_nameindex`] lists it.  No `AF_INET6` entries: this system has no
/// IPv6.
///
/// Returns 0, or -1 with `EFAULT` for a NULL `ifap` or `ENOMEM`.
///
/// # Safety
///
/// `ifap` must be NULL or valid to write the result.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getifaddrs(ifap: *mut *mut Ifaddrs) -> i32 {
    if ifap.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let list = build_ifaddrs(nic(), nic_stats());
    if list.is_null() {
        errno::set_errno(errno::ENOMEM);
        return -1;
    }
    // SAFETY: the caller's contract: `ifap` is valid to write.
    unsafe { *ifap = list };
    0
}

/// Free a list [`getifaddrs`] returned.  NULL is ignored.
///
/// # Safety
///
/// `ifa` is NULL, or the head of a list from [`getifaddrs`] not freed before.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn freeifaddrs(ifa: *mut Ifaddrs) {
    // SAFETY: the caller's contract: the head of a `getifaddrs` list, which
    // is the start of its one `malloc` allocation.
    unsafe { crate::malloc::free(ifa.cast()) };
}

// ---------------------------------------------------------------------------
// Error translation
// ---------------------------------------------------------------------------

/// Translate a kernel error code (a negative `i64` syscall return) to the
/// POSIX errno a socket call should report.
///
/// This is [`errno::errno_for`] with exactly one socket-specific override,
/// and it used to be a second full copy of that table.  The copy had drifted:
/// it was missing eighteen of the kernel's codes, among them the entire
/// `-700` range, which exists for sockets and for nothing else.  Every one of
/// those arrived here as `EIO` via the catch-all -- so a non-blocking
/// `connect` still handshaking reported a dead socket instead of
/// `EINPROGRESS`, and a `bind` to a taken port reported an I/O failure
/// instead of `EADDRINUSE`.  A table that answers plausibly for codes it has
/// never heard of cannot report its own staleness, which is why there is now
/// one of them and a test that reads the kernel enum.
///
/// **The override.** `AlreadyExists` (-501) is `EEXIST` for a filesystem call
/// and `EADDRINUSE` for a socket one: the only thing a socket call creates by
/// name is a local address binding, so the generic answer is never the right
/// one here.  It stays a special case rather than being pushed into the
/// shared table because the shared table has no way to know which call it is
/// answering for.
pub(crate) fn translate_net_error(code: i64) -> i32 {
    match code {
        errno::native::ALREADY_EXISTS => errno::EADDRINUSE,
        other => errno::errno_for(other),
    }
}

// ---------------------------------------------------------------------------
// sockatmark — test whether socket is at out-of-band mark
// ---------------------------------------------------------------------------

/// `sockatmark` — test whether a socket is at the out-of-band mark.
///
/// Returns 1 if the socket is at the OOB mark, 0 if not, -1 on error.
///
/// Our kernel doesn't support OOB data, so we always return 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sockatmark(fd: i32) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // No OOB data support — never at mark.
    0
}

// ---------------------------------------------------------------------------
// sendmmsg / recvmmsg — one call, several messages
// ---------------------------------------------------------------------------

/// Return from `recvmmsg` as soon as one message has arrived, rather than
/// waiting for `vlen` of them.
pub const MSG_WAITFORONE: i32 = 0x10000;

/// One entry of the array `sendmmsg`/`recvmmsg` walk: a message and the byte
/// count that call produced for it.
#[repr(C)]
pub struct Mmsghdr {
    /// The message itself.
    pub msg_hdr: Msghdr,
    /// Bytes transferred for this message. Written by both calls.
    pub msg_len: u32,
}

/// Send several messages with one call.
///
/// Returns the number of messages sent, which **may be fewer than `vlen`** —
/// that is the API's contract and not a shortcut here: Linux stops at the
/// first message that fails and reports how many went before it. `-1` is
/// returned only when the *first* message fails, because there is then no
/// partial success to report and `errno` is the only answer available.
///
/// Each entry's `msg_len` is set to the byte count for that message, which is
/// the whole reason a caller uses this in preference to a loop of `sendmsg`:
/// it wants the per-message counts back without writing the loop.
///
/// # Safety
///
/// `msgvec` must point to `vlen` valid `Mmsghdr` values, each holding a
/// `Msghdr` that satisfies [`sendmsg`]'s contract.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sendmmsg(fd: i32, msgvec: *mut Mmsghdr, vlen: u32, flags: i32) -> i32 {
    if msgvec.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let mut sent: i32 = 0;
    for i in 0..vlen {
        // SAFETY: caller guarantees `vlen` valid entries; `i < vlen`.
        let slot = unsafe { &mut *msgvec.add(i as usize) };
        // SAFETY: forwarding this function's own contract for one message.
        let n = unsafe { sendmsg(fd, &raw const slot.msg_hdr, flags) };
        if n < 0 {
            // A failure after at least one success is reported as the count,
            // with errno left as the failing call set it. Reporting -1 here
            // would tell the caller nothing was sent when some was, and the
            // messages already gone cannot be unsent.
            return if sent == 0 { -1 } else { sent };
        }
        slot.msg_len = u32::try_from(n).unwrap_or(u32::MAX);
        sent = sent.saturating_add(1);
    }
    sent
}

/// Receive several messages with one call.
///
/// Returns the number of messages received, which may be fewer than `vlen`.
///
/// **Only the first receive blocks.** The rest are issued with
/// `MSG_DONTWAIT`, so this returns what has actually arrived instead of
/// waiting for the array to fill. That is within the contract — a caller must
/// already handle a short count, because a timeout or an error can cause one —
/// and it is the behaviour `MSG_WAITFORONE` asks for, which is the flag the
/// callers that reach for this call tend to pass anyway.
///
/// **A non-null `timeout` is refused with `EINVAL`.** We have no way to bound
/// the first, blocking receive, and the alternative is worse than an error: a
/// caller that asked to wait 100 ms would wait for ever, on a call it chose
/// specifically because it wanted a bound. `EINVAL` is a documented answer;
/// blocking past a deadline is not.
///
/// # Safety
///
/// `msgvec` must point to `vlen` valid `Mmsghdr` values, each holding a
/// `Msghdr` that satisfies [`recvmsg`]'s contract.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn recvmmsg(
    fd: i32,
    msgvec: *mut Mmsghdr,
    vlen: u32,
    flags: i32,
    timeout: *mut crate::stat::Timespec,
) -> i32 {
    if msgvec.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !timeout.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let mut got: i32 = 0;
    for i in 0..vlen {
        // SAFETY: caller guarantees `vlen` valid entries; `i < vlen`.
        let slot = unsafe { &mut *msgvec.add(i as usize) };
        let this_flags = if got == 0 {
            flags & !MSG_WAITFORONE
        } else {
            (flags & !MSG_WAITFORONE) | MSG_DONTWAIT
        };
        // SAFETY: forwarding this function's own contract for one message.
        let n = unsafe { recvmsg(fd, &raw mut slot.msg_hdr, this_flags) };
        if n < 0 {
            return if got == 0 { -1 } else { got };
        }
        slot.msg_len = u32::try_from(n).unwrap_or(u32::MAX);
        got = got.saturating_add(1);
        if flags & MSG_WAITFORONE != 0 {
            break;
        }
    }
    got
}

// ---------------------------------------------------------------------------
// Tests — pure logic functions only (no syscalls needed)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- The IPv6 address constants --

    #[test]
    fn in6addr_any_is_the_wildcard_and_loopback_is_one() {
        assert_eq!(in6addr_any.s6_addr, [0u8; 16]);
        let mut want = [0u8; 16];
        want[15] = 1;
        assert_eq!(in6addr_loopback.s6_addr, want);
        // They are distinct storage, not two names for one object: binding to
        // the wildcard and connecting to loopback are opposite intentions.
        assert_ne!(in6addr_any.s6_addr, in6addr_loopback.s6_addr);
    }

    // -- recvmmsg's refusals, which happen before any syscall --

    #[test]
    fn recvmmsg_refuses_a_timeout_it_cannot_honour() {
        // Ignoring the timeout would make a caller that asked to wait 100 ms
        // wait for ever, on the one call it chose because it wanted a bound.
        let mut ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 1,
        };
        let mut slot = zeroed_mmsghdr();
        // SAFETY: `msgvec` points to one valid Mmsghdr; the call returns on
        // the timeout check before touching it.
        let rc = unsafe { recvmmsg(-1, &raw mut slot, 1, 0, &raw mut ts) };
        assert_eq!(rc, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn both_batch_calls_reject_a_null_vector() {
        // SAFETY: a null `msgvec` is exactly what is under test; both return
        // before dereferencing it.
        unsafe {
            assert_eq!(
                recvmmsg(-1, core::ptr::null_mut(), 1, 0, core::ptr::null_mut()),
                -1
            );
            assert_eq!(errno::get_errno(), errno::EFAULT);
            assert_eq!(sendmmsg(-1, core::ptr::null_mut(), 1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }
    }

    #[test]
    fn a_zero_length_batch_transfers_nothing_and_succeeds() {
        // `vlen == 0` is not an error: there is nothing to send and nothing
        // went wrong. Returning -1 would make an empty batch indistinguishable
        // from a failed one.
        let mut slot = zeroed_mmsghdr();
        // SAFETY: `msgvec` is valid; `vlen` is 0 so no entry is read.
        unsafe {
            assert_eq!(sendmmsg(-1, &raw mut slot, 0, 0), 0);
            assert_eq!(recvmmsg(-1, &raw mut slot, 0, 0, core::ptr::null_mut()), 0);
        }
    }

    #[test]
    fn mmsghdr_matches_the_c_layout() {
        // `scripts/check-libc-abi.py` compares this against a real
        // <sys/socket.h> with zig, and skips the comparison on a machine with
        // no zig on PATH -- which is every Windows machine here, i.e. the one
        // I am on. This pins the same numbers portably so the skip is not the
        // only thing standing between a wrong offset and the image.
        //
        // `struct mmsghdr { struct msghdr msg_hdr; unsigned int msg_len; }`.
        // The second field is `unsigned int` and NOT `size_t`: on x86-64 the
        // wrong guess changes nothing about where the field starts and changes
        // the stride of the array, so it would corrupt every entry after the
        // first and look correct for a batch of one.
        assert_eq!(core::mem::size_of::<Msghdr>(), 56);
        assert_eq!(core::mem::align_of::<Mmsghdr>(), 8);
        assert_eq!(core::mem::size_of::<Mmsghdr>(), 64);
        assert_eq!(core::mem::offset_of!(Mmsghdr, msg_hdr), 0);
        assert_eq!(core::mem::offset_of!(Mmsghdr, msg_len), 56);
        assert_eq!(core::mem::size_of_val(&zeroed_mmsghdr().msg_len), 4);
    }

    fn zeroed_mmsghdr() -> Mmsghdr {
        Mmsghdr {
            msg_hdr: Msghdr {
                msg_name: core::ptr::null_mut(),
                msg_namelen: 0,
                msg_iov: core::ptr::null_mut(),
                msg_iovlen: 0,
                msg_control: core::ptr::null_mut(),
                msg_controllen: 0,
                msg_flags: 0,
            },
            msg_len: 0,
        }
    }

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
        // htons converts host byte order to network (big-endian) byte order.
        // On little-endian x86_64, this means swapping bytes.
        let val: u16 = 0x1234;
        let net = htons(val);
        // htons(x) == x.to_be() — that IS the conversion.
        assert_eq!(net, val.to_be());
        // Round-trip must recover original.
        assert_eq!(ntohs(net), val);
    }

    #[test]
    fn test_htonl_big_endian() {
        let val: u32 = 0x12345678;
        let net = htonl(val);
        assert_eq!(ntohl(net), val);
    }

    // -- translate_net_error tests --

    #[test]
    fn test_translate_net_error_known() {
        // General errors.
        assert_eq!(translate_net_error(-1), errno::EIO); // InternalError
        assert_eq!(translate_net_error(-2), errno::ENOTSUP); // NotSupported
        assert_eq!(translate_net_error(-3), errno::EINVAL); // InvalidArgument
        assert_eq!(translate_net_error(-4), errno::EAGAIN); // WouldBlock
        assert_eq!(translate_net_error(-5), errno::ECANCELED); // Cancelled
        assert_eq!(translate_net_error(-6), errno::ETIMEDOUT); // TimedOut
        // Memory.
        assert_eq!(translate_net_error(-100), errno::ENOMEM); // OutOfMemory
        // Capability / permission.
        assert_eq!(translate_net_error(-400), errno::EACCES); // PermissionDenied
        // Filesystem.
        assert_eq!(translate_net_error(-500), errno::ENOENT); // NotFound
        assert_eq!(translate_net_error(-501), errno::EADDRINUSE); // AlreadyExists
        // Device.
        assert_eq!(translate_net_error(-601), errno::ENODEV); // NoSuchDevice
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

    #[test]
    fn test_sockaddr_in6_layout() {
        // Linux struct sockaddr_in6 = 28 bytes.
        assert_eq!(core::mem::size_of::<SockaddrIn6>(), 28);
        assert_eq!(core::mem::offset_of!(SockaddrIn6, sin6_family), 0);
        assert_eq!(core::mem::offset_of!(SockaddrIn6, sin6_port), 2);
        assert_eq!(core::mem::offset_of!(SockaddrIn6, sin6_flowinfo), 4);
        assert_eq!(core::mem::offset_of!(SockaddrIn6, sin6_addr), 8);
        assert_eq!(core::mem::offset_of!(SockaddrIn6, sin6_scope_id), 24);
    }

    #[test]
    fn test_in6_addr_size() {
        assert_eq!(core::mem::size_of::<In6Addr>(), 16);
    }

    #[test]
    fn test_sockaddr_un_layout() {
        // Linux struct sockaddr_un = 110 bytes.
        assert_eq!(core::mem::size_of::<SockaddrUn>(), 110);
        assert_eq!(core::mem::offset_of!(SockaddrUn, sun_family), 0);
        assert_eq!(core::mem::offset_of!(SockaddrUn, sun_path), 2);
    }

    #[test]
    fn test_sockaddr_storage_layout() {
        // Linux struct sockaddr_storage = 128 bytes, 8-byte aligned.
        assert_eq!(core::mem::size_of::<SockaddrStorage>(), 128);
        assert_eq!(core::mem::align_of::<SockaddrStorage>(), 8);
    }

    // -- Address family and protocol constants --

    #[test]
    fn test_af_constants() {
        assert_eq!(AF_UNSPEC, 0);
        assert_eq!(AF_UNIX, 1);
        assert_eq!(AF_LOCAL, AF_UNIX);
        assert_eq!(AF_INET, 2);
        assert_eq!(AF_INET6, 10);
        assert_eq!(PF_UNIX, AF_UNIX);
        assert_eq!(PF_INET, AF_INET);
        assert_eq!(PF_INET6, AF_INET6);
    }

    #[test]
    fn test_sock_type_constants() {
        assert_eq!(SOCK_STREAM, 1);
        assert_eq!(SOCK_DGRAM, 2);
        assert_eq!(SOCK_RAW, 3);
        assert_eq!(SOCK_SEQPACKET, 5);
    }

    #[test]
    fn test_ipproto_constants() {
        assert_eq!(IPPROTO_IP, 0);
        assert_eq!(IPPROTO_ICMP, 1);
        assert_eq!(IPPROTO_TCP, 6);
        assert_eq!(IPPROTO_UDP, 17);
        assert_eq!(IPPROTO_IPV6, 41);
        assert_eq!(IPPROTO_ICMPV6, 58);
        assert_eq!(IPPROTO_RAW, 255);
    }

    #[test]
    fn test_in6addr_constants() {
        assert_eq!(IN6ADDR_ANY_INIT.s6_addr, [0u8; 16]);
        let mut expected = [0u8; 16];
        expected[15] = 1;
        assert_eq!(IN6ADDR_LOOPBACK_INIT.s6_addr, expected);
    }

    // -- hstrerror tests --

    #[test]
    fn test_hstrerror_no_error() {
        let msg = unsafe { c_str_to_slice(hstrerror(0)) };
        assert!(msg.starts_with(b"Resolver Error 0"));
    }

    #[test]
    fn test_hstrerror_no_data() {
        let msg = unsafe { c_str_to_slice(hstrerror(NO_DATA)) };
        assert_eq!(msg, b"No address associated with name");
    }

    #[test]
    fn test_hstrerror_unknown() {
        let msg = unsafe { c_str_to_slice(hstrerror(9999)) };
        assert_eq!(msg, b"Unknown resolver error");
    }

    // -- gai_strerror tests (pure function) --

    // -- EAI_* constant values --

    #[test]
    fn test_eai_constants() {
        // musl's `<netdb.h>` (and glibc's), probed 2026-09-27.
        assert_eq!(EAI_BADFLAGS, -1);
        assert_eq!(EAI_NONAME, -2);
        assert_eq!(EAI_AGAIN, -3);
        assert_eq!(EAI_FAIL, -4);
        assert_eq!(EAI_NODATA, -5);
        assert_eq!(EAI_FAMILY, -6);
        assert_eq!(EAI_SOCKTYPE, -7);
        assert_eq!(EAI_SERVICE, -8);
        assert_eq!(EAI_ADDRFAMILY, -9);
        assert_eq!(EAI_MEMORY, -10);
        assert_eq!(EAI_SYSTEM, -11);
        assert_eq!(EAI_OVERFLOW, -12);
    }

    // -- AI_* flag constants --

    #[test]
    fn test_ai_flag_constants() {
        assert_eq!(AI_PASSIVE, 0x0001);
        assert_eq!(AI_CANONNAME, 0x0002);
        assert_eq!(AI_NUMERICHOST, 0x0004);
        assert_eq!(AI_NUMERICSERV, 0x0400);
    }

    // -- NI_* flag constants --

    #[test]
    fn test_ni_flag_constants() {
        use crate::gai::{NI_DGRAM, NI_NAMEREQD, NI_NOFQDN, NI_NUMERICHOST, NI_NUMERICSERV};
        assert_eq!(NI_NUMERICHOST, 1);
        assert_eq!(NI_NUMERICSERV, 2);
        assert_eq!(NI_NOFQDN, 4);
        assert_eq!(NI_NAMEREQD, 8);
        assert_eq!(NI_DGRAM, 16);
    }

    // -- MSG_* flag constants --

    #[test]
    fn test_msg_flag_constants() {
        assert_eq!(MSG_OOB, 1);
        assert_eq!(MSG_PEEK, 2);
        assert_eq!(MSG_DONTROUTE, 4);
        assert_eq!(MSG_TRUNC, 0x20);
        assert_eq!(MSG_DONTWAIT, 0x40);
        assert_eq!(MSG_EOR, 0x80);
        assert_eq!(MSG_WAITALL, 0x100);
        assert_eq!(MSG_MORE, 0x8000);
        assert_eq!(MSG_NOSIGNAL, 0x4000);
    }

    // -- SHUT_* constants --

    #[test]
    fn test_shut_constants() {
        assert_eq!(SHUT_RD, 0);
        assert_eq!(SHUT_WR, 1);
        assert_eq!(SHUT_RDWR, 2);
    }

    // -- SOL_* constants --

    #[test]
    fn test_sol_constants() {
        assert_eq!(SOL_SOCKET, 1);
        assert_eq!(SOL_TCP, 6);
        assert_eq!(SOL_IP, 0);
    }

    // -- SO_* option constants --

    #[test]
    fn test_so_option_constants() {
        assert_eq!(SO_REUSEADDR, 2);
        assert_eq!(SO_TYPE, 3);
        assert_eq!(SO_ERROR, 4);
        assert_eq!(SO_BROADCAST, 6);
        assert_eq!(SO_SNDBUF, 7);
        assert_eq!(SO_RCVBUF, 8);
        assert_eq!(SO_KEEPALIVE, 9);
        assert_eq!(SO_LINGER, 13);
        assert_eq!(SO_REUSEPORT, 15);
        assert_eq!(SO_RCVLOWAT, 18);
        assert_eq!(SO_SNDLOWAT, 19);
        assert_eq!(SO_RCVTIMEO, 20);
        assert_eq!(SO_SNDTIMEO, 21);
        assert_eq!(SO_ACCEPTCONN, 30);
        assert_eq!(SO_PROTOCOL, 38);
        assert_eq!(SO_DOMAIN, 39);
    }

    // -- socketpair argument validation --
    //
    // These tests exercise only the pre-syscall validation branches.
    // The happy path issues SYS_SOCKETPAIR_CREATE via a raw `syscall`
    // instruction, which cannot run under the host test harness, so it
    // is covered by the in-kernel boot/integration path instead.

    #[test]
    fn test_socketpair_null_sv_efault() {
        let ret = socketpair(AF_UNIX, SOCK_STREAM, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_socketpair_bad_domain_eafnosupport() {
        let mut sv = [0i32; 2];
        // AF_INET is not supported by socketpair (only AF_UNIX/AF_LOCAL).
        let ret = socketpair(AF_INET, SOCK_STREAM, 0, &mut sv);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EAFNOSUPPORT);
    }

    #[test]
    fn test_socketpair_dgram_eopnotsupp() {
        let mut sv = [0i32; 2];
        // SOCK_DGRAM pairs are not implemented yet.
        let ret = socketpair(AF_UNIX, SOCK_DGRAM, 0, &mut sv);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_socketpair_bad_protocol_eprotonosupport() {
        let mut sv = [0i32; 2];
        let ret = socketpair(AF_UNIX, SOCK_STREAM, 99, &mut sv);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPROTONOSUPPORT);
    }

    #[test]
    fn test_socketpair_negative_type_einval() {
        let mut sv = [0i32; 2];
        let ret = socketpair(AF_UNIX, -1, 0, &mut sv);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_socketpair_stray_flag_bits_einval() {
        let mut sv = [0i32; 2];
        // A high flag bit that is neither SOCK_NONBLOCK nor SOCK_CLOEXEC.
        let ret = socketpair(AF_UNIX, SOCK_STREAM | 0x0080_0000, 0, &mut sv);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- freeaddrinfo tolerates NULL (should not crash) --

    // -- ntohl / ntohs --

    #[test]
    fn test_ntohl_identity_on_be() {
        // ntohl(htonl(x)) == x for all x.
        let x: u32 = 0x12345678;
        assert_eq!(ntohl(htonl(x)), x);
    }

    #[test]
    fn test_ntohs_identity_on_be() {
        let x: u16 = 0x1234;
        assert_eq!(ntohs(htons(x)), x);
    }

    #[test]
    fn test_ntohl_zero() {
        assert_eq!(ntohl(0), 0);
    }

    #[test]
    fn test_ntohs_zero() {
        assert_eq!(ntohs(0), 0);
    }

    #[test]
    fn test_ntohl_max() {
        assert_eq!(ntohl(htonl(u32::MAX)), u32::MAX);
    }

    #[test]
    fn test_ntohs_max() {
        assert_eq!(ntohs(htons(u16::MAX)), u16::MAX);
    }

    // -- __h_errno_location --

    #[test]
    fn test_h_errno_location_not_null() {
        let loc = __h_errno_location();
        assert!(!loc.is_null());
    }

    #[test]
    fn h_errno_location_is_stable_within_a_thread() {
        assert_eq!(__h_errno_location(), __h_errno_location());
    }

    #[test]
    fn h_errno_is_not_shared_between_threads() {
        // The whole point of moving it into `perthread`: one thread's
        // resolver error must not be visible as another's.
        let mine = __h_errno_location();
        set_h_errno(TRY_AGAIN);
        let theirs = std::thread::spawn(|| {
            let loc = __h_errno_location();
            let seen = get_h_errno();
            set_h_errno(NO_DATA);
            (loc as usize, seen, get_h_errno())
        })
        .join()
        .expect("resolver-error thread panicked");
        assert_ne!(theirs.0, mine as usize, "threads share one h_errno slot");
        assert_eq!(theirs.1, 0, "a fresh thread's h_errno must start at 0");
        assert_eq!(theirs.2, NO_DATA);
        assert_eq!(get_h_errno(), TRY_AGAIN, "the child clobbered our h_errno");
    }

    #[test]
    fn h_errno_is_distinct_storage_from_errno() {
        // They overlap numerically (HOST_NOT_FOUND == EPERM == 1), so a
        // shared slot would be an invisible-but-wrong answer.
        assert_ne!(
            __h_errno_location() as usize,
            crate::errno::__errno_location() as usize
        );
        set_h_errno(NO_RECOVERY);
        crate::errno::set_errno(crate::errno::EINVAL);
        assert_eq!(get_h_errno(), NO_RECOVERY);
    }

    /// A NULL (or empty) prefix prints the message alone, as glibc's `herror`
    /// does; it is not an error.
    #[test]
    fn herror_takes_a_null_prefix() {
        herror(core::ptr::null());
        herror(b"\0".as_ptr());
    }

    #[test]
    fn herror_accepts_every_resolver_code() {
        // Exercises the `get_h_errno` read path for each code; `herror`
        // writes to descriptor 2.
        for code in [0, HOST_NOT_FOUND, TRY_AGAIN, NO_RECOVERY, NO_DATA, 99] {
            set_h_errno(code);
            herror(b"test\0".as_ptr());
            assert_eq!(get_h_errno(), code, "herror must not clobber h_errno");
        }
    }

    // -- setservent / getservent / endservent --
    //
    // The cursor lives in the per-thread block and each `#[test]` gets its own
    // thread, so these do not need to undo each other's state.  They still call
    // `endservent` where the point is what a *fresh* enumeration sees, so the
    // intent survives if the harness ever stops giving each test a thread.

    // -- setprotoent / getprotoent / endprotoent --

    // -- shutdown with invalid how --

    #[test]
    fn test_shutdown_invalid_how() {
        let ret = shutdown(9999, 99); // Invalid how value
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- if_nametoindex / if_indextoname: lo is 1, eth0 2, as on Linux --

    #[test]
    fn if_nametoindex_numbers_the_loopback_first() {
        assert_eq!(unsafe { if_nametoindex(b"lo\0".as_ptr()) }, 1);
        assert_eq!(unsafe { if_nametoindex(b"eth0\0".as_ptr()) }, 2);
    }

    #[test]
    fn if_nametoindex_of_no_interface_is_zero_and_enodev() {
        errno::set_errno(0);
        assert_eq!(unsafe { if_nametoindex(b"nonexist99\0".as_ptr()) }, 0);
        assert_eq!(errno::get_errno(), errno::ENODEV);
        assert_eq!(unsafe { if_nametoindex(b"\0".as_ptr()) }, 0);
    }

    #[test]
    fn test_if_nametoindex_null() {
        let idx = unsafe { if_nametoindex(core::ptr::null()) };
        assert_eq!(idx, 0);
    }

    #[test]
    fn if_indextoname_names_each_index() {
        let mut buf = [0xAAu8; IF_NAMESIZE];
        assert_eq!(
            unsafe { if_indextoname(1, buf.as_mut_ptr()) },
            buf.as_mut_ptr()
        );
        assert_eq!(&buf[..3], b"lo\0");
        let mut buf = [0xAAu8; IF_NAMESIZE];
        assert_eq!(
            unsafe { if_indextoname(2, buf.as_mut_ptr()) },
            buf.as_mut_ptr()
        );
        assert_eq!(&buf[..5], b"eth0\0");
    }

    #[test]
    fn if_indextoname_of_no_interface_is_enxio() {
        let mut buf = [0u8; IF_NAMESIZE];
        for index in [0, 3, 999] {
            errno::set_errno(0);
            assert!(unsafe { if_indextoname(index, buf.as_mut_ptr()) }.is_null());
            assert_eq!(errno::get_errno(), errno::ENXIO, "index {index}");
        }
    }

    // -- if_nameindex / if_freenameindex --

    /// Every interface, in index order, then the terminator -- and the names
    /// agree with `if_nametoindex`.
    #[test]
    fn if_nameindex_lists_every_interface() {
        let table = if_nameindex();
        assert!(!table.is_null());
        let mut seen = Vec::new();
        for k in 0.. {
            // SAFETY: the array ends with an index-0 entry, not yet reached.
            let e = unsafe { &*table.add(k) };
            if e.if_index == 0 {
                assert!(e.if_name.is_null());
                break;
            }
            let name = unsafe { c_str_to_slice(e.if_name.cast_const()) }.to_vec();
            let mut z = name.clone();
            z.push(0);
            assert_eq!(unsafe { if_nametoindex(z.as_ptr()) }, e.if_index);
            seen.push((e.if_index, name));
        }
        assert_eq!(seen, vec![(1, b"lo".to_vec()), (2, b"eth0".to_vec())]);
        unsafe { if_freenameindex(table) };
    }

    /// Each call's array is its own: freeing one leaves another intact.
    #[test]
    fn if_nameindex_gives_each_caller_its_own_array() {
        let a = if_nameindex();
        let b = if_nameindex();
        assert!(!a.is_null() && !b.is_null());
        assert_ne!(a, b);
        unsafe { if_freenameindex(a) };
        let name = unsafe { c_str_to_slice((*b).if_name.cast_const()) };
        assert_eq!(name, b"lo");
        unsafe { if_freenameindex(b) };
    }

    #[test]
    fn test_if_freenameindex_null() {
        unsafe { if_freenameindex(core::ptr::null_mut()) };
    }

    // -- getifaddrs / freeifaddrs, against glibc --

    /// What `posix/tools/oracle/ifaddrs_oracle.c` printed for glibc 2.39's
    /// `getifaddrs` in network sandboxes shaped like this system -- `lo`,
    /// and a veth named `eth0` with QEMU's MAC and DHCP address
    /// (`posix/tools/oracle/ifaddrs_run.sh`; the veth's peer left out, IPv6 off).
    /// `eth0` up, 10.0.2.15/24 with its broadcast address:
    const GLIBC_UP: &[&str] = &[
        "name=lo family=17 flags=0x10049 addr=ll(proto=0,ifindex=1,hatype=772,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) netmask=- broadaddr=ll(proto=0,ifindex=1,hatype=772,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) data=stats",
        "name=eth0 family=17 flags=0x11043 addr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=52:54:00:12:34:56:00:00) netmask=- broadaddr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=ff:ff:ff:ff:ff:ff:00:00) data=stats",
        "name=lo family=2 flags=0x10049 addr=in(127.0.0.1,port=0,zero=0000000000000000) netmask=in(255.0.0.0,port=0,zero=0000000000000000) broadaddr=in(127.0.0.1,port=0,zero=0000000000000000) data=-",
        "name=eth0 family=2 flags=0x11043 addr=in(10.0.2.15,port=0,zero=0000000000000000) netmask=in(255.255.255.0,port=0,zero=0000000000000000) broadaddr=in(10.0.2.255,port=0,zero=0000000000000000) data=-",
    ];

    /// The same, with `eth0` then set down: it keeps its address.
    const GLIBC_DOWN: &[&str] = &[
        "name=lo family=17 flags=0x10049 addr=ll(proto=0,ifindex=1,hatype=772,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) netmask=- broadaddr=ll(proto=0,ifindex=1,hatype=772,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) data=stats",
        "name=eth0 family=17 flags=0x1002 addr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=52:54:00:12:34:56:00:00) netmask=- broadaddr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=ff:ff:ff:ff:ff:ff:00:00) data=stats",
        "name=lo family=2 flags=0x10049 addr=in(127.0.0.1,port=0,zero=0000000000000000) netmask=in(255.0.0.0,port=0,zero=0000000000000000) broadaddr=in(127.0.0.1,port=0,zero=0000000000000000) data=-",
        "name=eth0 family=2 flags=0x1002 addr=in(10.0.2.15,port=0,zero=0000000000000000) netmask=in(255.255.255.0,port=0,zero=0000000000000000) broadaddr=in(10.0.2.255,port=0,zero=0000000000000000) data=-",
    ];

    /// `eth0` up, before DHCP has answered.
    const GLIBC_NO_ADDRESS: &[&str] = &[
        "name=lo family=17 flags=0x10049 addr=ll(proto=0,ifindex=1,hatype=772,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) netmask=- broadaddr=ll(proto=0,ifindex=1,hatype=772,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) data=stats",
        "name=eth0 family=17 flags=0x11043 addr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=52:54:00:12:34:56:00:00) netmask=- broadaddr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=ff:ff:ff:ff:ff:ff:00:00) data=stats",
        "name=lo family=2 flags=0x10049 addr=in(127.0.0.1,port=0,zero=0000000000000000) netmask=in(255.0.0.0,port=0,zero=0000000000000000) broadaddr=in(127.0.0.1,port=0,zero=0000000000000000) data=-",
    ];

    /// `eth0` up with 10.0.2.15/31, which Linux gives no broadcast address.
    const GLIBC_SLASH_31: &[&str] = &[
        "name=lo family=17 flags=0x10049 addr=ll(proto=0,ifindex=1,hatype=772,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) netmask=- broadaddr=ll(proto=0,ifindex=1,hatype=772,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) data=stats",
        "name=eth0 family=17 flags=0x11043 addr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=52:54:00:12:34:56:00:00) netmask=- broadaddr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=ff:ff:ff:ff:ff:ff:00:00) data=stats",
        "name=lo family=2 flags=0x10049 addr=in(127.0.0.1,port=0,zero=0000000000000000) netmask=in(255.0.0.0,port=0,zero=0000000000000000) broadaddr=in(127.0.0.1,port=0,zero=0000000000000000) data=-",
        "name=eth0 family=2 flags=0x11043 addr=in(10.0.2.15,port=0,zero=0000000000000000) netmask=in(255.255.255.254,port=0,zero=0000000000000000) broadaddr=in(10.0.2.15,port=0,zero=0000000000000000) data=-",
    ];

    /// QEMU's MAC, which the oracle's `eth0` was given.
    const QEMU_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

    /// An `eth0` as QEMU's user network makes it: 10.0.2.15, gateway
    /// 10.0.2.2, with this mask.
    fn qemu_nic(up: bool, mask: [u8; 4]) -> Nic {
        Nic {
            up,
            mac: QEMU_MAC,
            ip: u32::from_ne_bytes([10, 0, 2, 15]),
            mask: u32::from_ne_bytes(mask),
            gateway: u32::from_ne_bytes([10, 0, 2, 2]),
        }
    }

    /// A `sockaddr`, as the oracle prints it, onto `out`.
    fn oracle_sockaddr(out: &mut String, tag: &str, sa: *const Sockaddr) {
        use core::fmt::Write as _;
        // `write!` to a `String` cannot fail.
        if sa.is_null() {
            let _ = write!(out, " {tag}=-");
            return;
        }
        let family = i32::from(unsafe { (*sa).sa_family });
        if family == AF_INET {
            let sin = unsafe { &*sa.cast::<SockaddrIn>() };
            let [a, b, c, d] = sin.sin_addr.s_addr.to_ne_bytes();
            let port = u16::from_be(sin.sin_port);
            let _ = write!(out, " {tag}=in({a}.{b}.{c}.{d},port={port},zero=");
            for z in sin.sin_zero {
                let _ = write!(out, "{z:02x}");
            }
        } else if family == AF_PACKET {
            let ll = unsafe { &*sa.cast::<SockaddrLl>() };
            let _ = write!(
                out,
                " {tag}=ll(proto={},ifindex={},hatype={},pkttype={},halen={},addr=",
                u16::from_be(ll.sll_protocol),
                ll.sll_ifindex,
                ll.sll_hatype,
                ll.sll_pkttype,
                ll.sll_halen
            );
            for (k, x) in ll.sll_addr.iter().enumerate() {
                let _ = write!(out, "{}{x:02x}", if k == 0 { "" } else { ":" });
            }
        } else {
            let _ = write!(out, " {tag}=family{family}");
            return;
        }
        out.push(')');
    }

    /// A list as the oracle prints it, one line per entry; the list freed.
    fn oracle_lines(list: *mut Ifaddrs) -> Vec<String> {
        let mut lines = Vec::new();
        let mut at = list;
        while !at.is_null() {
            let a = unsafe { &*at };
            let name = core::str::from_utf8(unsafe { c_str_to_slice(a.ifa_name) }).unwrap_or("?");
            let family = if a.ifa_addr.is_null() {
                0
            } else {
                unsafe { (*a.ifa_addr).sa_family }
            };
            let mut line = format!("name={name} family={family} flags={:#x}", a.ifa_flags);
            oracle_sockaddr(&mut line, "addr", a.ifa_addr);
            oracle_sockaddr(&mut line, "netmask", a.ifa_netmask);
            oracle_sockaddr(&mut line, "broadaddr", a.ifa_broadaddr);
            line.push_str(if a.ifa_data.is_null() {
                " data=-"
            } else {
                " data=stats"
            });
            lines.push(line);
            at = a.ifa_next;
        }
        unsafe { freeifaddrs(list) };
        lines
    }

    /// `getifaddrs` for this NIC, as the oracle prints a list.
    fn listed(nic: Nic) -> Vec<String> {
        set_test_nic(nic, None);
        let mut list: *mut Ifaddrs = core::ptr::null_mut();
        assert_eq!(unsafe { getifaddrs(&mut list) }, 0);
        assert!(!list.is_null());
        oracle_lines(list)
    }

    #[test]
    fn getifaddrs_is_glibcs_with_eth0_up() {
        assert_eq!(listed(qemu_nic(true, [255, 255, 255, 0])), GLIBC_UP);
    }

    #[test]
    fn getifaddrs_is_glibcs_with_eth0_down_and_keeps_its_address() {
        assert_eq!(listed(qemu_nic(false, [255, 255, 255, 0])), GLIBC_DOWN);
    }

    #[test]
    fn getifaddrs_is_glibcs_before_dhcp_answers() {
        let nic = Nic {
            ip: 0,
            mask: 0,
            gateway: 0,
            ..qemu_nic(true, [0; 4])
        };
        assert_eq!(listed(nic), GLIBC_NO_ADDRESS);
    }

    #[test]
    fn getifaddrs_is_glibcs_for_a_31_bit_mask() {
        assert_eq!(listed(qemu_nic(true, [255, 255, 255, 254])), GLIBC_SLASH_31);
    }

    /// No NIC found: `eth0` is still there, as `if_nameindex` says -- down,
    /// with a zero MAC, and no address.
    #[test]
    fn getifaddrs_without_a_nic_lists_eth0_down() {
        let lines = listed(Nic::NONE);
        let lo_link = GLIBC_UP.first().copied().unwrap_or("");
        let lo_address = GLIBC_NO_ADDRESS.get(2).copied().unwrap_or("");
        let eth0_link = concat!(
            "name=eth0 family=17 flags=0x1002 ",
            "addr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=00:00:00:00:00:00:00:00) ",
            "netmask=- ",
            "broadaddr=ll(proto=0,ifindex=2,hatype=1,pkttype=0,halen=6,addr=ff:ff:ff:ff:ff:ff:00:00) ",
            "data=stats"
        );
        assert_eq!(lines, [lo_link, eth0_link, lo_address]);
    }

    /// `eth0`'s link entry carries the kernel's counters for the NIC, each
    /// cut to its low 32 bits as Linux cuts them; the loopback's are zero.
    #[test]
    fn getifaddrs_link_entries_carry_the_counters() {
        // tx bytes, tx packets, tx errors, rx bytes, rx packets, rx drops
        let counters = [0x1_0000_0005, 7, 1, 900, 11, 2];
        set_test_nic(qemu_nic(true, [255, 255, 255, 0]), Some(counters));
        let mut list: *mut Ifaddrs = core::ptr::null_mut();
        assert_eq!(unsafe { getifaddrs(&mut list) }, 0);
        let lo = unsafe { &*list };
        assert!(!lo.ifa_next.is_null());
        let eth0 = unsafe { &*lo.ifa_next };
        let stats = |a: &Ifaddrs| unsafe { *a.ifa_data.cast::<RtnlLinkStats>() };
        assert_eq!(stats(lo), RtnlLinkStats::default());
        let want = RtnlLinkStats {
            rx_packets: 11,
            tx_packets: 7,
            rx_bytes: 900,
            tx_bytes: 5,
            tx_errors: 1,
            rx_dropped: 2,
            ..RtnlLinkStats::default()
        };
        assert_eq!(stats(eth0), want);
        unsafe { freeifaddrs(list) };
    }

    /// The two structures are the size, and `sockaddr_ll` the shape, glibc's
    /// headers give them (the oracle's `layout` line).
    #[test]
    fn sockaddr_ll_and_rtnl_link_stats_have_glibcs_layout() {
        assert_eq!(size_of::<SockaddrLl>(), 20);
        assert_eq!(core::mem::offset_of!(SockaddrLl, sll_protocol), 2);
        assert_eq!(core::mem::offset_of!(SockaddrLl, sll_ifindex), 4);
        assert_eq!(core::mem::offset_of!(SockaddrLl, sll_hatype), 8);
        assert_eq!(core::mem::offset_of!(SockaddrLl, sll_pkttype), 10);
        assert_eq!(core::mem::offset_of!(SockaddrLl, sll_halen), 11);
        assert_eq!(core::mem::offset_of!(SockaddrLl, sll_addr), 12);
        assert_eq!(size_of::<RtnlLinkStats>(), 96);
    }

    /// The links' names and indices are the ones `if_nametoindex` answers.
    #[test]
    fn getifaddrs_links_are_the_interfaces_if_nameindex_lists() {
        set_test_nic(Nic::NONE, None);
        let mut list: *mut Ifaddrs = core::ptr::null_mut();
        assert_eq!(unsafe { getifaddrs(&mut list) }, 0);
        let mut links: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut at = list;
        while !at.is_null() {
            let a = unsafe { &*at };
            if i32::from(unsafe { (*a.ifa_addr).sa_family }) == AF_PACKET {
                let ll = unsafe { &*a.ifa_addr.cast::<SockaddrLl>() };
                let index = u32::try_from(ll.sll_ifindex).unwrap_or(0);
                links.push((index, unsafe { c_str_to_slice(a.ifa_name) }.to_vec()));
            }
            at = a.ifa_next;
        }
        unsafe { freeifaddrs(list) };
        let want: Vec<(u32, Vec<u8>)> = INTERFACES.iter().map(|&(i, n)| (i, n.to_vec())).collect();
        assert_eq!(links, want);
    }

    /// A /31 or /32 reports the address itself, as glibc does where Linux
    /// sets no broadcast address; a wider subnet its all-ones address.
    #[test]
    fn broadcast_for_is_the_subnets_or_the_address_for_31_and_32() {
        let ip = u32::from_ne_bytes([10, 0, 2, 15]);
        let mask = |m: [u8; 4]| u32::from_ne_bytes(m);
        assert_eq!(
            broadcast_for(ip, mask([255, 255, 255, 0])),
            u32::from_ne_bytes([10, 0, 2, 255])
        );
        assert_eq!(
            broadcast_for(ip, mask([255, 255, 0, 0])),
            u32::from_ne_bytes([10, 0, 255, 255])
        );
        assert_eq!(broadcast_for(ip, mask([255, 255, 255, 254])), ip);
        assert_eq!(broadcast_for(ip, mask([255, 255, 255, 255])), ip);
        assert_eq!(broadcast_for(ip, 0), u32::MAX);
    }

    /// A down `eth0` keeps its address in the interface list -- as Linux
    /// lists it, and `AI_ADDRCONFIG` counts it -- but routes nothing.
    #[test]
    fn a_down_eth0_is_listed_but_routes_nothing() {
        let peer = u32::from_ne_bytes([10, 0, 2, 2]);
        let ip = u32::from_ne_bytes([10, 0, 2, 15]);
        let mask = u32::from_ne_bytes([255, 255, 255, 0]);
        let lo = (u32::to_be(INADDR_LOOPBACK), u32::to_be(0xFF00_0000));
        set_test_nic(qemu_nic(false, [255, 255, 255, 0]), None);
        let mut seen = Vec::new();
        for_each_ipv4_interface(|a, m| seen.push((a, m)));
        assert_eq!(seen, [lo, (ip, mask)]);
        assert_eq!(route_source(peer), None);
        set_test_nic(qemu_nic(true, [255, 255, 255, 0]), None);
        assert_eq!(route_source(peer), Some(ip));
    }

    /// Each call's list is its own: a second call does not rewrite the first,
    /// which it did while the list lived in static storage.
    #[test]
    fn getifaddrs_gives_each_caller_its_own_list() {
        let first = build_ifaddrs(qemu_nic(true, [255, 255, 255, 0]), RtnlLinkStats::default());
        let second = build_ifaddrs(Nic::NONE, RtnlLinkStats::default());
        assert!(!first.is_null() && !second.is_null());
        assert_ne!(first, second);
        let second_lines = oracle_lines(second);
        let first_lines = oracle_lines(first);
        assert_eq!(first_lines, GLIBC_UP);
        assert_eq!(second_lines.len(), 3);
    }

    #[test]
    fn getifaddrs_null_is_efault_and_freeifaddrs_null_is_nothing() {
        errno::set_errno(0);
        assert_eq!(unsafe { getifaddrs(core::ptr::null_mut()) }, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        unsafe { freeifaddrs(core::ptr::null_mut()) };
    }

    #[test]
    fn test_if_nameindex_size() {
        assert!(
            core::mem::size_of::<IfNameindex>() >= 8,
            "IfNameindex should be at least 8 bytes (u32 + pointer)"
        );
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

    // -----------------------------------------------------------------------
    // sockatmark
    // -----------------------------------------------------------------------

    #[test]
    fn test_sockatmark_negative_fd() {
        crate::errno::set_errno(0);
        let ret = sockatmark(-1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_sockatmark_valid_fd() {
        // On any valid fd, sockatmark returns 0 (no OOB support).
        let ret = sockatmark(0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_sockatmark_arbitrary_fd() {
        // Any non-negative fd is accepted.
        let ret = sockatmark(999);
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // socket — domain/type/protocol validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_socket_unsupported_domain_unix() {
        crate::errno::set_errno(0);
        let ret = socket(AF_UNIX, SOCK_STREAM, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAFNOSUPPORT);
    }

    #[test]
    fn test_socket_unsupported_domain_inet6() {
        crate::errno::set_errno(0);
        let ret = socket(AF_INET6, SOCK_STREAM, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAFNOSUPPORT);
    }

    #[test]
    fn test_socket_invalid_domain() {
        crate::errno::set_errno(0);
        let ret = socket(9999, SOCK_STREAM, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAFNOSUPPORT);
    }

    #[test]
    fn test_socket_stream_wrong_protocol() {
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_STREAM, IPPROTO_UDP);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EPROTONOSUPPORT);
    }

    #[test]
    fn test_socket_dgram_wrong_protocol() {
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_DGRAM, IPPROTO_TCP);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EPROTONOSUPPORT);
    }

    /// Phase 205: SOCK_RAW is now recognized — with CAP_NET_RAW held
    /// (default), it returns ENOSYS (not implemented) rather than
    /// EPROTONOSUPPORT (unknown type).
    #[test]
    fn test_socket_raw_with_cap_enosys() {
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_RAW, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // listen — validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_listen_invalid_fd() {
        crate::errno::set_errno(0);
        let ret = listen(-1, 128);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -----------------------------------------------------------------------
    // setsockopt — validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_setsockopt_invalid_fd() {
        crate::errno::set_errno(0);
        let val: i32 = 1;
        let ret = setsockopt(-1, SOL_SOCKET, SO_REUSEADDR, &raw const val as *const u8, 4);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -----------------------------------------------------------------------
    // Phase 101: accept4 flag-mask validation
    //
    // Linux semantics (net/socket.c::__sys_accept4_file):
    //   if (flags & ~(SOCK_CLOEXEC | SOCK_NONBLOCK)) return -EINVAL;
    // The check precedes any fd / addr / addrlen inspection.  Our
    // previous implementation just called accept() and then silently
    // ignored unknown bits, so e.g. accept4(-1, .., .., i32::MIN) would
    // return -1 with EBADF (from accept) instead of EINVAL (from the
    // flag mask).
    // -----------------------------------------------------------------------

    #[test]
    fn test_accept4_valid_mask_constants() {
        // Sanity: the two valid flags must be distinct, single-bit, and
        // non-zero — any aliasing would silently let a third bit slip
        // through the mask check.
        assert_ne!(SOCK_NONBLOCK, 0);
        assert_ne!(SOCK_CLOEXEC, 0);
        assert_ne!(SOCK_NONBLOCK, SOCK_CLOEXEC);
        assert_eq!(
            SOCK_NONBLOCK & (SOCK_NONBLOCK - 1),
            0,
            "SOCK_NONBLOCK must be a single bit, got {:#x}",
            SOCK_NONBLOCK
        );
        assert_eq!(
            SOCK_CLOEXEC & (SOCK_CLOEXEC - 1),
            0,
            "SOCK_CLOEXEC must be a single bit, got {:#x}",
            SOCK_CLOEXEC
        );
        assert_eq!(
            SOCK_NONBLOCK & SOCK_CLOEXEC,
            0,
            "SOCK_NONBLOCK and SOCK_CLOEXEC must not overlap"
        );
    }

    #[test]
    fn test_accept4_unknown_flag_einval() {
        // An arbitrary bit not in the valid mask must yield EINVAL,
        // BEFORE the fd is inspected.  We pass an obviously-bad fd
        // (-1) to ensure that if the mask check were missing, we'd
        // see EBADF instead.
        crate::errno::set_errno(0);
        let bad = 1 << 16; // not SOCK_NONBLOCK and not SOCK_CLOEXEC
        let ret = unsafe { accept4(-1, core::ptr::null_mut(), core::ptr::null_mut(), bad) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_accept4_high_bit_einval() {
        // i32::MIN sets the sign bit — definitely outside the mask.
        crate::errno::set_errno(0);
        let ret = unsafe { accept4(-1, core::ptr::null_mut(), core::ptr::null_mut(), i32::MIN) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_accept4_einval_wins_over_ebadf() {
        // Both garbage flags AND a bad fd would normally trigger
        // separate error paths.  Linux's order is: flags first, so
        // EINVAL wins over EBADF.  Regression guard: previously
        // accept() ran first and we'd see EBADF.
        crate::errno::set_errno(0);
        let ret = unsafe {
            accept4(
                -12345,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                1 << 18,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_accept4_o_append_rejected() {
        // O_APPEND is an open-mode bit, not a socket flag.  In our
        // numbering it's 0o2000 which differs from SOCK_NONBLOCK
        // (0o4000) and SOCK_CLOEXEC (0o2_000_000), so it must hit
        // the mask and EINVAL.
        crate::errno::set_errno(0);
        let ret = unsafe {
            accept4(
                -1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                crate::fcntl::O_APPEND,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_accept4_zero_flags_passes_mask() {
        // Zero flags is valid; the call should fall through to
        // accept() and fail there with EBADF (fd=-1), NOT with
        // EINVAL (which would mean the mask wrongly rejected zero).
        crate::errno::set_errno(0);
        let ret = unsafe { accept4(-1, core::ptr::null_mut(), core::ptr::null_mut(), 0) };
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EBADF,
            "zero flags must pass the mask; expected EBADF from accept"
        );
    }

    #[test]
    fn test_accept4_sock_nonblock_alone_passes_mask() {
        // SOCK_NONBLOCK alone is valid.  Should fall through and
        // fail in accept with EBADF, not in the mask with EINVAL.
        crate::errno::set_errno(0);
        let ret = unsafe {
            accept4(
                -1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                SOCK_NONBLOCK,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_accept4_sock_cloexec_alone_passes_mask() {
        // SOCK_CLOEXEC alone is valid.
        crate::errno::set_errno(0);
        let ret = unsafe {
            accept4(
                -1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                SOCK_CLOEXEC,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_accept4_both_valid_bits_pass_mask() {
        // SOCK_NONBLOCK | SOCK_CLOEXEC is the canonical valid combination.
        crate::errno::set_errno(0);
        let combined = SOCK_NONBLOCK | SOCK_CLOEXEC;
        let ret = unsafe { accept4(-1, core::ptr::null_mut(), core::ptr::null_mut(), combined) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_accept4_valid_plus_unknown_einval() {
        // Mixing a valid bit with an unknown bit must still EINVAL —
        // no partial acceptance.
        crate::errno::set_errno(0);
        // NOTE: SOCK_CLOEXEC is 0o2_000_000 == (1 << 19), so we must
        // pick a stray bit that is NOT 1<<11 (SOCK_NONBLOCK) and NOT
        // 1<<19 (SOCK_CLOEXEC).  1<<22 is safely outside both.
        let mixed = SOCK_CLOEXEC | (1 << 22);
        let ret = unsafe { accept4(-1, core::ptr::null_mut(), core::ptr::null_mut(), mixed) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_accept4_recovery_after_einval() {
        // A rejected call must not corrupt the syscall surface — a
        // subsequent valid-flags call still hits the regular EBADF.
        crate::errno::set_errno(0);
        let r1 = unsafe { accept4(-1, core::ptr::null_mut(), core::ptr::null_mut(), 1 << 17) };
        assert_eq!(r1, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::errno::set_errno(0);
        let r2 = unsafe { accept4(-1, core::ptr::null_mut(), core::ptr::null_mut(), 0) };
        assert_eq!(r2, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    // -----------------------------------------------------------------------
    // Phase 104: socket() type-flag mask validation
    //
    // Linux semantics (net/socket.c::__sys_socket):
    //   flags = type & ~SOCK_TYPE_MASK;
    //   if (flags & ~(SOCK_CLOEXEC | SOCK_NONBLOCK)) return -EINVAL;
    //   type &= SOCK_TYPE_MASK;
    //   ... then family/type/protocol checks ...
    // Previously the check was missing; stray flag bits silently
    // passed through and the type was masked via the SOCK_NONBLOCK |
    // SOCK_CLOEXEC carve-out only, so e.g. socket(AF_INET, SOCK_STREAM
    // | (1<<15), 0) would proceed with base_type = SOCK_STREAM|0x8000
    // and fail with EPROTONOSUPPORT instead of EINVAL.
    // -----------------------------------------------------------------------

    #[test]
    fn test_socket_type_mask_invariants() {
        // Sanity: SOCK_TYPE_MASK is the low 4 bits, SOCK_NONBLOCK and
        // SOCK_CLOEXEC live above it, and they don't overlap each
        // other or the type field.
        assert_eq!(SOCK_TYPE_MASK, 0xf);
        assert_eq!(SOCK_TYPE_MASK & SOCK_NONBLOCK, 0);
        assert_eq!(SOCK_TYPE_MASK & SOCK_CLOEXEC, 0);
        // The known type IDs all fit in the low nibble.
        assert!(SOCK_STREAM <= SOCK_TYPE_MASK);
        assert!(SOCK_DGRAM <= SOCK_TYPE_MASK);
        assert!(SOCK_RAW <= SOCK_TYPE_MASK);
        assert!(SOCK_SEQPACKET <= SOCK_TYPE_MASK);
    }

    #[test]
    fn test_socket_negative_type_einval() {
        // Negative sock_type can't be a valid flag combo (the sign bit
        // is far above SOCK_CLOEXEC).  Must EINVAL.
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_socket_high_bit_in_type_einval() {
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, i32::MIN | SOCK_STREAM, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_socket_unknown_flag_bit_in_type_einval() {
        // (1 << 15) is above SOCK_TYPE_MASK (0xf) and is neither
        // SOCK_NONBLOCK (1<<11) nor SOCK_CLOEXEC (1<<19).  Must EINVAL.
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_STREAM | (1 << 15), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_socket_einval_wins_over_eafnosupport() {
        // Both stray flag bit AND bad family: Linux validates flags
        // first.  Regression guard: previously we'd hit
        // EAFNOSUPPORT first because the domain check ran first.
        crate::errno::set_errno(0);
        let ret = socket(99_999, SOCK_STREAM | (1 << 16), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_socket_einval_wins_over_eprotonosupport() {
        // Even with a bad type ID, a stray flag bit must yield EINVAL
        // first.  (The bad type would otherwise produce
        // EPROTONOSUPPORT through the match-default branch.)
        crate::errno::set_errno(0);
        // base_type=7 is reserved/unimplemented; combined with bit 14.
        let ret = socket(AF_INET, 7 | (1 << 14), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_socket_zero_flags_does_not_einval() {
        // Plain SOCK_STREAM (no flags) must not be rejected by the
        // mask path.  It should reach the domain/type check and
        // succeed (or fail with a NON-EINVAL error if fd table is
        // full / kernel rejects).
        crate::errno::set_errno(0);
        let fd = socket(AF_INET, SOCK_STREAM, 0);
        if fd < 0 {
            assert_ne!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "valid SOCK_STREAM must not be rejected by the mask"
            );
        } else {
            // Clean up to avoid leaking fds across tests.
            let _ = fdtable::close_fd(fd);
        }
    }

    #[test]
    fn test_socket_nonblock_alone_passes_mask() {
        crate::errno::set_errno(0);
        let fd = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
        if fd < 0 {
            assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
        } else {
            let _ = fdtable::close_fd(fd);
        }
    }

    #[test]
    fn test_socket_cloexec_alone_passes_mask() {
        crate::errno::set_errno(0);
        let fd = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
        if fd < 0 {
            assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
        } else {
            let _ = fdtable::close_fd(fd);
        }
    }

    #[test]
    fn test_socket_both_type_flags_pass_mask() {
        crate::errno::set_errno(0);
        let fd = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
        if fd < 0 {
            assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
        } else {
            let _ = fdtable::close_fd(fd);
        }
    }

    #[test]
    fn test_socket_valid_flag_plus_unknown_rejected() {
        // Mixing a valid type-flag with an unknown bit must still
        // EINVAL — no partial acceptance.
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK | (1 << 17), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_socket_recovery_after_einval() {
        // A rejected call must not corrupt the syscall surface — a
        // subsequent valid call still works.
        crate::errno::set_errno(0);
        let r1 = socket(AF_INET, SOCK_STREAM | (1 << 13), 0);
        assert_eq!(r1, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::errno::set_errno(0);
        let r2 = socket(AF_INET, SOCK_STREAM, 0);
        if r2 < 0 {
            assert_ne!(crate::errno::get_errno(), crate::errno::EINVAL);
        } else {
            let _ = fdtable::close_fd(r2);
        }
    }

    #[test]
    fn test_socket_single_high_bits_outside_mask_all_rejected() {
        // Defensive: every single bit above SOCK_TYPE_MASK (i.e.
        // bits 4..30) that is NEITHER SOCK_NONBLOCK (bit 11) NOR
        // SOCK_CLOEXEC (bit 19) must be rejected with EINVAL when
        // OR'd with SOCK_STREAM.  Skip bit 31 — that's the sign bit
        // and is covered by test_socket_high_bit_in_type_einval.
        for shift in 4..31 {
            let bit = 1i32 << shift;
            if bit == SOCK_NONBLOCK || bit == SOCK_CLOEXEC {
                continue;
            }
            crate::errno::set_errno(0);
            let ret = socket(AF_INET, SOCK_STREAM | bit, 0);
            assert_eq!(
                ret, -1,
                "bit {:#x} should be rejected by socket type mask",
                bit
            );
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "bit {:#x} should set EINVAL",
                bit
            );
        }
    }

    #[test]
    fn test_accept4_single_bits_outside_mask_all_rejected() {
        // Defensive: every single-bit value in [0, 31) that is NOT
        // SOCK_NONBLOCK or SOCK_CLOEXEC must be rejected.  Guards
        // against a future constant change silently widening the
        // accepted mask.
        for shift in 0..31 {
            let bit = 1i32 << shift;
            if bit == SOCK_NONBLOCK || bit == SOCK_CLOEXEC {
                continue;
            }
            crate::errno::set_errno(0);
            let ret = unsafe { accept4(-1, core::ptr::null_mut(), core::ptr::null_mut(), bit) };
            assert_eq!(ret, -1, "bit {:#x} should be rejected by accept4 mask", bit);
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "bit {:#x} should set EINVAL",
                bit
            );
        }
    }

    // ===================================================================
    // Phase 201 — CAP_NET_BIND_SERVICE gate on bind() for privileged ports
    //
    // Linux requires CAP_NET_BIND_SERVICE to bind to ports 1..1023.
    // The gate runs after all argument validation (EFAULT, EBADF,
    // ENOTSOCK, EINVAL, EAFNOSUPPORT), so bad-argument callers see
    // their original errors regardless of cap state.
    // ===================================================================

    mod phase201_cap {
        pub(super) struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            pub(super) fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        pub(super) fn drop_cap_net_bind_service() {
            let cap = crate::sys_capability::CAP_NET_BIND_SERVICE;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << cap);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed dropping cap");
            assert!(!crate::sys_capability::has_capability(cap));
        }
    }

    /// Helper: create a TCP socket fd for bind tests.
    fn make_tcp_socket() -> i32 {
        let fd = socket(AF_INET, SOCK_STREAM, 0);
        assert!(fd >= 0, "socket() must succeed");
        fd
    }

    /// Helper: build a SockaddrIn for a given port (host byte order).
    fn make_sockaddr_in(port: u16) -> SockaddrIn {
        SockaddrIn {
            sin_family: AF_INET as u16,
            sin_port: port.to_be(), // network byte order
            sin_addr: InAddr {
                s_addr: htonl(0x7F000001),
            }, // 127.0.0.1
            sin_zero: [0; 8],
        }
    }

    // -- cap held: privileged bind succeeds --------------------------------

    /// With CAP_NET_BIND_SERVICE (default), binding port 80 succeeds
    /// (TCP defers to listen, just stores metadata).
    #[test]
    fn test_phase201_bind_privileged_port_with_cap_ok() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_NET_BIND_SERVICE,
        ));
        let fd = make_tcp_socket();
        let addr = make_sockaddr_in(80);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, 0, "privileged bind with cap should succeed");
        crate::file::close(fd);
    }

    // -- cap dropped: privileged bind → EACCES ----------------------------

    /// Without CAP_NET_BIND_SERVICE, port 80 → EACCES.
    #[test]
    fn test_phase201_bind_port80_no_cap_eacces() {
        let _g = phase201_cap::CapGuard::snapshot();
        phase201_cap::drop_cap_net_bind_service();
        let fd = make_tcp_socket();
        let addr = make_sockaddr_in(80);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EACCES);
        crate::file::close(fd);
    }

    /// Without cap, port 443 → EACCES.
    #[test]
    fn test_phase201_bind_port443_no_cap_eacces() {
        let _g = phase201_cap::CapGuard::snapshot();
        phase201_cap::drop_cap_net_bind_service();
        let fd = make_tcp_socket();
        let addr = make_sockaddr_in(443);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EACCES);
        crate::file::close(fd);
    }

    /// Without cap, port 1 (lowest valid privileged port) → EACCES.
    #[test]
    fn test_phase201_bind_port1_no_cap_eacces() {
        let _g = phase201_cap::CapGuard::snapshot();
        phase201_cap::drop_cap_net_bind_service();
        let fd = make_tcp_socket();
        let addr = make_sockaddr_in(1);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EACCES);
        crate::file::close(fd);
    }

    /// Without cap, port 1023 (highest privileged port) → EACCES.
    #[test]
    fn test_phase201_bind_port1023_no_cap_eacces() {
        let _g = phase201_cap::CapGuard::snapshot();
        phase201_cap::drop_cap_net_bind_service();
        let fd = make_tcp_socket();
        let addr = make_sockaddr_in(1023);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EACCES);
        crate::file::close(fd);
    }

    // -- cap dropped: non-privileged ports bypass the gate ----------------

    /// Bind `port` with `CAP_NET_BIND_SERVICE` dropped and assert the
    /// privileged-port gate did *not* reject it.
    ///
    /// The assertion is deliberately "not `EACCES`" rather than "returned 0".
    /// These tests bind a *fixed real host port* on 127.0.0.1, so a bind can
    /// legitimately fail with `EADDRINUSE` — either because some unrelated
    /// program on the dev machine holds the port, or because a socket from an
    /// earlier run of this very suite is still in `TIME_WAIT`.  Asserting
    /// success therefore made these tests fail intermittently for a reason
    /// that has nothing to do with the code under test.  `EACCES` is the only
    /// error the gate itself can produce, so excluding it tests exactly the
    /// property these cases exist to pin down and nothing else.
    fn assert_bind_not_gated(port: u16) {
        let _g = phase201_cap::CapGuard::snapshot();
        phase201_cap::drop_cap_net_bind_service();
        let fd = make_tcp_socket();
        let addr = make_sockaddr_in(port);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        if ret != 0 {
            assert_ne!(
                crate::errno::get_errno(),
                crate::errno::EACCES,
                "port {port} is unprivileged and must bypass the privileged-port gate",
            );
        }
        crate::file::close(fd);
    }

    /// Without cap, port 1024 (first non-privileged) is not gated.
    #[test]
    fn test_phase201_bind_port1024_no_cap_ok() {
        assert_bind_not_gated(1024);
    }

    /// Without cap, port 8080 (common unprivileged) is not gated.
    #[test]
    fn test_phase201_bind_port8080_no_cap_ok() {
        assert_bind_not_gated(8080);
    }

    /// Without cap, port 0 (ephemeral) succeeds.
    #[test]
    fn test_phase201_bind_port0_no_cap_ok() {
        let _g = phase201_cap::CapGuard::snapshot();
        phase201_cap::drop_cap_net_bind_service();
        let fd = make_tcp_socket();
        let addr = make_sockaddr_in(0);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, 0, "port 0 must bypass privileged port gate");
        crate::file::close(fd);
    }

    // -- ordering: argument errors beat EACCES ----------------------------

    /// NULL addr + no cap → EFAULT (argument check before cap).
    ///
    /// The descriptor must be a real socket: `__sys_bind` (net/socket.c:1835)
    /// resolves it via `sockfd_lookup_light` *before* `move_addr_to_kernel`
    /// looks at the address, so a bad fd would mask the EFAULT with EBADF.
    #[test]
    fn test_phase201_bind_null_addr_efault_before_eacces() {
        let _g = phase201_cap::CapGuard::snapshot();
        phase201_cap::drop_cap_net_bind_service();
        let fd = make_tcp_socket();
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                core::ptr::null(),
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EFAULT,
            "EFAULT for NULL addr must precede EACCES"
        );
        crate::file::close(fd);
    }

    /// Bad fd + no cap → EBADF (fd check before cap).
    #[test]
    fn test_phase201_bind_bad_fd_ebadf_before_eacces() {
        let _g = phase201_cap::CapGuard::snapshot();
        phase201_cap::drop_cap_net_bind_service();
        let addr = make_sockaddr_in(80);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                -1,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EBADF,
            "EBADF for bad fd must precede EACCES"
        );
    }

    // -- restoration: CapGuard drop re-enables privileged bind ------------

    /// After restoring CAP_NET_BIND_SERVICE, privileged bind works again.
    #[test]
    fn test_phase201_bind_cap_restore_re_enables() {
        {
            let _g = phase201_cap::CapGuard::snapshot();
            phase201_cap::drop_cap_net_bind_service();
            let fd = make_tcp_socket();
            let addr = make_sockaddr_in(80);
            crate::errno::set_errno(0);
            let ret = unsafe {
                bind(
                    fd,
                    &raw const addr as *const Sockaddr,
                    core::mem::size_of::<SockaddrIn>() as SocklenT,
                )
            };
            assert_eq!(ret, -1, "must fail without cap");
            crate::file::close(fd);
        }
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_NET_BIND_SERVICE,
        ));
        let fd = make_tcp_socket();
        let addr = make_sockaddr_in(80);
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                fd,
                &raw const addr as *const Sockaddr,
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, 0, "must succeed after cap restored");
        crate::file::close(fd);
    }

    // ===================================================================
    // Phase 205 — CAP_NET_RAW gate on socket(SOCK_RAW)
    //
    // Linux gates raw socket creation on CAP_NET_RAW inside inet_create().
    // SOCK_RAW is now recognized as a valid type: with cap → ENOSYS
    // (not implemented), without cap → EACCES.
    // ===================================================================

    mod phase205_cap {
        pub(super) struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            pub(super) fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        pub(super) fn drop_cap_net_raw() {
            let cap = crate::sys_capability::CAP_NET_RAW;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let new_lo = lo & !(1u32 << cap);
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed dropping cap");
            assert!(!crate::sys_capability::has_capability(cap));
        }
    }

    // -- cap held: SOCK_RAW → ENOSYS (not implemented) --------------------

    /// With CAP_NET_RAW (default), socket(SOCK_RAW) → ENOSYS.
    #[test]
    fn test_phase205_socket_raw_with_cap_enosys() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_NET_RAW,
        ));
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_RAW, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- cap dropped: SOCK_RAW → EACCES -----------------------------------

    /// Without CAP_NET_RAW, socket(SOCK_RAW) → EACCES.
    #[test]
    fn test_phase205_socket_raw_no_cap_eacces() {
        let _g = phase205_cap::CapGuard::snapshot();
        phase205_cap::drop_cap_net_raw();
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_RAW, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EACCES);
    }

    /// Without CAP_NET_RAW, SOCK_RAW with IPPROTO_ICMP → EACCES.
    #[test]
    fn test_phase205_socket_raw_icmp_no_cap_eacces() {
        let _g = phase205_cap::CapGuard::snapshot();
        phase205_cap::drop_cap_net_raw();
        crate::errno::set_errno(0);
        // IPPROTO_ICMP = 1
        let ret = socket(AF_INET, SOCK_RAW, 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EACCES);
    }

    // -- other socket types unaffected by cap drop ------------------------

    /// Without CAP_NET_RAW, SOCK_STREAM still works.
    #[test]
    fn test_phase205_socket_stream_no_cap_ok() {
        let _g = phase205_cap::CapGuard::snapshot();
        phase205_cap::drop_cap_net_raw();
        let fd = socket(AF_INET, SOCK_STREAM, 0);
        assert!(
            fd >= 0,
            "SOCK_STREAM must not be affected by CAP_NET_RAW drop"
        );
        crate::file::close(fd);
    }

    /// Without CAP_NET_RAW, SOCK_DGRAM still works.
    #[test]
    fn test_phase205_socket_dgram_no_cap_ok() {
        let _g = phase205_cap::CapGuard::snapshot();
        phase205_cap::drop_cap_net_raw();
        let fd = socket(AF_INET, SOCK_DGRAM, 0);
        assert!(
            fd >= 0,
            "SOCK_DGRAM must not be affected by CAP_NET_RAW drop"
        );
        crate::file::close(fd);
    }

    // -- ordering: domain check beats cap check ---------------------------

    /// Bad domain + SOCK_RAW + no cap → EAFNOSUPPORT (domain check first).
    #[test]
    fn test_phase205_socket_raw_bad_domain_eafnosupport() {
        let _g = phase205_cap::CapGuard::snapshot();
        phase205_cap::drop_cap_net_raw();
        crate::errno::set_errno(0);
        let ret = socket(AF_UNIX, SOCK_RAW, 0);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EAFNOSUPPORT,
            "domain check must precede cap check"
        );
    }

    /// Bad flag bits + SOCK_RAW + no cap → EINVAL (flag check first).
    #[test]
    fn test_phase205_socket_raw_bad_flags_einval() {
        let _g = phase205_cap::CapGuard::snapshot();
        phase205_cap::drop_cap_net_raw();
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_RAW | (1 << 15), 0);
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "flag check must precede cap check"
        );
    }

    // -- restoration: cap restore re-enables ENOSYS path ------------------

    /// After restoring CAP_NET_RAW, SOCK_RAW returns ENOSYS again.
    #[test]
    fn test_phase205_socket_raw_cap_restore() {
        {
            let _g = phase205_cap::CapGuard::snapshot();
            phase205_cap::drop_cap_net_raw();
            crate::errno::set_errno(0);
            let ret = socket(AF_INET, SOCK_RAW, 0);
            assert_eq!(ret, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EACCES);
        }
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_NET_RAW,
        ));
        crate::errno::set_errno(0);
        let ret = socket(AF_INET, SOCK_RAW, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // Argument-validation *order*
    //
    // Every assertion below names the upstream function and the line whose
    // position in the source it encodes.  Ordering is part of the ABI: a
    // caller that switches on errno to decide what to retry gets a different
    // answer when two checks trade places, so a test that only pins the
    // constant is not enough.
    // -----------------------------------------------------------------

    /// `__sys_bind` (net/socket.c:1835) resolves the descriptor via
    /// `sockfd_lookup_light` before `move_addr_to_kernel` (:247) reads the
    /// address, so EBADF outranks EFAULT.
    #[test]
    fn bind_bad_fd_outranks_a_null_address() {
        crate::errno::set_errno(0);
        let ret = unsafe {
            bind(
                -1,
                core::ptr::null(),
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// Same for `__sys_connect` (net/socket.c:2056), whose `fdget` is also
    /// ahead of `move_addr_to_kernel`.
    #[test]
    fn connect_bad_fd_outranks_a_null_address() {
        crate::errno::set_errno(0);
        let ret = unsafe {
            connect(
                -1,
                core::ptr::null(),
                core::mem::size_of::<SockaddrIn>() as SocklenT,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// `move_addr_to_kernel` returns 0 at `ulen == 0` (net/socket.c:251)
    /// *before* the `copy_from_user` at :253, so a zero addrlen never touches
    /// the pointer and the verdict comes from the protocol's own length test:
    /// EINVAL, not EFAULT.
    #[test]
    fn bind_zero_addrlen_does_not_fault_on_a_null_address() {
        let fd = make_tcp_socket();
        crate::errno::set_errno(0);
        let ret = unsafe { bind(fd, core::ptr::null(), 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        crate::file::close(fd);
    }

    /// And a short-but-nonzero addrlen *does* reach the copy, so there EFAULT
    /// wins over the protocol's EINVAL.
    #[test]
    fn bind_short_nonzero_addrlen_still_faults_on_a_null_address() {
        let fd = make_tcp_socket();
        crate::errno::set_errno(0);
        let ret = unsafe { bind(fd, core::ptr::null(), 4) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        crate::file::close(fd);
    }

    /// `__sys_socketpair` (net/socket.c:1729) tests the type's flag bits at
    /// :1737, before it has reserved a descriptor — so a bad flag outranks the
    /// `put_user(fd1, &usockvec[0])` fault at :1758.
    #[test]
    fn socketpair_bad_type_flags_outrank_a_null_vector() {
        crate::errno::set_errno(0);
        let ret = socketpair(AF_UNIX, SOCK_STREAM | 0x0100_0000, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// But `put_user` at :1758 comes *before* `sock_create` at :1771, so the
    /// fault outranks EAFNOSUPPORT — the family is never examined.
    #[test]
    fn socketpair_null_vector_outranks_a_bad_family() {
        crate::errno::set_errno(0);
        let ret = socketpair(0xbeef, SOCK_STREAM, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// `do_ip_setsockopt` (net/ipv4/ip_sockglue.c:1222) rejects a short optlen
    /// before `copy_from_sockptr` at :1226, and the two verdicts are distinct:
    /// short → EINVAL, NULL at an adequate length → EFAULT.
    #[test]
    fn setsockopt_mreq_short_optlen_and_null_optval_are_different_errors() {
        let fd = socket(AF_INET, SOCK_DGRAM, 0);
        assert!(fd >= 0, "socket() must succeed");

        crate::errno::set_errno(0);
        let short = setsockopt(fd, IPPROTO_IP, IP_ADD_MEMBERSHIP, core::ptr::null(), 4);
        assert_eq!(short, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let faulted = setsockopt(
            fd,
            IPPROTO_IP,
            IP_ADD_MEMBERSHIP,
            core::ptr::null(),
            core::mem::size_of::<IpMreq>() as SocklenT,
        );
        assert_eq!(faulted, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        crate::file::close(fd);
    }

    /// `__sys_sendmsg` (net/socket.c:2634) runs `sockfd_lookup_light` before
    /// `___sys_sendmsg` copies the header, so EBADF outranks EFAULT.
    #[test]
    fn sendmsg_bad_fd_outranks_a_null_header() {
        crate::errno::set_errno(0);
        let ret = unsafe { sendmsg(-1, core::ptr::null(), 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// Same for `__sys_recvmsg`.
    #[test]
    fn recvmsg_bad_fd_outranks_a_null_header() {
        crate::errno::set_errno(0);
        let ret = unsafe { recvmsg(-1, core::ptr::null_mut(), 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// `__sys_sendto` (net/socket.c:2161) looks the descriptor up whatever the
    /// length is, so a zero-length send on a closed fd is EBADF — not the
    /// zero-byte success POSIX grants a *valid* one.
    #[test]
    fn zero_length_send_on_a_bad_fd_is_still_ebadf() {
        crate::errno::set_errno(0);
        let ret = unsafe { send(-1, core::ptr::null(), 0, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// The same for the receive side (`__sys_recvfrom`, net/socket.c:2224).
    #[test]
    fn zero_length_recv_on_a_bad_fd_is_still_ebadf() {
        crate::errno::set_errno(0);
        let ret = unsafe { recv(-1, core::ptr::null_mut(), 0, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);

        crate::errno::set_errno(0);
        let ret = unsafe {
            recvfrom(
                -1,
                core::ptr::null_mut(),
                0,
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    /// A zero-length send on a *valid* socket still succeeds with 0 — the
    /// reordering above must not have turned the POSIX no-op into an error.
    #[test]
    fn zero_length_send_on_a_valid_socket_is_still_a_no_op() {
        let fd = make_tcp_socket();
        crate::errno::set_errno(0);
        assert_eq!(unsafe { send(fd, core::ptr::null(), 0, 0) }, 0);
        assert_eq!(unsafe { recv(fd, core::ptr::null_mut(), 0, 0) }, 0);
        crate::file::close(fd);
    }
}
