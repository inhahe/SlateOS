//! `netstack` — userspace network-stack daemon (Phase 2 skeleton).
//!
//! Part of the "move the TCP/IP stack to userspace" migration (Path B — see
//! `net-userspace-migration.md` and `design-decisions.md` §63). This is the
//! Phase 2 *skeleton*: it proves the raw-frame boundary end-to-end by opening
//! the NIC through the capability-gated `SYS_NET_RAW_*` syscalls (landed in
//! Phase 1), running a poll loop over raw Ethernet frames, and speaking two
//! protocols entirely in userspace:
//!
//! - **ARP**: it broadcasts an ARP request for the default gateway, waits for
//!   the reply (proving raw TX *and* RX work against QEMU's slirp), and also
//!   answers inbound ARP requests addressed to our IP (the "responder" role
//!   the full daemon will play).
//! - **ICMP echo**: it answers inbound ICMP echo requests (ping) addressed to
//!   our IP — the classic "does the loop work" smoke test.
//!
//! Lifecycle for the boot self-test:
//! 1. Query interface config (`SYS_NET_IF_INFO`) for our IP / MAC / gateway.
//! 2. Claim the NIC (`SYS_NET_RAW_OPEN`).
//! 3. Broadcast an ARP request for the gateway.
//! 4. Poll for frames up to a deadline, resolving the gateway and answering
//!    any ARP/ICMP requests seen.
//! 5. Release the NIC (`SYS_NET_RAW_CLOSE`) and exit 0 on success (gateway
//!    resolved), 1 on timeout.
//!
//! The in-kernel stack stays the *active* path in production until later
//! phases cut over; this daemon only holds the NIC claim for the brief
//! self-test, after which `net::poll()` resumes draining the physical NIC.
//!
//! Everything here is deliberately `no_std` / `no_main` with hand-rolled
//! syscall wrappers, matching the other bare-metal services in `services/`.

#![no_std]
#![no_main]

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

const SYS_EXIT: u64 = 1;
const SYS_SLEEP: u64 = 11;
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_NOTIFY_READY: u64 = 508;
const SYS_NET_IF_INFO: u64 = 842;
const SYS_NET_RAW_OPEN: u64 = 865;
const SYS_NET_RAW_TX: u64 = 866;
const SYS_NET_RAW_RX: u64 = 867;
const SYS_NET_RAW_CLOSE: u64 = 868;

/// `EAGAIN`/`WouldBlock`: raw RX had no frame ready.
const E_WOULD_BLOCK: i64 = -4;

// ---------------------------------------------------------------------------
// Syscall wrappers
// ---------------------------------------------------------------------------

#[inline(always)]
fn syscall0(nr: u64) -> i64 {
    let ret: i64;
    // SAFETY: `syscall` with only the clobbers the ABI documents (rcx, r11);
    // no memory operands, so `nostack` is sound.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
fn syscall1(nr: u64, arg0: u64) -> i64 {
    let ret: i64;
    // SAFETY: see `syscall0`; arg0 passed in rdi per the SlateOS syscall ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") arg0,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
fn syscall2(nr: u64, arg0: u64, arg1: u64) -> i64 {
    let ret: i64;
    // SAFETY: see `syscall0`; args in rdi/rsi per the SlateOS syscall ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") arg0,
            in("rsi") arg1,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn print(s: &str) {
    syscall2(SYS_CONSOLE_WRITE, s.as_ptr() as u64, s.len() as u64);
}

fn exit(code: i64) -> ! {
    syscall1(SYS_EXIT, code as u64);
    loop {
        // SAFETY: `hlt` after exit; the process is already being torn down.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

fn sleep_ns(ns: u64) {
    syscall1(SYS_SLEEP, ns);
}

// ---------------------------------------------------------------------------
// Raw-frame helpers
// ---------------------------------------------------------------------------

/// Claim the NIC. Returns `true` on success.
fn raw_open() -> bool {
    // arg0 = interface index 0 (the sole physical NIC); arg1 = flags (0).
    syscall2(SYS_NET_RAW_OPEN, 0, 0) >= 0
}

fn raw_close() {
    syscall0(SYS_NET_RAW_CLOSE);
}

/// Transmit one raw Ethernet frame.
fn raw_tx(frame: &[u8]) -> i64 {
    syscall2(SYS_NET_RAW_TX, frame.as_ptr() as u64, frame.len() as u64)
}

/// Receive one raw Ethernet frame into `buf`. Returns bytes read, or a
/// negative errno (`E_WOULD_BLOCK` when no frame is queued).
fn raw_rx(buf: &mut [u8]) -> i64 {
    syscall2(SYS_NET_RAW_RX, buf.as_mut_ptr() as u64, buf.len() as u64)
}

// ---------------------------------------------------------------------------
// Interface info
// ---------------------------------------------------------------------------

struct IfInfo {
    ip: [u8; 4],
    gateway: [u8; 4],
    mac: [u8; 6],
}

fn query_if_info() -> Option<IfInfo> {
    let mut rec = [0u8; 24];
    let rc = syscall2(SYS_NET_IF_INFO, rec.as_mut_ptr() as u64, rec.len() as u64);
    if rc < 0 {
        return None;
    }
    Some(IfInfo {
        ip: [rec[0], rec[1], rec[2], rec[3]],
        gateway: [rec[8], rec[9], rec[10], rec[11]],
        mac: [rec[16], rec[17], rec[18], rec[19], rec[20], rec[21]],
    })
}

// ---------------------------------------------------------------------------
// Protocol handling (via the shared `netproto` crate)
// ---------------------------------------------------------------------------
//
// All wire-format parsing and construction lives in `netproto` — the same
// no_std crate the kernel stack will migrate onto, so there is a single source
// of truth for frame layout and the RFC 1071 checksum. This daemon only wires
// those parsers/builders to the raw-frame syscalls and the interface config.

use netproto::{arp, ethernet, icmp, ipv4};

/// Maximum standard Ethernet frame we handle (jumbo frames are out of scope).
const MAX_FRAME: usize = 1522;

/// Result of processing one received frame.
enum Handled {
    /// The ARP reply resolving our gateway arrived.
    GatewayResolved,
    /// Something else (or nothing actionable) — keep polling.
    Other,
}

/// Process one received Ethernet frame. Answers ARP requests + ICMP echo
/// addressed to us, and detects the gateway ARP reply.
fn handle_frame(frame: &[u8], me: &IfInfo) -> Handled {
    let eth = match ethernet::Frame::parse(frame) {
        Some(f) => f,
        None => return Handled::Other,
    };
    match eth.ethertype {
        ethernet::ETHERTYPE_ARP => handle_arp(eth.payload, me),
        ethernet::ETHERTYPE_IPV4 => {
            handle_ipv4(eth.payload, &eth.src, me);
            Handled::Other
        }
        _ => Handled::Other,
    }
}

fn handle_arp(body: &[u8], me: &IfInfo) -> Handled {
    let pkt = match arp::Packet::parse(body) {
        Some(p) => p,
        None => return Handled::Other,
    };
    match pkt.op {
        arp::Op::Reply => {
            // Did the gateway answer our request?
            if pkt.sender_ip == me.gateway {
                print("[netstack] ARP reply: gateway resolved\n");
                return Handled::GatewayResolved;
            }
        }
        arp::Op::Request => {
            if pkt.target_ip == me.ip {
                // Answer: tell the requester our MAC (unicast back to it).
                if let Some(reply) = arp::reply_to(&pkt, &me.mac) {
                    let _ = raw_tx(&reply);
                    print("[netstack] answered ARP request for our IP\n");
                }
            }
        }
    }
    Handled::Other
}

fn handle_ipv4(body: &[u8], src_mac: &[u8; 6], me: &IfInfo) {
    let pkt = match ipv4::Packet::parse(body) {
        Some(p) => p,
        None => return,
    };
    if pkt.protocol != ipv4::PROTO_ICMP || pkt.dst != me.ip {
        return;
    }
    // Only answer echo *requests* addressed to us.
    let echo = match icmp::Echo::parse(pkt.payload) {
        Some(e) if e.is_request => e,
        _ => return,
    };
    reply_icmp_echo(&pkt.src, src_mac, &echo, me);
}

/// Build and transmit an ICMP echo reply for a received echo request.
///
/// `dst_mac` is the requester's L2 source address (taken straight from the
/// received Ethernet header), so the reply is unicast back to whoever pinged
/// us — no ARP lookup needed for the reply path.
fn reply_icmp_echo(src_ip: &[u8; 4], dst_mac: &[u8; 6], req: &icmp::Echo, me: &IfInfo) {
    let mut buf = [0u8; MAX_FRAME];
    let icmp_off = ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN;

    // ICMP reply first (it sits after the Ethernet + IPv4 headers). This also
    // gives us the exact ICMP length for the IPv4 total-length field.
    let icmp_len = match icmp::reply_to(&mut buf[icmp_off..], req) {
        Some(n) => n,
        None => return, // Oversized or not a request; drop.
    };

    // IPv4 header (DF set, TTL 64), carrying `icmp_len` bytes of ICMP.
    let ip_hdr = ipv4::Builder {
        dscp_ecn: 0,
        id: 0,
        flags_frag: 0x4000, // Don't Fragment
        ttl: 64,
        protocol: ipv4::PROTO_ICMP,
        src: me.ip,
        dst: *src_ip,
    }
    .build_header(icmp_len as u16);
    buf[ethernet::HEADER_LEN..ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN]
        .copy_from_slice(&ip_hdr);

    // Ethernet header: unicast the reply straight back to the requester's MAC.
    ethernet::write_header(&mut buf, dst_mac, &me.mac, ethernet::ETHERTYPE_IPV4);

    let total = icmp_off + icmp_len;
    let _ = raw_tx(&buf[..total]);
    print("[netstack] answered ICMP echo request\n");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Poll budget: iterations * per-iteration sleep. 400 * 5ms = 2s.
const POLL_ITERS: u32 = 400;
const POLL_SLEEP_NS: u64 = 5_000_000;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("[netstack] starting (Phase 2 skeleton)\n");

    let me = match query_if_info() {
        Some(i) => i,
        None => {
            print("[netstack] FAIL: could not query interface info\n");
            exit(2);
        }
    };

    if !raw_open() {
        print("[netstack] FAIL: could not claim raw NIC (SYS_NET_RAW_OPEN)\n");
        exit(3);
    }
    print("[netstack] claimed raw NIC\n");

    // Signal readiness now that we own the NIC and can serve.
    syscall0(SYS_NOTIFY_READY);

    // Broadcast an ARP request for the gateway to prove TX + RX end-to-end.
    let arp_req = arp::request(&me.mac, &me.ip, &me.gateway);
    if raw_tx(&arp_req) < 0 {
        print("[netstack] FAIL: raw TX of ARP request failed\n");
        raw_close();
        exit(4);
    }
    print("[netstack] sent ARP request for gateway\n");

    let mut buf = [0u8; MAX_FRAME];
    let mut resolved = false;
    for _ in 0..POLL_ITERS {
        loop {
            let n = raw_rx(&mut buf);
            if n == E_WOULD_BLOCK {
                break; // Drain done for this tick.
            }
            if n < 0 {
                break; // Other error — back off.
            }
            let len = n as usize;
            if len <= buf.len()
                && let Handled::GatewayResolved = handle_frame(&buf[..len], &me)
            {
                resolved = true;
            }
        }
        if resolved {
            break;
        }
        sleep_ns(POLL_SLEEP_NS);
    }

    raw_close();
    print("[netstack] released raw NIC\n");

    if resolved {
        print("[netstack] SUCCESS: raw-frame path proven end-to-end\n");
        exit(0);
    }
    print("[netstack] TIMEOUT: no gateway ARP reply\n");
    exit(1);
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    print("!!! PANIC in netstack !!!\n");
    exit(-1);
}
