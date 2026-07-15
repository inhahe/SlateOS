//! `udpget` — a minimal **Linux-ABI** ring-3 UDP/DNS client.
//!
//! This is the datagram sibling of the `httpget` "ring-3 socket-syscall
//! capstone" (netstack Phase 5, UDP `SOCK_DGRAM` cutover). Its whole purpose
//! is to exercise the **daemon-backed datagram socket-fd path from an actual
//! ring-3 process**, end to end, using the raw Linux x86_64 syscall ABI:
//!
//! ```text
//!   socket(AF_INET, SOCK_DGRAM, 0)
//!   bind(fd, sockaddr_in{0.0.0.0, 0}, 16)          // ephemeral auto-port
//!   sendto(fd, dns_query, 29, 0, sockaddr_in{dns,53}, 16)
//!   recvfrom(fd, buf, .., 0, &src, &srclen)        // expect DNS reply
//!   close(fd)
//! ```
//!
//! When the kernel is booted with `net.userspace` on, those syscalls are
//! routed by `dispatch_linux` → `net::socket::{create_dgram,dgram_bind,
//! dgram_send_to,dgram_recv_from}` → the persistent `net.stack` daemon over the
//! SHM ring. Prior to this program that dispatch wiring (user-pointer copies,
//! sockaddr parse, fd install, errno mapping) was only exercised by the
//! kernel-context `NetstackConn::self_test_udp_dns`, never by a live ring-3
//! call — this closes that gap for the datagram path.
//!
//! The program is spawned by `proc::spawn::run_persistent_netstack` with an
//! explicit `AbiMode::Linux` override. The **IPv4** arm takes `argv = ["udpget",
//! "<a.b.c.d>", "<port>"]` (the kernel passes the interface's resolver IP and
//! port 53) and validates a real DNS reply. The **IPv6** arm takes a 4th argv
//! `"6"` — `argv = ["udpget", "<32-hex me.ip6>", "<port>", "6"]` — and, since
//! slirp has no IPv6 upstream, does a self-loopback: it sends a marker payload to
//! the daemon's own link-local `me.ip6` (which the daemon diverts back into its
//! RX FIFO, bypassing NDP) and asserts the exact bytes echo back with an
//! `AF_INET6` source. Both arms report their result purely through the **exit
//! code** so the kernel self-test can decode it deterministically without parsing
//! console output:
//!
//! | code | meaning                                                     |
//! |------|-------------------------------------------------------------|
//! | 0    | success — v4: DNS reply (TXID + QR); v6: loopback echo       |
//! | 10   | `socket()` failed                                           |
//! | 11   | `bind()` failed                                             |
//! | 12   | `sendto()` failed                                           |
//! | 13   | `recvfrom()` returned <= 0 (no reply)                       |
//! | 14   | v4: bad DNS reply; v6: payload/source mismatch              |
//! | 20   | wrong argc                                                  |
//! | 21   | could not parse the IPv4 argument                          |
//! | 22   | could not parse the port argument                          |
//! | 23   | could not parse the IPv6 (32-hex) argument                 |

#![no_std]
#![no_main]

// ---- Linux x86_64 syscall numbers ----
const SYS_WRITE: usize = 1;
const SYS_CLOSE: usize = 3;
const SYS_SOCKET: usize = 41;
const SYS_BIND: usize = 49;
const SYS_SENDTO: usize = 44;
const SYS_RECVFROM: usize = 45;
const SYS_EXIT: usize = 60;

const AF_INET: usize = 2;
const AF_INET6: usize = 10;
const SOCK_DGRAM: usize = 2;

// Distinct marker payload for the IPv6 loopback mode. Under QEMU/slirp there is
// no IPv6 DNS upstream, so the v6 arm is a self-loopback: we send this to the
// daemon's own EUI-64 link-local (`me.ip6`), which the daemon diverts back into
// its RX FIFO, and assert the exact bytes come back. (The v4 arm, by contrast,
// hits a real DNS resolver and validates a DNS *reply*.)
const V6_PAYLOAD: &[u8] = b"slate-udpget6:ring3-loopback!";

// DNS `A?` query for example.com — TXID 0x1234, RD set. 12-byte header +
// 13-byte QNAME (7"example"3"com"0) + QTYPE(A=1) + QCLASS(IN=1) = 29 bytes.
const QUERY: [u8; 29] = [
    0x12, 0x34, // ID
    0x01, 0x00, // flags: RD
    0x00, 0x01, // QDCOUNT = 1
    0x00, 0x00, // ANCOUNT
    0x00, 0x00, // NSCOUNT
    0x00, 0x00, // ARCOUNT
    7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', //
    3, b'c', b'o', b'm', //
    0, // root label
    0x00, 0x01, // QTYPE = A
    0x00, 0x01, // QCLASS = IN
];

// ---- raw syscall wrappers (Linux ABI: nr=rax, args=rdi,rsi,rdx,r10,r8,r9) ----

#[inline(always)]
unsafe fn sys1(nr: usize, a: usize) -> isize {
    let r: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr => r,
            in("rdi") a,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    r
}

#[inline(always)]
unsafe fn sys3(nr: usize, a: usize, b: usize, c: usize) -> isize {
    let r: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr => r,
            in("rdi") a,
            in("rsi") b,
            in("rdx") c,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    r
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn sys6(
    nr: usize,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    e: usize,
    f: usize,
) -> isize {
    let r: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr => r,
            in("rdi") a,
            in("rsi") b,
            in("rdx") c,
            in("r10") d,
            in("r8") e,
            in("r9") f,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    r
}

fn print(s: &[u8]) {
    // Best-effort progress to stdout; the exit code is the authoritative result.
    unsafe {
        let _ = sys3(SYS_WRITE, 1, s.as_ptr() as usize, s.len());
    }
}

fn exit(code: usize) -> ! {
    unsafe {
        sys1(SYS_EXIT, code);
    }
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) }
    }
}

/// Length of a NUL-terminated C string.
///
/// # Safety
/// `p` must point at a NUL-terminated byte sequence readable up to and
/// including the terminator.
unsafe fn strlen(p: *const u8) -> usize {
    let mut n = 0usize;
    while n < 4096 {
        if unsafe { *p.add(n) } == 0 {
            break;
        }
        n += 1;
    }
    n
}

/// Parse a dotted-decimal IPv4 into 4 network-order octets.
fn parse_ipv4(s: &[u8]) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    let mut have_digit = false;
    let mut cur: u32 = 0;
    for &b in s {
        match b {
            b'0'..=b'9' => {
                cur = cur.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
                if cur > 255 {
                    return None;
                }
                have_digit = true;
            }
            b'.' => {
                if !have_digit || idx >= 3 {
                    return None;
                }
                octets[idx] = cur as u8;
                idx += 1;
                cur = 0;
                have_digit = false;
            }
            _ => return None,
        }
    }
    if !have_digit || idx != 3 {
        return None;
    }
    octets[3] = cur as u8;
    Some(octets)
}

/// Parse a decimal port into a `u16`.
fn parse_port(s: &[u8]) -> Option<u16> {
    let mut v: u32 = 0;
    let mut any = false;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
        if v > 65535 {
            return None;
        }
        any = true;
    }
    if !any {
        return None;
    }
    Some(v as u16)
}

/// Build a `struct sockaddr_in` — family(2 host-endian), port(BE), addr(4), pad(8).
fn sockaddr_in(ip: &[u8; 4], port: u16) -> [u8; 16] {
    let mut sa = [0u8; 16];
    sa[0] = (AF_INET & 0xff) as u8;
    sa[1] = ((AF_INET >> 8) & 0xff) as u8;
    sa[2] = (port >> 8) as u8;
    sa[3] = (port & 0xff) as u8;
    sa[4] = ip[0];
    sa[5] = ip[1];
    sa[6] = ip[2];
    sa[7] = ip[3];
    sa
}

/// Decode one ASCII hex nibble.
fn hexval(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse a 32-hex-char string into 16 network-order bytes (an IPv6 address). The
/// kernel passes `me.ip6` this way — plain hex is trivial to parse without a full
/// RFC 4291 `::`-compression parser, and it round-trips exactly.
fn parse_hex16(s: &[u8]) -> Option<[u8; 16]> {
    if s.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    let mut i = 0usize;
    while i < 16 {
        let hi = hexval(s[i * 2])?;
        let lo = hexval(s[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
        i += 1;
    }
    Some(out)
}

/// Build a `struct sockaddr_in6` (28 bytes): sin6_family(2 host-endian),
/// sin6_port(2 BE), sin6_flowinfo(4)=0, sin6_addr(16), sin6_scope_id(4)=0.
fn sockaddr_in6(ip: &[u8; 16], port: u16) -> [u8; 28] {
    let mut sa = [0u8; 28];
    sa[0] = (AF_INET6 & 0xff) as u8;
    sa[1] = ((AF_INET6 >> 8) & 0xff) as u8;
    sa[2] = (port >> 8) as u8;
    sa[3] = (port & 0xff) as u8;
    // flowinfo [4..8] stays zero.
    let mut i = 0usize;
    while i < 16 {
        sa[8 + i] = ip[i];
        i += 1;
    }
    // scope_id [24..28] stays zero.
    sa
}

/// AF_INET6 loopback arm: `socket(AF_INET6, SOCK_DGRAM)` → `bind([::]:port)` →
/// `sendto([me.ip6]:port, V6_PAYLOAD)` → `recvfrom` and assert the datagram
/// echoes back from `me.ip6`.
///
/// `ip_hex` is the 32-hex-char encoding of the daemon's link-local `me.ip6`
/// (passed in argv by the kernel). Because the socket is bound to `port` and the
/// datagram is addressed to `[me.ip6]:port`, the daemon's in-process IPv6
/// loopback divert (a frame to its own link-local, bypassing NDP) delivers the
/// datagram back to this very socket — proving the ring-3 `sockaddr_in6`
/// send/recv dispatch path end to end without needing a real IPv6 upstream.
///
/// # Safety
/// Uses raw Linux syscalls; the sockaddr/buffer pointers are to local stack
/// arrays valid for the call duration.
unsafe fn run_v6(ip_hex: &[u8], port: u16) -> ! {
    let dst = match parse_hex16(ip_hex) {
        Some(v) => v,
        None => {
            print(b"[udpget] FAIL: v6 addr parse\n");
            exit(23);
        }
    };

    // socket(AF_INET6, SOCK_DGRAM, 0)
    let fd = unsafe { sys3(SYS_SOCKET, AF_INET6, SOCK_DGRAM, 0) };
    if fd < 0 {
        print(b"[udpget] FAIL: socket6\n");
        exit(10);
    }
    let fd = fd as usize;

    // bind(fd, sockaddr_in6{[::], port}, 28) — a fixed port so the loopback
    // datagram (dst_port == port) routes back to this same socket.
    let local = sockaddr_in6(&[0u8; 16], port);
    let rc = unsafe { sys3(SYS_BIND, fd, local.as_ptr() as usize, local.len()) };
    if rc < 0 {
        print(b"[udpget] FAIL: bind6\n");
        let _ = unsafe { sys1(SYS_CLOSE, fd) };
        exit(11);
    }

    // sendto(fd, V6_PAYLOAD, .., 0, sockaddr_in6{me.ip6, port}, 28)
    let dst_sa = sockaddr_in6(&dst, port);
    let sent = unsafe {
        sys6(
            SYS_SENDTO,
            fd,
            V6_PAYLOAD.as_ptr() as usize,
            V6_PAYLOAD.len(),
            0,
            dst_sa.as_ptr() as usize,
            dst_sa.len(),
        )
    };
    if sent < 0 {
        print(b"[udpget] FAIL: sendto6\n");
        let _ = unsafe { sys1(SYS_CLOSE, fd) };
        exit(12);
    }
    print(b"[udpget] v6 query sent\n");

    // recvfrom(fd, buf, .., 0, &src, &srclen) — one blocking datagram receive.
    let mut buf = [0u8; 512];
    let mut src = [0u8; 28];
    let mut srclen: u32 = src.len() as u32;
    let n = unsafe {
        sys6(
            SYS_RECVFROM,
            fd,
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
            src.as_mut_ptr() as usize,
            (&mut srclen as *mut u32) as usize,
        )
    };
    let _ = unsafe { sys1(SYS_CLOSE, fd) };
    if n <= 0 {
        print(b"[udpget] FAIL: recvfrom6\n");
        exit(13);
    }
    let got = &buf[..n as usize];
    // The loopback echoes our exact payload back; the source should be a
    // sockaddr_in6 whose address (offset 8..24) is me.ip6.
    let src_ok = src[0] == (AF_INET6 & 0xff) as u8 && src[8..24] == dst;
    if got == V6_PAYLOAD && src_ok {
        print(b"[udpget] OK: v6 loopback echo\n");
        exit(0);
    }
    print(b"[udpget] FAIL: bad v6 reply\n");
    exit(14);
}

/// The real work, given the initial process stack pointer (points at `argc`).
///
/// # Safety
/// `sp` must be the SysV process-entry stack pointer: `[argc][argv0]…[NULL]…`.
unsafe fn run(sp: *const usize) -> ! {
    print(b"[udpget] start\n");

    let argc = unsafe { *sp };
    if argc < 3 {
        print(b"[udpget] FAIL: argc\n");
        exit(20);
    }
    let argv1 = unsafe { *sp.add(2) } as *const u8;
    let argv2 = unsafe { *sp.add(3) } as *const u8;
    if argv1.is_null() || argv2.is_null() {
        exit(20);
    }

    let ip_bytes = unsafe { core::slice::from_raw_parts(argv1, strlen(argv1)) };
    let port_bytes = unsafe { core::slice::from_raw_parts(argv2, strlen(argv2)) };

    // A 4th argv of "6" selects the AF_INET6 loopback arm (argv1 is then a
    // 32-hex-char v6 address — the daemon's own link-local `me.ip6`).
    let v6 = argc >= 4 && {
        let argv3 = unsafe { *sp.add(4) } as *const u8;
        !argv3.is_null() && {
            let s = unsafe { core::slice::from_raw_parts(argv3, strlen(argv3)) };
            s == b"6"
        }
    };

    let port = match parse_port(port_bytes) {
        Some(v) => v,
        None => {
            print(b"[udpget] FAIL: port parse\n");
            exit(22);
        }
    };

    if v6 {
        // Never returns.
        unsafe { run_v6(ip_bytes, port) };
    }

    let ip = match parse_ipv4(ip_bytes) {
        Some(v) => v,
        None => {
            print(b"[udpget] FAIL: ip parse\n");
            exit(21);
        }
    };

    // socket(AF_INET, SOCK_DGRAM, 0)
    let fd = unsafe { sys3(SYS_SOCKET, AF_INET, SOCK_DGRAM, 0) };
    if fd < 0 {
        print(b"[udpget] FAIL: socket\n");
        exit(10);
    }
    let fd = fd as usize;

    // bind(fd, sockaddr_in{0.0.0.0, 0}, 16) — request an ephemeral local port.
    let local = sockaddr_in(&[0, 0, 0, 0], 0);
    let rc = unsafe { sys3(SYS_BIND, fd, local.as_ptr() as usize, local.len()) };
    if rc < 0 {
        print(b"[udpget] FAIL: bind\n");
        let _ = unsafe { sys1(SYS_CLOSE, fd) };
        exit(11);
    }

    // sendto(fd, QUERY, 29, 0, sockaddr_in{dns, 53}, 16)
    let dst = sockaddr_in(&ip, port);
    let sent = unsafe {
        sys6(
            SYS_SENDTO,
            fd,
            QUERY.as_ptr() as usize,
            QUERY.len(),
            0,
            dst.as_ptr() as usize,
            dst.len(),
        )
    };
    if sent < 0 {
        print(b"[udpget] FAIL: sendto\n");
        let _ = unsafe { sys1(SYS_CLOSE, fd) };
        exit(12);
    }
    print(b"[udpget] query sent\n");

    // recvfrom(fd, buf, .., 0, &src, &srclen) — one blocking datagram receive.
    let mut buf = [0u8; 512];
    let mut src = [0u8; 16];
    let mut srclen: u32 = src.len() as u32;
    let n = unsafe {
        sys6(
            SYS_RECVFROM,
            fd,
            buf.as_mut_ptr() as usize,
            buf.len(),
            0,
            src.as_mut_ptr() as usize,
            (&mut srclen as *mut u32) as usize,
        )
    };
    let _ = unsafe { sys1(SYS_CLOSE, fd) };
    if n <= 0 {
        print(b"[udpget] FAIL: recvfrom\n");
        exit(13);
    }
    let got = &buf[..n as usize];
    // A valid DNS reply echoes our TXID (0x1234) and sets the QR (response) bit
    // (high bit of the flags byte at offset 2).
    if got.len() >= 12 && got[0] == 0x12 && got[1] == 0x34 && (got[2] & 0x80) != 0 {
        print(b"[udpget] OK: DNS reply\n");
        exit(0);
    }
    print(b"[udpget] FAIL: bad reply\n");
    exit(14);
}

// _start: capture the SysV entry stack pointer (rsp -> argc) and forward it.
core::arch::global_asm!(
    ".section .text._start,\"ax\",@progbits",
    ".globl _start",
    "_start:",
    "  mov rdi, rsp",
    "  and rsp, -16",
    "  call {run}",
    run = sym rust_entry,
);

#[unsafe(no_mangle)]
unsafe extern "C" fn rust_entry(sp: *const usize) -> ! {
    unsafe { run(sp) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    print(b"[udpget] PANIC\n");
    exit(99);
}
