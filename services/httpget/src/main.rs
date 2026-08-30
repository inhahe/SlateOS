//! `httpget` — a minimal **Linux-ABI** ring-3 HTTP client.
//!
//! This is the netstack Phase-5 "ring-3 socket-syscall HTTP capstone"
//! (see `todo.txt` Judgment Calls, 2026-07-14, and `roadmap.md` §2.4
//! Phase 5.6). Its whole purpose is to exercise the **daemon-backed
//! socket-fd path from an actual ring-3 process**, end to end, using the
//! raw Linux x86_64 syscall ABI:
//!
//! ```text
//!   socket(AF_INET, SOCK_STREAM, 0)
//!   connect(fd, sockaddr_in{ip,port}, 16)
//!   write(fd, "HEAD / HTTP/1.0\r\n...")
//!   read(fd, buf)          // expect "HTTP/1..."
//!   close(fd)
//! ```
//!
//! When the kernel is booted with `net.userspace` on, those syscalls are
//! routed by `dispatch_linux` → `net::socket::*` → the persistent
//! `net.stack` daemon over the SHM ring. Prior to this program that
//! dispatch wiring (user-pointer copies, fd install, errno mapping) was
//! only exercised by code review + a thin-delegation argument, never by a
//! live ring-3 call — this closes that gap.
//!
//! The program is spawned by `proc::spawn::run_persistent_netstack` with an
//! explicit `AbiMode::Linux` override and `argv = ["httpget", "<a.b.c.d>",
//! "<port>"]` (the kernel resolves `example.com` first and passes the IP).
//! It reports its result purely through its **exit code** so the kernel
//! self-test can decode it deterministically without parsing console
//! output:
//!
//! | code | meaning                                   |
//! |------|-------------------------------------------|
//! | 0    | success — response began with `HTTP/1`    |
//! | 10   | `socket()` failed                         |
//! | 11   | `connect()` failed                        |
//! | 12   | `write()` failed                          |
//! | 13   | `read()` returned <= 0 (no response)      |
//! | 14   | response did not begin with `HTTP/1`      |
//! | 20   | wrong argc                                |
//! | 21   | could not parse the IP argument           |
//! | 22   | could not parse the port argument         |

#![no_std]
#![no_main]

// ---- Linux x86_64 syscall numbers ----
const SYS_READ: usize = 0;
const SYS_WRITE: usize = 1;
const SYS_CLOSE: usize = 3;
const SYS_SOCKET: usize = 41;
const SYS_CONNECT: usize = 42;
const SYS_EXIT: usize = 60;

const AF_INET: usize = 2;
const SOCK_STREAM: usize = 1;

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

fn print(s: &[u8]) {
    // Best-effort progress to stdout (installed as the console for Linux-ABI
    // processes). The exit code, not this output, is the authoritative result.
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
    // Bounded so a missing terminator can't spin forever on hostile input.
    while n < 4096 {
        if unsafe { *p.add(n) } == 0 {
            break;
        }
        n += 1;
    }
    n
}

/// Parse a dotted-decimal IPv4 (`"93.184.216.34"`) into 4 network-order octets.
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

/// Parse a decimal port (`"80"`) into a `u16`.
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

/// The real work, given the initial process stack pointer (points at `argc`).
///
/// # Safety
/// `sp` must be the SysV process-entry stack pointer: `[argc][argv0]…[NULL]…`.
unsafe fn run(sp: *const usize) -> ! {
    print(b"[httpget] start\n");

    let argc = unsafe { *sp };
    // argv[0] = *(sp+1), argv[1] = *(sp+2), argv[2] = *(sp+3)
    if argc < 3 {
        print(b"[httpget] FAIL: argc\n");
        exit(20);
    }
    let argv1 = unsafe { *sp.add(2) } as *const u8;
    let argv2 = unsafe { *sp.add(3) } as *const u8;
    if argv1.is_null() || argv2.is_null() {
        exit(20);
    }

    let ip_bytes = unsafe { core::slice::from_raw_parts(argv1, strlen(argv1)) };
    let port_bytes = unsafe { core::slice::from_raw_parts(argv2, strlen(argv2)) };

    let ip = match parse_ipv4(ip_bytes) {
        Some(v) => v,
        None => {
            print(b"[httpget] FAIL: ip parse\n");
            exit(21);
        }
    };
    let port = match parse_port(port_bytes) {
        Some(v) => v,
        None => {
            print(b"[httpget] FAIL: port parse\n");
            exit(22);
        }
    };

    // socket(AF_INET, SOCK_STREAM, 0)
    let fd = unsafe { sys3(SYS_SOCKET, AF_INET, SOCK_STREAM, 0) };
    if fd < 0 {
        print(b"[httpget] FAIL: socket\n");
        exit(10);
    }
    let fd = fd as usize;

    // struct sockaddr_in { u16 family; u16 port_be; u8 addr[4]; u8 pad[8]; }
    let mut sa = [0u8; 16];
    sa[0] = (AF_INET & 0xff) as u8;
    sa[1] = ((AF_INET >> 8) & 0xff) as u8;
    // port in network (big-endian) byte order
    sa[2] = (port >> 8) as u8;
    sa[3] = (port & 0xff) as u8;
    sa[4] = ip[0];
    sa[5] = ip[1];
    sa[6] = ip[2];
    sa[7] = ip[3];

    let rc = unsafe { sys3(SYS_CONNECT, fd, sa.as_ptr() as usize, sa.len()) };
    if rc < 0 {
        print(b"[httpget] FAIL: connect\n");
        let _ = unsafe { sys1(SYS_CLOSE, fd) };
        exit(11);
    }
    print(b"[httpget] connected\n");

    const REQ: &[u8] = b"HEAD / HTTP/1.0\r\nHost: example.com\r\nConnection: close\r\n\r\n";
    let mut off = 0usize;
    while off < REQ.len() {
        let n = unsafe {
            sys3(
                SYS_WRITE,
                fd,
                REQ.as_ptr().add(off) as usize,
                REQ.len() - off,
            )
        };
        if n <= 0 {
            print(b"[httpget] FAIL: write\n");
            let _ = unsafe { sys1(SYS_CLOSE, fd) };
            exit(12);
        }
        off += n as usize;
    }

    let mut buf = [0u8; 256];
    let n = unsafe { sys3(SYS_READ, fd, buf.as_mut_ptr() as usize, buf.len()) };
    let _ = unsafe { sys1(SYS_CLOSE, fd) };
    if n <= 0 {
        print(b"[httpget] FAIL: read\n");
        exit(13);
    }
    let got = &buf[..n as usize];
    // Expect the status line to begin "HTTP/1".
    if got.len() >= 6 && &got[..6] == b"HTTP/1" {
        print(b"[httpget] OK: HTTP response\n");
        exit(0);
    }
    print(b"[httpget] FAIL: bad response\n");
    exit(14);
}

// _start: capture the SysV entry stack pointer (rsp -> argc) and forward it.
// A naked entry avoids any prologue perturbing rsp before we read argc/argv.
core::arch::global_asm!(
    ".section .text._start,\"ax\",@progbits",
    ".globl _start",
    "_start:",
    "  mov rdi, rsp",   // arg0 = initial stack pointer
    "  and rsp, -16",   // 16-byte align for the SysV call below
    "  call {run}",
    run = sym rust_entry,
);

#[unsafe(no_mangle)]
unsafe extern "C" fn rust_entry(sp: *const usize) -> ! {
    unsafe { run(sp) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    print(b"[httpget] PANIC\n");
    exit(99);
}
