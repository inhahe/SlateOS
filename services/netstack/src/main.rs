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
const SYS_SHM_MAP: u64 = 233;
const SYS_SHM_UNMAP: u64 = 234;
const SYS_SERVICE_REGISTER: u64 = 280;

/// `SYS_SHM_MAP` flags: readable + writable (execute is never granted).
const SHM_MAP_RW: u64 = (1 << 0) | (1 << 1);
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
///
/// A frame destined to *our own IP* never reaches the wire — QEMU's slirp (and a
/// real switch) would drop a packet a host sends to itself — so we divert it into
/// the in-process [`loopback`] RX queue instead. That lets the daemon speak to its
/// own listening sockets: a client connection opened to `me.ip:port` reaches a
/// [`Listener`] bound on that port entirely within the daemon (the server-socket
/// self-test, and genuine intra-host connections). Diversion is purely by
/// destination IP, so the (otherwise gateway-shaped) Ethernet/next-hop MAC is
/// irrelevant for loopback frames. Non-loopback frames go out the NIC unchanged.
fn raw_tx(frame: &[u8]) -> i64 {
    if loopback::maybe_divert(frame) {
        return frame.len() as i64;
    }
    syscall2(SYS_NET_RAW_TX, frame.as_ptr() as u64, frame.len() as u64)
}

/// Receive one raw Ethernet frame into `buf`. Returns bytes read, or a
/// negative errno (`E_WOULD_BLOCK` when no frame is queued).
///
/// Any queued [`loopback`] frame is delivered first (ahead of the NIC) so a
/// self-connection makes progress; only when the loopback queue is empty do we
/// read the physical NIC.
fn raw_rx(buf: &mut [u8]) -> i64 {
    if let Some(n) = loopback::maybe_deliver(buf) {
        return n;
    }
    syscall2(SYS_NET_RAW_RX, buf.as_mut_ptr() as u64, buf.len() as u64)
}

/// In-process software loopback for frames the daemon sends to *its own IP*.
///
/// The daemon is otherwise a pure NIC client: everything it emits goes out the
/// wire and it never receives a frame it sent. Server sockets break that
/// assumption — to test (and to support) a connection to `me.ip:port` served by a
/// [`Listener`] in the *same* daemon, the SYN/data/ACK/FIN frames must circulate
/// internally rather than hit slirp (which silently drops a host-to-self packet).
///
/// [`raw_tx`] calls [`maybe_divert`](loopback::maybe_divert): if the outgoing
/// frame is IPv4 with `dst == my_ip` (and `my_ip` has been set), the whole frame
/// is copied into a small FIFO and *not* sent on the NIC. [`raw_rx`] calls
/// [`maybe_deliver`](loopback::maybe_deliver) first, draining that FIFO ahead of
/// the physical NIC. The demux downstream matches by IP + 4-tuple, so both ends of
/// a loopback connection coexist in the one daemon without collision.
///
/// Single-threaded daemon → a plain `static mut` guarded by a "not reentrant"
/// discipline (we never hold a `&`/`&mut` across a `raw_tx`/`raw_rx`) is sound; the
/// accessors below encapsulate every `unsafe`.
mod loopback {
    use super::MAX_FRAME;

    /// FIFO depth. A single request/response exchange queues only a handful of
    /// frames at a time (SYN, SYN-ACK, ACK, data, FIN…); 32 is ample headroom.
    const QCAP: usize = 32;

    struct State {
        /// Our IP; loopback is inert until this is set non-zero at startup.
        my_ip: [u8; 4],
        /// Our IPv6 address (link-local); loopback ignores v6 until this is set.
        my_ip6: [u8; 16],
        frames: [[u8; MAX_FRAME]; QCAP],
        lens: [u16; QCAP],
        head: usize,
        len: usize,
    }

    static mut STATE: State = State {
        my_ip: [0, 0, 0, 0],
        my_ip6: [0u8; 16],
        frames: [[0u8; MAX_FRAME]; QCAP],
        lens: [0u16; QCAP],
        head: 0,
        len: 0,
    };

    /// Record our IP so loopback can recognise self-addressed frames. Called once
    /// at startup before any `raw_tx`/`raw_rx`.
    pub fn set_my_ip(ip: [u8; 4]) {
        // SAFETY: single-threaded daemon; no borrow of STATE is live across this
        // call, and this runs at startup before the serve loop begins.
        unsafe {
            STATE.my_ip = ip;
        }
    }

    /// Record our IPv6 address so loopback recognises self-addressed IPv6 frames.
    /// Called once at startup alongside [`set_my_ip`].
    pub fn set_my_ip6(ip6: [u8; 16]) {
        // SAFETY: single-threaded daemon; no live borrow of STATE spans this call,
        // and this runs at startup before the serve loop begins.
        unsafe {
            STATE.my_ip6 = ip6;
        }
    }

    /// If `frame` is IPv4/IPv6 addressed to our own IP, copy it into the loopback
    /// FIFO and return `true` (the caller must not also send it on the NIC).
    /// Returns `false` (frame goes to the wire) for any non-self-addressed frame,
    /// a full FIFO, or before the corresponding `set_my_ip*`.
    pub fn maybe_divert(frame: &[u8]) -> bool {
        // SAFETY: single-threaded; no live borrow of STATE spans this call.
        let st = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
        if frame.len() < 14 {
            return false;
        }
        let self_addressed = if frame[12] == 0x08 && frame[13] == 0x00 {
            // IPv4 (ethertype 0x0800): dst address sits at offset 30 (14-byte
            // Ethernet header + 16 into the IPv4 header). Avoid a full parse on
            // the TX hot path.
            st.my_ip != [0, 0, 0, 0] && frame.len() >= 34 && frame[30..34] == st.my_ip
        } else if frame[12] == 0x86 && frame[13] == 0xDD {
            // IPv6 (ethertype 0x86DD): dst address sits at offset 38 (14-byte
            // Ethernet header + 24 into the fixed 40-byte IPv6 header).
            st.my_ip6 != [0u8; 16] && frame.len() >= 54 && frame[38..54] == st.my_ip6
        } else {
            false
        };
        if !self_addressed {
            return false; // not addressed to us
        }
        if st.len >= QCAP || frame.len() > MAX_FRAME {
            return false; // FIFO full / oversized → let it hit the wire (dropped)
        }
        let tail = (st.head + st.len) % QCAP;
        st.frames[tail][..frame.len()].copy_from_slice(frame);
        st.lens[tail] = frame.len() as u16;
        st.len += 1;
        true
    }

    /// If a loopback frame is queued, copy it into `buf` and return its length;
    /// otherwise `None` (the caller reads the physical NIC).
    pub fn maybe_deliver(buf: &mut [u8]) -> Option<i64> {
        // SAFETY: single-threaded; no live borrow of STATE spans this call.
        let st = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
        if st.len == 0 {
            return None;
        }
        let idx = st.head;
        let n = st.lens[idx] as usize;
        if n > buf.len() {
            // Caller's buffer too small: drop this frame rather than truncate.
            st.head = (st.head + 1) % QCAP;
            st.len -= 1;
            return Some(super::E_WOULD_BLOCK);
        }
        buf[..n].copy_from_slice(&st.frames[idx][..n]);
        st.head = (st.head + 1) % QCAP;
        st.len -= 1;
        Some(n as i64)
    }
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
    /// Our IPv6 link-local address (`fe80::/64` + EUI-64 from `mac`), derived at
    /// startup. The daemon has no IPv6 configuration syscall yet, so the
    /// link-local — which is deterministically formed from the MAC and needs no
    /// router — is the source identity for daemon-originated IPv6 TCP (and the
    /// address the IPv6 loopback self-test connects to). SLAAC/DHCPv6 global
    /// addressing is a later increment.
    ip6: [u8; 16],
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
    let mac = [rec[16], rec[17], rec[18], rec[19], rec[20], rec[21]];
    Some(IfInfo {
        ip: [rec[0], rec[1], rec[2], rec[3]],
        mask: [rec[4], rec[5], rec[6], rec[7]],
        gateway: [rec[8], rec[9], rec[10], rec[11]],
        dns: [rec[12], rec[13], rec[14], rec[15]],
        mac,
        ip6: icmpv6::link_local_from_mac(&mac),
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

use netproto::{arp, dns, ethernet, icmp, icmpv6, ipv4, ipv6, tcp, udp};

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

/// Resolve `target_ip6`'s link-layer address via IPv6 Neighbor Discovery
/// (RFC 4861): multicast a Neighbor Solicitation to the target's solicited-node
/// group and wait for the matching Neighbor Advertisement. The IPv6 sibling of
/// [`arp_resolve`]. Returns `None` if no advertisement arrives in the poll budget.
///
/// Note: this is only exercised for *real* on-link IPv6 peers. The daemon's own
/// loopback path ([`loopback::maybe_divert`]) short-circuits self-addressed
/// frames before they hit the wire, so an IPv6 connect to `me.ip6` never calls
/// this (its next-hop MAC is irrelevant). There is no IPv6 router in the QEMU
/// slirp test environment, so the NDP round-trip itself is not end-to-end tested
/// here; the NS/NA wire formats are unit-tested in `netproto::icmpv6`.
fn ndp_resolve(target_ip6: &[u8; 16], me: &IfInfo) -> Option<[u8; 6]> {
    let sol = icmpv6::solicited_node_multicast(target_ip6);
    // Ethernet destination for an IPv6 solicited-node multicast: 33:33 followed
    // by the low 32 bits of the multicast address (RFC 2464 §7).
    let eth_dst = [0x33, 0x33, sol[12], sol[13], sol[14], sol[15]];

    let mut frame = [0u8; MAX_FRAME];
    let l4_off = ethernet::HEADER_LEN + ipv6::HEADER_LEN;
    let mut ns = [0u8; 32];
    let ns_len = icmpv6::write_neighbor_solicitation(&mut ns, &me.ip6, &sol, target_ip6, &me.mac)?;
    let total = l4_off.checked_add(ns_len)?;
    if total > frame.len() {
        return None;
    }
    frame[l4_off..total].copy_from_slice(&ns[..ns_len]);
    let ip_hdr = ipv6::Builder {
        traffic_class: 0,
        flow_label: 0,
        next_header: icmpv6::NH_ICMPV6,
        hop_limit: 255, // NDP requires a hop limit of 255 (RFC 4861 §7.1.1).
        src: me.ip6,
        dst: sol,
    }
    .build_header(ns_len as u16);
    frame[ethernet::HEADER_LEN..l4_off].copy_from_slice(&ip_hdr);
    ethernet::write_header(&mut frame, &eth_dst, &me.mac, ethernet::ETHERTYPE_IPV6);
    if raw_tx(&frame[..total]) < 0 {
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
            if let Some(mac) = ndp_advert_mac(&buf[..len], target_ip6) {
                return Some(mac);
            }
        }
        sleep_ns(POLL_SLEEP_NS);
    }
    None
}

/// If `frame` is a Neighbor Advertisement for `target_ip6` carrying a Target
/// Link-Layer Address option, return the advertised MAC.
fn ndp_advert_mac(frame: &[u8], target_ip6: &[u8; 16]) -> Option<[u8; 6]> {
    let eth = ethernet::Frame::parse(frame)?;
    if eth.ethertype != ethernet::ETHERTYPE_IPV6 {
        return None;
    }
    let ip = ipv6::Packet::parse(eth.payload)?;
    if ip.next_header != icmpv6::NH_ICMPV6 {
        return None;
    }
    let na = icmpv6::parse_neighbor_advertisement(ip.payload, &ip.src, &ip.dst)?;
    if na.target == *target_ip6 { na.link_addr } else { None }
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
            if let Some(msg) = dns_response_msg(&buf[..len], me, txid)
                && let Some(w) = msg.first_ptr(out)
                && w > 0
            {
                return Some(w);
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

/// Send buffer: the most-recently-sent segment is retained so the data phase can
/// retransmit it if nothing comes back. One MSS-ish segment is enough for the
/// request/response transactions this client serves.
const TCP_SND_BUF: usize = 1024;

/// Per-connection receive buffer: in-order stream bytes are accumulated here as
/// segments arrive and are drained on `OP_RECV`/`recv`. Sized to hold a response
/// window comfortably (the ring recv windows are <= `MSG_CAP`). This buffer is
/// what lets the shared RX pump route a sibling connection's inbound frames into
/// *its* connection while another connection is the one blocked in `recv` — the
/// data waits here instead of being dropped.
const TCP_RCV_BUF: usize = 1024;

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

/// Frame an arbitrary L4 payload as Ethernet | IPv6 toward `dst_ip6` and transmit
/// it on the raw NIC. The IPv6 sibling of [`send_ipv4`]; the source address is the
/// daemon's link-local identity (`me.ip6`). Returns `true` on a successful TX.
fn send_ipv6(
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    dst_ip6: &[u8; 16],
    next_header: u8,
    l4: &[u8],
) -> bool {
    let mut frame = [0u8; MAX_FRAME];
    let l4_off = ethernet::HEADER_LEN + ipv6::HEADER_LEN;
    let total = match l4_off.checked_add(l4.len()) {
        Some(t) if t <= frame.len() => t,
        _ => return false,
    };
    frame[l4_off..total].copy_from_slice(l4);
    let ip_hdr = ipv6::Builder {
        traffic_class: 0,
        flow_label: 0,
        next_header,
        hop_limit: 64,
        src: me.ip6,
        dst: *dst_ip6,
    }
    .build_header(l4.len() as u16);
    frame[ethernet::HEADER_LEN..l4_off].copy_from_slice(&ip_hdr);
    ethernet::write_header(&mut frame, next_hop_mac, &me.mac, ethernet::ETHERTYPE_IPV6);
    raw_tx(&frame[..total]) >= 0
}

/// Build and transmit one TCP segment over IPv6 for our connection. The IPv6
/// sibling of [`send_tcp`]; there is no IPv4 identification counter on the wire.
#[allow(clippy::too_many_arguments)]
fn send_tcp6(
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    dst_ip6: &[u8; 16],
    dst_port: u16,
    src_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> bool {
    // TCP segment sits inside the IPv6 payload; bound it by the framing headroom.
    let mut seg = [0u8; MAX_FRAME - (ethernet::HEADER_LEN + ipv6::HEADER_LEN)];
    let builder = tcp::Builder { src_port, dst_port, seq, ack, flags, window: TCP_WINDOW };
    let n = match builder.write_v6(&mut seg, &me.ip6, dst_ip6, payload) {
        Some(n) => n,
        None => return false,
    };
    send_ipv6(me, next_hop_mac, dst_ip6, ipv4::PROTO_TCP, &seg[..n])
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

/// The IPv6 sibling of [`recv_tcp_seg`]: receive one TCP segment carried in an
/// IPv6 datagram from `dst_ip6`, matching our local/remote ports and endpoint
/// IPs. On a match, copies the payload into `pl` and returns the metadata.
fn recv_tcp_seg6(
    me: &IfInfo,
    dst_ip6: &[u8; 16],
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
    if eth.ethertype != ethernet::ETHERTYPE_IPV6 {
        return None;
    }
    let ip = ipv6::Packet::parse(eth.payload)?;
    if ip.next_header != ipv4::PROTO_TCP || ip.dst != me.ip6 || ip.src != *dst_ip6 {
        return None;
    }
    let seg = tcp::Segment::parse_v6(ip.payload, &ip.src, &ip.dst)?;
    if seg.src_port != remote_port || seg.dst_port != local_port {
        return None;
    }
    let cplen = seg.payload.len().min(pl.len());
    pl[..cplen].copy_from_slice(&seg.payload[..cplen]);
    Some(TcpRx { seq: seg.seq, ack: seg.ack, flags: seg.flags, payload_len: cplen })
}

/// Outcome of one non-filtered NIC read for the multiplexed RX pump.
enum RawRx {
    /// No frame available (`WOULD_BLOCK`) or a read error — draining is done.
    None,
    /// A frame arrived but is not a TCP segment addressed to us — skip it, but
    /// keep draining (there may be more).
    Ignore,
    /// A TCP segment addressed to us: peer identity `(src_ip, src_port, dst_port)`
    /// for routing to the owning connection, plus the parsed segment metadata. The
    /// payload has been copied into the caller's `pl` buffer (`payload_len` bytes).
    Seg([u8; 4], u16, u16, TcpRx),
    /// An IPv6 TCP segment addressed to us: peer identity `(src_ip6, src_port,
    /// dst_port)` plus the parsed segment metadata. The IPv6 sibling of [`Seg`].
    Seg6([u8; 16], u16, u16, TcpRx),
}

/// Read one frame off the NIC without filtering to a specific connection's
/// 4-tuple, so the caller can *route* it to whichever connection owns it. This is
/// the shared-RX-demux counterpart to [`recv_tcp_seg`]: where that function drops
/// any frame not matching one connection, this one hands back the peer identity so
/// a sibling connection's frames are delivered to *it* instead of being lost.
fn recv_tcp_any(me: &IfInfo, frame: &mut [u8], pl: &mut [u8]) -> RawRx {
    let n = raw_rx(frame);
    if n < 0 {
        return RawRx::None; // WOULD_BLOCK or error → drain complete.
    }
    let len = n as usize;
    if len > frame.len() {
        return RawRx::Ignore;
    }
    let eth = match ethernet::Frame::parse(frame.get(..len).unwrap_or(&[])) {
        Some(eth) => eth,
        None => return RawRx::Ignore,
    };
    match eth.ethertype {
        ethernet::ETHERTYPE_IPV4 => {
            let parsed = (|| {
                let ip = ipv4::Packet::parse(eth.payload)?;
                if ip.protocol != ipv4::PROTO_TCP || ip.dst != me.ip {
                    return None;
                }
                let seg = tcp::Segment::parse(ip.payload, &ip.src, &ip.dst)?;
                let cplen = seg.payload.len().min(pl.len());
                pl.get_mut(..cplen)?.copy_from_slice(seg.payload.get(..cplen)?);
                Some((
                    ip.src,
                    seg.src_port,
                    seg.dst_port,
                    TcpRx { seq: seg.seq, ack: seg.ack, flags: seg.flags, payload_len: cplen },
                ))
            })();
            match parsed {
                Some((src_ip, src_port, dst_port, rx)) => RawRx::Seg(src_ip, src_port, dst_port, rx),
                None => RawRx::Ignore,
            }
        }
        ethernet::ETHERTYPE_IPV6 => {
            let parsed = (|| {
                let ip = ipv6::Packet::parse(eth.payload)?;
                if ip.next_header != ipv4::PROTO_TCP || ip.dst != me.ip6 {
                    return None;
                }
                let seg = tcp::Segment::parse_v6(ip.payload, &ip.src, &ip.dst)?;
                let cplen = seg.payload.len().min(pl.len());
                pl.get_mut(..cplen)?.copy_from_slice(seg.payload.get(..cplen)?);
                Some((
                    ip.src,
                    seg.src_port,
                    seg.dst_port,
                    TcpRx { seq: seg.seq, ack: seg.ack, flags: seg.flags, payload_len: cplen },
                ))
            })();
            match parsed {
                Some((src_ip6, src_port, dst_port, rx)) => {
                    RawRx::Seg6(src_ip6, src_port, dst_port, rx)
                }
                None => RawRx::Ignore,
            }
        }
        _ => RawRx::Ignore,
    }
}

/// A single live TCP client connection. Holds the sequence-number and IP-ident
/// state that must persist across the connect / send / recv / close phases, so
/// the same state machine backs both the one-shot [`tcp_fetch`] control op and
/// the ring-driven socket opcodes (`OP_CONNECT`/`OP_SEND`/`OP_RECV`/`OP_CLOSE`)
/// — one TCP implementation, no duplication.
///
/// This is a deliberately minimal client: one outstanding (retransmittable)
/// segment, cumulative ACK, graceful FIN teardown. It does not implement
/// congestion control, window scaling, or SACK — it exists to drive the
/// request/response transactions the Phase-4 self-tests exercise.
struct TcpConn {
    dst_ip: [u8; 4],
    /// Peer IPv6 address when this is an IPv6 connection; `None` for IPv4. All
    /// segment emission and RX matching dispatch on this: `Some` frames via
    /// [`send_tcp6`] and matches on `me.ip6`, `None` uses the IPv4 path. Keeping
    /// one state machine (rather than a parallel `TcpConn6`) means the handshake,
    /// send/ACK, and teardown logic are shared byte-for-byte across families.
    dst6: Option<[u8; 16]>,
    dst_port: u16,
    local_port: u16,
    /// Next-hop L2 address (gateway or on-link peer) for outbound frames.
    mac: [u8; 6],
    /// Next sequence number we will send (advances past sent data).
    snd_nxt: u32,
    /// Next sequence number we expect to receive (cumulative ACK point).
    rcv_nxt: u32,
    /// Peer's initial sequence number; `rcv_nxt == server_isn + 1` means we have
    /// not yet accepted any in-order data (gates the early retransmit).
    server_isn: u32,
    /// IPv4 identification counter (incremented per emitted datagram).
    ipid: u16,
    /// Our initial sequence number (the SYN's seq). Retained so a non-blocking
    /// connect's handshake can validate the SYN-ACK's `ack` (`== isn + 1`) when it
    /// completes later, in [`ingest_seg`](Self::ingest_seg), rather than inline.
    isn: u32,
    /// `true` once the TCP handshake has completed (ESTABLISHED). A synchronous
    /// `connect` returns already-established; a non-blocking connect starts in
    /// `SYN_SENT` (`false`) and flips to `true` when the SYN-ACK is processed.
    established: bool,
    /// `true` if the connect attempt failed after `connect_start` (a RST, or the
    /// SYN retransmit budget was exhausted with no SYN-ACK). Surfaced to the kernel
    /// as a `POLL_ERR` readiness bit → `getsockopt(SO_ERROR) = ECONNREFUSED`.
    connect_failed: bool,
    /// Number of SYNs transmitted so far (non-blocking handshake). Bounds the
    /// per-poll SYN retransmit at [`TCP_SYN_ATTEMPTS`].
    syn_sends: u32,
    /// Set once the peer's FIN (or an RST) has ended the receive stream.
    peer_fin: bool,
    /// The most recently sent segment, retained for retransmission.
    snd_buf: [u8; TCP_SND_BUF],
    /// Valid length of `snd_buf` (0 = nothing buffered to retransmit).
    snd_buf_len: usize,
    /// In-order received stream bytes awaiting delivery to the caller. Filled by
    /// [`ingest_seg`](Self::ingest_seg) (whether this connection is the one in
    /// `recv` or a sibling routed here by the shared RX pump) and drained by
    /// [`take_rx`](Self::take_rx).
    rx_buf: [u8; TCP_RCV_BUF],
    /// Valid length of `rx_buf`.
    rx_len: usize,
    /// `true` for a **passively-opened** connection (born from an inbound SYN to a
    /// [`Listener`], SYN_RCVD → ESTABLISHED via the peer's ACK of our SYN-ACK)
    /// rather than an active `connect`. It changes only the handshake-completion
    /// arm in [`ingest_seg`](Self::ingest_seg): a passive open completes on a bare
    /// ACK (`ack == isn+1`, `isn` = our SYN-ACK's seq) and emits nothing, whereas
    /// an active open completes on a SYN-ACK and emits the final ACK.
    passive: bool,
    /// `true` once `shutdown(SHUT_WR)`/`SHUT_RDWR` has closed the write side: our
    /// FIN has been emitted and subsequent [`OP_SEND`] submissions are rejected
    /// with [`ERR_BROKEN_PIPE`](netipc::ring::ERR_BROKEN_PIPE). The read side stays
    /// open, so `recv` keeps delivering buffered/inbound bytes until the peer FIN.
    write_shut: bool,
    /// `true` once `shutdown(SHUT_RD)`/`SHUT_RDWR` has closed the read side:
    /// subsequent [`OP_RECV`] completes with EOF (`0`) without waiting. The write
    /// side stays open, so `send` still works until [`OP_CLOSE`].
    read_shut: bool,
}

impl TcpConn {
    /// Transmit one TCP segment for this connection, dispatching on the address
    /// family recorded at open time. An IPv6 connection (`dst6 == Some`) frames
    /// via [`send_tcp6`] using the daemon's link-local source; an IPv4 connection
    /// frames via [`send_tcp`] using `self.ipid` for the on-wire IP identification.
    /// Callers advance `self.ipid` before calling (kept identical to the former
    /// inline `send_tcp` calls so the IPv4 wire behaviour is unchanged).
    fn emit(&self, me: &IfInfo, seq: u32, ack: u32, flags: u8, payload: &[u8]) -> bool {
        match self.dst6 {
            Some(ref dst6) => send_tcp6(
                me,
                &self.mac,
                dst6,
                self.dst_port,
                self.local_port,
                seq,
                ack,
                flags,
                payload,
            ),
            None => send_tcp(
                me,
                &self.mac,
                &self.dst_ip,
                self.dst_port,
                self.local_port,
                seq,
                ack,
                flags,
                payload,
                self.ipid,
            ),
        }
    }

    /// Receive one TCP segment addressed to this connection, dispatching on the
    /// address family. The single-connection RX counterpart to [`emit`]; used by
    /// [`recv`](Self::recv) and [`close`](Self::close), which each service exactly
    /// one connection (the multiplexed ring path uses [`recv_tcp_any`] instead).
    fn recv_one_seg(&self, me: &IfInfo, frame: &mut [u8], pl: &mut [u8]) -> Option<TcpRx> {
        match self.dst6 {
            Some(ref dst6) => {
                recv_tcp_seg6(me, dst6, self.local_port, self.dst_port, frame, pl)
            }
            None => recv_tcp_seg(me, &self.dst_ip, self.local_port, self.dst_port, frame, pl),
        }
    }

    /// Open a connection to `dst_ip:dst_port` via `mac`, seeding the IPv4 ident
    /// counter from `seed_ipid`. Performs the SYN / SYN-ACK / ACK handshake.
    /// Returns `None` if the peer refused (RST) or never answered.
    fn connect(
        me: &IfInfo,
        dst_ip: [u8; 4],
        dst_port: u16,
        mac: [u8; 6],
        seed_ipid: u16,
    ) -> Option<Self> {
        // Rotate the ephemeral local port (and initial sequence number) per
        // connection so successive connections to the *same* server do not reuse
        // an identical 4-tuple. A server that still holds the prior 4-tuple in
        // TIME_WAIT would otherwise treat the new SYN as a stale duplicate and
        // drop/challenge it — which silently broke back-to-back fetches. Keep the
        // port in the ephemeral range 0xC000..=0xFFFF.
        let local_port = EPHEMERAL_PORT | (seed_ipid & 0x3FFF);
        let isn: u32 = 0x0001_0000u32.wrapping_add((seed_ipid as u32) << 8);
        let mut ipid = seed_ipid;

        let mut frame = [0u8; MAX_FRAME];
        let mut pl = [0u8; MAX_FRAME];

        // --- Handshake: (re)transmit SYN until a SYN-ACK arrives. ---
        let mut server_isn = 0u32;
        let mut established = false;
        'syn: for _ in 0..TCP_SYN_ATTEMPTS {
            ipid = ipid.wrapping_add(1);
            if !send_tcp(me, &mac, &dst_ip, dst_port, local_port, isn, 0, tcp::FLAG_SYN, &[], ipid) {
                return None;
            }
            for _ in 0..RESOLVE_POLL_ITERS {
                while let Some(rx) = recv_tcp_seg(me, &dst_ip, local_port, dst_port, &mut frame, &mut pl) {
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
        let rcv_nxt = server_isn.wrapping_add(1); // Their SYN consumed one.

        // ACK the SYN-ACK, completing the handshake.
        ipid = ipid.wrapping_add(1);
        send_tcp(me, &mac, &dst_ip, dst_port, local_port, snd_nxt, rcv_nxt, tcp::FLAG_ACK, &[], ipid);

        Some(TcpConn {
            dst_ip,
            dst6: None,
            dst_port,
            local_port,
            mac,
            snd_nxt,
            rcv_nxt,
            server_isn,
            ipid,
            isn,
            established: true,
            connect_failed: false,
            syn_sends: 0,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: false,
            write_shut: false,
            read_shut: false,
        })
    }

    /// Start a **non-blocking** connect: transmit the SYN and return a connection
    /// in the `SYN_SENT` state (`established == false`) *without* waiting for the
    /// SYN-ACK. The handshake is completed later by [`ingest_seg`](Self::ingest_seg)
    /// when the SYN-ACK arrives (driven by the RX pump on a subsequent `OP_POLL` /
    /// `OP_RECV`), and [`poll_connect`](Self::poll_connect) retransmits the SYN a
    /// bounded number of times if it is lost. Returns `None` only if the SYN could
    /// not be transmitted at all.
    fn connect_start(
        me: &IfInfo,
        dst_ip: [u8; 4],
        dst_port: u16,
        mac: [u8; 6],
        seed_ipid: u16,
    ) -> Option<Self> {
        // Same ephemeral-port / ISN derivation as the synchronous path so a
        // non-blocking connect to a server recently torn down still avoids a
        // TIME_WAIT 4-tuple clash.
        let local_port = EPHEMERAL_PORT | (seed_ipid & 0x3FFF);
        let isn: u32 = 0x0001_0000u32.wrapping_add((seed_ipid as u32) << 8);
        let ipid = seed_ipid.wrapping_add(1);
        if !send_tcp(me, &mac, &dst_ip, dst_port, local_port, isn, 0, tcp::FLAG_SYN, &[], ipid) {
            return None;
        }
        Some(TcpConn {
            dst_ip,
            dst6: None,
            dst_port,
            local_port,
            mac,
            // Provisional; recomputed from `isn` when the handshake completes.
            snd_nxt: isn.wrapping_add(1),
            rcv_nxt: 0,
            server_isn: 0,
            ipid,
            isn,
            established: false,
            connect_failed: false,
            syn_sends: 1,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: false,
            write_shut: false,
            read_shut: false,
        })
    }

    /// Passively open a connection from an inbound SYN to a [`Listener`]: choose our
    /// ISN, transmit the SYN-ACK, and return a connection in the SYN_RCVD state
    /// (`established == false`, `passive == true`). The handshake completes in
    /// [`ingest_seg`](Self::ingest_seg) when the peer's ACK of our SYN-ACK arrives.
    /// `peer_ip`/`peer_port` identify the remote end (become `dst_ip`/`dst_port`),
    /// `local_port` is the listening port, `peer_isn` is the SYN's sequence number,
    /// and `mac` is the next-hop L2 address for our replies (irrelevant for a
    /// loopback peer, which is demuxed by IP). Returns `None` if the SYN-ACK could
    /// not be transmitted.
    #[allow(clippy::too_many_arguments)]
    fn accept_syn(
        me: &IfInfo,
        peer_ip: [u8; 4],
        peer_port: u16,
        local_port: u16,
        peer_isn: u32,
        mac: [u8; 6],
        seed_ipid: u16,
    ) -> Option<Self> {
        let isn: u32 = 0x0002_0000u32.wrapping_add((seed_ipid as u32) << 8);
        let mut ipid = seed_ipid;
        let rcv_nxt = peer_isn.wrapping_add(1); // Their SYN consumes one seq.
        // Transmit SYN-ACK (seq = our ISN, ack = peer_isn + 1).
        ipid = ipid.wrapping_add(1);
        if !send_tcp(
            me,
            &mac,
            &peer_ip,
            peer_port,
            local_port,
            isn,
            rcv_nxt,
            tcp::FLAG_SYN | tcp::FLAG_ACK,
            &[],
            ipid,
        ) {
            return None;
        }
        Some(TcpConn {
            dst_ip: peer_ip,
            dst6: None,
            dst_port: peer_port,
            local_port,
            mac,
            snd_nxt: isn.wrapping_add(1), // Our SYN consumed one seq.
            rcv_nxt,
            server_isn: peer_isn,
            ipid,
            isn,
            established: false, // SYN_RCVD until the peer ACKs our SYN-ACK.
            connect_failed: false,
            syn_sends: 1,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: true,
            write_shut: false,
            read_shut: false,
        })
    }

    // --- IPv6 constructors -------------------------------------------------
    //
    // Each is the byte-for-byte analogue of its IPv4 sibling above, differing
    // only in that it frames the handshake segments over IPv6 (`send_tcp6` /
    // `recv_tcp_seg6`) and records `dst6: Some(..)`, `dst_ip: [0;4]`. Once
    // constructed, the connection is driven by the *same* `ingest_seg` / `send`
    // / `close` state machine as an IPv4 connection — `emit`/`recv_one_seg`
    // dispatch on `dst6`, so there is no duplicated data-transfer logic.

    /// Open an IPv6 connection to `dst_ip6:dst_port` via `mac` (blocking full
    /// handshake). The IPv6 sibling of [`connect`](Self::connect).
    fn connect6(
        me: &IfInfo,
        dst_ip6: [u8; 16],
        dst_port: u16,
        mac: [u8; 6],
        seed_ipid: u16,
    ) -> Option<Self> {
        let local_port = EPHEMERAL_PORT | (seed_ipid & 0x3FFF);
        let isn: u32 = 0x0001_0000u32.wrapping_add((seed_ipid as u32) << 8);

        let mut frame = [0u8; MAX_FRAME];
        let mut pl = [0u8; MAX_FRAME];

        let mut server_isn = 0u32;
        let mut established = false;
        'syn: for _ in 0..TCP_SYN_ATTEMPTS {
            if !send_tcp6(me, &mac, &dst_ip6, dst_port, local_port, isn, 0, tcp::FLAG_SYN, &[]) {
                return None;
            }
            for _ in 0..RESOLVE_POLL_ITERS {
                while let Some(rx) =
                    recv_tcp_seg6(me, &dst_ip6, local_port, dst_port, &mut frame, &mut pl)
                {
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

        let snd_nxt = isn.wrapping_add(1);
        let rcv_nxt = server_isn.wrapping_add(1);
        send_tcp6(me, &mac, &dst_ip6, dst_port, local_port, snd_nxt, rcv_nxt, tcp::FLAG_ACK, &[]);

        Some(TcpConn {
            dst_ip: [0; 4],
            dst6: Some(dst_ip6),
            dst_port,
            local_port,
            mac,
            snd_nxt,
            rcv_nxt,
            server_isn,
            ipid: seed_ipid,
            isn,
            established: true,
            connect_failed: false,
            syn_sends: 0,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: false,
            write_shut: false,
            read_shut: false,
        })
    }

    /// Start a **non-blocking** IPv6 connect (transmit the SYN, return in
    /// SYN_SENT). The IPv6 sibling of [`connect_start`](Self::connect_start).
    fn connect_start6(
        me: &IfInfo,
        dst_ip6: [u8; 16],
        dst_port: u16,
        mac: [u8; 6],
        seed_ipid: u16,
    ) -> Option<Self> {
        let local_port = EPHEMERAL_PORT | (seed_ipid & 0x3FFF);
        let isn: u32 = 0x0001_0000u32.wrapping_add((seed_ipid as u32) << 8);
        if !send_tcp6(me, &mac, &dst_ip6, dst_port, local_port, isn, 0, tcp::FLAG_SYN, &[]) {
            return None;
        }
        Some(TcpConn {
            dst_ip: [0; 4],
            dst6: Some(dst_ip6),
            dst_port,
            local_port,
            mac,
            snd_nxt: isn.wrapping_add(1),
            rcv_nxt: 0,
            server_isn: 0,
            ipid: seed_ipid,
            isn,
            established: false,
            connect_failed: false,
            syn_sends: 1,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: false,
            write_shut: false,
            read_shut: false,
        })
    }

    /// Passively open an IPv6 connection from an inbound SYN to a [`Listener`].
    /// The IPv6 sibling of [`accept_syn`](Self::accept_syn).
    #[allow(clippy::too_many_arguments)]
    fn accept_syn6(
        me: &IfInfo,
        peer_ip6: [u8; 16],
        peer_port: u16,
        local_port: u16,
        peer_isn: u32,
        mac: [u8; 6],
        seed_ipid: u16,
    ) -> Option<Self> {
        let isn: u32 = 0x0002_0000u32.wrapping_add((seed_ipid as u32) << 8);
        let rcv_nxt = peer_isn.wrapping_add(1); // Their SYN consumes one seq.
        if !send_tcp6(
            me,
            &mac,
            &peer_ip6,
            peer_port,
            local_port,
            isn,
            rcv_nxt,
            tcp::FLAG_SYN | tcp::FLAG_ACK,
            &[],
        ) {
            return None;
        }
        Some(TcpConn {
            dst_ip: [0; 4],
            dst6: Some(peer_ip6),
            dst_port: peer_port,
            local_port,
            mac,
            snd_nxt: isn.wrapping_add(1),
            rcv_nxt,
            server_isn: peer_isn,
            ipid: seed_ipid,
            isn,
            established: false,
            connect_failed: false,
            syn_sends: 1,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: true,
            write_shut: false,
            read_shut: false,
        })
    }

    /// Advance a non-blocking connect's handshake: if still `SYN_SENT` and the SYN
    /// retransmit budget ([`TCP_SYN_ATTEMPTS`]) is not yet spent, retransmit the
    /// SYN (in case it or the SYN-ACK was lost); if the budget is exhausted without
    /// a SYN-ACK, mark the attempt failed. The actual handshake *completion* (on a
    /// received SYN-ACK) happens in [`ingest_seg`](Self::ingest_seg); this is only
    /// the retransmit/timeout driver, called once per `OP_POLL`. No-op once the
    /// connection is established or already failed.
    fn poll_connect(&mut self, me: &IfInfo) {
        if self.established || self.connect_failed {
            return;
        }
        if self.syn_sends >= TCP_SYN_ATTEMPTS {
            // Exhausted the SYN budget with no SYN-ACK → treat as refused/timed out.
            self.connect_failed = true;
            return;
        }
        self.ipid = self.ipid.wrapping_add(1);
        self.syn_sends = self.syn_sends.saturating_add(1);
        self.emit(me, self.isn, 0, tcp::FLAG_SYN, &[]);
    }

    /// True when the send window has no room: a previously-sent segment is still
    /// unacknowledged (`snd_buf_len > 0`). Our sender keeps at most one segment in
    /// flight, so this is the "window full" condition that gates a further send —
    /// a non-blocking send returns EAGAIN, a blocking one waits for the peer's ACK
    /// (which [`ingest_seg`](Self::ingest_seg) clears the window on).
    fn send_window_full(&self) -> bool {
        self.snd_buf_len > 0
    }

    /// Send `payload` as a single PSH|ACK segment and advance `snd_nxt`. The
    /// segment is buffered so a retransmit can resend it. An empty payload is a
    /// no-op (returns `Some(0)`); a payload larger than the send buffer, or a send
    /// while the window is full (a prior segment still unacknowledged), is rejected
    /// (`None`) — the caller must not overwrite the outstanding retransmit buffer,
    /// or a lost segment could never be recovered.
    fn send(&mut self, me: &IfInfo, payload: &[u8]) -> Option<usize> {
        if payload.is_empty() {
            return Some(0);
        }
        if payload.len() > TCP_SND_BUF {
            return None; // Beyond our single-segment send buffer.
        }
        if self.send_window_full() {
            return None; // Window occupied by an unacked segment; caller must wait/EAGAIN.
        }
        self.ipid = self.ipid.wrapping_add(1);
        if !self.emit(me, self.snd_nxt, self.rcv_nxt, tcp::FLAG_PSH | tcp::FLAG_ACK, payload) {
            return None;
        }
        // Retain for retransmit; `snd_nxt - snd_buf_len` recovers this seq.
        self.snd_buf[..payload.len()].copy_from_slice(payload);
        self.snd_buf_len = payload.len();
        self.snd_nxt = self.snd_nxt.wrapping_add(payload.len() as u32);
        Some(payload.len())
    }

    /// Ingest one parsed TCP segment into this connection's receive state: buffer
    /// in-order payload into `rx_buf`, advance the cumulative ACK point, honor a
    /// FIN or RST, and emit an ACK as needed. This is the single point of TCP
    /// receive logic, shared by the one-connection [`recv`](Self::recv) loop and
    /// the multiplexed [`ring_pump`] — so a segment routed here by the shared RX
    /// demux is processed identically whether or not this connection is the one
    /// currently blocked in a receive. `payload` is the segment's data bytes
    /// (length equals `rx.payload_len`).
    fn ingest_seg(&mut self, me: &IfInfo, rx: &TcpRx, payload: &[u8]) {
        if !self.established {
            if self.connect_failed {
                return;
            }
            if rx.flags & tcp::FLAG_RST != 0 {
                self.connect_failed = true; // Refused → surfaces as POLL_ERR.
                return;
            }
            if self.passive {
                // SYN_RCVD (passive open): we already sent the SYN-ACK. A duplicate
                // SYN (our SYN-ACK was lost) → resend it. The handshake completes on
                // the peer's ACK of our SYN-ACK (`ack == isn+1`); on that ACK we flip
                // to ESTABLISHED and *fall through* so any data/FIN piggybacked on
                // the completing segment is not lost.
                if rx.flags & tcp::FLAG_SYN != 0 && rx.flags & tcp::FLAG_ACK == 0 {
                    self.ipid = self.ipid.wrapping_add(1);
                    self.emit(me, self.isn, self.rcv_nxt, tcp::FLAG_SYN | tcp::FLAG_ACK, &[]);
                    return;
                }
                if rx.flags & tcp::FLAG_ACK != 0 && rx.ack == self.isn.wrapping_add(1) {
                    self.established = true;
                    // snd_nxt / rcv_nxt were set when we sent the SYN-ACK.
                    // Fall through to the established data-handling below.
                } else {
                    return; // Not the completing ACK; nothing to buffer yet.
                }
            } else {
                // SYN_SENT (active open): complete on a matching SYN-ACK, emitting
                // the final ACK. Ignore anything else while handshaking.
                if rx.flags & tcp::FLAG_SYN != 0
                    && rx.flags & tcp::FLAG_ACK != 0
                    && rx.ack == self.isn.wrapping_add(1)
                {
                    self.server_isn = rx.seq;
                    self.snd_nxt = self.isn.wrapping_add(1); // Our SYN consumed one seq.
                    self.rcv_nxt = rx.seq.wrapping_add(1); // Their SYN consumed one.
                    self.ipid = self.ipid.wrapping_add(1);
                    self.emit(me, self.snd_nxt, self.rcv_nxt, tcp::FLAG_ACK, &[]);
                    self.established = true;
                }
                return; // While SYN_SENT we never buffer data.
            }
        }
        if self.peer_fin {
            return; // Stream already ended; ignore stragglers.
        }
        if rx.flags & tcp::FLAG_RST != 0 {
            self.peer_fin = true; // Treat a reset as end-of-stream.
            return;
        }
        // Process the peer's cumulative ACK of our outstanding data. Our sender
        // keeps at most one unacknowledged segment in flight; when the peer's ACK
        // reaches `snd_nxt` it has accepted that whole segment, so we release the
        // retransmit buffer and re-open the send window (`snd_buf_len == 0`). This
        // is what lets a full-window send drain and a non-blocking send stop
        // returning EAGAIN. A stale/duplicate ACK carries `rx.ack < snd_nxt`, so it
        // never spuriously clears the window.
        if self.snd_buf_len > 0 && rx.flags & tcp::FLAG_ACK != 0 && rx.ack == self.snd_nxt {
            self.snd_buf_len = 0;
        }
        if rx.seq == self.rcv_nxt {
            // In-order segment: buffer any payload (up to capacity), then any FIN.
            if rx.payload_len > 0 {
                let room = TCP_RCV_BUF.saturating_sub(self.rx_len);
                let take = payload.len().min(room);
                self.rx_buf[self.rx_len..self.rx_len + take].copy_from_slice(&payload[..take]);
                self.rx_len += take;
                // Advance past the whole in-order segment even if our buffer was
                // full (matches the prior client's cumulative-ACK behaviour: the
                // overflow is ACKed and dropped rather than stalling the peer).
                self.rcv_nxt = self.rcv_nxt.wrapping_add(rx.payload_len as u32);
            }
            if rx.flags & tcp::FLAG_FIN != 0 {
                self.rcv_nxt = self.rcv_nxt.wrapping_add(1); // FIN occupies one seq.
                self.peer_fin = true;
            }
            if rx.payload_len > 0 || self.peer_fin {
                self.ipid = self.ipid.wrapping_add(1);
                self.emit(me, self.snd_nxt, self.rcv_nxt, tcp::FLAG_ACK, &[]);
            }
        } else if rx.payload_len > 0 {
            // Out-of-order data: dup-ACK to prompt the peer to retransmit.
            self.ipid = self.ipid.wrapping_add(1);
            self.emit(me, self.snd_nxt, self.rcv_nxt, tcp::FLAG_ACK, &[]);
        }
    }

    /// Drain up to `out.len()` buffered in-order bytes into `out`, shifting any
    /// remainder to the front of `rx_buf`. Returns the number of bytes delivered.
    fn take_rx(&mut self, out: &mut [u8]) -> usize {
        let n = self.rx_len.min(out.len());
        out[..n].copy_from_slice(&self.rx_buf[..n]);
        if n < self.rx_len {
            self.rx_buf.copy_within(n..self.rx_len, 0);
        }
        self.rx_len -= n;
        n
    }

    /// Copy up to `out.len()` buffered in-order bytes into `out` **without**
    /// consuming them (`MSG_PEEK`): `rx_buf`/`rx_len` are left untouched, so the
    /// next [`take_rx`](Self::take_rx) still returns the same bytes. Returns the
    /// number of bytes copied.
    fn peek_rx(&self, out: &mut [u8]) -> usize {
        let n = self.rx_len.min(out.len());
        out[..n].copy_from_slice(&self.rx_buf[..n]);
        n
    }

    /// Early in an idle window (nothing received yet), retransmit the buffered
    /// send segment up to three times in case our request was lost. Returns `true`
    /// if it retransmitted (the caller then resets its idle counter). Shared by
    /// both receive paths.
    fn maybe_retransmit(&mut self, me: &IfInfo, idle: u32, retransmits: &mut u32) -> bool {
        if self.snd_buf_len > 0
            && self.rcv_nxt == self.server_isn.wrapping_add(1)
            && idle == 40
            && *retransmits < 3
        {
            *retransmits += 1;
            self.ipid = self.ipid.wrapping_add(1);
            let seq = self.snd_nxt.wrapping_sub(self.snd_buf_len as u32);
            self.emit(me, seq, self.rcv_nxt, tcp::FLAG_PSH | tcp::FLAG_ACK, &self.snd_buf[..self.snd_buf_len]);
            true
        } else {
            false
        }
    }

    /// Receive up to `out.len()` bytes of in-order stream data, cumulative-ACKing
    /// and honoring the peer's FIN. Retransmits the buffered send segment a few
    /// times early in the idle window if nothing comes back. Returns the number
    /// of bytes written to `out` (0 is valid: empty response or immediate EOF).
    ///
    /// This is the single-connection path (used by the one-shot [`tcp_fetch`]
    /// control op, where no sibling connections exist to demux for). The ring
    /// socket path uses [`ring_tcp_recv`] + [`ring_pump`], which route to multiple
    /// connections but share the same [`ingest_seg`](Self::ingest_seg) core.
    fn recv(&mut self, me: &IfInfo, out: &mut [u8]) -> usize {
        let mut frame = [0u8; MAX_FRAME];
        let mut pl = [0u8; MAX_FRAME];

        let mut idle = 0u32;
        let mut retransmits = 0u32;
        while idle < TCP_DATA_ITERS && !self.peer_fin {
            let mut got = false;
            while let Some(rx) = self.recv_one_seg(me, &mut frame, &mut pl) {
                got = true;
                self.ingest_seg(me, &rx, &pl[..rx.payload_len]);
                if self.peer_fin {
                    break;
                }
            }
            if got {
                idle = 0;
            } else {
                idle = idle.saturating_add(1);
                if self.maybe_retransmit(me, idle, &mut retransmits) {
                    idle = 0;
                }
                sleep_ns(POLL_SLEEP_NS);
            }
        }
        self.take_rx(out)
    }

    /// Half- or full-close per `shutdown(2)`. `SHUT_WR`/`SHUT_RDWR` emit our FIN
    /// exactly once (idempotent across a later [`close`](Self::close), which will
    /// see `write_shut` and skip re-sending it) and reject subsequent sends; the
    /// read side stays open so buffered/inbound bytes keep flowing. `SHUT_RD`/
    /// `SHUT_RDWR` mark the read side closed so subsequent recvs report EOF. The
    /// connection is *not* torn down — the fd remains valid until `close`.
    fn shutdown(&mut self, me: &IfInfo, how: u64) {
        let close_read = how == netipc::ring::SHUT_RD || how == netipc::ring::SHUT_RDWR;
        let close_write = how == netipc::ring::SHUT_WR || how == netipc::ring::SHUT_RDWR;
        if close_read {
            self.read_shut = true;
        }
        if close_write && !self.write_shut {
            self.write_shut = true;
            // Emit the FIN|ACK at the current send sequence (mirrors `close`'s
            // convention of not advancing `snd_nxt` — the FIN's one seq is tracked
            // locally as `snd_nxt + 1` there). Idempotent: `close` won't re-send.
            self.ipid = self.ipid.wrapping_add(1);
            self.emit(me, self.snd_nxt, self.rcv_nxt, tcp::FLAG_FIN | tcp::FLAG_ACK, &[]);
        }
    }

    /// Gracefully close: send our FIN|ACK and briefly drain the peer's final ACK
    /// (and a late FIN, which we ACK for a clean teardown). If the write side was
    /// already shut down (`shutdown(SHUT_WR)`), the FIN is already on the wire, so
    /// we skip re-emitting it and only drain the teardown.
    fn close(&mut self, me: &IfInfo) {
        if !self.write_shut {
            self.ipid = self.ipid.wrapping_add(1);
            self.emit(me, self.snd_nxt, self.rcv_nxt, tcp::FLAG_FIN | tcp::FLAG_ACK, &[]);
        }
        let fin_seq = self.snd_nxt.wrapping_add(1); // Our FIN consumed one seq.

        let mut frame = [0u8; MAX_FRAME];
        let mut pl = [0u8; MAX_FRAME];
        for _ in 0..40 {
            let mut any = false;
            while let Some(rx) = self.recv_one_seg(me, &mut frame, &mut pl) {
                any = true;
                // A late FIN from the peer still needs an ACK for a clean teardown.
                if rx.flags & tcp::FLAG_FIN != 0 && rx.seq == self.rcv_nxt {
                    self.rcv_nxt = self.rcv_nxt.wrapping_add(rx.payload_len as u32).wrapping_add(1);
                    self.ipid = self.ipid.wrapping_add(1);
                    self.emit(me, fin_seq, self.rcv_nxt, tcp::FLAG_ACK, &[]);
                }
            }
            if !any {
                sleep_ns(POLL_SLEEP_NS);
            }
        }
    }
}

/// Perform a one-shot TCP fetch: connect to `dst_ip:dst_port`, send `payload`,
/// read the response into `out`, and close gracefully. `id` seeds the IPv4
/// identification counter. Returns the number of response bytes written to
/// `out` (0 is a valid result for an empty response), or `None` if the
/// connection could not be established. A thin wrapper over [`TcpConn`] — the
/// same state machine the ring-driven socket opcodes drive.
fn tcp_fetch(
    dst_ip: &[u8; 4],
    dst_port: u16,
    next_hop_mac: &[u8; 6],
    me: &IfInfo,
    id: u16,
    payload: &[u8],
    out: &mut [u8],
) -> Option<usize> {
    let mut conn = TcpConn::connect(me, *dst_ip, dst_port, *next_hop_mac, id)?;
    conn.send(me, payload)?;
    let written = conn.recv(me, out);
    conn.close(me);
    Some(written)
}

// ---------------------------------------------------------------------------
// One-shot UDP client (Phase 4 control op `OP_UDP_EXCHANGE`)
// ---------------------------------------------------------------------------
//
// Send one datagram to `ip:port` and return the first response datagram. This
// is the generic sibling of the DNS resolver (which is UDP under the hood but
// DNS-specific); it suits any request/response UDP protocol (NTP, STUN, custom).

/// If `frame` is a UDP datagram from `src_ip:src_port` addressed to us on our
/// ephemeral port, return its payload (borrows `frame`).
fn udp_response<'a>(
    frame: &'a [u8],
    me: &IfInfo,
    src_ip: &[u8; 4],
    src_port: u16,
) -> Option<&'a [u8]> {
    let eth = ethernet::Frame::parse(frame)?;
    if eth.ethertype != ethernet::ETHERTYPE_IPV4 {
        return None;
    }
    let ip = ipv4::Packet::parse(eth.payload)?;
    if ip.protocol != ipv4::PROTO_UDP || ip.dst != me.ip || ip.src != *src_ip {
        return None;
    }
    let dg = udp::Datagram::parse(ip.payload, &ip.src, &ip.dst)?;
    if dg.src_port != src_port || dg.dst_port != EPHEMERAL_PORT {
        return None;
    }
    Some(dg.payload)
}

/// Perform a one-shot UDP exchange: send `payload` as a single datagram to
/// `dst_ip:dst_port` and return the first matching response datagram's payload
/// in `out`. Returns bytes written (0 is a valid empty response), or `None` on
/// TX failure or receive timeout.
fn udp_exchange(
    dst_ip: &[u8; 4],
    dst_port: u16,
    next_hop_mac: &[u8; 6],
    me: &IfInfo,
    id: u16,
    payload: &[u8],
    out: &mut [u8],
) -> Option<usize> {
    let mut dgram = [0u8; MAX_FRAME - (ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN)];
    let dlen = udp::write(&mut dgram, &me.ip, dst_ip, EPHEMERAL_PORT, dst_port, payload)?;
    if !send_ipv4(me, next_hop_mac, dst_ip, ipv4::PROTO_UDP, &dgram[..dlen], id) {
        return None;
    }
    let mut frame = [0u8; MAX_FRAME];
    for _ in 0..RESOLVE_POLL_ITERS {
        loop {
            let n = raw_rx(&mut frame);
            if n < 0 {
                break;
            }
            let len = n as usize;
            if len > frame.len() {
                continue;
            }
            if let Some(pl) = udp_response(&frame[..len], me, dst_ip, dst_port) {
                let take = pl.len().min(out.len());
                out[..take].copy_from_slice(&pl[..take]);
                return Some(take);
            }
        }
        sleep_ns(POLL_SLEEP_NS);
    }
    None
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

/// Run the netstack IPC service (DNS/TCP/UDP over the `net.stack` channel).
///
/// When `persistent` is false (the Phase-4 bounded self-test path, argv
/// `serve-dns`) the daemon exits after [`SERVICE_IDLE_ITERS`] idle iterations and
/// releases the NIC, so the still-live kernel stack regains it (§64). When
/// `persistent` is true (the Phase-5 boot daemon, argv `serve-net`) the idle
/// deadline is ignored: the daemon owns the NIC for the lifetime of the system
/// and serves socket clients indefinitely — correct only once the kernel stack
/// has stood down (the `net.userspace` switch is on). Returns the process exit
/// code (a persistent daemon only returns on an unrecoverable service fault).
fn run_dns_service(me: &IfInfo, persistent: bool) -> i64 {
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
    // Persistent ring-TCP session: survives across separate OP_RING_TCP control
    // calls so a connection opened in one call is addressable in later ones.
    let mut ring_session = RingSession::new();

    // Persistent mode ignores the idle deadline and serves for the system's
    // lifetime; bounded mode exits after SERVICE_IDLE_ITERS idle iterations.
    while persistent || idle_ticks < SERVICE_IDLE_ITERS {
        let ch = syscall2(SYS_SERVICE_ACCEPT_TIMEOUT, listener as u64, ACCEPT_TIMEOUT_NS);
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
            ACCEPT_TIMEOUT_NS,
        );
        if rlen > 0 {
            let req = &req[..rlen as usize];
            let mut reply = [0u8; MSG_CAP];
            let reply_len =
                handle_request(req, &next_hop_mac, me, &mut txid, &mut ring_session, &mut reply);
            let _ = syscall3(SYS_CHANNEL_SEND, ch, reply.as_ptr() as u64, reply_len as u64);
            served += 1;
        }
        syscall1(SYS_CHANNEL_CLOSE, ch);
    }

    // Tear down any ring session the client left open (unmaps + closes conns).
    ring_session.teardown(me);

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
    ring_session: &mut RingSession,
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
        Some(netipc::Request::UdpExchange { ip, port, payload }) => {
            let mut body = [0u8; MSG_CAP];
            let cap = MSG_CAP - 1;
            let got = match next_hop_mac {
                Some(mac) => {
                    *txid = txid.wrapping_add(1);
                    udp_exchange(&ip, port, mac, me, *txid, payload, &mut body[..cap])
                }
                None => None,
            };
            match got {
                Some(n) => netipc::encode_ok_bytes(out, body.get(..n).unwrap_or(&[]))
                    .unwrap_or_else(|| fail(out)),
                None => fail(out),
            }
        }
        Some(netipc::Request::ShmPing { handle, size }) => {
            // Shared-memory handshake: map the kernel-created region, verify the
            // magic the kernel wrote, and write our response magic back. This is
            // the bootstrap that proves cross-address-space SYS_SHM_MAP sharing
            // — the mechanism the Phase-5 data ring uses to hand us its region.
            if shm_ping(handle, size) {
                netipc::encode_ok_bytes(out, &[]).unwrap_or_else(|| fail(out))
            } else {
                fail(out)
            }
        }
        Some(netipc::Request::RingEcho { handle, size }) => {
            // Shared-memory *ring* handshake: map the kernel-created region,
            // attach as a Ring, pop the kernel's OP_SEND SQE, upper-case its
            // payload in place, and post a completion. First end-to-end
            // exercise of the SQ/CQ driver the Phase-5 socket API rides on.
            if ring_echo(handle, size) {
                netipc::encode_ok_bytes(out, &[]).unwrap_or_else(|| fail(out))
            } else {
                fail(out)
            }
        }
        Some(netipc::Request::RingTcp { handle, size }) => {
            // Shared-memory *ring TCP*: map the kernel's ring region and drain
            // the socket-opcode batch (connect → send → recv → close) driving a
            // single live TCP connection, with the request/response bytes flowing
            // through the ring data window rather than the control channel. The
            // ring-native equivalent of OP_TCP_FETCH — the Phase-5 socket shape.
            let ok = match next_hop_mac {
                Some(mac) => {
                    *txid = txid.wrapping_add(1);
                    ring_tcp(ring_session, handle, size, me, mac, *txid)
                }
                None => false,
            };
            if ok {
                netipc::encode_ok_bytes(out, &[]).unwrap_or_else(|| fail(out))
            } else {
                fail(out)
            }
        }
        // Unknown opcode or a structurally invalid request → uniform failure.
        Some(netipc::Request::Unknown(_)) | None => fail(out),
    }
}

/// Handle an [`netipc::OP_SHM_PING`] request: map the kernel-created shared
/// region `handle` (`size` bytes) read-write, confirm the kernel's request
/// magic is visible at offset 0 (proving we mapped the *same* physical frames),
/// write our response magic at offset 8 (which the kernel then reads back), and
/// unmap. Returns `true` iff the handshake succeeded.
fn shm_ping(handle: u64, size: u32) -> bool {
    // Need at least 16 bytes to hold the two u64 magics at offsets 0 and 8.
    if (size as usize) < 16 {
        return false;
    }
    let va = syscall2(SYS_SHM_MAP, handle, SHM_MAP_RW);
    if va < 0 {
        return false;
    }
    let base = va as u64 as *mut u8;

    // SAFETY: SYS_SHM_MAP returned a valid mapping of at least `size` (>= 16)
    // bytes; we only touch offsets 0..16, aligned reads/writes of u64.
    let ok = unsafe {
        let req = core::ptr::read_unaligned(base as *const u64);
        if req != netipc::SHM_PING_REQUEST_MAGIC {
            false
        } else {
            core::ptr::write_unaligned(
                base.add(8) as *mut u64,
                netipc::SHM_PING_RESPONSE_MAGIC,
            );
            true
        }
    };

    // Drop our mapping regardless (refcount-aware; the kernel keeps its own
    // reference until it closes the handle).
    syscall2(SYS_SHM_UNMAP, va as u64, size as u64);
    ok
}

/// Handle an [`netipc::OP_RING_ECHO`] request: map the kernel-created ring
/// region `handle` (`size` bytes) read-write, attach as a [`netring::Ring`], pop
/// the single `OP_SEND` SQE the kernel submitted, ASCII-upper-case its data-area
/// payload in place, push a completion carrying the echoed `user_data` and the
/// byte count as `result`, and unmap. Returns `true` iff a well-formed SQE was
/// consumed and a completion posted.
fn ring_echo(handle: u64, size: u32) -> bool {
    let va = syscall2(SYS_SHM_MAP, handle, SHM_MAP_RW);
    if va < 0 {
        return false;
    }
    let base = va as u64 as *mut u8;

    // SAFETY: SYS_SHM_MAP returned a valid, writable mapping of at least `size`
    // bytes; `attach` re-validates the ring geometry against that length and
    // never reads/writes outside the mapping. We are the sole consumer of the SQ
    // and sole producer of the CQ (the kernel is the other party), satisfying the
    // Ring SPSC contract.
    let ok = unsafe {
        match netring::Ring::attach(base, size as usize) {
            Some(ring) => ring_echo_process(&ring),
            None => false,
        }
    };

    // Drop our mapping regardless (refcount-aware; the kernel keeps its own
    // reference until it closes the handle).
    syscall2(SYS_SHM_UNMAP, va as u64, size as u64);
    ok
}

/// Drain the whole submission queue, dispatching each SQE by opcode and posting
/// exactly one completion per entry (the io_uring batched-submission model). The
/// kernel submits a batch of SQEs in one pass; the daemon processes them FIFO and
/// posts CQEs in the same order, so the kernel can match completions to
/// submissions by position or by echoed `user_data`. Split out so the `unsafe`
/// attach site stays small.
///
/// Per-opcode semantics (the mechanical foundation the real socket dispatch will
/// build on — connect/send/recv/close land here later):
/// - [`netipc::ring::OP_NOP`]: complete immediately with `result = 0`.
/// - [`netipc::ring::OP_SEND`]: read the SQE's `(data_off, data_len)` window,
///   ASCII-upper-case it in place, complete with `result = len`.
/// - any other opcode: complete with `result = -1` (unsupported) so the kernel
///   still gets a completion rather than a hang.
///
/// Returns `true` iff at least one SQE was processed and every completion was
/// posted (the CQ never overflowed).
fn ring_echo_process(ring: &netring::Ring) -> bool {
    let mut processed = 0u32;
    while let Some(sqe) = ring.sq_pop() {
        let result = match sqe.op {
            netipc::ring::OP_NOP => 0,
            netipc::ring::OP_SEND => ring_send_transform(ring, &sqe),
            _ => -1, // unsupported opcode: report failure, still complete
        };
        let cqe = netipc::ring::Cqe { user_data: sqe.user_data, result, flags: 0 };
        if !ring.cq_push(&cqe) {
            return false; // CQ full — would drop a completion; treat as failure
        }
        processed = processed.saturating_add(1);
    }
    processed > 0
}

/// Apply the `OP_SEND` transform: read the SQE's data window, upper-case it in
/// place, and return the byte count as the completion `result` (or `-1` if the
/// window is out of range or larger than our scratch buffer).
fn ring_send_transform(ring: &netring::Ring, sqe: &netipc::ring::Sqe) -> i32 {
    let off = sqe.data_off as usize;
    let len = sqe.data_len as usize;
    let mut buf = [0u8; 64];
    let window = match buf.get_mut(..len) {
        Some(w) => w,
        None => return -1, // payload larger than our scratch buffer
    };
    if !ring.read_data(off, window) {
        return -1;
    }
    for b in window.iter_mut() {
        *b = b.to_ascii_uppercase();
    }
    if !ring.write_data(off, window) {
        return -1;
    }
    len as i32
}

/// Handle an [`netipc::OP_RING_TCP`] request: map the kernel-created ring region
/// `handle` (`size` bytes) read-write, attach as a [`netring::Ring`], and drain
/// the socket-opcode batch driving one live TCP connection, then unmap. Returns
/// `true` iff at least one SQE was processed and every completion was posted.
/// `seed_ipid` seeds the connection's IPv4 identification counter.
fn ring_tcp(
    session: &mut RingSession,
    handle: u64,
    size: u32,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    seed_ipid: u16,
) -> bool {
    // Ensure the session is mapped for this ring handle. A different handle (or a
    // fresh start) opens a new session: tear down any prior mapping, map the new
    // region, and reset the connection table + ident seed.
    if session.handle != handle {
        session.teardown(me);
        let va = syscall2(SYS_SHM_MAP, handle, SHM_MAP_RW);
        if va < 0 {
            return false;
        }
        session.handle = handle;
        session.va = va;
        session.size = size;
        session.conns = RingConns::new();
        session.listeners = Listeners::new();
        session.ipid = seed_ipid;
    }

    let base = session.va as u64 as *mut u8;

    // SAFETY: the mapping is valid and writable for `session.size` bytes for as
    // long as the session holds `handle` (we only unmap in `teardown`); `attach`
    // re-validates the ring geometry against that length and never reads/writes
    // outside the mapping. Re-attaching each call is stateless — the SQ/CQ head/
    // tail indices live in the shared region, so a fresh `Ring` view resumes
    // exactly where the previous call left off. We are the sole SQ consumer and
    // sole CQ producer (the kernel is the other party), satisfying the SPSC
    // contract.
    let (ok, stop) = unsafe {
        match netring::Ring::attach(base, session.size as usize) {
            Some(ring) => {
                ring_tcp_process(
                    &ring,
                    &mut session.conns,
                    &mut session.listeners,
                    me,
                    next_hop_mac,
                    &mut session.ipid,
                )
            }
            None => (false, false),
        }
    };

    // On an explicit OP_STOP, close any still-live connections and unmap now.
    if stop {
        session.teardown(me);
    }
    ok
}

/// Maximum concurrent TCP connections one ring session can multiplex.
///
/// Each live [`TcpConn`] carries a `TCP_SND_BUF` (1 KiB) send buffer plus
/// bookkeeping, so this is a deliberate cap: 8 slots ≈ 8–9 KiB of connection
/// state — comfortable on the daemon's stack while covering the fan-out a
/// socket-forwarding client needs (one live `TcpConn` per userspace socket).
const MAX_RING_CONNS: usize = 8;

/// A `conn_id`-keyed table of live TCP connections for one ring session.
///
/// The io_uring socket opcodes address a connection by the [`netipc::ring::Sqe`]
/// `conn_id` field, chosen by the client (in the Phase-5 socket-forwarding design
/// it is the identity of the userspace socket). `OP_CONNECT` installs a
/// [`TcpConn`] under its `conn_id`; a later `OP_SEND`/`OP_RECV`/`OP_CLOSE` looks
/// it up; `OP_CLOSE` evicts it. This is what turns the one-shot fetch into a
/// multiplexed socket server — the Phase-5 prerequisite the socket-syscall
/// forwarders build on.
///
/// # Concurrency limitation
///
/// The underlying receive path ([`recv_tcp_seg`]) reads one frame directly off
/// the shared NIC and *drops* it if it does not match the connection's 4-tuple.
/// So two connections must not be *simultaneously* in their `OP_RECV` phase on
/// one ring — a sibling connection's inbound frames would be discarded. The
/// current model is therefore safe for connections whose active phases do not
/// overlap (one fully handshakes/sends/receives/closes before the next receives).
/// True concurrent multiplexing needs a shared RX demux that buffers per-4-tuple
/// frames; that is tracked in `known-issues.md` (D-NETSTACK-RX-DEMUX) as the next
/// receive-path piece and is partly shaped by the Q22b persistent-daemon lifecycle.
struct RingConns {
    slots: [Option<(u32, TcpConn)>; MAX_RING_CONNS],
}

impl RingConns {
    /// An empty table (all slots free).
    fn new() -> Self {
        Self { slots: core::array::from_fn(|_| None) }
    }

    /// Borrow the live connection registered under `id`, if any.
    fn get_mut(&mut self, id: u32) -> Option<&mut TcpConn> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|(sid, _)| *sid == id)
            .map(|(_, c)| c)
    }

    /// Borrow the connection whose 4-tuple matches an inbound frame's peer
    /// identity — i.e. the connection this frame belongs to. Used by the shared
    /// RX pump to route each received segment to its owner. `src_ip`/`src_port`
    /// are the frame's source (our peer); `dst_port` is the frame's destination
    /// port (our connection's local ephemeral port).
    fn find_by_tuple(
        &mut self,
        src_ip: &[u8; 4],
        src_port: u16,
        dst_port: u16,
    ) -> Option<&mut TcpConn> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .map(|(_, c)| c)
            .find(|c| c.dst_ip == *src_ip && c.dst_port == src_port && c.local_port == dst_port)
    }

    /// The IPv6 sibling of [`find_by_tuple`](Self::find_by_tuple): match a live
    /// connection by its IPv6 peer address (`dst6`) and ports.
    fn find_by_tuple6(
        &mut self,
        src_ip6: &[u8; 16],
        src_port: u16,
        dst_port: u16,
    ) -> Option<&mut TcpConn> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .map(|(_, c)| c)
            .find(|c| {
                c.dst6 == Some(*src_ip6) && c.dst_port == src_port && c.local_port == dst_port
            })
    }

    /// Reserve the free slot that should hold `id`, if `id` is not already in use
    /// and the table has room. Returns a mutable handle to the empty slot for the
    /// caller to fill (`*slot = Some((id, conn))`), or `None` on a duplicate id or
    /// a full table. Reserving before moving the connection lets the caller retain
    /// ownership on failure (to close it gracefully) without a large-`Err` Result.
    fn reserve(&mut self, id: u32) -> Option<&mut Option<(u32, TcpConn)>> {
        if self.slots.iter().flatten().any(|(sid, _)| *sid == id) {
            return None; // duplicate conn_id
        }
        self.slots.iter_mut().find(|s| s.is_none())
    }

    /// Remove and return the connection registered under `id`, if present, freeing
    /// its slot for reuse.
    fn remove(&mut self, id: u32) -> Option<TcpConn> {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|(sid, _)| *sid == id) {
                return slot.take().map(|(_, c)| c);
            }
        }
        None
    }

    /// Gracefully close every live connection and empty the table. Used when a
    /// session ends (explicit `OP_STOP` or daemon shutdown) so no connection is
    /// left half-open at the peer if the client did not `OP_CLOSE` it itself.
    fn close_all(&mut self, me: &IfInfo) {
        for slot in &mut self.slots {
            if let Some((_, mut c)) = slot.take() {
                c.close(me);
            }
        }
    }
}

/// Maximum concurrent listening sockets one ring session can hold.
const MAX_LISTENERS: usize = 4;
/// Maximum backlog (pending + established-but-unaccepted connections) per listener.
const MAX_BACKLOG: usize = 8;

/// A passively-listening server socket bound to a local TCP port.
///
/// Inbound SYNs to [`port`](Self::port) that don't match an existing connection are
/// turned into passive-open [`TcpConn`]s (SYN_RCVD) queued in [`backlog`]; each
/// completes its handshake (via [`ingest_seg`](TcpConn::ingest_seg)) when the
/// peer's ACK arrives, then waits to be dequeued by an `OP_ACCEPT`. A listener owns
/// no ring `conn_id` of its own beyond the id the client chose in `OP_LISTEN`; an
/// accepted connection is moved out of the backlog into [`RingConns`] under the
/// caller-chosen id at accept time.
struct Listener {
    /// Local port this listener is bound to.
    port: u16,
    /// Pending/established passive connections awaiting `OP_ACCEPT`. FIFO order is
    /// not strictly preserved on accept (we return the first *established* one), but
    /// slots are reused as connections are accepted or dropped.
    backlog: [Option<TcpConn>; MAX_BACKLOG],
    /// IPv4 identification seed for this listener's SYN-ACKs, advanced per accept.
    ipid: u16,
}

impl Listener {
    fn new(port: u16, seed_ipid: u16) -> Self {
        Self { port, backlog: core::array::from_fn(|_| None), ipid: seed_ipid }
    }

    /// Borrow the backlog connection whose 4-tuple matches an inbound frame's peer
    /// identity, if any (routes a segment to a pending passive connection).
    fn find_backlog_by_tuple(
        &mut self,
        src_ip: &[u8; 4],
        src_port: u16,
        dst_port: u16,
    ) -> Option<&mut TcpConn> {
        self.backlog
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|c| c.dst_ip == *src_ip && c.dst_port == src_port && c.local_port == dst_port)
    }

    /// The IPv6 sibling of [`find_backlog_by_tuple`](Self::find_backlog_by_tuple):
    /// match a pending passive connection by its IPv6 peer address and ports.
    fn find_backlog_by_tuple6(
        &mut self,
        src_ip6: &[u8; 16],
        src_port: u16,
        dst_port: u16,
    ) -> Option<&mut TcpConn> {
        self.backlog.iter_mut().filter_map(|s| s.as_mut()).find(|c| {
            c.dst6 == Some(*src_ip6) && c.dst_port == src_port && c.local_port == dst_port
        })
    }

    /// Register a freshly passive-opened connection in a free backlog slot. Returns
    /// `false` (dropped) if the backlog is full — the peer will retransmit its SYN.
    fn push_backlog(&mut self, conn: TcpConn) -> bool {
        if let Some(slot) = self.backlog.iter_mut().find(|s| s.is_none()) {
            *slot = Some(conn);
            true
        } else {
            false
        }
    }

    /// Remove and return the first *established* backlog connection (ready to be
    /// handed to the caller by `OP_ACCEPT`), or `None` if none is ready yet.
    fn take_established(&mut self) -> Option<TcpConn> {
        for slot in &mut self.backlog {
            if slot.as_ref().is_some_and(|c| c.established && !c.connect_failed) {
                return slot.take();
            }
        }
        None
    }

    /// Gracefully close every backlog connection (listener teardown).
    fn close_all(&mut self, me: &IfInfo) {
        for slot in &mut self.backlog {
            if let Some(mut c) = slot.take() {
                c.close(me);
            }
        }
    }
}

/// Table of active listeners, keyed by the `OP_LISTEN` `conn_id`.
struct Listeners {
    slots: [Option<(u32, Listener)>; MAX_LISTENERS],
}

impl Listeners {
    fn new() -> Self {
        Self { slots: core::array::from_fn(|_| None) }
    }

    /// Register a listener `id` bound to `port`. Rejects a duplicate id, a port
    /// already bound by another listener, or a full table (`false`).
    fn add(&mut self, id: u32, port: u16, seed_ipid: u16) -> bool {
        if self.slots.iter().flatten().any(|(lid, l)| *lid == id || l.port == port) {
            return false; // duplicate id or port already bound
        }
        if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
            *slot = Some((id, Listener::new(port, seed_ipid)));
            true
        } else {
            false
        }
    }

    /// Borrow the listener registered under `id`, if any.
    fn get_mut(&mut self, id: u32) -> Option<&mut Listener> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|(lid, _)| *lid == id)
            .map(|(_, l)| l)
    }

    /// Borrow the listener bound to `port`, if any (inbound-SYN demux).
    fn find_by_port(&mut self, port: u16) -> Option<&mut Listener> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .map(|(_, l)| l)
            .find(|l| l.port == port)
    }

    /// Route an inbound segment to a matching backlog connection, or (for a fresh
    /// SYN to a listening port with no existing owner) start a passive open. Returns
    /// `true` if the segment was consumed by a listener. `mac` is the reply next-hop
    /// and `ipid_seed` seeds a new passive connection's identification counter.
    #[allow(clippy::too_many_arguments)]
    fn route_seg(
        &mut self,
        me: &IfInfo,
        src_ip: &[u8; 4],
        src_port: u16,
        dst_port: u16,
        rx: &TcpRx,
        payload: &[u8],
        mac: &[u8; 6],
    ) -> bool {
        // 1) An existing pending/established backlog connection for this 4-tuple.
        for slot in &mut self.slots {
            if let Some((_, l)) = slot.as_mut()
                && let Some(c) = l.find_backlog_by_tuple(src_ip, src_port, dst_port)
            {
                c.ingest_seg(me, rx, payload);
                return true;
            }
        }
        // 2) A fresh SYN (SYN set, ACK clear) to a listening port → passive open.
        if rx.flags & tcp::FLAG_SYN != 0
            && rx.flags & tcp::FLAG_ACK == 0
            && let Some(l) = self.find_by_port(dst_port)
        {
            l.ipid = l.ipid.wrapping_add(0x10);
            let seed = l.ipid;
            if let Some(conn) =
                TcpConn::accept_syn(me, *src_ip, src_port, dst_port, rx.seq, *mac, seed)
            {
                l.push_backlog(conn);
            }
            return true; // consumed (whether or not the backlog had room)
        }
        false
    }

    /// The IPv6 sibling of [`route_seg`](Self::route_seg): route an inbound IPv6
    /// segment to a matching passive backlog connection, or start a passive open
    /// for a fresh SYN to a listening port. A [`Listener`]'s `port` is
    /// family-agnostic, so the same listener accepts both IPv4 and IPv6 SYNs.
    #[allow(clippy::too_many_arguments)]
    fn route_seg6(
        &mut self,
        me: &IfInfo,
        src_ip6: &[u8; 16],
        src_port: u16,
        dst_port: u16,
        rx: &TcpRx,
        payload: &[u8],
        mac: &[u8; 6],
    ) -> bool {
        // 1) An existing pending/established backlog connection for this 6-tuple.
        for slot in &mut self.slots {
            if let Some((_, l)) = slot.as_mut()
                && let Some(c) = l.find_backlog_by_tuple6(src_ip6, src_port, dst_port)
            {
                c.ingest_seg(me, rx, payload);
                return true;
            }
        }
        // 2) A fresh SYN to a listening port → passive IPv6 open.
        if rx.flags & tcp::FLAG_SYN != 0
            && rx.flags & tcp::FLAG_ACK == 0
            && let Some(l) = self.find_by_port(dst_port)
        {
            l.ipid = l.ipid.wrapping_add(0x10);
            let seed = l.ipid;
            if let Some(conn) =
                TcpConn::accept_syn6(me, *src_ip6, src_port, dst_port, rx.seq, *mac, seed)
            {
                l.push_backlog(conn);
            }
            return true;
        }
        false
    }

    /// Gracefully close every listener's backlog and empty the table.
    fn close_all(&mut self, me: &IfInfo) {
        for slot in &mut self.slots {
            if let Some((_, mut l)) = slot.take() {
                l.close_all(me);
            }
        }
    }
}

/// A persistent ring-TCP session: the mapped ring region plus the connection
/// table that survives across *separate* `OP_RING_TCP` control calls.
///
/// The one-shot [`ring_tcp`] path mapped the ring, drained a single batch, and
/// unmapped immediately — so a connection could not outlive one submission. This
/// session keeps the mapping and the [`RingConns`] table alive between control
/// calls, so a client can `OP_CONNECT` in one round and `OP_SEND`/`OP_RECV`/
/// `OP_CLOSE` in later rounds against the *same* live [`TcpConn`] — the shape the
/// persistent socket-forwarding daemon needs. The session is torn down (all
/// connections closed, ring unmapped) on an explicit `OP_STOP` SQE or when the
/// daemon's serve loop exits.
struct RingSession {
    /// SHM handle of the currently-mapped ring (0 = no session open).
    handle: u64,
    /// Mapped virtual address of the ring region (valid iff `handle != 0`).
    va: i64,
    /// Byte length of the mapping.
    size: u32,
    /// Per-connection table, keyed by SQE `conn_id`.
    conns: RingConns,
    /// Listening sockets, keyed by the `OP_LISTEN` `conn_id`.
    listeners: Listeners,
    /// IPv4 identification seed, advanced per new connection.
    ipid: u16,
}

impl RingSession {
    /// An idle session (nothing mapped).
    fn new() -> Self {
        Self {
            handle: 0,
            va: 0,
            size: 0,
            conns: RingConns::new(),
            listeners: Listeners::new(),
            ipid: 0,
        }
    }

    /// Tear the session down: gracefully close any live connections and unmap the
    /// ring. Idempotent — a no-op when no session is open.
    fn teardown(&mut self, me: &IfInfo) {
        if self.handle == 0 {
            return;
        }
        self.conns.close_all(me);
        self.listeners.close_all(me);
        syscall2(SYS_SHM_UNMAP, self.va as u64, self.size as u64);
        self.handle = 0;
        self.va = 0;
        self.size = 0;
    }
}

/// Drain the submission queue, driving one or more [`TcpConn`]s through the
/// socket opcodes and posting exactly one completion per SQE (the io_uring
/// batched model). This is the ring-native equivalent of [`tcp_fetch`]: the
/// kernel submits `OP_CONNECT` → `OP_SEND` → `OP_RECV` → `OP_CLOSE` (per
/// connection), and the request/response bytes flow through the ring data window
/// instead of the control channel. Connections are addressed by the SQE
/// `conn_id`, so a single ring can multiplex several sockets (see [`RingConns`]).
///
/// Per-opcode semantics:
/// - [`netipc::ring::OP_CONNECT`]: unpack the endpoint from `aux`, open the
///   connection, and install it under `sqe.conn_id`; completion `result = 0` on
///   success, or `-1` (peer refused, or the `conn_id` is a duplicate / the table
///   is full).
/// - [`netipc::ring::OP_SEND`]: look up `sqe.conn_id`, read the SQE's data window
///   and send it; completion `result` = bytes accepted, or `-1` (no such conn /
///   window error).
/// - [`netipc::ring::OP_RECV`]: receive for `sqe.conn_id` via the shared RX pump
///   ([`ring_pump`]) — so sibling connections' frames are routed to *them* rather
///   than dropped — then copy the response into the SQE's data window; completion
///   `result` = bytes received, or `-1`.
/// - [`netipc::ring::OP_POLL`]: non-destructively report `sqe.conn_id`'s readiness
///   via [`ring_tcp_poll`]; completion `result` is a [`POLL_READABLE`]/
///   [`POLL_WRITABLE`](netipc::ring::POLL_WRITABLE) bitmask, or `-1` (no such conn).
/// - [`netipc::ring::OP_CLOSE`]: look up and evict `sqe.conn_id`, graceful
///   teardown; completion `result = 0`, or `-1` if no such connection.
/// - [`netipc::ring::OP_NOP`]: complete with `result = 0`.
/// - [`netipc::ring::OP_STOP`]: complete with `result = 0` and request session
///   teardown (return flag) once the current batch is drained.
/// - any unknown opcode: `result = -1`.
///
/// `conns` and `ipid` are the *persistent* session state (see [`RingSession`]), so
/// connections opened in an earlier call are still addressable here. Returns
/// `(processed, stop)`: `processed` is true iff at least one SQE was handled and
/// the CQ never overflowed; `stop` is true iff an `OP_STOP` was seen (the caller
/// then tears the session down).
fn ring_tcp_process(
    ring: &netring::Ring,
    conns: &mut RingConns,
    listeners: &mut Listeners,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    ipid: &mut u16,
) -> (bool, bool) {
    let mut processed = 0u32;
    let mut stop = false;
    while let Some(sqe) = ring.sq_pop() {
        let result = match sqe.op {
            netipc::ring::OP_NOP => 0,
            netipc::ring::OP_STOP => {
                // Drain any remaining SQEs first, then the caller tears down.
                stop = true;
                0
            }
            netipc::ring::OP_CONNECT => {
                let (ip, port) = netipc::ring::Sqe::unpack_endpoint(sqe.aux);
                // Advance the ident seed well past the handshake's own increments
                // so each connection gets a distinct ephemeral port + ISN.
                *ipid = ipid.wrapping_add(0x10);
                if sqe.aux & netipc::ring::CONNECT_NONBLOCK != 0 {
                    // Non-blocking connect: transmit the SYN and install the conn in
                    // SYN_SENT without waiting for the SYN-ACK. Pump once so a SYN-ACK
                    // that has already arrived can complete the handshake immediately
                    // (a loopback/fast peer), letting us report success synchronously.
                    match TcpConn::connect_start(me, ip, port, *next_hop_mac, *ipid) {
                        Some(c) => match conns.reserve(sqe.conn_id) {
                            Some(slot) => {
                                *slot = Some((sqe.conn_id, c));
                                ring_pump(conns, listeners, me, next_hop_mac);
                                match conns.get_mut(sqe.conn_id) {
                                    Some(c) if c.established => 0,
                                    Some(_) => netipc::ring::ERR_IN_PROGRESS,
                                    None => -1,
                                }
                            }
                            None => {
                                // Duplicate id or table full: nothing installed yet
                                // (we only sent a SYN); report failure.
                                -1
                            }
                        },
                        None => -1,
                    }
                } else {
                    match TcpConn::connect(me, ip, port, *next_hop_mac, *ipid) {
                        Some(mut c) => match conns.reserve(sqe.conn_id) {
                            Some(slot) => {
                                *slot = Some((sqe.conn_id, c));
                                0
                            }
                            None => {
                                // Duplicate id or table full: tear the fresh conn down
                                // gracefully rather than leaking the peer's half-open
                                // connection, then report failure.
                                c.close(me);
                                -1
                            }
                        },
                        None => -1,
                    }
                }
            }
            netipc::ring::OP_CONNECT6 => {
                // IPv6 connect: the 16-byte peer address travels in the SQE's data
                // window; the port is the low 16 bits of `aux`. Read the address,
                // resolve the next-hop MAC (skipping NDP for a self/loopback target,
                // whose frames are diverted by IP before hitting the wire), then run
                // the same connect state machine as IPv4 via the v6 constructors.
                let mut addr = [0u8; 16];
                let ok = sqe.data_len as usize >= 16 && ring.read_data(sqe.data_off as usize, &mut addr);
                if !ok {
                    -1
                } else {
                    let port = (sqe.aux & 0xFFFF) as u16;
                    *ipid = ipid.wrapping_add(0x10);
                    // Loopback (dst == our own IPv6) is demuxed by IP; any MAC works.
                    // Otherwise resolve the peer's link-layer address via NDP.
                    let mac = if addr == me.ip6 {
                        Some(me.mac)
                    } else {
                        ndp_resolve(&addr, me)
                    };
                    match mac {
                        None => -1, // next-hop unresolved → report failure
                        Some(mac) => {
                            if sqe.aux & netipc::ring::CONNECT_NONBLOCK != 0 {
                                match TcpConn::connect_start6(me, addr, port, mac, *ipid) {
                                    Some(c) => match conns.reserve(sqe.conn_id) {
                                        Some(slot) => {
                                            *slot = Some((sqe.conn_id, c));
                                            ring_pump(conns, listeners, me, next_hop_mac);
                                            match conns.get_mut(sqe.conn_id) {
                                                Some(c) if c.established => 0,
                                                Some(_) => netipc::ring::ERR_IN_PROGRESS,
                                                None => -1,
                                            }
                                        }
                                        None => -1,
                                    },
                                    None => -1,
                                }
                            } else {
                                match TcpConn::connect6(me, addr, port, mac, *ipid) {
                                    Some(mut c) => match conns.reserve(sqe.conn_id) {
                                        Some(slot) => {
                                            *slot = Some((sqe.conn_id, c));
                                            0
                                        }
                                        None => {
                                            c.close(me);
                                            -1
                                        }
                                    },
                                    None => -1,
                                }
                            }
                        }
                    }
                }
            }
            netipc::ring::OP_SEND => {
                ring_tcp_send(ring, conns, listeners, sqe.conn_id, me, next_hop_mac, &sqe)
            }
            netipc::ring::OP_RECV => {
                ring_tcp_recv(ring, conns, listeners, sqe.conn_id, me, next_hop_mac, &sqe)
            }
            netipc::ring::OP_POLL => ring_tcp_poll(conns, listeners, sqe.conn_id, me, next_hop_mac),
            netipc::ring::OP_CLOSE => match conns.remove(sqe.conn_id) {
                Some(mut c) => {
                    c.close(me);
                    0
                }
                None => -1,
            },
            netipc::ring::OP_LISTEN => {
                // Bind a listener id to a local port (low 16 bits of aux).
                let port = (sqe.aux & 0xFFFF) as u16;
                *ipid = ipid.wrapping_add(0x10);
                if listeners.add(sqe.conn_id, port, *ipid) { 0 } else { -1 }
            }
            netipc::ring::OP_ACCEPT => {
                ring_tcp_accept(ring, conns, listeners, sqe.conn_id, me, next_hop_mac, &sqe)
            }
            netipc::ring::OP_LOCALADDR => {
                // getsockname: report this connection's local endpoint. The source IP
                // is the daemon's interface address (`me.ip`/`me.ip6`); the local port
                // is the ephemeral port the connection chose. Family is conveyed by the
                // written length (6 = v4, 18 = v6).
                match conns.get_mut(sqe.conn_id) {
                    None => -1, // no such connection
                    Some(c) => {
                        if c.dst6.is_some() {
                            if sqe.data_len as usize >= 18 {
                                let mut addr = [0u8; 18];
                                addr[..16].copy_from_slice(&me.ip6);
                                addr[16..18].copy_from_slice(&c.local_port.to_be_bytes());
                                if ring.write_data(sqe.data_off as usize, &addr) { 18 } else { -1 }
                            } else {
                                -1
                            }
                        } else if sqe.data_len as usize >= 6 {
                            let mut addr = [0u8; 6];
                            addr[..4].copy_from_slice(&me.ip);
                            addr[4..6].copy_from_slice(&c.local_port.to_be_bytes());
                            if ring.write_data(sqe.data_off as usize, &addr) { 6 } else { -1 }
                        } else {
                            -1
                        }
                    }
                }
            }
            netipc::ring::OP_SHUTDOWN => {
                // shutdown(2): half- or full-close the connection without tearing it
                // down. `aux` carries the Linux `how` (SHUT_RD/WR/RDWR). SHUT_WR emits
                // our FIN and marks the write side closed (subsequent sends → EPIPE);
                // SHUT_RD marks the read side closed (subsequent recvs → EOF).
                match conns.get_mut(sqe.conn_id) {
                    None => -1, // no such connection
                    Some(c) => {
                        c.shutdown(me, sqe.aux);
                        0
                    }
                }
            }
            _ => -1, // unsupported opcode: report failure, still complete
        };
        let cqe = netipc::ring::Cqe { user_data: sqe.user_data, result, flags: 0 };
        if !ring.cq_push(&cqe) {
            return (false, stop); // CQ full — would drop a completion; treat as failure
        }
        processed = processed.saturating_add(1);
    }
    (processed > 0, stop)
}

/// Execute an `OP_SEND` SQE for connection `target_id`: read the data window into
/// a scratch buffer and send it, honouring the single-outstanding-segment send
/// window. Returns bytes accepted, [`netipc::ring::ERR_WOULD_BLOCK`] (non-blocking
/// send on a full window), or `-1` on a missing/unestablished connection or a
/// geometry error.
///
/// The window is *full* while a prior segment is still unacknowledged
/// ([`TcpConn::send_window_full`]). We drain pending ACKs via [`ring_pump`] (which
/// also serves sibling connections, so one blocked sender never starves another).
/// - [`netipc::ring::SEND_NONBLOCK`] set: pump once; if the window is still full,
///   report `ERR_WOULD_BLOCK` (kernel → `EAGAIN`) rather than waiting.
/// - Blocking: pump/sleep up to the send deadline until the window drains, then
///   send. If the peer never ACKs within the deadline, report `ERR_WOULD_BLOCK`
///   so the caller can retry rather than us silently dropping the write.
fn ring_tcp_send(
    ring: &netring::Ring,
    conns: &mut RingConns,
    listeners: &mut Listeners,
    target_id: u32,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    sqe: &netipc::ring::Sqe,
) -> i32 {
    match conns.get_mut(target_id) {
        None => return -1, // no such connection
        // A send after `shutdown(SHUT_WR)` is EPIPE, exactly like Linux: the write
        // side is closed and our FIN is already on the wire.
        Some(c) if c.write_shut => return netipc::ring::ERR_BROKEN_PIPE,
        // A send before the handshake completed is a protocol error on our minimal
        // client (no send buffering across connect): reject it so the kernel
        // surfaces ENOTCONN rather than us emitting a bad-seq segment.
        Some(c) if !c.established => return -1,
        Some(_) => {}
    }
    let nonblock = sqe.aux & netipc::ring::SEND_NONBLOCK != 0;
    // Drain any already-arrived ACKs so a window freed by the peer is visible.
    ring_pump(conns, listeners, me, next_hop_mac);
    if conns.get_mut(target_id).is_some_and(|c| c.send_window_full()) {
        if nonblock {
            return netipc::ring::ERR_WOULD_BLOCK; // kernel → EAGAIN
        }
        // Blocking send: wait for the outstanding segment to be ACKed, driving its
        // retransmit while we wait. Bounded by the send deadline.
        let mut idle = 0u32;
        let mut retransmits = 0u32;
        loop {
            if idle >= TCP_DATA_ITERS {
                break;
            }
            if ring_pump(conns, listeners, me, next_hop_mac) {
                idle = 0;
            } else {
                idle = idle.saturating_add(1);
                if conns
                    .get_mut(target_id)
                    .is_some_and(|c| c.maybe_retransmit(me, idle, &mut retransmits))
                {
                    idle = 0;
                }
                sleep_ns(POLL_SLEEP_NS);
            }
            match conns.get_mut(target_id) {
                Some(c) if !c.send_window_full() => break, // window drained → send below
                Some(_) => {}
                None => return -1, // connection vanished mid-send
            }
        }
        if conns.get_mut(target_id).is_some_and(|c| c.send_window_full()) {
            return netipc::ring::ERR_WOULD_BLOCK; // deadline elapsed, still full
        }
    }
    let off = sqe.data_off as usize;
    let len = sqe.data_len as usize;
    let mut buf = [0u8; TCP_SND_BUF];
    let window = match buf.get_mut(..len) {
        Some(w) => w,
        None => return -1, // send window larger than our buffer
    };
    if !ring.read_data(off, window) {
        return -1;
    }
    match conns.get_mut(target_id) {
        Some(c) => match c.send(me, window) {
            Some(n) => n as i32,
            None => -1,
        },
        None => -1,
    }
}

/// Drain *every* frame currently queued on the NIC (and the loopback FIFO ahead of
/// it), routing each TCP segment to the connection that owns its 4-tuple (via
/// [`RingConns::find_by_tuple`]) and feeding it through the shared
/// [`ingest_seg`](TcpConn::ingest_seg) core. This is the shared RX demux: because
/// one NIC delivers frames for *all* connections on the ring, a naive
/// per-connection read would discard a sibling's frames. The pump instead buffers
/// each segment into its owner so no connection loses data while another is blocked
/// in a receive.
///
/// A segment that matches no established connection is offered to the [`Listeners`]
/// table ([`Listeners::route_seg`]): it either advances a pending passive-open
/// backlog connection or, for a fresh SYN to a listening port, starts a new passive
/// open (SYN_RCVD). Only segments matching neither a connection nor a listener are
/// dropped. `next_hop_mac` is the reply next-hop for any SYN-ACK a passive open
/// emits. Returns `true` if at least one frame was processed (the caller resets its
/// idle counter so it keeps polling while traffic is flowing).
fn ring_pump(
    conns: &mut RingConns,
    listeners: &mut Listeners,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
) -> bool {
    let mut frame = [0u8; MAX_FRAME];
    let mut pl = [0u8; MAX_FRAME];
    let mut any = false;
    loop {
        match recv_tcp_any(me, &mut frame, &mut pl) {
            RawRx::None => break, // WOULD_BLOCK / error → NIC drained.
            RawRx::Ignore => {
                any = true; // A frame arrived but wasn't ours; keep draining.
            }
            RawRx::Seg(src_ip, src_port, dst_port, rx) => {
                any = true;
                let plen = rx.payload_len.min(pl.len());
                let payload = pl.get(..plen).unwrap_or(&[]);
                if let Some(c) = conns.find_by_tuple(&src_ip, src_port, dst_port) {
                    c.ingest_seg(me, &rx, payload);
                } else {
                    // No established connection owns this segment: offer it to the
                    // listeners (backlog match, or a fresh SYN → passive open).
                    let _consumed = listeners.route_seg(
                        me, &src_ip, src_port, dst_port, &rx, payload, next_hop_mac,
                    );
                    // Unmatched by both → drop (nothing to do).
                }
            }
            RawRx::Seg6(src_ip6, src_port, dst_port, rx) => {
                any = true;
                let plen = rx.payload_len.min(pl.len());
                let payload = pl.get(..plen).unwrap_or(&[]);
                if let Some(c) = conns.find_by_tuple6(&src_ip6, src_port, dst_port) {
                    c.ingest_seg(me, &rx, payload);
                } else {
                    let _consumed = listeners.route_seg6(
                        me, &src_ip6, src_port, dst_port, &rx, payload, next_hop_mac,
                    );
                }
            }
        }
    }
    any
}

/// Execute an `OP_RECV` SQE for connection `target_id`: poll the shared RX pump
/// (so sibling connections' frames are delivered to *them*, not dropped) until the
/// target has data / hits EOF / times out, then copy the target's buffered bytes
/// back into the SQE's data window. Returns bytes received (0 = empty response /
/// EOF), or `-1` on a missing connection or window error.
///
/// If `sqe.aux` has [`netipc::ring::RECV_NONBLOCK`] set, the receive is
/// non-blocking: the RX pump is drained exactly once and, if the target still has
/// no buffered data and the stream is open, [`netipc::ring::ERR_WOULD_BLOCK`] is
/// returned instead of polling — this is how the kernel honours `O_NONBLOCK`.
///
/// Unlike the single-connection [`TcpConn::recv`], this routes through
/// [`ring_pump`] rather than a 4-tuple-filtered read, so concurrent connections on
/// the same ring can all receive without starving one another (D-NETSTACK-RX-DEMUX).
fn ring_tcp_recv(
    ring: &netring::Ring,
    conns: &mut RingConns,
    listeners: &mut Listeners,
    target_id: u32,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    sqe: &netipc::ring::Sqe,
) -> i32 {
    match conns.get_mut(target_id) {
        None => return -1, // no such connection
        // A recv before the handshake completed cannot return stream data; reject so
        // the kernel surfaces ENOTCONN rather than us spinning the whole receive
        // deadline on a half-open connection.
        Some(c) if !c.established => return -1,
        Some(_) => {}
    }
    // A recv after `shutdown(SHUT_RD)` reports EOF immediately, like Linux — the
    // read side is closed, so we neither deliver buffered bytes nor wait.
    if conns.get_mut(target_id).is_some_and(|c| c.read_shut) {
        return 0;
    }
    if sqe.aux & netipc::ring::RECV_NONBLOCK != 0 {
        // Non-blocking receive (kernel honouring O_NONBLOCK): drain whatever has
        // already arrived exactly once, then decide immediately — never poll for
        // the full receive deadline. If nothing is buffered and the stream is
        // still open, report WOULD_BLOCK so the kernel returns EAGAIN; a buffered
        // segment (or EOF) falls through to the shared copy-out below.
        ring_pump(conns, listeners, me, next_hop_mac);
        match conns.get_mut(target_id) {
            Some(c) if c.rx_len == 0 && !c.peer_fin => return netipc::ring::ERR_WOULD_BLOCK,
            Some(_) => {}
            None => return -1,
        }
    } else {
        let mut idle = 0u32;
        let mut retransmits = 0u32;
        loop {
            // Stop as soon as the target has in-order bytes to deliver (a blocking
            // recv returns available data promptly, like `read(2)` — it must not
            // spin the whole receive deadline once data has arrived), or its stream
            // has ended, or we've waited long enough. Without the `rx_len > 0` break
            // a recv burned the full `TCP_DATA_ITERS` deadline even with data already
            // buffered — harmless for a fast path that stayed under the kernel's
            // control-channel timeout, but on the slower IPv6 loopback path it pushed
            // the round past `RECV_TIMEOUT_NS`, surfacing as a spurious `TimedOut`.
            match conns.get_mut(target_id) {
                Some(c) if c.rx_len > 0 || c.peer_fin => break,
                Some(_) => {}
                None => break, // connection vanished (shouldn't happen mid-recv)
            }
            if idle >= TCP_DATA_ITERS {
                break;
            }
            if ring_pump(conns, listeners, me, next_hop_mac) {
                idle = 0;
            } else {
                idle = idle.saturating_add(1);
                if conns
                    .get_mut(target_id)
                    .is_some_and(|c| c.maybe_retransmit(me, idle, &mut retransmits))
                {
                    idle = 0;
                }
                sleep_ns(POLL_SLEEP_NS);
            }
        }
    }
    let off = sqe.data_off as usize;
    let cap = (sqe.data_len as usize).min(MSG_CAP);
    let mut scratch = [0u8; MSG_CAP];
    let out = match scratch.get_mut(..cap) {
        Some(w) => w,
        None => return -1, // recv window larger than our buffer
    };
    // MSG_PEEK: copy buffered bytes out without consuming them, so a later
    // OP_RECV returns the same data again.
    let peek = sqe.aux & netipc::ring::RECV_PEEK != 0;
    let n = match conns.get_mut(target_id) {
        Some(c) => {
            if peek {
                c.peek_rx(out)
            } else {
                c.take_rx(out)
            }
        }
        None => return -1,
    };
    if n > 0 && !ring.write_data(off, &out[..n]) {
        return -1;
    }
    n as i32
}

/// Execute an `OP_POLL` SQE for connection `target_id`: report its readiness
/// **without consuming any buffered data**. The RX pump is drained exactly once
/// (like the non-blocking `OP_RECV` probe) so arrived frames are routed to their
/// owning connections and the target's buffered state reflects the wire, then the
/// target is inspected in place — no bytes are copied into the data window and the
/// receive buffer is left untouched, so a subsequent `OP_RECV` still returns them.
///
/// Returns a non-negative readiness bitmask
/// ([`POLL_READABLE`](netipc::ring::POLL_READABLE) |
/// [`POLL_WRITABLE`](netipc::ring::POLL_WRITABLE) |
/// [`POLL_ERR`](netipc::ring::POLL_ERR)), or `-1` if there is no such connection.
///
/// A connection still completing a non-blocking handshake (SYN_SENT) is reported as
/// *neither* readable nor writable — the kernel keeps waiting for `POLLOUT`. Once
/// ESTABLISHED it is writable *when the send window has room* (no unacknowledged
/// segment outstanding — the single-outstanding-segment sender cannot take a new
/// segment while a prior one is unacked), and readable when it has buffered in-order
/// bytes or the peer has closed (so a `recv` returns data / EOF promptly). If the
/// connect failed (a RST or
/// exhausted SYN budget) it is reported as `POLL_ERR` **and** writable, mirroring
/// Linux — a failed non-blocking connect wakes `poll(POLLOUT)`, then
/// `getsockopt(SO_ERROR)` reveals the error.
///
/// Each poll also advances the handshake's retransmit/timeout driver
/// ([`poll_connect`](TcpConn::poll_connect)) so a lost SYN is resent and an
/// unanswered handshake eventually fails rather than hanging forever.
fn ring_tcp_poll(
    conns: &mut RingConns,
    listeners: &mut Listeners,
    target_id: u32,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
) -> i32 {
    if conns.get_mut(target_id).is_none() {
        return -1; // no such connection
    }
    // Non-destructive: route any arrived frames into their owners (this completes a
    // pending handshake via `ingest_seg` if the SYN-ACK has arrived), then peek.
    ring_pump(conns, listeners, me, next_hop_mac);
    // Drive the SYN retransmit / connect timeout for a still-pending handshake.
    if let Some(c) = conns.get_mut(target_id) {
        c.poll_connect(me);
    }
    match conns.get_mut(target_id) {
        Some(c) => {
            if c.connect_failed {
                // Failed connect: signal POLLOUT (so poll wakes) with the error bit;
                // the kernel then reports getsockopt(SO_ERROR) = ECONNREFUSED.
                return netipc::ring::POLL_ERR | netipc::ring::POLL_WRITABLE;
            }
            if !c.established {
                return 0; // SYN_SENT: not yet writable, keep waiting for POLLOUT.
            }
            // Writable only when the send window has room: our single-outstanding-
            // segment sender cannot accept a new segment while a prior one is still
            // unacknowledged, so reporting POLLOUT then would be a lie (the send
            // would return EAGAIN). This makes poll honest for a full-window socket.
            let mut bits = 0;
            // Writable if the send window has room, or if the write side is shut:
            // in the latter case a send never blocks (it returns EPIPE immediately),
            // so poll must report "ready" rather than "would block".
            if c.write_shut || !c.send_window_full() {
                bits |= netipc::ring::POLL_WRITABLE;
            }
            // Readable if there are buffered bytes or the stream has ended — and a
            // `shutdown(SHUT_RD)` read side counts as ended (recv returns EOF now).
            if c.rx_len > 0 || c.peer_fin || c.read_shut {
                bits |= netipc::ring::POLL_READABLE;
            }
            bits
        }
        None => -1,
    }
}

/// Execute an `OP_ACCEPT` SQE against the listener named by `target_id`: pump the
/// RX path so any pending passive-open handshakes complete, then dequeue the first
/// *established* backlog connection, install it in [`RingConns`] under the
/// caller-chosen id (low 32 bits of `sqe.aux`), and write the peer address
/// `[ip:4][port_be:2]` into the SQE's data window. Returns `0` on success,
/// [`netipc::ring::ERR_WOULD_BLOCK`] if no connection is ready yet, or `-1` on an
/// unknown listener, a too-small data window, or a duplicate/overflowing new id.
fn ring_tcp_accept(
    ring: &netring::Ring,
    conns: &mut RingConns,
    listeners: &mut Listeners,
    target_id: u32,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    sqe: &netipc::ring::Sqe,
) -> i32 {
    if listeners.get_mut(target_id).is_none() {
        return -1; // no such listener
    }
    // Validate the peer-address window up front so we never consume a backlog
    // connection only to fail after — the caller must supply at least 6 bytes.
    if (sqe.data_len as usize) < 6 {
        return -1;
    }
    // Drive pending passive-open handshakes (and any already-queued data) forward.
    ring_pump(conns, listeners, me, next_hop_mac);
    let mut conn = match listeners.get_mut(target_id).and_then(Listener::take_established) {
        Some(c) => c,
        None => return netipc::ring::ERR_WOULD_BLOCK, // accept queue empty
    };
    // Write the peer address into the data window before installing the connection,
    // so a window write error tears the (already-dequeued) connection down cleanly
    // rather than leaking it. An IPv6 peer needs 18 bytes (`[ip6:16][port_be:2]`);
    // when the caller supplied a large-enough window we write that form, else the
    // 6-byte IPv4 form (`[ip:4][port_be:2]`) — an IPv6 peer then reads back as
    // 0.0.0.0, which a v4-only caller ignores anyway.
    let wrote = if let Some(dst6) = conn.dst6 {
        if sqe.data_len as usize >= 18 {
            let mut addr = [0u8; 18];
            addr[..16].copy_from_slice(&dst6);
            addr[16..18].copy_from_slice(&conn.dst_port.to_be_bytes());
            ring.write_data(sqe.data_off as usize, &addr)
        } else {
            let mut addr = [0u8; 6];
            addr[4..6].copy_from_slice(&conn.dst_port.to_be_bytes());
            ring.write_data(sqe.data_off as usize, &addr)
        }
    } else {
        let mut addr = [0u8; 6];
        addr[..4].copy_from_slice(&conn.dst_ip);
        addr[4..6].copy_from_slice(&conn.dst_port.to_be_bytes());
        ring.write_data(sqe.data_off as usize, &addr)
    };
    if !wrote {
        conn.close(me);
        return -1;
    }
    // Install under the caller-chosen id (low 32 bits of aux).
    let new_id = (sqe.aux & 0xFFFF_FFFF) as u32;
    match conns.reserve(new_id) {
        Some(slot) => {
            *slot = Some((new_id, conn));
            0
        }
        None => {
            // Duplicate id or table full: close the accepted connection rather than
            // leak the peer's established half, then report failure.
            conn.close(me);
            -1
        }
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

    // Arm the in-process loopback so frames the daemon addresses to its own IP
    // (server-socket self-connections) circulate internally instead of being
    // dropped by slirp. Inert for every other frame.
    loopback::set_my_ip(me.ip);
    // Same for the daemon's link-local IPv6 identity, so IPv6 self-connections
    // (the OP_CONNECT6 loopback self-test) circulate internally too.
    loopback::set_my_ip6(me.ip6);

    if !raw_open() {
        print("[netstack] FAIL: could not claim raw NIC (SYS_NET_RAW_OPEN)\n");
        exit(3);
    }
    print("[netstack] claimed raw NIC\n");

    // Mode selection via argv[1]. Default = Phase-2 ARP round-trip self-test;
    // `serve-dns` = Phase-4 bounded DNS/TCP/UDP-over-IPC service (register
    // `net.stack`, serve kernel clients, then release the NIC at the idle
    // deadline); `serve-net` = Phase-5 persistent boot daemon (same service, but
    // owns the NIC for the system's lifetime and never idles out — used once the
    // kernel stack has stood down under the `net.userspace` switch).
    let mut arg = [0u8; 32];
    let arglen = read_argv1(&mut arg);
    if &arg[..arglen] == b"serve-dns" {
        print("[netstack] mode: serve-dns (Phase 4 bounded DNS/TCP/UDP-over-IPC)\n");
        let code = run_dns_service(&me, false);
        raw_close();
        print("[netstack] released raw NIC\n");
        exit(code);
    }
    if &arg[..arglen] == b"serve-net" {
        print("[netstack] mode: serve-net (Phase 5 persistent netstack daemon)\n");
        // Persistent: owns the NIC for the system's lifetime. Only returns on an
        // unrecoverable service fault, in which case release the NIC and exit.
        let code = run_dns_service(&me, true);
        raw_close();
        print("[netstack] persistent service returned; released raw NIC\n");
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
