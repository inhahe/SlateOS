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
// Protocol constants + frame builders
// ---------------------------------------------------------------------------

const ETH_HDR: usize = 14;
const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;
const BROADCAST_MAC: [u8; 6] = [0xFF; 6];

const ARP_HW_ETHERNET: u16 = 1;
const ARP_PROTO_IPV4: u16 = 0x0800;
const ARP_REQUEST: u16 = 1;
const ARP_REPLY: u16 = 2;

const IP_PROTO_ICMP: u8 = 1;
const ICMP_ECHO_REQUEST: u8 = 8;
const ICMP_ECHO_REPLY: u8 = 0;

/// Maximum standard Ethernet frame we handle (jumbo frames are out of scope).
const MAX_FRAME: usize = 1522;

/// Write the 14-byte Ethernet header into `out[..14]`.
fn write_eth_header(out: &mut [u8], dst: &[u8; 6], src: &[u8; 6], ethertype: u16) {
    out[0..6].copy_from_slice(dst);
    out[6..12].copy_from_slice(src);
    out[12..14].copy_from_slice(&ethertype.to_be_bytes());
}

/// Build an ARP frame (request or reply) into a fixed 42-byte buffer.
fn build_arp(
    op: u16,
    src_mac: &[u8; 6],
    src_ip: &[u8; 4],
    dst_mac: &[u8; 6],
    dst_ip: &[u8; 4],
    eth_dst: &[u8; 6],
) -> [u8; ETH_HDR + 28] {
    let mut f = [0u8; ETH_HDR + 28];
    write_eth_header(&mut f, eth_dst, src_mac, ETHERTYPE_ARP);
    let a = &mut f[ETH_HDR..];
    a[0..2].copy_from_slice(&ARP_HW_ETHERNET.to_be_bytes());
    a[2..4].copy_from_slice(&ARP_PROTO_IPV4.to_be_bytes());
    a[4] = 6; // hw addr len
    a[5] = 4; // proto addr len
    a[6..8].copy_from_slice(&op.to_be_bytes());
    a[8..14].copy_from_slice(src_mac);
    a[14..18].copy_from_slice(src_ip);
    a[18..24].copy_from_slice(dst_mac);
    a[24..28].copy_from_slice(dst_ip);
    f
}

// ---------------------------------------------------------------------------
// Internet checksum (RFC 1071)
// ---------------------------------------------------------------------------

fn checksum16(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum = sum.wrapping_add(u16::from_be_bytes([data[i], data[i + 1]]) as u32);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

// ---------------------------------------------------------------------------
// Frame handling
// ---------------------------------------------------------------------------

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
    if frame.len() < ETH_HDR {
        return Handled::Other;
    }
    let mut src_mac = [0u8; 6];
    src_mac.copy_from_slice(&frame[6..12]);
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    let payload = &frame[ETH_HDR..];
    match ethertype {
        ETHERTYPE_ARP => handle_arp(payload, me),
        ETHERTYPE_IPV4 => {
            handle_ipv4(payload, &src_mac, me);
            Handled::Other
        }
        _ => Handled::Other,
    }
}

fn handle_arp(arp: &[u8], me: &IfInfo) -> Handled {
    if arp.len() < 28 {
        return Handled::Other;
    }
    let op = u16::from_be_bytes([arp[6], arp[7]]);
    let mut sender_mac = [0u8; 6];
    sender_mac.copy_from_slice(&arp[8..14]);
    let sender_ip = [arp[14], arp[15], arp[16], arp[17]];
    let target_ip = [arp[24], arp[25], arp[26], arp[27]];

    if op == ARP_REPLY {
        // Did the gateway answer our request?
        if sender_ip == me.gateway {
            print("[netstack] ARP reply: gateway resolved\n");
            return Handled::GatewayResolved;
        }
        return Handled::Other;
    }

    if op == ARP_REQUEST && target_ip == me.ip {
        // Answer: tell the requester our MAC.
        let reply = build_arp(
            ARP_REPLY,
            &me.mac,
            &me.ip,
            &sender_mac,
            &sender_ip,
            &sender_mac,
        );
        let _ = raw_tx(&reply);
        print("[netstack] answered ARP request for our IP\n");
    }
    Handled::Other
}

fn handle_ipv4(ip: &[u8], src_mac: &[u8; 6], me: &IfInfo) {
    if ip.len() < 20 {
        return;
    }
    let ihl = ((ip[0] & 0x0F) as usize) * 4;
    if ihl < 20 || ip.len() < ihl {
        return;
    }
    let proto = ip[9];
    let dst_ip = [ip[16], ip[17], ip[18], ip[19]];
    if proto != IP_PROTO_ICMP || dst_ip != me.ip {
        return;
    }
    let mut src_ip = [0u8; 4];
    src_ip.copy_from_slice(&ip[12..16]);
    let icmp = &ip[ihl..];
    if icmp.len() < 8 || icmp[0] != ICMP_ECHO_REQUEST {
        return;
    }
    reply_icmp_echo(&src_ip, src_mac, icmp, me);
}

/// Build and transmit an ICMP echo reply for a received echo request.
///
/// `dst_mac` is the requester's L2 source address (taken straight from the
/// received Ethernet header), so the reply is unicast back to whoever pinged
/// us — no ARP lookup needed for the reply path.
fn reply_icmp_echo(src_ip: &[u8; 4], dst_mac: &[u8; 6], req_icmp: &[u8], me: &IfInfo) {
    let icmp_len = req_icmp.len();
    let total_ip = 20 + icmp_len;
    let total = ETH_HDR + total_ip;
    if total > MAX_FRAME {
        return; // Oversized; drop.
    }
    let mut buf = [0u8; MAX_FRAME];

    // Unicast the reply straight back to the requester's MAC.
    write_eth_header(&mut buf, dst_mac, &me.mac, ETHERTYPE_IPV4);

    // IPv4 header.
    let ip = &mut buf[ETH_HDR..ETH_HDR + 20];
    ip[0] = 0x45; // version 4, IHL 5
    ip[1] = 0; // DSCP/ECN
    ip[2..4].copy_from_slice(&(total_ip as u16).to_be_bytes());
    ip[4..6].copy_from_slice(&0u16.to_be_bytes()); // id
    ip[6..8].copy_from_slice(&0x4000u16.to_be_bytes()); // DF
    ip[8] = 64; // TTL
    ip[9] = IP_PROTO_ICMP;
    ip[10..12].copy_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    ip[12..16].copy_from_slice(&me.ip);
    ip[16..20].copy_from_slice(src_ip);
    let ip_csum = checksum16(&buf[ETH_HDR..ETH_HDR + 20]);
    buf[ETH_HDR + 10..ETH_HDR + 12].copy_from_slice(&ip_csum.to_be_bytes());

    // ICMP payload: copy the request, flip type to reply, recompute checksum.
    let icmp_off = ETH_HDR + 20;
    buf[icmp_off..icmp_off + icmp_len].copy_from_slice(req_icmp);
    buf[icmp_off] = ICMP_ECHO_REPLY;
    buf[icmp_off + 1] = 0; // code
    buf[icmp_off + 2] = 0; // checksum placeholder
    buf[icmp_off + 3] = 0;
    let icmp_csum = checksum16(&buf[icmp_off..icmp_off + icmp_len]);
    buf[icmp_off + 2..icmp_off + 4].copy_from_slice(&icmp_csum.to_be_bytes());

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
    let arp_req = build_arp(
        ARP_REQUEST,
        &me.mac,
        &me.ip,
        &[0u8; 6],
        &me.gateway,
        &BROADCAST_MAC,
    );
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
            if len <= buf.len() {
                if let Handled::GatewayResolved = handle_frame(&buf[..len], &me) {
                    resolved = true;
                }
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
