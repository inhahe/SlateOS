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
const SYS_CHANNEL_SEND: u64 = 201;
const SYS_CHANNEL_RECV_TIMEOUT: u64 = 205;
const SYS_CHANNEL_CLOSE: u64 = 204;
const SYS_SERVICE_REGISTER: u64 = 280;
const SYS_SERVICE_ACCEPT_TIMEOUT: u64 = 284;
const SYS_SERVICE_UNREGISTER: u64 = 285;
const SYS_NOTIFY_READY: u64 = 508;
const SYS_PROCESS_GET_ARGS: u64 = 519;
const SYS_NET_IF_INFO: u64 = 842;
const SYS_NET_RAW_OPEN: u64 = 865;
const SYS_NET_RAW_TX: u64 = 866;
const SYS_NET_RAW_RX: u64 = 867;
const SYS_NET_RAW_CLOSE: u64 = 868;

/// `EAGAIN`/`WouldBlock`: raw RX had no frame ready.
const E_WOULD_BLOCK: i64 = -4;

// ---------------------------------------------------------------------------
// netstack IPC control protocol (Phase 4)
// ---------------------------------------------------------------------------
//
// The request/reply wire schema (opcodes, status codes, encode/decode) lives in
// the shared `netipc` crate — the single source of truth linked into both this
// daemon and the kernel socket-syscall forwarders. Messages ride a Service-
// Registry channel (`net.stack`) as one-shot request/reply pairs; bulk TCP/UDP
// data will later add a shared-memory data ring alongside this control path.

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

#[inline(always)]
fn syscall3(nr: u64, arg0: u64, arg1: u64, arg2: u64) -> i64 {
    let ret: i64;
    // SAFETY: see `syscall0`; args in rdi/rsi/rdx per the SlateOS syscall ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[inline(always)]
fn syscall4(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let ret: i64;
    // SAFETY: see `syscall0`; args in rdi/rsi/rdx/r10 per the SlateOS syscall
    // ABI (r10 is used for arg3 because rcx is clobbered by `syscall`).
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            in("r10") arg3,
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
    mask: [u8; 4],
    gateway: [u8; 4],
    dns: [u8; 4],
    mac: [u8; 6],
}

impl IfInfo {
    /// Next-hop IP toward `dst`: the destination itself if it is on our local
    /// subnet (per the netmask), otherwise the default gateway. This is the L3
    /// routing decision needed to pick the ARP target for outbound frames.
    fn next_hop(&self, dst: &[u8; 4]) -> [u8; 4] {
        let mut on_link = true;
        let mut i = 0;
        while i < 4 {
            if (self.ip[i] & self.mask[i]) != (dst[i] & self.mask[i]) {
                on_link = false;
            }
            i += 1;
        }
        if on_link { *dst } else { self.gateway }
    }
}

fn query_if_info() -> Option<IfInfo> {
    let mut rec = [0u8; 24];
    let rc = syscall2(SYS_NET_IF_INFO, rec.as_mut_ptr() as u64, rec.len() as u64);
    if rc < 0 {
        return None;
    }
    Some(IfInfo {
        ip: [rec[0], rec[1], rec[2], rec[3]],
        mask: [rec[4], rec[5], rec[6], rec[7]],
        gateway: [rec[8], rec[9], rec[10], rec[11]],
        dns: [rec[12], rec[13], rec[14], rec[15]],
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

use netproto::{arp, dns, ethernet, icmp, ipv4, tcp, udp};

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
// DNS-over-UDP resolver (Phase 4 control op)
// ---------------------------------------------------------------------------
//
// The daemon serves `OP_RESOLVE_A` by doing a real DNS query over its raw NIC:
// ARP-resolve the next hop, build a DNS/UDP/IPv4/Ethernet frame stack via
// `netproto`, transmit it, and poll raw RX for the matching response. This is
// the userspace analogue of the in-kernel `net::dns::resolve` the Phase-4
// `sys_dns_resolve` forwarder will delegate to.

/// Fixed ephemeral UDP source port for our DNS queries. A single-shot resolver
/// doesn't need a port table; any high port distinct from well-known ones works.
const EPHEMERAL_PORT: u16 = 0xC000;

/// Per-resolution RX poll budget (iterations * `POLL_SLEEP_NS`).
const RESOLVE_POLL_ITERS: u32 = 120; // 120 * 5ms = 600ms

/// ARP-resolve `target_ip` on the local link: broadcast a request and poll raw
/// RX for the reply, returning the sender's MAC. `None` on timeout.
fn arp_resolve(target_ip: &[u8; 4], me: &IfInfo) -> Option<[u8; 6]> {
    let req = arp::request(&me.mac, &me.ip, target_ip);
    if raw_tx(&req) < 0 {
        return None;
    }
    let mut buf = [0u8; MAX_FRAME];
    for _ in 0..RESOLVE_POLL_ITERS {
        loop {
            let n = raw_rx(&mut buf);
            if n < 0 {
                break; // WouldBlock or error — sleep and retry.
            }
            let len = n as usize;
            if len > buf.len() {
                continue;
            }
            if let Some(mac) = arp_reply_mac(&buf[..len], target_ip) {
                return Some(mac);
            }
        }
        sleep_ns(POLL_SLEEP_NS);
    }
    None
}

/// If `frame` is an ARP reply from `target_ip`, return its sender MAC.
fn arp_reply_mac(frame: &[u8], target_ip: &[u8; 4]) -> Option<[u8; 6]> {
    let eth = ethernet::Frame::parse(frame)?;
    if eth.ethertype != ethernet::ETHERTYPE_ARP {
        return None;
    }
    let pkt = arp::Packet::parse(eth.payload)?;
    if matches!(pkt.op, arp::Op::Reply) && pkt.sender_ip == *target_ip {
        Some(pkt.sender_mac)
    } else {
        None
    }
}

/// Frame a DNS query payload `qbuf` as Ethernet | IPv4 | UDP toward `me.dns`
/// (from our ephemeral port to port 53) and transmit it on the raw NIC. `id`
/// seeds the IPv4 identification field. Returns `true` on a successful TX.
fn tx_dns_query(qbuf: &[u8], next_hop_mac: &[u8; 6], me: &IfInfo, id: u16) -> bool {
    let mut frame = [0u8; MAX_FRAME];
    let udp_off = ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN;
    let udp_len = match udp::write(&mut frame[udp_off..], &me.ip, &me.dns, EPHEMERAL_PORT, 53, qbuf) {
        Some(l) => l,
        None => return false,
    };
    let ip_hdr = ipv4::Builder {
        dscp_ecn: 0,
        id,
        flags_frag: 0x4000, // Don't Fragment
        ttl: 64,
        protocol: ipv4::PROTO_UDP,
        src: me.ip,
        dst: me.dns,
    }
    .build_header(udp_len as u16);
    frame[ethernet::HEADER_LEN..udp_off].copy_from_slice(&ip_hdr);
    ethernet::write_header(&mut frame, next_hop_mac, &me.mac, ethernet::ETHERTYPE_IPV4);
    let total = udp_off + udp_len;
    raw_tx(&frame[..total]) >= 0
}

/// Validate a received frame as *our* DNS response (matching `txid`, our
/// ephemeral port, sourced from `me.dns`) and return the parsed DNS message.
/// The message borrows `frame`.
fn dns_response_msg<'a>(frame: &'a [u8], me: &IfInfo, txid: u16) -> Option<dns::Message<'a>> {
    let eth = ethernet::Frame::parse(frame)?;
    if eth.ethertype != ethernet::ETHERTYPE_IPV4 {
        return None;
    }
    let ip = ipv4::Packet::parse(eth.payload)?;
    if ip.protocol != ipv4::PROTO_UDP || ip.dst != me.ip || ip.src != me.dns {
        return None;
    }
    let dg = udp::Datagram::parse(ip.payload, &ip.src, &ip.dst)?;
    if dg.src_port != 53 || dg.dst_port != EPHEMERAL_PORT {
        return None;
    }
    let msg = dns::Message::parse(dg.payload)?;
    if msg.id != txid || !msg.is_response() {
        return None;
    }
    Some(msg)
}

/// Resolve `hostname`'s first A record via DNS-over-UDP. `next_hop_mac` is the
/// L2 destination for outbound frames toward the DNS server. `txid` disambiguates
/// concurrent queries (we use a monotonic counter). Returns the IPv4 on success.
fn resolve_dns(hostname: &[u8], next_hop_mac: &[u8; 6], me: &IfInfo, txid: u16) -> Option<[u8; 4]> {
    let mut qbuf = [0u8; 300];
    let qlen = dns::write_query(&mut qbuf, txid, hostname, dns::TYPE_A)?;
    if !tx_dns_query(&qbuf[..qlen], next_hop_mac, me, txid) {
        return None;
    }
    let mut buf = [0u8; MAX_FRAME];
    for _ in 0..RESOLVE_POLL_ITERS {
        loop {
            let n = raw_rx(&mut buf);
            if n < 0 {
                break;
            }
            let len = n as usize;
            if len > buf.len() {
                continue;
            }
            if let Some(msg) = dns_response_msg(&buf[..len], me, txid) {
                let mut out = [0u8; 4];
                if msg.first_ipv4(&mut out) {
                    return Some(out);
                }
            }
        }
        sleep_ns(POLL_SLEEP_NS);
    }
    None
}

/// Reverse-resolve `ip` (PTR record) via DNS-over-UDP, writing the decoded
/// dotted-ASCII hostname into `out` and returning its length. Same transport
/// as [`resolve_dns`]; differs only in query type and response decoding.
fn resolve_ptr(
    ip: &[u8; 4],
    next_hop_mac: &[u8; 6],
    me: &IfInfo,
    txid: u16,
    out: &mut [u8],
) -> Option<usize> {
    let mut qbuf = [0u8; 300];
    let qlen = dns::write_ptr_query(&mut qbuf, txid, ip)?;
    if !tx_dns_query(&qbuf[..qlen], next_hop_mac, me, txid) {
        return None;
    }
    let mut buf = [0u8; MAX_FRAME];
    for _ in 0..RESOLVE_POLL_ITERS {
        loop {
            let n = raw_rx(&mut buf);
            if n < 0 {
                break;
            }
            let len = n as usize;
            if len > buf.len() {
                continue;
            }
            if let Some(msg) = dns_response_msg(&buf[..len], me, txid) {
                if let Some(w) = msg.first_ptr(out) {
                    if w > 0 {
                        return Some(w);
                    }
                }
            }
        }
        sleep_ns(POLL_SLEEP_NS);
    }
    None
}

// ---------------------------------------------------------------------------
// One-shot TCP client (Phase 4 control op `OP_TCP_FETCH`)
// ---------------------------------------------------------------------------
//
// A deliberately minimal TCP client for the reliable QEMU-slirp path: open a
// connection to `ip:port`, send a request payload, read the response into a
// caller buffer, and close. It implements just enough of RFC 793 to be correct
// on a loss-free link — SYN/SYN-ACK/ACK handshake, in-order data reception with
// cumulative ACKs, SYN/payload retransmission, and a graceful FIN close. It has
// NO congestion control, NO window management beyond a fixed advertised window,
// NO out-of-order reassembly (out-of-order data is dup-ACKed to trigger a
// retransmit), and NO segmentation of the outbound request (it must fit one
// segment). These limits are acceptable for the bounded self-test / control
// path; a full streaming socket API arrives with the Phase-5 data ring. See
// `todo.txt` for the tracked limitations.

/// Advertised receive window for our segments (bytes). Fixed; we drain promptly.
const TCP_WINDOW: u16 = 64240;

/// Per-attempt RX poll budget for the data phase (iterations * `POLL_SLEEP_NS`).
/// 400 * 5ms = 2s of quiescence before we give up on more data.
const TCP_DATA_ITERS: u32 = 400;

/// Number of SYN / payload (re)transmission attempts before giving up.
const TCP_SYN_ATTEMPTS: u32 = 5;

/// Metadata copied out of one received TCP segment (avoids borrowing the RX
/// frame buffer across the poll loop).
struct TcpRx {
    seq: u32,
    ack: u32,
    flags: u8,
    payload_len: usize,
}

/// Frame an arbitrary L4 payload as Ethernet | IPv4 toward `dst_ip` and transmit
/// it on the raw NIC. Generalizes [`tx_dns_query`]'s framing for any IP protocol.
/// Returns `true` on a successful TX.
fn send_ipv4(
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    dst_ip: &[u8; 4],
    proto: u8,
    l4: &[u8],
    id: u16,
) -> bool {
    let mut frame = [0u8; MAX_FRAME];
    let l4_off = ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN;
    let total = match l4_off.checked_add(l4.len()) {
        Some(t) if t <= frame.len() => t,
        _ => return false,
    };
    frame[l4_off..total].copy_from_slice(l4);
    let ip_hdr = ipv4::Builder {
        dscp_ecn: 0,
        id,
        flags_frag: 0x4000, // Don't Fragment
        ttl: 64,
        protocol: proto,
        src: me.ip,
        dst: *dst_ip,
    }
    .build_header(l4.len() as u16);
    frame[ethernet::HEADER_LEN..l4_off].copy_from_slice(&ip_hdr);
    ethernet::write_header(&mut frame, next_hop_mac, &me.mac, ethernet::ETHERTYPE_IPV4);
    raw_tx(&frame[..total]) >= 0
}

/// Build and transmit one TCP segment for our connection.
#[allow(clippy::too_many_arguments)]
fn send_tcp(
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    dst_ip: &[u8; 4],
    dst_port: u16,
    src_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
    id: u16,
) -> bool {
    // TCP segment sits inside the IPv4 payload; bound it by the framing headroom.
    let mut seg = [0u8; MAX_FRAME - (ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN)];
    let builder = tcp::Builder { src_port, dst_port, seq, ack, flags, window: TCP_WINDOW };
    let n = match builder.write(&mut seg, &me.ip, dst_ip, payload) {
        Some(n) => n,
        None => return false,
    };
    send_ipv4(me, next_hop_mac, dst_ip, ipv4::PROTO_TCP, &seg[..n], id)
}

/// Try to receive one TCP segment addressed to our connection (matching the
/// local/remote ports and endpoint IPs). On a match, copies the payload into
/// `pl` and returns the segment metadata; returns `None` when no matching frame
/// is currently queued (caller sleeps and retries).
fn recv_tcp_seg(
    me: &IfInfo,
    dst_ip: &[u8; 4],
    local_port: u16,
    remote_port: u16,
    frame: &mut [u8],
    pl: &mut [u8],
) -> Option<TcpRx> {
    let n = raw_rx(frame);
    if n < 0 {
        return None;
    }
    let len = n as usize;
    if len > frame.len() {
        return None;
    }
    let eth = ethernet::Frame::parse(&frame[..len])?;
    if eth.ethertype != ethernet::ETHERTYPE_IPV4 {
        return None;
    }
    let ip = ipv4::Packet::parse(eth.payload)?;
    if ip.protocol != ipv4::PROTO_TCP || ip.dst != me.ip || ip.src != *dst_ip {
        return None;
    }
    let seg = tcp::Segment::parse(ip.payload, &ip.src, &ip.dst)?;
    if seg.src_port != remote_port || seg.dst_port != local_port {
        return None;
    }
    let cplen = seg.payload.len().min(pl.len());
    pl[..cplen].copy_from_slice(&seg.payload[..cplen]);
    Some(TcpRx { seq: seg.seq, ack: seg.ack, flags: seg.flags, payload_len: cplen })
}

/// Perform a one-shot TCP fetch: connect to `dst_ip:dst_port`, send `payload`,
/// read the response into `out`, and close gracefully. `id` seeds the IPv4
/// identification counter. Returns the number of response bytes written to
/// `out` (0 is a valid result for an empty response), or `None` if the
/// connection could not be established.
fn tcp_fetch(
    dst_ip: &[u8; 4],
    dst_port: u16,
    next_hop_mac: &[u8; 6],
    me: &IfInfo,
    id: u16,
    payload: &[u8],
    out: &mut [u8],
) -> Option<usize> {
    let local_port = EPHEMERAL_PORT;
    let isn: u32 = 0x0001_0000;
    let mut ipid = id;

    let mut frame = [0u8; MAX_FRAME];
    let mut pl = [0u8; MAX_FRAME];

    // --- Handshake: (re)transmit SYN until a SYN-ACK arrives. ---
    let mut server_isn = 0u32;
    let mut established = false;
    'syn: for _ in 0..TCP_SYN_ATTEMPTS {
        ipid = ipid.wrapping_add(1);
        if !send_tcp(me, next_hop_mac, dst_ip, dst_port, local_port, isn, 0, tcp::FLAG_SYN, &[], ipid) {
            return None;
        }
        for _ in 0..RESOLVE_POLL_ITERS {
            while let Some(rx) = recv_tcp_seg(me, dst_ip, local_port, dst_port, &mut frame, &mut pl) {
                if rx.flags & tcp::FLAG_RST != 0 {
                    return None; // Connection refused.
                }
                if rx.flags & tcp::FLAG_SYN != 0
                    && rx.flags & tcp::FLAG_ACK != 0
                    && rx.ack == isn.wrapping_add(1)
                {
                    server_isn = rx.seq;
                    established = true;
                    break 'syn;
                }
            }
            sleep_ns(POLL_SLEEP_NS);
        }
    }
    if !established {
        return None;
    }

    let snd_nxt = isn.wrapping_add(1); // Our SYN consumed one sequence number.
    let mut rcv_nxt = server_isn.wrapping_add(1); // Their SYN consumed one.

    // ACK the SYN-ACK, completing the handshake.
    ipid = ipid.wrapping_add(1);
    send_tcp(me, next_hop_mac, dst_ip, dst_port, local_port, snd_nxt, rcv_nxt, tcp::FLAG_ACK, &[], ipid);

    // --- Send the request payload (single segment; must fit one MSS). ---
    // `snd_final` is the sequence number just past our data; a later FIN uses it.
    if !payload.is_empty() {
        ipid = ipid.wrapping_add(1);
        send_tcp(
            me,
            next_hop_mac,
            dst_ip,
            dst_port,
            local_port,
            snd_nxt,
            rcv_nxt,
            tcp::FLAG_PSH | tcp::FLAG_ACK,
            payload,
            ipid,
        );
    }
    let snd_final = snd_nxt.wrapping_add(payload.len() as u32);

    // --- Receive loop: accept in-order data, cumulative-ACK, honor FIN. ---
    let mut written = 0usize;
    let mut peer_fin = false;
    let mut idle = 0u32;
    let mut retransmits = 0u32;
    while idle < TCP_DATA_ITERS && !peer_fin {
        let mut got = false;
        while let Some(rx) = recv_tcp_seg(me, dst_ip, local_port, dst_port, &mut frame, &mut pl) {
            got = true;
            if rx.flags & tcp::FLAG_RST != 0 {
                peer_fin = true; // Treat a reset as end-of-stream.
                break;
            }
            if rx.seq == rcv_nxt {
                // In-order segment: consume any payload, then any FIN.
                if rx.payload_len > 0 {
                    let room = out.len().saturating_sub(written);
                    let take = rx.payload_len.min(room);
                    out[written..written + take].copy_from_slice(&pl[..take]);
                    written += take;
                    rcv_nxt = rcv_nxt.wrapping_add(rx.payload_len as u32);
                }
                if rx.flags & tcp::FLAG_FIN != 0 {
                    rcv_nxt = rcv_nxt.wrapping_add(1); // FIN occupies one seq.
                    peer_fin = true;
                }
                if rx.payload_len > 0 || peer_fin {
                    ipid = ipid.wrapping_add(1);
                    send_tcp(me, next_hop_mac, dst_ip, dst_port, local_port, snd_final, rcv_nxt, tcp::FLAG_ACK, &[], ipid);
                }
            } else if rx.payload_len > 0 {
                // Out-of-order data: dup-ACK to prompt the peer to retransmit.
                ipid = ipid.wrapping_add(1);
                send_tcp(me, next_hop_mac, dst_ip, dst_port, local_port, snd_final, rcv_nxt, tcp::FLAG_ACK, &[], ipid);
            }
        }
        if got {
            idle = 0;
        } else {
            idle = idle.saturating_add(1);
            // If nothing has come back and our data may have been lost, retransmit
            // the request payload a few times early in the idle window.
            if !payload.is_empty()
                && rcv_nxt == server_isn.wrapping_add(1)
                && idle == 40
                && retransmits < 3
            {
                retransmits += 1;
                idle = 0;
                ipid = ipid.wrapping_add(1);
                send_tcp(
                    me,
                    next_hop_mac,
                    dst_ip,
                    dst_port,
                    local_port,
                    snd_nxt,
                    rcv_nxt,
                    tcp::FLAG_PSH | tcp::FLAG_ACK,
                    payload,
                    ipid,
                );
            }
            sleep_ns(POLL_SLEEP_NS);
        }
    }

    // --- Graceful close: send our FIN, briefly drain the peer's final ACK. ---
    ipid = ipid.wrapping_add(1);
    send_tcp(me, next_hop_mac, dst_ip, dst_port, local_port, snd_final, rcv_nxt, tcp::FLAG_FIN | tcp::FLAG_ACK, &[], ipid);
    for _ in 0..40 {
        let mut any = false;
        while let Some(rx) = recv_tcp_seg(me, dst_ip, local_port, dst_port, &mut frame, &mut pl) {
            any = true;
            // A late FIN from the peer still needs an ACK for a clean teardown.
            if rx.flags & tcp::FLAG_FIN != 0 && rx.seq == rcv_nxt {
                rcv_nxt = rcv_nxt.wrapping_add(rx.payload_len as u32).wrapping_add(1);
                ipid = ipid.wrapping_add(1);
                send_tcp(me, next_hop_mac, dst_ip, dst_port, local_port, snd_final.wrapping_add(1), rcv_nxt, tcp::FLAG_ACK, &[], ipid);
            }
        }
        if !any {
            sleep_ns(POLL_SLEEP_NS);
        }
    }

    Some(written)
}

// ---------------------------------------------------------------------------
// Service mode (Phase 4): serve socket-syscall requests over `net.stack`
// ---------------------------------------------------------------------------
//
// Bounded-lifetime service loop (see `design-decisions.md` §64): the daemon
// claims the NIC, registers `net.stack`, serves control requests over accepted
// channels until an idle deadline, then unregisters and releases the NIC. It is
// NOT a permanent NIC takeover — while the kernel-resident stack is still the
// live path (until Phase 5), the daemon must yield the NIC back. Persistent
// operation lands in Phase 5, when the kernel stack is deleted.

const SERVICE_NAME: &[u8] = b"net.stack";

/// Accept-timeout for one service iteration (ns). Long enough to absorb the
/// kernel client's connect latency; short enough to re-check the deadline.
const ACCEPT_TIMEOUT_NS: u64 = 500_000_000; // 500ms

/// Total idle budget: after this long with no new connection, the daemon exits.
const SERVICE_IDLE_ITERS: u32 = 6; // 6 * 500ms = 3s

/// Maximum control-message size we accept/produce.
const MSG_CAP: usize = 512;

/// Run the Phase-4 DNS-over-IPC service. Returns the process exit code.
fn run_dns_service(me: &IfInfo) -> i64 {
    // Register first (fast) so a waiting client can connect immediately and then
    // block on the reply, rather than spinning while we do slow network I/O.
    let listener = syscall2(SYS_SERVICE_REGISTER, SERVICE_NAME.as_ptr() as u64, SERVICE_NAME.len() as u64);
    if listener < 0 {
        print("[netstack] FAIL: could not register net.stack service\n");
        return 5;
    }
    print("[netstack] registered net.stack service\n");
    syscall0(SYS_NOTIFY_READY);

    // Now resolve the next hop toward the DNS server. If it fails, every resolve
    // reports ST_FAIL (the client sees a definite failure reply, not a hang).
    let next_hop = me.next_hop(&me.dns);
    let next_hop_mac = arp_resolve(&next_hop, me);
    if next_hop_mac.is_some() {
        print("[netstack] resolved next-hop MAC toward DNS server\n");
    } else {
        print("[netstack] WARN: could not ARP-resolve DNS next hop\n");
    }

    let mut txid: u16 = 0x1000;
    let mut served: u32 = 0;
    let mut idle_ticks: u32 = 0;

    while idle_ticks < SERVICE_IDLE_ITERS {
        let ch = syscall2(SYS_SERVICE_ACCEPT_TIMEOUT, listener as u64, ACCEPT_TIMEOUT_NS as u64);
        if ch < 0 {
            idle_ticks += 1; // Timed out (or transient) — count toward idle exit.
            continue;
        }
        idle_ticks = 0;
        let ch = ch as u64;

        // One request per connection (one-shot control path).
        let mut req = [0u8; MSG_CAP];
        let rlen = syscall4(
            SYS_CHANNEL_RECV_TIMEOUT,
            ch,
            req.as_mut_ptr() as u64,
            req.len() as u64,
            ACCEPT_TIMEOUT_NS as u64,
        );
        if rlen > 0 {
            let req = &req[..rlen as usize];
            let mut reply = [0u8; MSG_CAP];
            let reply_len = handle_request(req, &next_hop_mac, me, &mut txid, &mut reply);
            let _ = syscall3(SYS_CHANNEL_SEND, ch, reply.as_ptr() as u64, reply_len as u64);
            served += 1;
        }
        syscall1(SYS_CHANNEL_CLOSE, ch);
    }

    let _ = syscall1(SYS_SERVICE_UNREGISTER, listener as u64);
    if served > 0 {
        print("[netstack] served DNS-over-IPC request(s); unregistered\n");
    } else {
        print("[netstack] no requests before idle deadline; unregistered\n");
    }
    0
}

/// Handle one control request, writing the reply into `out` and returning its
/// length. `out` must be at least `MSG_CAP` bytes. The request/reply schema is
/// owned by the shared `netipc` crate.
fn handle_request(
    req: &[u8],
    next_hop_mac: &Option<[u8; 6]>,
    me: &IfInfo,
    txid: &mut u16,
    out: &mut [u8],
) -> usize {
    let fail = |out: &mut [u8]| netipc::encode_fail(out).unwrap_or(0);
    match netipc::Request::parse(req) {
        Some(netipc::Request::ResolveA(hostname)) => {
            let ip = match next_hop_mac {
                Some(mac) if !hostname.is_empty() => {
                    *txid = txid.wrapping_add(1);
                    resolve_dns(hostname, mac, me, *txid)
                }
                _ => None,
            };
            match ip {
                Some(addr) => netipc::encode_ok_ipv4(out, &addr).unwrap_or_else(|| fail(out)),
                None => fail(out),
            }
        }
        Some(netipc::Request::ResolvePtr(ip)) => {
            // Reverse-resolve into a scratch name buffer, then frame the reply.
            let mut name = [0u8; MSG_CAP];
            let name_len = match next_hop_mac {
                Some(mac) => {
                    *txid = txid.wrapping_add(1);
                    resolve_ptr(&ip, mac, me, *txid, &mut name)
                }
                None => None,
            };
            match name_len {
                Some(w) => {
                    netipc::encode_ok_name(out, name.get(..w).unwrap_or(&[]))
                        .unwrap_or_else(|| fail(out))
                }
                None => fail(out),
            }
        }
        Some(netipc::Request::TcpFetch { ip, port, payload }) => {
            // One-shot TCP transaction into a scratch buffer, then frame the
            // response bytes. Cap the fetch to leave room for the ST_OK byte.
            let mut body = [0u8; MSG_CAP];
            let cap = MSG_CAP - 1;
            let got = match next_hop_mac {
                Some(mac) => {
                    *txid = txid.wrapping_add(1);
                    tcp_fetch(&ip, port, mac, me, *txid, payload, &mut body[..cap])
                }
                None => None,
            };
            match got {
                Some(n) => netipc::encode_ok_bytes(out, body.get(..n).unwrap_or(&[]))
                    .unwrap_or_else(|| fail(out)),
                None => fail(out),
            }
        }
        // Unknown opcode or a structurally invalid request → uniform failure.
        Some(netipc::Request::Unknown(_)) | None => fail(out),
    }
}

/// Read argv[1] into `out`, returning the slice actually populated. Native
/// SlateOS processes fetch argv via `SYS_PROCESS_GET_ARGS` (the stack is bare):
/// a `SpawnArgsHeader` (argc:u32, envc:u32, argv_len:u32, envp_len:u32) followed
/// by packed NUL-terminated argv strings, then envp strings.
fn read_argv1(out: &mut [u8; 32]) -> usize {
    let mut raw = [0u8; 256];
    let n = syscall2(SYS_PROCESS_GET_ARGS, raw.as_mut_ptr() as u64, raw.len() as u64);
    if n < 16 {
        return 0; // No args or header didn't fit.
    }
    let argc = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    if argc < 2 {
        return 0;
    }
    // Walk past argv[0] (first NUL-terminated string) in the packed argv data.
    let mut off = 16usize;
    let end = (n as usize).min(raw.len());
    // Skip argv[0].
    while off < end && raw[off] != 0 {
        off += 1;
    }
    off += 1; // step over the NUL
    // Copy argv[1].
    let mut i = 0;
    while off < end && raw[off] != 0 && i < out.len() {
        out[i] = raw[off];
        off += 1;
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Poll budget: iterations * per-iteration sleep. 400 * 5ms = 2s.
const POLL_ITERS: u32 = 400;
const POLL_SLEEP_NS: u64 = 5_000_000;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    print("[netstack] starting\n");

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

    // Mode selection via argv[1]. Default = Phase-2 ARP round-trip self-test;
    // `serve-dns` = Phase-4 DNS-over-IPC service (register `net.stack`, resolve
    // hostnames for kernel clients, then release the NIC at the idle deadline).
    let mut arg = [0u8; 32];
    let arglen = read_argv1(&mut arg);
    if &arg[..arglen] == b"serve-dns" {
        print("[netstack] mode: serve-dns (Phase 4 DNS-over-IPC)\n");
        let code = run_dns_service(&me);
        raw_close();
        print("[netstack] released raw NIC\n");
        exit(code);
    }

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
