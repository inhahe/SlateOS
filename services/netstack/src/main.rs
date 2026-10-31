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
const SYS_CLOCK_MONOTONIC: u64 = 10;
const SYS_GETRANDOM: u64 = 90;
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
/// Set the multicast Ethernet addresses the network cards pass up (the raw
/// owner's call; kernel `net::mcast_filter`).
const SYS_NET_RAW_MCAST: u64 = 1135;
/// The CPU given up, for a loop that polls.
const SYS_YIELD: u64 = 0;

/// `EAGAIN`/`WouldBlock`: raw RX had no frame ready.
const E_WOULD_BLOCK: i64 = -4;
/// SlateOS's `TimedOut`: a wait, an accept among them, that saw nothing come.
const E_TIMED_OUT: i64 = -6;

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
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

fn sleep_ns(ns: u64) {
    syscall1(SYS_SLEEP, ns);
}

/// Nanoseconds since boot (`SYS_CLOCK_MONOTONIC`): the clock TCP's
/// retransmission timers run on, and every wait's deadline. A clock that
/// cannot be read ends the daemon, said so: every wait is bounded by this
/// clock alone, and a time held still would make each one endless.
fn now_ns() -> u64 {
    let Ok(now) = u64::try_from(syscall0(SYS_CLOCK_MONOTONIC)) else {
        print("[netstack] FATAL: the monotonic clock could not be read\n");
        exit(9);
    };
    now
}

// ---------------------------------------------------------------------------
// A new connection's local port (RFC 6056) and initial sequence number (RFC 6528)
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// The secret that ports and ISNs are keyed with: a 128-bit SipHash key, as two
/// 64-bit halves, drawn once at start-up by [`init_tcp_key`] and never changed.
static TCP_KEY: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// RFC 6056's `next_ephemeral`. One counter for the whole daemon, so a port
/// is not reused until the range has been walked, whichever ring session asked.
///
/// Until 2026-09-27 each connection's port and ISN came from a 16-bit seed that
/// every new ring session restarted from its control-request count. Two sessions
/// a multiple of 16 requests apart gave their first connections the same port
/// *and* the same ISN: a byte-identical SYN for a connection that had just
/// closed, which a peer or NAT still holding that pair may answer with the old
/// connection's reset (known-issues `A-NONBLOCK-CONNECT-GAVE-UP-AFTER-FIVE-POLLS`).
static NEXT_EPHEMERAL: AtomicU32 = AtomicU32::new(0);

/// Draw [`TCP_KEY`] from the kernel's CSPRNG.
///
/// If the kernel refuses, falls back to the clock and our MAC, and says so. The
/// ports and ISNs are then predictable to an attacker who can guess the boot
/// time, but still differ between connections, which is what correctness needs.
fn init_tcp_key(me: &IfInfo) {
    let mut key = [0u8; 16];
    let got = syscall3(SYS_GETRANDOM, key.as_mut_ptr() as u64, key.len() as u64, 0);
    if got != key.len() as i64 {
        print("[netstack] WARN: getrandom refused; TCP ports and ISNs keyed from the clock\n");
        let t = now_ns().to_le_bytes();
        for (i, b) in key.iter_mut().enumerate() {
            *b = t[i % 8] ^ me.mac[i % 6] ^ (i as u8).wrapping_mul(0x9d);
        }
    }
    let (lo, hi) = key.split_at(8);
    let mut half = [0u8; 8];
    half.copy_from_slice(lo);
    TCP_KEY[0].store(u64::from_le_bytes(half), Ordering::Relaxed);
    half.copy_from_slice(hi);
    TCP_KEY[1].store(u64::from_le_bytes(half), Ordering::Relaxed);
}

/// [`TCP_KEY`] as the 16 bytes `netproto::tcp_ids` takes.
fn tcp_key() -> [u8; 16] {
    let mut key = [0u8; 16];
    let (lo, hi) = key.split_at_mut(8);
    lo.copy_from_slice(&TCP_KEY[0].load(Ordering::Relaxed).to_le_bytes());
    hi.copy_from_slice(&TCP_KEY[1].load(Ordering::Relaxed).to_le_bytes());
    key
}

/// A local port for a new connection from `local_ip` to `remote_ip`:`remote_port`,
/// skipping any `in_use` says is taken. `None` when the whole ephemeral range is.
fn pick_local_port(
    local_ip: &[u8],
    remote_ip: &[u8],
    remote_port: u16,
    in_use: impl FnMut(u16) -> bool,
) -> Option<u16> {
    let tuple = netproto::tcp_ids::Tuple {
        local_ip,
        local_port: 0,
        remote_ip,
        remote_port,
    };
    let mut ports =
        netproto::tcp_ids::EphemeralPorts::from_counter(NEXT_EPHEMERAL.load(Ordering::Relaxed));
    let port = ports.pick(&tcp_key(), &tuple, in_use);
    NEXT_EPHEMERAL.store(ports.counter(), Ordering::Relaxed);
    port
}

/// The initial sequence number for a new connection with this 4-tuple.
fn new_isn(local_ip: &[u8], local_port: u16, remote_ip: &[u8], remote_port: u16) -> u32 {
    let tuple = netproto::tcp_ids::Tuple {
        local_ip,
        local_port,
        remote_ip,
        remote_port,
    };
    netproto::tcp_ids::isn(now_ns(), &tcp_key(), &tuple)
}

/// Whether a reset received during our SYN-SENT refuses the connect.
///
/// RFC 793 (SYN-SENT, "first check the ACK bit"): only a reset that
/// acknowledges our SYN is acceptable. Any other belongs to an earlier
/// connection on the same port pair and is dropped, or a stale reset could
/// refuse a connection that the peer is in fact accepting.
fn rst_refuses_syn(rx: &TcpRx, isn: u32) -> bool {
    rx.flags & tcp::FLAG_RST != 0 && rx.flags & tcp::FLAG_ACK != 0 && rx.ack == isn.wrapping_add(1)
}

// ---------------------------------------------------------------------------
// Raw-frame helpers
// ---------------------------------------------------------------------------

/// Claim the kernel's network: its primary card, and every card it reads.
/// Returns `true` on success.
fn raw_open() -> bool {
    // arg0 0 = the primary claim; arg1 is read only for arg0 1, a card alone
    // ([`nic_bench_card`]).
    syscall2(SYS_NET_RAW_OPEN, 0, 0) >= 0
}

fn raw_close() {
    syscall0(SYS_NET_RAW_CLOSE);
}

/// A network card a driver process serves: when the daemon has attached to
/// one, its frames go there rather than to the kernel's NIC.
mod port;

/// The daemon's card, chosen once: every use of it after.
mod nic;

/// Waiting on several kernel objects at once (`SYS_WAIT_MULTIPLE`).
mod wait;

/// In-process software loopback for frames the daemon sends to *its own IP*.
///
/// The daemon is otherwise a pure NIC client: everything it emits goes out the
/// wire and it never receives a frame it sent. Server sockets break that
/// assumption — to test (and to support) a connection to `me.ip:port` served by a
/// [`Listener`] in the *same* daemon, the SYN/data/ACK/FIN frames must circulate
/// internally rather than hit slirp (which silently drops a host-to-self packet).
///
/// [`nic::send`] calls [`maybe_divert`](loopback::maybe_divert): if the outgoing
/// frame is IPv4 with `dst == my_ip` (and `my_ip` has been set), the whole frame
/// is copied into a small FIFO and *not* sent on the NIC. [`nic::recv`] calls
/// [`maybe_deliver`](loopback::maybe_deliver) first, draining that FIFO ahead of
/// the physical NIC. The demux downstream matches by IP + 4-tuple, so both ends of
/// a loopback connection coexist in the one daemon without collision.
///
/// Single-threaded daemon → a plain `static mut` guarded by a "not reentrant"
/// discipline (we never hold a `&`/`&mut` across a `nic::send`/`nic::recv`) is sound; the
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
    /// at startup before any `nic::send`/`nic::recv`.
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

use netproto::{
    arp, dhcp_client, dns, ethernet, icmp, icmpv6, igmp, ipv4, ipv6, mcast, mld, tcp, udp,
};

/// Most multicast groups one UDP socket can join (Linux allows 20 by
/// default, `net.ipv4.igmp_max_memberships`).
const MAX_SOCK_GROUPS: usize = 8;
/// Most multicast groups the host can be in at once, over every socket.
const MAX_HOST_GROUPS: usize = 32;
/// Most addresses the cards' multicast filter holds for us: the host's
/// groups, plus all-hosts, all-nodes and our solicited-node group.
const MAX_FILTER: usize = MAX_HOST_GROUPS + 3;

/// The multicast filter last given to the kernel ([`sync_mcast_filter`]):
/// sorted, no duplicates, `len` of `addrs` in use. `pushed` is false until a
/// push succeeds, so a refused one is retried at the next change.
struct McastFilter {
    addrs: [[u8; 6]; MAX_FILTER],
    len: usize,
    pushed: bool,
}

/// Add `mac` to the first `*len` entries of `set` unless it is there or the
/// set is full.
fn push_unique(set: &mut [[u8; 6]; MAX_FILTER], len: &mut usize, mac: [u8; 6]) {
    if set.get(..*len).is_some_and(|s| s.contains(&mac)) {
        return;
    }
    if let Some(slot) = set.get_mut(*len) {
        *slot = mac;
        *len += 1;
    }
}

/// Tell the card which multicast addresses it must pass ([`nic::set_mcast`]):
/// all-hosts (IGMP queries), all-nodes (MLD queries, router
/// advertisements), our solicited-node group (neighbour discovery) and every
/// group a socket is in -- when that set has changed since the last push,
/// or a driver process's card refused the last list ([`nic::mcast_refused`]).
/// Without it an e1000 or rtl8139 card drops every multicast frame (kernel
/// known-issues `A-E1000-AND-RTL8139-DROP-EVERY-MULTICAST-FRAME`).
fn sync_mcast_filter(net: &mut Net, me: &IfInfo) {
    let mut set = [[0u8; 6]; MAX_FILTER];
    let mut len = 0;
    push_unique(&mut set, &mut len, igmp::multicast_mac(&igmp::ALL_HOSTS));
    push_unique(
        &mut set,
        &mut len,
        mld::multicast_mac(&icmpv6::ALL_NODES_LINK_LOCAL),
    );
    push_unique(
        &mut set,
        &mut len,
        mld::multicast_mac(&icmpv6::solicited_node_multicast(&me.ip6)),
    );
    for g in net.mcast.iter() {
        push_unique(&mut set, &mut len, g.mac());
    }
    let wanted = set.get_mut(..len).unwrap_or(&mut []);
    wanted.sort_unstable();
    let have = &net.filter;
    if have.pushed && !nic::mcast_refused() && have.addrs.get(..have.len) == Some(&*wanted) {
        return;
    }
    if !nic::set_mcast(wanted) {
        // The card keeps the previous filter; the next change retries.
        print("[netstack] WARN: the card's multicast filter could not be set\n");
        net.filter.pushed = false;
        return;
    }
    net.filter = McastFilter {
        addrs: set,
        len,
        pushed: true,
    };
}

// The kernel answers `getsockopt` for a socket the daemon holds nothing for
// yet from `netipc::sockopt::default_value`; a new socket here starts from
// `mcast::SocketGroups::new`. The two must be the same values.
const _: () = {
    use netipc::ring as r;
    use netipc::sockopt::default_value;
    let d = mcast::SocketGroups::<1>::new();
    assert!(matches!(default_value(r::UDP_OPT_MCAST_TTL), Some(v) if v == d.ttl as i32));
    assert!(matches!(default_value(r::UDP_OPT_MCAST_HOPS), Some(v) if v == d.hops as i32));
    assert!(matches!(default_value(r::UDP_OPT_MCAST_LOOP4), Some(v) if v == d.loop4 as i32));
    assert!(matches!(default_value(r::UDP_OPT_MCAST_LOOP6), Some(v) if v == d.loop6 as i32));
};

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
                    let _ = nic::send(&reply);
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
    buf[ethernet::HEADER_LEN..ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN].copy_from_slice(&ip_hdr);

    // Ethernet header: unicast the reply straight back to the requester's MAC.
    ethernet::write_header(&mut buf, dst_mac, &me.mac, ethernet::ETHERTYPE_IPV4);

    let total = icmp_off + icmp_len;
    let _ = nic::send(&buf[..total]);
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

/// How long an ARP, NDP, DNS or UDP answer -- and the answer to each TCP
/// SYN -- is waited for. A deadline, so that a wait woken early by someone
/// else's frame ([`nic::idle`]) is not cut short.
const RESOLVE_WAIT_NS: u64 = 600_000_000;

/// ARP-resolve `target_ip` on the local link: broadcast a request and poll raw
/// RX for the reply, returning the sender's MAC. `None` on timeout.
fn arp_resolve(target_ip: &[u8; 4], me: &IfInfo) -> Option<[u8; 6]> {
    let req = arp::request(&me.mac, &me.ip, target_ip);
    if nic::send(&req) < 0 {
        return None;
    }
    let mut buf = [0u8; MAX_FRAME];
    let until = now_ns().saturating_add(RESOLVE_WAIT_NS);
    while now_ns() < until {
        loop {
            let n = nic::recv(&mut buf);
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
        nic::idle();
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
    if nic::send(&frame[..total]) < 0 {
        return None;
    }

    let mut buf = [0u8; MAX_FRAME];
    let until = now_ns().saturating_add(RESOLVE_WAIT_NS);
    while now_ns() < until {
        loop {
            let n = nic::recv(&mut buf);
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
        nic::idle();
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
    if na.target == *target_ip6 {
        na.link_addr
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
    let udp_len = match udp::write(
        &mut frame[udp_off..],
        &me.ip,
        &me.dns,
        EPHEMERAL_PORT,
        53,
        qbuf,
    ) {
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
    nic::send(&frame[..total]) >= 0
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
    let until = now_ns().saturating_add(RESOLVE_WAIT_NS);
    while now_ns() < until {
        loop {
            let n = nic::recv(&mut buf);
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
        nic::idle();
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
    let until = now_ns().saturating_add(RESOLVE_WAIT_NS);
    while now_ns() < until {
        loop {
            let n = nic::recv(&mut buf);
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
        nic::idle();
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

/// How long a TCP wait allows a connection to stay quiet -- nothing heard
/// from it -- before it gives up on more data: timed from the last frame,
/// not counted in polls, so that a wait woken early by someone else's
/// frame ([`nic::idle`]) is not cut short.
const TCP_QUIET_NS: u64 = 2_000_000_000;

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
    send_ipv4_ttl(me, next_hop_mac, dst_ip, proto, l4, id, 64)
}

/// [`send_ipv4`] with an explicit TTL: a multicast send uses its socket's
/// (`IP_MULTICAST_TTL`, default 1) and the group's Ethernet address.
fn send_ipv4_ttl(
    me: &IfInfo,
    dst_mac: &[u8; 6],
    dst_ip: &[u8; 4],
    proto: u8,
    l4: &[u8],
    id: u16,
    ttl: u8,
) -> bool {
    let next_hop_mac = dst_mac;
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
        ttl,
        protocol: proto,
        src: me.ip,
        dst: *dst_ip,
    }
    .build_header(l4.len() as u16);
    frame[ethernet::HEADER_LEN..l4_off].copy_from_slice(&ip_hdr);
    ethernet::write_header(&mut frame, next_hop_mac, &me.mac, ethernet::ETHERTYPE_IPV4);
    nic::send(&frame[..total]) >= 0
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
    let builder = tcp::Builder {
        src_port,
        dst_port,
        seq,
        ack,
        flags,
        window: TCP_WINDOW,
    };
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
    send_ipv6_hops(me, next_hop_mac, dst_ip6, next_header, l4, 64)
}

/// [`send_ipv6`] with an explicit hop limit: a multicast send uses its
/// socket's (`IPV6_MULTICAST_HOPS`, default 1) and the group's Ethernet
/// address.
fn send_ipv6_hops(
    me: &IfInfo,
    dst_mac: &[u8; 6],
    dst_ip6: &[u8; 16],
    next_header: u8,
    l4: &[u8],
    hop_limit: u8,
) -> bool {
    let next_hop_mac = dst_mac;
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
        hop_limit,
        src: me.ip6,
        dst: *dst_ip6,
    }
    .build_header(l4.len() as u16);
    frame[ethernet::HEADER_LEN..l4_off].copy_from_slice(&ip_hdr);
    ethernet::write_header(&mut frame, next_hop_mac, &me.mac, ethernet::ETHERTYPE_IPV6);
    nic::send(&frame[..total]) >= 0
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
    let builder = tcp::Builder {
        src_port,
        dst_port,
        seq,
        ack,
        flags,
        window: TCP_WINDOW,
    };
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
    let n = nic::recv(frame);
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
    Some(TcpRx {
        seq: seg.seq,
        ack: seg.ack,
        flags: seg.flags,
        payload_len: cplen,
    })
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
    let n = nic::recv(frame);
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
    Some(TcpRx {
        seq: seg.seq,
        ack: seg.ack,
        flags: seg.flags,
        payload_len: cplen,
    })
}

/// Outcome of one non-filtered NIC read for the multiplexed RX pump.
enum RawRx {
    /// Not a TCP segment addressed to us.
    Ignore,
    /// A TCP segment addressed to us: peer identity `(src_ip, src_port, dst_port)`
    /// for routing to the owning connection, plus the parsed segment metadata. The
    /// payload has been copied into the caller's `pl` buffer (`payload_len` bytes).
    Seg([u8; 4], u16, u16, TcpRx),
    /// An IPv6 TCP segment addressed to us: peer identity `(src_ip6, src_port,
    /// dst_port)` plus the parsed segment metadata. The IPv6 sibling of [`Seg`].
    Seg6([u8; 16], u16, u16, TcpRx),
}

/// Classify one frame [`recv_any`] read off the NIC, if it is a TCP segment
/// addressed to us, without filtering to a specific connection's 4-tuple, so the
/// caller can *route* it to whichever connection owns it. This is the
/// shared-RX-demux counterpart to [`recv_tcp_seg`]: where that function drops any
/// frame not matching one connection, this one hands back the peer identity so a
/// sibling connection's frames are delivered to *it* instead of being lost.
fn parse_tcp(me: &IfInfo, bytes: &[u8], pl: &mut [u8]) -> RawRx {
    let eth = match ethernet::Frame::parse(bytes) {
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
                pl.get_mut(..cplen)?
                    .copy_from_slice(seg.payload.get(..cplen)?);
                Some((
                    ip.src,
                    seg.src_port,
                    seg.dst_port,
                    TcpRx {
                        seq: seg.seq,
                        ack: seg.ack,
                        flags: seg.flags,
                        payload_len: cplen,
                    },
                ))
            })();
            match parsed {
                Some((src_ip, src_port, dst_port, rx)) => {
                    RawRx::Seg(src_ip, src_port, dst_port, rx)
                }
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
                pl.get_mut(..cplen)?
                    .copy_from_slice(seg.payload.get(..cplen)?);
                Some((
                    ip.src,
                    seg.src_port,
                    seg.dst_port,
                    TcpRx {
                        seq: seg.seq,
                        ack: seg.ack,
                        flags: seg.flags,
                        payload_len: cplen,
                    },
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

/// A received UDP datagram's routing identity, produced by [`parse_udp`]: the
/// sender's address (`family` + fixed 16-byte `src_ip`, IPv4 in `src_ip[0..4]`)
/// and port, plus our local `dst_port`. The payload has been copied into the
/// caller's `pl` buffer (`payload_len` bytes; a longer datagram is truncated to
/// the buffer, matching `recvfrom`).
struct UdpRx {
    family: u16,
    src_ip: [u8; 16],
    src_port: u16,
    dst_port: u16,
    payload_len: usize,
    /// The multicast group the datagram was sent to, when it was sent to one:
    /// it goes only to a socket that joined the group.
    group: Option<mcast::Group>,
}

/// Classify one frame [`recv_any`] read off the NIC: if it is a UDP datagram
/// (IPv4 *or* IPv6) addressed to us, return its routing identity (the
/// [`parse_tcp`] sibling for UDP). Any other frame yields `None`.
///
/// [`recv_any`] reads each frame once and offers it to both parsers, so a
/// datagram is never consumed by a TCP drain or a segment by a UDP one. Both IPv4
/// (`ETHERTYPE_IPV4` → [`ipv4::Packet`]) and IPv6 (`ETHERTYPE_IPV6` →
/// [`ipv6::Packet`]) UDP datagrams are classified; the family is reported so
/// `OP_UDP_RECV` can report the correct `sockaddr` family.
fn parse_udp(me: &IfInfo, bytes: &[u8], pl: &mut [u8]) -> Option<UdpRx> {
    let eth = ethernet::Frame::parse(bytes)?;
    match eth.ethertype {
        ethernet::ETHERTYPE_IPV4 => {
            let ip = ipv4::Packet::parse(eth.payload)?;
            // Ours, or a multicast group's: `pump` gives a group's datagram
            // only to a socket that joined it.
            let group = igmp::is_multicast(&ip.dst).then_some(mcast::Group::V4(ip.dst));
            if ip.protocol != ipv4::PROTO_UDP || (ip.dst != me.ip && group.is_none()) {
                return None;
            }
            let dg = udp::Datagram::parse(ip.payload, &ip.src, &ip.dst)?;
            let cplen = dg.payload.len().min(pl.len());
            pl.get_mut(..cplen)?
                .copy_from_slice(dg.payload.get(..cplen)?);
            let mut src_ip = [0u8; 16];
            src_ip[..4].copy_from_slice(&ip.src);
            Some(UdpRx {
                family: netipc::ring::UDP_AF_INET,
                src_ip,
                src_port: dg.src_port,
                dst_port: dg.dst_port,
                payload_len: cplen,
                group,
            })
        }
        ethernet::ETHERTYPE_IPV6 => {
            let ip = ipv6::Packet::parse(eth.payload)?;
            let group = mld::is_multicast(&ip.dst).then_some(mcast::Group::V6(ip.dst));
            if ip.next_header != ipv4::PROTO_UDP || (ip.dst != me.ip6 && group.is_none()) {
                return None;
            }
            let dg = udp::Datagram::parse_v6(ip.payload, &ip.src, &ip.dst)?;
            let cplen = dg.payload.len().min(pl.len());
            pl.get_mut(..cplen)?
                .copy_from_slice(dg.payload.get(..cplen)?);
            Some(UdpRx {
                family: netipc::ring::UDP_AF_INET6,
                src_ip: ip.src,
                src_port: dg.src_port,
                dst_port: dg.dst_port,
                payload_len: cplen,
                group,
            })
        }
        _ => None,
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
    /// `true` if the connect attempt failed after `connect_start`: a reset that
    /// acknowledged our SYN, or a SYN that went unanswered through every
    /// retransmission `rtx` allows. Surfaced to the kernel as a `POLL_ERR`
    /// readiness bit → `getsockopt(SO_ERROR) = ECONNREFUSED`.
    connect_failed: bool,
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
    /// FIN has been emitted and subsequent [`OP_SEND`](netipc::ring::OP_SEND)
    /// submissions are rejected with
    /// [`ERR_BROKEN_PIPE`](netipc::ring::ERR_BROKEN_PIPE). The read side stays
    /// open, so `recv` keeps delivering buffered/inbound bytes until the peer FIN.
    write_shut: bool,
    /// `true` once `shutdown(SHUT_RD)`/`SHUT_RDWR` has closed the read side:
    /// subsequent [`OP_RECV`](netipc::ring::OP_RECV) completes with EOF (`0`)
    /// without waiting. The write side stays open, so `send` still works until
    /// [`OP_CLOSE`](netipc::ring::OP_CLOSE).
    read_shut: bool,
    /// When the segment in `snd_buf` was last sent, and how often it has been
    /// resent: the retransmission timer, whose policy is `netproto::tcp_rtx`.
    /// Serviced by [`service_retransmit`](Self::service_retransmit) whenever the
    /// daemon serves this connection at all.
    rtx: netproto::tcp_rtx::Retransmit,
    /// `true` once every resend of the in-flight segment went unanswered: the
    /// peer is presumed gone. A receive with nothing buffered and a send then
    /// answer [`ERR_TIMED_OUT`](netipc::ring::ERR_TIMED_OUT) (`ETIMEDOUT`), and
    /// a poll reports the error bit.
    timed_out: bool,
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
    /// one connection (the multiplexed ring path reads through [`pump`] instead).
    fn recv_one_seg(&self, me: &IfInfo, frame: &mut [u8], pl: &mut [u8]) -> Option<TcpRx> {
        match self.dst6 {
            Some(ref dst6) => recv_tcp_seg6(me, dst6, self.local_port, self.dst_port, frame, pl),
            None => recv_tcp_seg(me, &self.dst_ip, self.local_port, self.dst_port, frame, pl),
        }
    }

    /// Open a connection from `local_port` to `dst_ip:dst_port` via `mac`, seeding
    /// the IPv4 ident counter from `seed_ipid`. Performs the SYN / SYN-ACK / ACK
    /// handshake. Returns `None` if the peer refused (a reset acknowledging our
    /// SYN) or never answered.
    ///
    /// `local_port` comes from [`pick_local_port`] and the ISN from [`new_isn`], so
    /// successive connections to the *same* server do not reuse a 4-tuple and
    /// sequence space a server may still hold in TIME_WAIT.
    fn connect(
        me: &IfInfo,
        dst_ip: [u8; 4],
        dst_port: u16,
        mac: [u8; 6],
        seed_ipid: u16,
        local_port: u16,
    ) -> Option<Self> {
        let isn = new_isn(&me.ip, local_port, &dst_ip, dst_port);
        let mut ipid = seed_ipid;

        let mut frame = [0u8; MAX_FRAME];
        let mut pl = [0u8; MAX_FRAME];

        // --- Handshake: (re)transmit SYN until a SYN-ACK arrives. ---
        let mut server_isn = 0u32;
        let mut established = false;
        'syn: for _ in 0..TCP_SYN_ATTEMPTS {
            ipid = ipid.wrapping_add(1);
            if !send_tcp(
                me,
                &mac,
                &dst_ip,
                dst_port,
                local_port,
                isn,
                0,
                tcp::FLAG_SYN,
                &[],
                ipid,
            ) {
                return None;
            }
            let until = now_ns().saturating_add(RESOLVE_WAIT_NS);
            while now_ns() < until {
                while let Some(rx) =
                    recv_tcp_seg(me, &dst_ip, local_port, dst_port, &mut frame, &mut pl)
                {
                    if rx.flags & tcp::FLAG_RST != 0 {
                        if rst_refuses_syn(&rx, isn) {
                            return None; // Connection refused.
                        }
                        continue; // An earlier connection's reset: not ours.
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
                nic::idle();
            }
        }
        if !established {
            return None;
        }

        let snd_nxt = isn.wrapping_add(1); // Our SYN consumed one sequence number.
        let rcv_nxt = server_isn.wrapping_add(1); // Their SYN consumed one.

        // ACK the SYN-ACK, completing the handshake.
        ipid = ipid.wrapping_add(1);
        send_tcp(
            me,
            &mac,
            &dst_ip,
            dst_port,
            local_port,
            snd_nxt,
            rcv_nxt,
            tcp::FLAG_ACK,
            &[],
            ipid,
        );

        Some(TcpConn {
            dst_ip,
            dst6: None,
            dst_port,
            local_port,
            mac,
            snd_nxt,
            rcv_nxt,
            ipid,
            isn,
            established: true,
            connect_failed: false,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: false,
            write_shut: false,
            read_shut: false,
            rtx: netproto::tcp_rtx::Retransmit::new(),
            timed_out: false,
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
        local_port: u16,
    ) -> Option<Self> {
        // The same port / ISN choice as the synchronous path, so a non-blocking
        // connect to a server recently torn down still avoids a TIME_WAIT clash.
        let isn = new_isn(&me.ip, local_port, &dst_ip, dst_port);
        let ipid = seed_ipid.wrapping_add(1);
        if !send_tcp(
            me,
            &mac,
            &dst_ip,
            dst_port,
            local_port,
            isn,
            0,
            tcp::FLAG_SYN,
            &[],
            ipid,
        ) {
            return None;
        }
        let mut conn = TcpConn {
            dst_ip,
            dst6: None,
            dst_port,
            local_port,
            mac,
            // Provisional; recomputed from `isn` when the handshake completes.
            snd_nxt: isn.wrapping_add(1),
            rcv_nxt: 0,
            ipid,
            isn,
            established: false,
            connect_failed: false,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: false,
            write_shut: false,
            read_shut: false,
            rtx: netproto::tcp_rtx::Retransmit::new(),
            timed_out: false,
        };
        // The SYN is the segment in flight until the SYN-ACK acknowledges it:
        // `poll_connect` resends it on this clock, and gives up when it does.
        conn.rtx.sent(now_ns());
        Some(conn)
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
        let isn = new_isn(&me.ip, local_port, &peer_ip, peer_port);
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
            ipid,
            isn,
            established: false, // SYN_RCVD until the peer ACKs our SYN-ACK.
            connect_failed: false,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: true,
            write_shut: false,
            read_shut: false,
            rtx: netproto::tcp_rtx::Retransmit::new(),
            timed_out: false,
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
        local_port: u16,
    ) -> Option<Self> {
        let isn = new_isn(&me.ip6, local_port, &dst_ip6, dst_port);

        let mut frame = [0u8; MAX_FRAME];
        let mut pl = [0u8; MAX_FRAME];

        let mut server_isn = 0u32;
        let mut established = false;
        'syn: for _ in 0..TCP_SYN_ATTEMPTS {
            if !send_tcp6(
                me,
                &mac,
                &dst_ip6,
                dst_port,
                local_port,
                isn,
                0,
                tcp::FLAG_SYN,
                &[],
            ) {
                return None;
            }
            let until = now_ns().saturating_add(RESOLVE_WAIT_NS);
            while now_ns() < until {
                while let Some(rx) =
                    recv_tcp_seg6(me, &dst_ip6, local_port, dst_port, &mut frame, &mut pl)
                {
                    if rx.flags & tcp::FLAG_RST != 0 {
                        if rst_refuses_syn(&rx, isn) {
                            return None; // Connection refused.
                        }
                        continue; // An earlier connection's reset: not ours.
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
                nic::idle();
            }
        }
        if !established {
            return None;
        }

        let snd_nxt = isn.wrapping_add(1);
        let rcv_nxt = server_isn.wrapping_add(1);
        send_tcp6(
            me,
            &mac,
            &dst_ip6,
            dst_port,
            local_port,
            snd_nxt,
            rcv_nxt,
            tcp::FLAG_ACK,
            &[],
        );

        Some(TcpConn {
            dst_ip: [0; 4],
            dst6: Some(dst_ip6),
            dst_port,
            local_port,
            mac,
            snd_nxt,
            rcv_nxt,
            ipid: seed_ipid,
            isn,
            established: true,
            connect_failed: false,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: false,
            write_shut: false,
            read_shut: false,
            rtx: netproto::tcp_rtx::Retransmit::new(),
            timed_out: false,
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
        local_port: u16,
    ) -> Option<Self> {
        let isn = new_isn(&me.ip6, local_port, &dst_ip6, dst_port);
        if !send_tcp6(
            me,
            &mac,
            &dst_ip6,
            dst_port,
            local_port,
            isn,
            0,
            tcp::FLAG_SYN,
            &[],
        ) {
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
            ipid: seed_ipid,
            isn,
            established: false,
            connect_failed: false,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: false,
            write_shut: false,
            read_shut: false,
            rtx: netproto::tcp_rtx::Retransmit::new(),
            timed_out: false,
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
        let isn = new_isn(&me.ip6, local_port, &peer_ip6, peer_port);
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
            ipid: seed_ipid,
            isn,
            established: false,
            connect_failed: false,
            peer_fin: false,
            snd_buf: [0u8; TCP_SND_BUF],
            snd_buf_len: 0,
            rx_buf: [0u8; TCP_RCV_BUF],
            rx_len: 0,
            passive: true,
            write_shut: false,
            read_shut: false,
            rtx: netproto::tcp_rtx::Retransmit::new(),
            timed_out: false,
        })
    }

    /// Advance a non-blocking connect's handshake: while still `SYN_SENT`, resend
    /// the SYN when `rtx`'s timeout for it expires (in case it or the SYN-ACK was
    /// lost), and mark the attempt failed when `rtx` gives up. The actual
    /// handshake *completion* (on a received SYN-ACK) happens in
    /// [`ingest_seg`](Self::ingest_seg); this is only the retransmit/timeout
    /// driver, called on every `OP_POLL`, however often. No-op once the
    /// connection is established or already failed.
    fn poll_connect(&mut self, me: &IfInfo) {
        if self.established || self.connect_failed {
            return;
        }
        // On `rtx`'s clock, not per call. This used to resend the SYN on every
        // OP_POLL and fail the connect after five, so how *often* a caller
        // polled decided whether a connect to a live server survived: five polls
        // inside one round trip -- a tight loop, or a far server -- refused it
        // (known-issues `A-NONBLOCK-CONNECT-GAVE-UP-AFTER-FIVE-POLLS`).
        match self.rtx.poll(now_ns()) {
            netproto::tcp_rtx::RtxAction::Wait => {}
            netproto::tcp_rtx::RtxAction::Resend => {
                self.ipid = self.ipid.wrapping_add(1);
                self.emit(me, self.isn, 0, tcp::FLAG_SYN, &[]);
            }
            netproto::tcp_rtx::RtxAction::GiveUp => {
                self.connect_failed = true; // The SYN went unanswered.
            }
        }
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
        if !self.emit(
            me,
            self.snd_nxt,
            self.rcv_nxt,
            tcp::FLAG_PSH | tcp::FLAG_ACK,
            payload,
        ) {
            return None;
        }
        // Retain for retransmit; `snd_nxt - snd_buf_len` recovers this seq.
        self.snd_buf[..payload.len()].copy_from_slice(payload);
        self.snd_buf_len = payload.len();
        self.snd_nxt = self.snd_nxt.wrapping_add(payload.len() as u32);
        self.rtx.sent(now_ns());
        Some(payload.len())
    }

    /// Ingest one parsed TCP segment into this connection's receive state: buffer
    /// in-order payload into `rx_buf`, advance the cumulative ACK point, honor a
    /// FIN or RST, and emit an ACK as needed. This is the single point of TCP
    /// receive logic, shared by the one-connection [`recv`](Self::recv) loop and
    /// the multiplexed [`pump`] — so a segment routed here by the shared RX
    /// demux is processed identically whether or not this connection is the one
    /// currently blocked in a receive. `payload` is the segment's data bytes
    /// (length equals `rx.payload_len`).
    fn ingest_seg(&mut self, me: &IfInfo, rx: &TcpRx, payload: &[u8]) {
        if !self.established {
            if self.connect_failed {
                return;
            }
            if rx.flags & tcp::FLAG_RST != 0 {
                // SYN-RECEIVED (passive): a reset aborts the half-open connection.
                // SYN-SENT: only one acknowledging our SYN refuses it; any other
                // belongs to an earlier connection on this port pair (RFC 793).
                if self.passive || rst_refuses_syn(rx, self.isn) {
                    self.connect_failed = true; // Refused → surfaces as POLL_ERR.
                }
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
                    self.emit(
                        me,
                        self.isn,
                        self.rcv_nxt,
                        tcp::FLAG_SYN | tcp::FLAG_ACK,
                        &[],
                    );
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
                    self.snd_nxt = self.isn.wrapping_add(1); // Our SYN consumed one seq.
                    self.rcv_nxt = rx.seq.wrapping_add(1); // Their SYN consumed one.
                    self.ipid = self.ipid.wrapping_add(1);
                    self.emit(me, self.snd_nxt, self.rcv_nxt, tcp::FLAG_ACK, &[]);
                    self.established = true;
                    self.rtx.acked(); // The SYN was the segment in flight.
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
        //
        // After `shutdown(SHUT_WR)` or `close` the peer's ACK also covers our
        // FIN, which occupies one sequence number that `snd_nxt` does not count
        // (see [`shutdown`](Self::shutdown)), so it arrives as `snd_nxt + 1`.
        // Before the retransmission timer ran on its own, an outstanding segment
        // left uncleared that way was harmless; now it would be resent until the
        // connection timed out.
        let acks_all =
            rx.ack == self.snd_nxt || (self.write_shut && rx.ack == self.snd_nxt.wrapping_add(1));
        if self.snd_buf_len > 0 && rx.flags & tcp::FLAG_ACK != 0 && acks_all {
            self.snd_buf_len = 0;
            self.rtx.acked();
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

    /// Resend the in-flight segment if its acknowledgement is overdue, or mark
    /// the connection timed out once every resend has gone unanswered -- the
    /// policy is `netproto::tcp_rtx`. Returns `true` if it resent.
    ///
    /// Run for every live connection by [`pump`], and by the one-shot
    /// [`recv`](Self::recv), so the timer advances whenever the daemon serves
    /// the connection at all -- including for a kernel that waits by asking
    /// without blocking. It used to run only inside the blocking receive and
    /// send loops, on a counter local to one call (resend at the 40th idle
    /// 5 ms poll, at most three times, and only before any reply had arrived),
    /// so a lost segment was resent only if some caller happened to be blocked
    /// on that very connection.
    fn service_retransmit(&mut self, me: &IfInfo, now: u64) -> bool {
        if self.snd_buf_len == 0 || !self.established || self.timed_out {
            return false;
        }
        match self.rtx.poll(now) {
            netproto::tcp_rtx::RtxAction::Wait => false,
            netproto::tcp_rtx::RtxAction::Resend => {
                self.ipid = self.ipid.wrapping_add(1);
                let seq = self.snd_nxt.wrapping_sub(self.snd_buf_len as u32);
                self.emit(
                    me,
                    seq,
                    self.rcv_nxt,
                    tcp::FLAG_PSH | tcp::FLAG_ACK,
                    &self.snd_buf[..self.snd_buf_len],
                );
                true
            }
            netproto::tcp_rtx::RtxAction::GiveUp => {
                self.timed_out = true;
                false
            }
        }
    }

    /// Receive up to `out.len()` bytes of in-order stream data, cumulative-ACKing
    /// and honoring the peer's FIN. Retransmits the buffered send segment a few
    /// times early in the idle window if nothing comes back. Returns the number
    /// of bytes written to `out` (0 is valid: empty response or immediate EOF).
    ///
    /// This is the single-connection path (used by the one-shot [`tcp_fetch`]
    /// control op, where no sibling connections exist to demux for). The ring
    /// socket path uses [`ring_tcp_recv`] + [`pump`], which route to multiple
    /// connections but share the same [`ingest_seg`](Self::ingest_seg) core.
    fn recv(&mut self, me: &IfInfo, out: &mut [u8]) -> usize {
        let mut frame = [0u8; MAX_FRAME];
        let mut pl = [0u8; MAX_FRAME];

        let mut heard = now_ns();
        while now_ns().saturating_sub(heard) < TCP_QUIET_NS && !self.peer_fin && !self.timed_out {
            let mut got = false;
            while let Some(rx) = self.recv_one_seg(me, &mut frame, &mut pl) {
                got = true;
                self.ingest_seg(me, &rx, &pl[..rx.payload_len]);
                if self.peer_fin {
                    break;
                }
            }
            if got {
                heard = now_ns();
            } else {
                if self.service_retransmit(me, now_ns()) {
                    heard = now_ns();
                }
                nic::idle();
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
            self.emit(
                me,
                self.snd_nxt,
                self.rcv_nxt,
                tcp::FLAG_FIN | tcp::FLAG_ACK,
                &[],
            );
        }
    }

    /// Start closing: send our FIN, unless a `shutdown(SHUT_WR)` already has,
    /// and return. The peer's answer -- its ACK, its own FIN -- is handled
    /// while the connection lingers in [`Closing`], through the pump, with the
    /// daemon serving everyone else meanwhile.
    ///
    /// Until 2026-09-27 the close waited here: it read up to 40 frames off the
    /// wire, sleeping between, to acknowledge the peer's FIN. Two faults. The
    /// daemon serves one request at a time, so every close held up every
    /// socket for at least 200 ms, and far longer under emulation. And the
    /// reads were the per-connection kind, which drop any frame that is not
    /// this connection's -- a close ate its siblings' traffic. rq15's
    /// late-data witness waited 12 s for bytes that way (known-issues
    /// `A-NETSTACK-CLOSE-BLOCKED-THE-DAEMON-AND-ATE-OTHERS-FRAMES`).
    fn begin_close(&mut self, me: &IfInfo) {
        if !self.write_shut {
            self.ipid = self.ipid.wrapping_add(1);
            self.emit(
                me,
                self.snd_nxt,
                self.rcv_nxt,
                tcp::FLAG_FIN | tcp::FLAG_ACK,
                &[],
            );
            self.write_shut = true;
        }
    }

    /// A segment for this connection after its owner closed it (it lingers in
    /// [`Closing`]): acknowledge the peer's FIN, as the old blocking close did
    /// while it waited. `true` once that is done and nothing is left to answer.
    fn closing_seg(&mut self, me: &IfInfo, rx: &TcpRx) -> bool {
        if rx.flags & tcp::FLAG_FIN != 0 && rx.seq == self.rcv_nxt {
            self.rcv_nxt = self
                .rcv_nxt
                .wrapping_add(rx.payload_len as u32)
                .wrapping_add(1);
            self.ipid = self.ipid.wrapping_add(1);
            let fin_seq = self.snd_nxt.wrapping_add(1); // Our FIN consumed one seq.
            self.emit(me, fin_seq, self.rcv_nxt, tcp::FLAG_ACK, &[]);
            return true;
        }
        false
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
    // No connection table here: the one-shot path holds a single connection.
    let local_port = pick_local_port(&me.ip, dst_ip, dst_port, |_| false)?;
    let mut conn = TcpConn::connect(me, *dst_ip, dst_port, *next_hop_mac, id, local_port)?;
    conn.send(me, payload)?;
    let written = conn.recv(me, out);
    // No `Net` here to linger in, so the peer's FIN goes unacknowledged; the
    // peer retransmits it a few times and gives up. The one-shot path is a
    // blocking legacy request that reads the wire itself anyway.
    conn.begin_close(me);
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
    let dlen = udp::write(
        &mut dgram,
        &me.ip,
        dst_ip,
        EPHEMERAL_PORT,
        dst_port,
        payload,
    )?;
    if !send_ipv4(
        me,
        next_hop_mac,
        dst_ip,
        ipv4::PROTO_UDP,
        &dgram[..dlen],
        id,
    ) {
        return None;
    }
    let mut frame = [0u8; MAX_FRAME];
    let until = now_ns().saturating_add(RESOLVE_WAIT_NS);
    while now_ns() < until {
        loop {
            let n = nic::recv(&mut frame);
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
        nic::idle();
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

/// How long the bounded service waits after its last request for another
/// before it ends.
const SERVICE_IDLE_NS: u64 = 3_000_000_000;

/// How long the persistent service waits before asking for a lease again
/// when no DHCP server answered, at first and at most: doubling between.
const RETAKE_FIRST_NS: u64 = 4_000_000_000;
const RETAKE_MOST_NS: u64 = 64_000_000_000;

/// Maximum control-message size we accept/produce.
const MSG_CAP: usize = 512;

/// Run the netstack IPC service (DNS/TCP/UDP over the `net.stack` channel).
///
/// When `persistent` is false (the Phase-4 bounded self-test path, argv
/// `serve-dns`) the daemon exits [`SERVICE_IDLE_NS`] after its last request and
/// releases the NIC, so the still-live kernel stack regains it (§64). When
/// `persistent` is true (the Phase-5 boot daemon, argv `serve-net`) the idle
/// deadline is ignored: the daemon owns the NIC for the lifetime of the system
/// and serves socket clients indefinitely — correct only once the kernel stack
/// has stood down (the `net.userspace` switch is on). Returns the process exit
/// code (a persistent daemon only returns on an unrecoverable service fault).
fn run_dns_service(
    me: &IfInfo,
    persistent: bool,
    dhcp: Option<dhcp_client::Client>,
    expect_relink: bool,
) -> i64 {
    // Register first (fast) so a waiting client can connect immediately and then
    // block on the reply, rather than spinning while we do slow network I/O.
    let listener = syscall2(
        SYS_SERVICE_REGISTER,
        SERVICE_NAME.as_ptr() as u64,
        SERVICE_NAME.len() as u64,
    );
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
    // Persistent ring-TCP session: survives across separate OP_RING_TCP control
    // calls so a connection opened in one call is addressable in later ones.
    let mut sessions = Sessions::new();
    let mut net = Net::new();
    net.dhcp = dhcp;
    // The cards pass no multicast until told: all-hosts, all-nodes and our
    // solicited-node group go in now.
    sync_mcast_filter(&mut net, me);

    // Persistent mode ignores the idle deadline and serves for the system's
    // lifetime; bounded mode exits once no client has come for
    // SERVICE_IDLE_NS.
    let mut last_client = now_ns();
    let mut code = 0;
    while persistent || now_ns().saturating_sub(last_client) < SERVICE_IDLE_NS {
        let ch = match next_client(listener as u64, &mut net, me, next_hop_mac.as_ref()) {
            Ok(Some(ch)) => ch,
            Ok(None) => continue,
            // Said already; the caller may take a new lease and serve again.
            Err(Ended::LeaseLost) => {
                code = NIC_LEASE;
                break;
            }
            Err(Ended::Fault(why)) => {
                Line::new()
                    .put(b"[netstack] ")
                    .put(why.as_bytes())
                    .put(b"\n")
                    .print();
                code = 6;
                break;
            }
        };

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
            let started = now_ns();
            let reply_len = handle_request(
                req,
                &next_hop_mac,
                me,
                &mut txid,
                &mut sessions,
                &mut net,
                &mut reply,
            );
            // The daemon serves one request at a time, so a request that takes
            // long holds up every socket. Say so, with its opcode: the next
            // stall names itself instead of being inferred from a timeout on
            // the kernel side (rq15, 2026-09-27).
            let took_ms = now_ns().saturating_sub(started) / 1_000_000;
            if took_ms >= SLOW_REQUEST_MS {
                print_slow_request(req.first().copied().unwrap_or(0), took_ms);
            }
            let _ = syscall3(
                SYS_CHANNEL_SEND,
                ch,
                reply.as_ptr() as u64,
                reply_len as u64,
            );
            served += 1;
        }
        syscall1(SYS_CHANNEL_CLOSE, ch);
        // Idle from the end of a request: its own time is not idleness.
        last_client = now_ns();
    }

    // A bounded service whose next hop never answered ARP sent nothing
    // anywhere -- every request failed before reaching the card -- which
    // its answers alone cannot tell from a network with no upstream.
    if !persistent && code == 0 && next_hop_mac.is_none() {
        print("[netstack] FAIL: the DNS server's next hop never answered ARP: nothing was sent\n");
        code = NIC_NO_GATEWAY;
    }
    // Nor did one whose card never put its multicast filter in force --
    // the kernel refusing it (`net.filter.pushed`) or a driver's card not
    // answering it in force: its neighbour discovery and multicast sockets
    // got nothing.
    if !persistent && code == 0 && !(net.filter.pushed && nic::mcast_in_force()) {
        print("[netstack] FAIL: the card never put the multicast filter in force\n");
        code = NIC_MCAST;
    }
    // Nor did one whose card's link came back and whose lease was then not
    // confirmed by its server -- or whose link's return was never taken.
    // Asked to expect one (`serve-dns <service> relink`, the boot test's
    // cable pulled under it), there must have been one.
    let unconfirmed = net.relinks > net.confirmed || nic::take_link_back();
    if !persistent && code == 0 && (unconfirmed || (expect_relink && net.confirmed == 0)) {
        print("[netstack] FAIL: the card's link came back and its lease was never confirmed\n");
        code = NIC_LEASE;
    }

    // Tear down any ring session the client left open (unmaps + closes conns).
    sessions.close_all(&mut net, me);

    let _ = syscall1(SYS_SERVICE_UNREGISTER, listener as u64);
    if served > 0 {
        print("[netstack] served DNS-over-IPC request(s); unregistered\n");
    } else {
        print("[netstack] no requests before idle deadline; unregistered\n");
    }
    code
}

/// The next client connected to `listener`, waited for up to
/// [`ACCEPT_TIMEOUT_NS`]: `None` for none in that time.
///
/// The card is served meanwhile, and first: its lease kept and a link come
/// back acted on ([`lease_step`]) before a client waiting is taken, so that
/// clients that never stop coming do not starve it; what comes between
/// requests -- a segment for an open session, an ARP request -- routed as
/// it comes ([`pump`]) rather than left for the next request. On a driver
/// process's card the wait is woken by its doorbell or the client,
/// whichever comes first; on the kernel's NIC, which has no wait, by the
/// client or the time. An error ([`Ended`]) when the card has gone, the
/// listener has, the wait failed, our addresses could not be announced, or
/// the card's lease was lost.
fn next_client(
    listener: u64,
    net: &mut Net,
    me: &IfInfo,
    next_hop_mac: Option<&[u8; 6]>,
) -> Result<Option<u64>, Ended> {
    let deadline = now_ns().saturating_add(ACCEPT_TIMEOUT_NS);
    loop {
        lease_step(net, me)?;
        let accepted = syscall2(SYS_SERVICE_ACCEPT_TIMEOUT, listener, 0);
        if let Ok(ch) = u64::try_from(accepted) {
            return Ok(Some(ch));
        }
        if accepted != E_TIMED_OUT {
            return Err(Ended::Fault("FAIL: the service's listener is gone"));
        }
        match next_hop_mac {
            Some(mac) => {
                pump(net, me, mac);
            }
            // Without a next hop nothing can be sent, retransmissions
            // included; what came is answered as the link expects -- an ARP
            // request or a neighbour solicitation for our address, a ping --
            // rather than left to end every wait at once, and a DHCP reply
            // still reaches the lease.
            None => {
                let mut frame = [0u8; MAX_FRAME];
                while let Ok(n) = usize::try_from(nic::recv(&mut frame)) {
                    let Some(bytes) = frame.get(..n) else {
                        continue;
                    };
                    match (dhcp_payload(bytes), net.dhcp.as_mut()) {
                        (Some(reply), Some(client)) => {
                            if let Some(event) = dhcp_take(client, reply) {
                                note_dhcp(&mut net.dhcp_event, event);
                            }
                        }
                        _ => answer_link(bytes, me),
                    }
                }
            }
        }
        let left = deadline.saturating_sub(now_ns());
        if left == 0 {
            return Ok(None);
        }
        nic::wait_or_client(listener, left).map_err(Ended::Fault)?;
    }
}

/// `event`, what happened to the lease, kept in `pending` for
/// [`lease_step`]: one at a time, the weightier kept when two come before it
/// looks -- a lease lost over all, a confirmation (which announces our
/// addresses) over a renewal.
fn note_dhcp(pending: &mut Option<dhcp_client::Event>, event: dhcp_client::Event) {
    use dhcp_client::Event;
    let weight = |e: &Event| match e {
        Event::Refused | Event::Expired => 2,
        Event::Confirmed | Event::Kept => 1,
        Event::Renewed => 0,
    };
    if pending.as_ref().is_none_or(|p| weight(&event) >= weight(p)) {
        *pending = Some(event);
    }
}

/// A request that holds the daemon this long is logged ([`print_slow_request`]).
const SLOW_REQUEST_MS: u64 = 1_000;

/// `[netstack] slow request: opcode 0x.. held the daemon for N ms`.
fn print_slow_request(opcode: u8, ms: u64) {
    Line::new()
        .put(b"[netstack] slow request: opcode 0x")
        .hex(opcode)
        .put(b" held the daemon for ")
        .dec(ms)
        .put(b" ms\n")
        .print();
}

/// A console line built without an allocator -- text, numbers and addresses
/// written into a buffer -- and printed whole. What does not fit is cut, and
/// the line then ends with `...` and a newline of its own, so that what is
/// printed next starts a line.
struct Line {
    buf: [u8; 320],
    len: usize,
    cut: bool,
}

impl Line {
    fn new() -> Self {
        Line {
            buf: [0; 320],
            len: 0,
            cut: false,
        }
    }

    fn put(&mut self, s: &[u8]) -> &mut Self {
        for &b in s {
            match self.buf.get_mut(self.len) {
                Some(d) => {
                    *d = b;
                    self.len += 1;
                }
                None => self.cut = true,
            }
        }
        self
    }

    fn dec(&mut self, n: u64) -> &mut Self {
        let mut digits = [0u8; 20];
        let mut n = n;
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 || i == 0 {
                break;
            }
        }
        self.put(&digits[i..])
    }

    /// `b` as two hexadecimal digits.
    fn hex(&mut self, b: u8) -> &mut Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        self.put(&[HEX[usize::from(b >> 4)], HEX[usize::from(b & 0xf)]])
    }

    fn ip(&mut self, a: &[u8; 4]) -> &mut Self {
        for (i, &b) in a.iter().enumerate() {
            if i > 0 {
                self.put(b".");
            }
            self.dec(u64::from(b));
        }
        self
    }

    fn mac(&mut self, m: &[u8; 6]) -> &mut Self {
        for (i, &b) in m.iter().enumerate() {
            if i > 0 {
                self.put(b":");
            }
            self.hex(b);
        }
        self
    }

    /// The line written to the console as its bytes: a service name, say,
    /// need not be UTF-8.
    fn print(&self) {
        let bytes = self.buf.get(..self.len).unwrap_or(&[]);
        syscall2(SYS_CONSOLE_WRITE, bytes.as_ptr() as u64, bytes.len() as u64);
        if self.cut {
            print("...\n");
        }
    }
}

/// Handle one control request, writing the reply into `out` and returning its
/// length. `out` must be at least `MSG_CAP` bytes. The request/reply schema is
/// owned by the shared `netipc` crate.
fn handle_request(
    req: &[u8],
    next_hop_mac: &Option<[u8; 6]>,
    me: &IfInfo,
    txid: &mut u16,
    sessions: &mut Sessions,
    net: &mut Net,
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
                Some(w) => netipc::encode_ok_name(out, name.get(..w).unwrap_or(&[]))
                    .unwrap_or_else(|| fail(out)),
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
                    ring_tcp(sessions, net, handle, size, me, mac, *txid)
                }
                None => false,
            };
            if ok {
                netipc::encode_ok_bytes(out, &[]).unwrap_or_else(|| fail(out))
            } else {
                fail(out)
            }
        }
        // Its own interface, whichever card it serves: the kernel holds the
        // addresses of its own NIC only.
        Some(netipc::Request::IfInfo) => {
            let info = netipc::IfInfo {
                mac: me.mac,
                ip4: me.ip,
                mask: me.mask,
                gateway: me.gateway,
                ip6: me.ip6,
                dns: me.dns,
                up: nic::link_up(),
            };
            netipc::encode_ok_bytes(out, &info.bytes()).unwrap_or_else(|| fail(out))
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
            core::ptr::write_unaligned(base.add(8) as *mut u64, netipc::SHM_PING_RESPONSE_MAGIC);
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
        let cqe = netipc::ring::Cqe {
            user_data: sqe.user_data,
            result,
            flags: 0,
        };
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

/// What the daemon's tables key an entry by: the session (ring) that created
/// it, and the id that session gave it, as `session << 32 | id`.
///
/// Ids are a client's own namespace, so two rings may use the same id for
/// different things -- the kernel's raw-ring boot tests use fixed ids, while
/// its sockets draw theirs from one counter. Keying by the pair keeps every
/// session's entries apart while the tables stay one per daemon, which is what
/// lets a frame for a connection on one ring be routed while another ring is
/// being served (design B of A-Q15, design-decisions §972).
type Key = u64;

/// The [`Key`] for `id` in `session`.
fn key(session: u32, id: u32) -> Key {
    (u64::from(session) << 32) | u64::from(id)
}

/// The session a [`Key`] belongs to.
#[allow(clippy::cast_possible_truncation)] // the top half is the session, by construction
fn key_session(k: Key) -> u32 {
    (k >> 32) as u32
}

/// One frame off the NIC, classified for [`pump`].
enum Rx {
    /// A router's membership query (IGMP, or MLD when `v6`): the host reports
    /// the groups it asks about -- every group of its family when `about` is
    /// `None`.
    Query {
        about: Option<mcast::Group>,
        v6: bool,
    },
    /// No frame was waiting (`WOULD_BLOCK`), or the read failed: draining is done.
    Drained,
    /// A TCP segment addressed to us.
    Tcp(RawRx),
    /// A UDP datagram addressed to us.
    Udp(UdpRx),
    /// A DHCP message to the client port not at our address -- broadcast,
    /// or before we hold one, which [`Rx::Udp`] passes over -- its bytes
    /// copied to the UDP buffer.
    Dhcp(usize),
    /// A frame that is neither: skip it and keep draining.
    Other,
}

/// Read one frame off the NIC and classify it once, for every protocol.
///
/// The TCP and UDP pumps used to read frames separately, and each discarded
/// what was not its own: a UDP datagram arriving while a TCP connection was
/// served was lost, and a TCP segment arriving while a datagram socket was
/// served waited for its sender's retransmission. With every socket in one
/// session (design A of A-Q15) that was ordinary traffic, not a corner case.
fn recv_any(me: &IfInfo, frame: &mut [u8], tcp_pl: &mut [u8], udp_pl: &mut [u8]) -> Rx {
    let n = nic::recv(frame);
    if n < 0 {
        return Rx::Drained;
    }
    let Some(bytes) = usize::try_from(n).ok().and_then(|len| frame.get(..len)) else {
        return Rx::Other;
    };
    match parse_tcp(me, bytes, tcp_pl) {
        RawRx::Ignore => {}
        seg => return Rx::Tcp(seg),
    }
    match parse_udp(me, bytes, udp_pl) {
        Some(dg) => Rx::Udp(dg),
        // A DHCP reply not at our address -- broadcast, or before we hold
        // one -- which `parse_udp` passes over.
        None if let Some(msg) = dhcp_payload(bytes)
            && let Some(slot) = udp_pl.get_mut(..msg.len()) =>
        {
            slot.copy_from_slice(msg);
            Rx::Dhcp(msg.len())
        }
        None => match parse_query(bytes) {
            Some(query) => query,
            None => {
                answer_link(bytes, me);
                Rx::Other
            }
        },
    }
}

/// Answer what any host on a link must, for a frame nothing else claimed:
/// an ARP request for our IPv4 address, a ping to it, a Neighbor
/// Solicitation for our IPv6 address, a ping6 to it. Until 2026-10-03 the
/// serving loop answered none of them (only the startup loop's
/// [`handle_frame`] did), so once a router's ARP entry for us aged out --
/// a minute or so on Linux -- it could no longer reach us, and IPv6
/// neighbours could never resolve us at all. Quiet, unlike `handle_frame`:
/// routers ask again every few minutes for as long as we run.
fn answer_link(frame: &[u8], me: &IfInfo) {
    let Some(eth) = ethernet::Frame::parse(frame) else {
        return;
    };
    match eth.ethertype {
        ethernet::ETHERTYPE_ARP => {
            if me.ip != [0; 4]
                && let Some(pkt) = arp::Packet::parse(eth.payload)
                && matches!(pkt.op, arp::Op::Request)
                && pkt.target_ip == me.ip
                && let Some(reply) = arp::reply_to(&pkt, &me.mac)
            {
                // A lost reply is asked for again.
                let _ = nic::send(&reply);
            }
        }
        ethernet::ETHERTYPE_IPV4 => handle_ipv4(eth.payload, &eth.src, me),
        ethernet::ETHERTYPE_IPV6 => answer_icmpv6(eth.payload, &eth.src, me),
        _ => {}
    }
}

/// The IPv6 half of [`answer_link`]: a Neighbor Solicitation for our
/// address gets a Neighbor Advertisement (RFC 4861 §7.2.4,
/// `icmpv6::answer_solicitation`), an Echo Request to it an Echo Reply.
fn answer_icmpv6(body: &[u8], src_mac: &[u8; 6], me: &IfInfo) {
    let Some(ip) = ipv6::Packet::parse(body) else {
        return;
    };
    if ip.next_header != icmpv6::NH_ICMPV6 {
        return;
    }
    let mut out = [0u8; MAX_FRAME];
    let l4 = out.as_mut_slice();
    let (dst, eth_dst, hop_limit, len) =
        if ip.payload.first() == Some(&icmpv6::TYPE_NEIGHBOR_SOLICITATION) {
            // Only from this link (hop limit 255), and a duplicate-address check
            // (from ::) carries no source link-layer address (RFC 4861 §7.1.1).
            if ip.hop_limit != icmpv6::NDP_HOP_LIMIT {
                return;
            }
            let Some(ns) = icmpv6::parse_neighbor_solicitation(ip.payload, &ip.src, &ip.dst) else {
                return;
            };
            if ip.src == icmpv6::UNSPECIFIED && ns.link_addr.is_some() {
                return;
            }
            let Some((dst, flags)) = icmpv6::answer_solicitation(&ip.src, &ns, &me.ip6) else {
                return;
            };
            let eth_dst = if dst == icmpv6::ALL_NODES_LINK_LOCAL {
                mld::multicast_mac(&dst)
            } else {
                ns.link_addr.unwrap_or(*src_mac)
            };
            let Some(n) =
                icmpv6::write_neighbor_advertisement(l4, &me.ip6, &dst, &me.ip6, flags, &me.mac)
            else {
                return;
            };
            (dst, eth_dst, icmpv6::NDP_HOP_LIMIT, n)
        } else if ip.dst == me.ip6 && icmpv6::is_echo_request(ip.payload, &ip.src, &ip.dst) {
            let Some(n) = icmpv6::write_echo_reply(l4, &me.ip6, &ip.src, ip.payload) else {
                return;
            };
            (ip.src, *src_mac, 64, n)
        } else {
            return;
        };
    // A lost answer is asked for again.
    let _sent = out
        .get(..len)
        .is_some_and(|l4| send_ipv6_hops(me, &eth_dst, &dst, icmpv6::NH_ICMPV6, l4, hop_limit));
}

/// Classify a frame as a router's membership query: an IGMP Membership
/// Query, or an MLD Multicast Listener Query behind its Hop-by-Hop header.
/// `None` for anything else.
fn parse_query(bytes: &[u8]) -> Option<Rx> {
    let eth = ethernet::Frame::parse(bytes)?;
    match eth.ethertype {
        ethernet::ETHERTYPE_IPV4 => {
            let ip = ipv4::Packet::parse(eth.payload)?;
            if ip.protocol != igmp::PROTO_IGMP || !igmp::is_multicast(&ip.dst) {
                return None;
            }
            let msg = igmp::parse(ip.payload)?;
            (msg.kind == igmp::TYPE_QUERY).then(|| Rx::Query {
                about: (!msg.is_general_query()).then_some(mcast::Group::V4(msg.group)),
                v6: false,
            })
        }
        ethernet::ETHERTYPE_IPV6 => {
            let ip = ipv6::Packet::parse(eth.payload)?;
            // MLD travels behind a Hop-by-Hop header: next header 0, whose own
            // next header must be ICMPv6, and whose length is in eight-byte
            // units past the first eight.
            if ip.next_header != 0 || ip.payload.first() != Some(&58) {
                return None;
            }
            let hbh_len = usize::from(*ip.payload.get(1)?)
                .checked_add(1)?
                .checked_mul(8)?;
            let msg = mld::parse(ip.payload.get(hbh_len..)?, &ip.src, &ip.dst)?;
            (msg.kind == mld::TYPE_QUERY).then(|| Rx::Query {
                about: (!msg.is_general_query()).then_some(mcast::Group::V6(msg.group)),
                v6: true,
            })
        }
        _ => None,
    }
}

/// Drain every frame waiting on the NIC and route each to its owner: a TCP
/// segment to the connection (or listener) it belongs to, a UDP datagram to the
/// socket bound to its port, in every session. Then run the retransmission
/// timers, which an ACK that just arrived may have stopped. Returns `true` if
/// any frame was read.
///
/// One pump for both protocols over one [`Net`]: see [`recv_any`] for what
/// the two separate pumps it replaced used to drop.
fn pump(net: &mut Net, me: &IfInfo, next_hop_mac: &[u8; 6]) -> bool {
    // The card's lease kept wherever the daemon waits on its card -- a long
    // request's loops included -- not only between requests.
    // An unsent message is asked again at its next retransmission.
    if let Some(client) = net.dhcp.as_mut()
        && let Ok(Some(event)) = dhcp_poll(client, me, &mut net.dhcp_server_mac)
    {
        note_dhcp(&mut net.dhcp_event, event);
    }
    let mut frame = [0u8; MAX_FRAME];
    let mut tcp_pl = [0u8; MAX_FRAME];
    let mut udp_pl = [0u8; UDP_DGRAM_MAX];
    let mut any = false;
    loop {
        match recv_any(me, &mut frame, &mut tcp_pl, &mut udp_pl) {
            Rx::Drained => break,
            Rx::Other | Rx::Tcp(RawRx::Ignore) => any = true,
            Rx::Tcp(RawRx::Seg(src_ip, src_port, dst_port, rx)) => {
                any = true;
                let plen = rx.payload_len.min(tcp_pl.len());
                let payload = tcp_pl.get(..plen).unwrap_or(&[]);
                if let Some(c) = net.conns.find_by_tuple(&src_ip, src_port, dst_port) {
                    c.ingest_seg(me, &rx, payload);
                } else if net.closing.ingest(me, &src_ip, src_port, dst_port, &rx) {
                    // A connection its owner closed, answering its peer.
                } else {
                    // No established connection owns this segment: offer it to the
                    // listeners (backlog match, or a fresh SYN → passive open).
                    // Unmatched by both → dropped.
                    let _consumed = net.listeners.route_seg(
                        me,
                        &src_ip,
                        src_port,
                        dst_port,
                        &rx,
                        payload,
                        next_hop_mac,
                    );
                }
            }
            Rx::Tcp(RawRx::Seg6(src_ip6, src_port, dst_port, rx)) => {
                any = true;
                let plen = rx.payload_len.min(tcp_pl.len());
                let payload = tcp_pl.get(..plen).unwrap_or(&[]);
                if let Some(c) = net.conns.find_by_tuple6(&src_ip6, src_port, dst_port) {
                    c.ingest_seg(me, &rx, payload);
                } else if net.closing.ingest6(me, &src_ip6, src_port, dst_port, &rx) {
                    // A connection its owner closed, answering its peer.
                } else {
                    let _consumed = net.listeners.route_seg6(
                        me,
                        &src_ip6,
                        src_port,
                        dst_port,
                        &rx,
                        payload,
                        next_hop_mac,
                    );
                }
            }
            Rx::Dhcp(len) => {
                any = true;
                if let Some(client) = net.dhcp.as_mut()
                    && let Some(event) = udp_pl.get(..len).and_then(|m| dhcp_take(client, m))
                {
                    note_dhcp(&mut net.dhcp_event, event);
                }
            }
            Rx::Udp(dg) => {
                any = true;
                // A DHCP reply at our address is the lease's when it answers
                // its client; else a socket's, bound to the port, as any.
                if let Some(client) = net.dhcp.as_mut()
                    && let Some(event) = dhcp_at_our_address(client, &dg, &udp_pl)
                {
                    note_dhcp(&mut net.dhcp_event, event);
                    continue;
                }
                // A datagram to an unbound port is dropped (no ICMP port-unreachable yet),
                // and one to a group only reaches a socket that joined it.
                if let Some(sock) = net.udp.by_port(dg.dst_port)
                    && dg.group.is_none_or(|g| sock.mcast.contains(&g))
                {
                    let n = dg.payload_len.min(udp_pl.len());
                    sock.push(
                        dg.family,
                        dg.src_ip,
                        dg.src_port,
                        udp_pl.get(..n).unwrap_or(&[]),
                    );
                }
            }
            Rx::Query { about, v6 } => {
                any = true;
                // Report at once rather than after a random delay up to the
                // query's maximum: a host may (RFC 2236 3), and one host's
                // reports are few.
                for group in net.mcast.answer(about, v6) {
                    send_membership(me, group, true);
                }
            }
        }
    }
    let now = now_ns();
    net.conns.service_retransmits(me, now);
    net.closing.expire(now);
    any
}

/// Handle an [`netipc::OP_RING_TCP`] request: serve the SQEs queued on the ring
/// `handle` (`size` bytes) against its session, mapping it first if it is new.
/// Returns `true` iff at least one SQE was processed and every completion was
/// posted. `seed_ipid` seeds a new session's IPv4 identification counter.
fn ring_tcp(
    sessions: &mut Sessions,
    net: &mut Net,
    handle: u64,
    size: u32,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    seed_ipid: u16,
) -> bool {
    let Some(slot) = sessions.open(handle, size, seed_ipid) else {
        return false;
    };
    let Some(session) = sessions.slots.get_mut(slot).and_then(Option::as_mut) else {
        return false;
    };
    #[allow(clippy::cast_possible_truncation)] // slot < MAX_SESSIONS
    let id_space = slot as u32;
    let base = session.va as u64 as *mut u8;

    // SAFETY: the mapping is valid and writable for `session.size` bytes for as
    // long as the session exists (it is unmapped only in `Sessions::close`);
    // `attach` re-validates the ring geometry against that length and never
    // reads/writes outside the mapping. Re-attaching each call is stateless — the
    // SQ/CQ head/tail indices live in the shared region, so a fresh `Ring` view
    // resumes exactly where the previous call left off. We are the sole SQ
    // consumer and sole CQ producer (the client is the other party), satisfying
    // the SPSC contract.
    let (ok, stop) = unsafe {
        match netring::Ring::attach(base, session.size as usize) {
            Some(ring) => {
                ring_tcp_process(&ring, net, id_space, me, next_hop_mac, &mut session.ipid)
            }
            None => (false, false),
        }
    };

    // On an explicit OP_STOP, close what this session created and unmap it.
    if stop {
        sessions.close(slot, net, me);
    }
    ok
}

/// Maximum concurrent TCP connections in the whole daemon.
///
/// Daemon-wide since 2026-09-27 ([`Net`]): until then this was per ring
/// session, at 8, and only one session existed, so it was also the
/// system-wide limit. 64 covers the load harness's largest tier (32 loopback
/// connections, each two entries: the client end and the accepted end) with a
/// program's ordinary sockets beside it. Each live [`TcpConn`] carries 1 KiB
/// send and receive buffers plus bookkeeping, about 2.3 KiB, so the table is
/// about 150 KiB of the serve loop's stack frame, well inside the stack's
/// growth limit (`mm.max_stack_frames`).
const MAX_RING_CONNS: usize = 64;

/// A table of live TCP connections, keyed by [`Key`] (session and `conn_id`).
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
    slots: [Option<(Key, TcpConn)>; MAX_RING_CONNS],
}

impl RingConns {
    /// An empty table (all slots free).
    fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
        }
    }

    /// Whether a live connection in this table has local port `port`.
    fn port_in_use(&self, port: u16) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|(_, c)| c.local_port == port)
    }

    /// Borrow the live connection registered under `id`, if any.
    fn get_mut(&mut self, id: Key) -> Option<&mut TcpConn> {
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
    fn reserve(&mut self, id: Key) -> Option<&mut Option<(Key, TcpConn)>> {
        if self.slots.iter().flatten().any(|(sid, _)| *sid == id) {
            return None; // duplicate conn_id
        }
        self.slots.iter_mut().find(|s| s.is_none())
    }

    /// Remove and return the connection registered under `id`, if present, freeing
    /// its slot for reuse.
    fn remove(&mut self, id: Key) -> Option<TcpConn> {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|(sid, _)| *sid == id) {
                return slot.take().map(|(_, c)| c);
            }
        }
        None
    }

    /// Run every live connection's retransmission timer at `now`
    /// ([`TcpConn::service_retransmit`]). Called by [`pump`] after each drain,
    /// so the timers advance whenever any ring is served.
    fn service_retransmits(&mut self, me: &IfInfo, now: u64) {
        for (_, c) in self.slots.iter_mut().flatten() {
            c.service_retransmit(me, now);
        }
    }
}

// ---------------------------------------------------------------------------
// UDP datagram sockets (SOCK_DGRAM over the ring)
// ---------------------------------------------------------------------------

/// Maximum concurrent bound UDP datagram sockets one ring session can hold.
const MAX_UDP_SOCKS: usize = 8;
/// Per-socket inbound datagram queue depth. UDP is lossy by contract, so a full
/// queue drops the *oldest* datagram (favouring fresh data) rather than blocking.
const UDP_QLEN: usize = 2;
/// Maximum UDP payload we buffer per datagram (one Ethernet MTU minus the IPv4
/// and UDP headers — a datagram that would exceed this on send is rejected with
/// [`netipc::ring::ERR_MSG_SIZE`]).
const UDP_DGRAM_MAX: usize = netipc::ring::UDP_DGRAM_MAX;
// The ring's limit is this daemon's frame, less its headers.
const _: () = assert!(
    UDP_DGRAM_MAX == MAX_FRAME - (ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN + udp::HEADER_LEN)
);
/// First ephemeral local port handed out by [`UdpSocks::bind`] when a caller
/// binds to port 0 (the IANA dynamic/ephemeral range base).
const UDP_EPHEMERAL_BASE: u16 = 49152;

/// One buffered inbound UDP datagram: the sender's address/port plus the copied
/// payload. The source address is stored as a fixed 16-byte field tagged with a
/// `family` ([`netipc::ring::UDP_AF_INET`] or [`UDP_AF_INET6`](netipc::ring::UDP_AF_INET6)),
/// so one queue slot carries either an IPv4 (address in `src_ip[0..4]`, rest zero)
/// or an IPv6 datagram without a parallel v6 queue. Stored `Copy` so the fixed
/// queue array can be built without a heap (the daemon is `no_std`, no `alloc`).
#[derive(Clone, Copy)]
struct UdpDatagram {
    family: u16,
    src_ip: [u8; 16],
    src_port: u16,
    len: usize,
    data: [u8; UDP_DGRAM_MAX],
}

impl UdpDatagram {
    /// An empty datagram slot (used to initialise the fixed queue array).
    const fn empty() -> Self {
        Self {
            family: netipc::ring::UDP_AF_INET,
            src_ip: [0; 16],
            src_port: 0,
            len: 0,
            data: [0; UDP_DGRAM_MAX],
        }
    }
}

/// A bound connectionless UDP datagram socket. Holds its local port and a small
/// FIFO of received datagrams (each tagged with its source), filled by
/// [`pump`] and drained by `OP_UDP_RECV`.
struct UdpSock {
    local_port: u16,
    /// The groups the socket has joined and its multicast send settings
    /// (`OP_UDP_SETOPT`), with Linux's defaults.
    mcast: mcast::SocketGroups<MAX_SOCK_GROUPS>,
    /// Fixed-capacity FIFO ring of buffered datagrams.
    q: [UdpDatagram; UDP_QLEN],
    /// Index of the oldest buffered datagram.
    head: usize,
    /// Number of buffered datagrams (`0..=UDP_QLEN`).
    count: usize,
}

impl UdpSock {
    /// A freshly-bound socket on `local_port` with an empty receive queue.
    fn new(local_port: u16) -> Self {
        Self {
            local_port,
            mcast: mcast::SocketGroups::new(),
            q: [UdpDatagram::empty(); UDP_QLEN],
            head: 0,
            count: 0,
        }
    }

    /// Buffer a received datagram. If the queue is full, the *oldest* datagram is
    /// evicted first (UDP is lossy — dropping the stalest data is the least-bad
    /// choice and keeps a slow reader from wedging on ancient packets). `family`
    /// tags the source address as IPv4 or IPv6; `src_ip` is the fixed 16-byte
    /// address (IPv4 in `src_ip[0..4]`, rest zero).
    fn push(&mut self, family: u16, src_ip: [u8; 16], src_port: u16, payload: &[u8]) {
        let n = payload.len().min(UDP_DGRAM_MAX);
        if self.count == UDP_QLEN {
            // Drop the head to make room, then append at the (freed) tail.
            self.head = (self.head + 1) % UDP_QLEN;
            self.count -= 1;
        }
        let tail = (self.head + self.count) % UDP_QLEN;
        let slot = &mut self.q[tail];
        slot.family = family;
        slot.src_ip = src_ip;
        slot.src_port = src_port;
        slot.len = n;
        slot.data[..n].copy_from_slice(&payload[..n]);
        self.count += 1;
    }

    /// Remove and copy out the oldest buffered datagram into `out` (payload only;
    /// truncated to `out.len()`). Returns `(family, src_ip16, src_port, payload_len)`,
    /// or `None` if the queue is empty.
    fn pop(&mut self, out: &mut [u8]) -> Option<(u16, [u8; 16], u16, usize)> {
        if self.count == 0 {
            return None;
        }
        let dg = self.q[self.head];
        self.head = (self.head + 1) % UDP_QLEN;
        self.count -= 1;
        let n = dg.len.min(out.len());
        out[..n].copy_from_slice(&dg.data[..n]);
        Some((dg.family, dg.src_ip, dg.src_port, n))
    }

    /// True iff at least one datagram is queued (drives `OP_POLL` readability
    /// once UDP poll is wired).
    fn has_data(&self) -> bool {
        self.count > 0
    }

    /// The payload length of the next datagram a receive would take, 0 when
    /// none is queued: `OP_POLL`'s count for a UDP socket
    /// ([`netipc::ring::POLL_COUNTED`]), as Linux's `SIOCINQ` answers one.
    fn next_len(&self) -> usize {
        if self.count == 0 {
            return 0;
        }
        self.q.get(self.head).map_or(0, |dg| dg.len)
    }
}

/// Table of bound UDP datagram sockets, keyed by the caller-chosen ring
/// `conn_id` (the identity of the userspace `SOCK_DGRAM` fd) — the datagram
/// sibling of [`RingConns`].
struct UdpSocks {
    slots: [Option<(Key, UdpSock)>; MAX_UDP_SOCKS],
}

impl UdpSocks {
    /// An empty table (no bound sockets).
    fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
        }
    }

    /// Bind the datagram socket under `conn_id` to `port` (`0` = pick an unused
    /// ephemeral port): create it, or give the unbound one [`open`](Self::open)
    /// made its port, keeping the options it was given and the groups it
    /// joined meanwhile. Returns the bound local port, or a negative
    /// [`netipc::ring`] errno sentinel: [`ERR_ADDR_IN_USE`](netipc::ring::ERR_ADDR_IN_USE)
    /// if `port` is already bound (or the socket under `conn_id` already has a
    /// port), `-1` if the table is full. A refused bind leaves an unbound
    /// socket as it was.
    fn bind(&mut self, conn_id: Key, port: u16) -> i32 {
        let existing = self
            .slots
            .iter()
            .flatten()
            .find(|(id, _)| *id == conn_id)
            .map(|(_, s)| s.local_port);
        if existing.is_some_and(|p| p != 0) {
            return netipc::ring::ERR_ADDR_IN_USE; // bound already
        }
        let local = if port == 0 {
            match self.pick_ephemeral() {
                Some(p) => p,
                None => return -1,
            }
        } else {
            if self.port_in_use(port) {
                return netipc::ring::ERR_ADDR_IN_USE;
            }
            port
        };
        if existing.is_some() {
            return match self.entry_mut(conn_id) {
                Some(sock) => {
                    sock.local_port = local;
                    i32::from(local)
                }
                None => -1, // just seen: unreachable, but not worth a panic
            };
        }
        match self.slots.iter_mut().find(|s| s.is_none()) {
            Some(slot) => {
                *slot = Some((conn_id, UdpSock::new(local)));
                i32::from(local)
            }
            None => -1, // table full
        }
    }

    /// Create the datagram socket under `conn_id` without a port
    /// ([`UDP_BIND_UNBOUND`](netipc::ring::UDP_BIND_UNBOUND)): Linux's socket
    /// between `socket(2)` and `bind(2)`, which may set options and join
    /// groups but sends and receives nothing until [`bind`](Self::bind) gives
    /// it a port. Returns `0`, [`ERR_ADDR_IN_USE`](netipc::ring::ERR_ADDR_IN_USE)
    /// for a `conn_id` already in use, or `-1` if the table is full.
    fn open(&mut self, conn_id: Key) -> i32 {
        if self.slots.iter().flatten().any(|(id, _)| *id == conn_id) {
            return netipc::ring::ERR_ADDR_IN_USE;
        }
        match self.slots.iter_mut().find(|s| s.is_none()) {
            Some(slot) => {
                *slot = Some((conn_id, UdpSock::new(0)));
                0
            }
            None => -1, // table full
        }
    }

    /// True iff some bound socket already owns `port`.
    fn port_in_use(&self, port: u16) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|(_, s)| s.local_port == port)
    }

    /// Pick the lowest unused ephemeral port at/above [`UDP_EPHEMERAL_BASE`], or
    /// `None` if the entire ephemeral range is exhausted.
    fn pick_ephemeral(&self) -> Option<u16> {
        (UDP_EPHEMERAL_BASE..=u16::MAX).find(|&p| !self.port_in_use(p))
    }

    /// Borrow the socket bound under `conn_id`, if it has a port: what sends
    /// and receives need. An unbound socket is "not bound" to them.
    fn get_mut(&mut self, conn_id: Key) -> Option<&mut UdpSock> {
        self.entry_mut(conn_id).filter(|s| s.local_port != 0)
    }

    /// Borrow the socket under `conn_id`, bound or not: what its options need.
    fn entry_mut(&mut self, conn_id: Key) -> Option<&mut UdpSock> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|(id, _)| *id == conn_id)
            .map(|(_, s)| s)
    }

    /// Borrow the socket bound to local `port` (the routing lookup [`pump`]
    /// uses to deliver an inbound datagram to its owner). Port 0 is no one's:
    /// an unbound socket's port reads 0, and a datagram to port 0 must not
    /// reach it.
    fn by_port(&mut self, port: u16) -> Option<&mut UdpSock> {
        if port == 0 {
            return None;
        }
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|(_, s)| s.local_port == port)
            .map(|(_, s)| s)
    }

    /// Remove the socket bound under `conn_id` (its `close(2)`), freeing the
    /// slot, and hand it back so the caller can leave the groups it was in
    /// ([`leave_all`]). UDP is connectionless, so there is no teardown
    /// handshake.
    fn take(&mut self, conn_id: Key) -> Option<UdpSock> {
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|(id, _)| *id == conn_id))?;
        slot.take().map(|(_, sock)| sock)
    }
}

/// Build and transmit one UDP datagram from `sock`'s local port to
/// `dst_ip:dst_port`. Returns the payload byte count sent (`≥ 0`), or a negative
/// [`netipc::ring`] errno: [`ERR_MSG_SIZE`](netipc::ring::ERR_MSG_SIZE) if the
/// payload exceeds [`UDP_DGRAM_MAX`], `-1` on a build/transmit failure. Shared by
/// `OP_UDP_SEND` and the UDP self-test.
fn udp_sock_send(
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    sock: &UdpSock,
    dst_ip: &[u8; 4],
    dst_port: u16,
    payload: &[u8],
    id: u16,
) -> i32 {
    udp_send_from(
        me,
        next_hop_mac,
        sock.local_port,
        dst_ip,
        dst_port,
        payload,
        id,
        64,
    )
}

/// [`udp_sock_send`] from local port `src_port`, to Ethernet address
/// `dst_mac`, with TTL `ttl`: what a multicast send needs, the socket having
/// been let go so its group's members can be reached ([`deliver_locally`]).
#[allow(clippy::too_many_arguments)]
fn udp_send_from(
    me: &IfInfo,
    dst_mac: &[u8; 6],
    src_port: u16,
    dst_ip: &[u8; 4],
    dst_port: u16,
    payload: &[u8],
    id: u16,
    ttl: u8,
) -> i32 {
    if payload.len() > UDP_DGRAM_MAX {
        return netipc::ring::ERR_MSG_SIZE;
    }
    let mut dgram = [0u8; MAX_FRAME - (ethernet::HEADER_LEN + ipv4::MIN_HEADER_LEN)];
    let dlen = match udp::write(&mut dgram, &me.ip, dst_ip, src_port, dst_port, payload) {
        Some(n) => n,
        None => return -1,
    };
    if !send_ipv4_ttl(
        me,
        dst_mac,
        dst_ip,
        ipv4::PROTO_UDP,
        &dgram[..dlen],
        id,
        ttl,
    ) {
        return -1;
    }
    // On success the caller learns how many payload bytes were accepted.
    i32::try_from(payload.len()).unwrap_or(i32::MAX)
}

/// Build and transmit one UDP datagram over IPv6 from `sock`'s local port to
/// `[dst_ip6]:dst_port`. The IPv6 sibling of [`udp_sock_send`] (there is no IPv4
/// identification counter on the wire). Returns the payload byte count sent
/// (`≥ 0`), or a negative [`netipc::ring`] errno:
/// [`ERR_MSG_SIZE`](netipc::ring::ERR_MSG_SIZE) if the payload exceeds
/// [`UDP_DGRAM_MAX`], `-1` on a build/transmit failure. Shared by `OP_UDP_SEND6`.
fn udp_sock_send6(
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    sock: &UdpSock,
    dst_ip6: &[u8; 16],
    dst_port: u16,
    payload: &[u8],
) -> i32 {
    udp_send6_from(
        me,
        next_hop_mac,
        sock.local_port,
        dst_ip6,
        dst_port,
        payload,
        64,
    )
}

/// [`udp_sock_send6`] from `src_port`, to `dst_mac`, with hop limit
/// `hop_limit`: the IPv6 counterpart of [`udp_send_from`].
fn udp_send6_from(
    me: &IfInfo,
    dst_mac: &[u8; 6],
    src_port: u16,
    dst_ip6: &[u8; 16],
    dst_port: u16,
    payload: &[u8],
    hop_limit: u8,
) -> i32 {
    if payload.len() > UDP_DGRAM_MAX {
        return netipc::ring::ERR_MSG_SIZE;
    }
    let mut dgram = [0u8; MAX_FRAME - (ethernet::HEADER_LEN + ipv6::HEADER_LEN)];
    let dlen = match udp::write_v6(&mut dgram, &me.ip6, dst_ip6, src_port, dst_port, payload) {
        Some(n) => n,
        None => return -1,
    };
    if !send_ipv6_hops(
        me,
        dst_mac,
        dst_ip6,
        ipv4::PROTO_UDP,
        &dgram[..dlen],
        hop_limit,
    ) {
        return -1;
    }
    i32::try_from(payload.len()).unwrap_or(i32::MAX)
}

/// Send the announcement `a` asks for ([`mcast::Announce`]).
fn announce(me: &IfInfo, a: mcast::Announce) {
    match a {
        mcast::Announce::Nothing => {}
        mcast::Announce::Report(g) => {
            send_membership(me, g, true);
        }
        mcast::Announce::Leave(g) => {
            send_membership(me, g, false);
        }
    }
}

/// A socket's groups, left as it goes: each is uncounted, and announced as
/// left when it was the last socket in it.
fn leave_all(
    host: &mut mcast::HostGroups<MAX_HOST_GROUPS>,
    sock: &mcast::SocketGroups<MAX_SOCK_GROUPS>,
    me: &IfInfo,
) {
    for g in sock.iter() {
        announce(me, host.leave(&g));
    }
}

/// Transmit a membership report (`report`) or leave for `group`: IGMPv2 for
/// IPv4 (RFC 2236), MLDv1 for IPv6 (RFC 2710), with TTL / hop limit 1 and a
/// Router Alert, to the group itself (report) or to all routers (leave).
fn send_membership(me: &IfInfo, group: mcast::Group, report: bool) -> bool {
    let mut frame = [0u8; MAX_FRAME];
    match group {
        mcast::Group::V4(g) => {
            let dst = if report { g } else { igmp::ALL_ROUTERS };
            let l4_off = ethernet::HEADER_LEN + ipv4::ROUTER_ALERT_HEADER_LEN;
            let Some(msg) = frame.get_mut(l4_off..) else {
                return false;
            };
            let written = if report {
                igmp::write_report(msg, &g)
            } else {
                igmp::write_leave(msg, &g)
            };
            let Some(n) = written else {
                return false;
            };
            let ip_hdr = ipv4::Builder {
                dscp_ecn: 0xC0, // network control, as Linux sends IGMP
                id: 0,
                flags_frag: 0x4000,
                ttl: 1,
                protocol: igmp::PROTO_IGMP,
                src: me.ip,
                dst,
            }
            .build_header_router_alert(n as u16);
            frame[ethernet::HEADER_LEN..l4_off].copy_from_slice(&ip_hdr);
            ethernet::write_header(
                &mut frame,
                &igmp::multicast_mac(&dst),
                &me.mac,
                ethernet::ETHERTYPE_IPV4,
            );
            nic::send(&frame[..l4_off + n]) >= 0
        }
        mcast::Group::V6(g) => {
            let dst = if report {
                g
            } else {
                icmpv6::ALL_ROUTERS_LINK_LOCAL
            };
            let hbh_off = ethernet::HEADER_LEN + ipv6::HEADER_LEN;
            let msg_off = hbh_off + mld::HOP_BY_HOP_ROUTER_ALERT.len();
            let Some(msg) = frame.get_mut(msg_off..) else {
                return false;
            };
            let written = if report {
                mld::write_report(msg, &me.ip6, &g)
            } else {
                mld::write_done(msg, &me.ip6, &g)
            };
            let Some(n) = written else {
                return false;
            };
            frame[hbh_off..msg_off].copy_from_slice(&mld::HOP_BY_HOP_ROUTER_ALERT);
            let ip_hdr = ipv6::Builder {
                traffic_class: 0,
                flow_label: 0,
                next_header: 0, // Hop-by-Hop, carrying the Router Alert
                hop_limit: 1,
                src: me.ip6,
                dst,
            }
            .build_header((mld::HOP_BY_HOP_ROUTER_ALERT.len() + n) as u16);
            frame[ethernet::HEADER_LEN..hbh_off].copy_from_slice(&ip_hdr);
            ethernet::write_header(
                &mut frame,
                &mld::multicast_mac(&dst),
                &me.mac,
                ethernet::ETHERTYPE_IPV6,
            );
            nic::send(&frame[..msg_off + n]) >= 0
        }
    }
}

/// Deliver a datagram this host sent to `group` to the local socket on
/// `dst_port` that joined it: the loop `IP_MULTICAST_LOOP` /
/// `IPV6_MULTICAST_LOOP` keep on by default. The sender itself is a member
/// like any other.
fn deliver_locally(
    udp: &mut UdpSocks,
    group: mcast::Group,
    src: [u8; 16],
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) {
    let family = match group {
        mcast::Group::V4(_) => netipc::ring::UDP_AF_INET,
        mcast::Group::V6(_) => netipc::ring::UDP_AF_INET6,
    };
    if let Some(sock) = udp.by_port(dst_port)
        && sock.mcast.contains(&group)
    {
        sock.push(family, src, src_port, payload);
    }
}

/// Map a refused join or leave to the errno Linux gives the same call.
fn refused_errno(r: mcast::Refused) -> i32 {
    match r {
        mcast::Refused::NotMulticast => netipc::ring::ERR_INVALID,
        mcast::Refused::AlreadyIn => netipc::ring::ERR_ADDR_IN_USE,
        mcast::Refused::NotIn => netipc::ring::ERR_ADDR_NOT_AVAIL,
        mcast::Refused::Full => netipc::ring::ERR_NO_BUFS,
    }
}

/// Serve [`netipc::ring::OP_UDP_SETOPT`] for the socket at `k`: `option`
/// with scalar `value`, or the group in `window` for a join or leave.
fn udp_setopt(net: &mut Net, me: &IfInfo, k: Key, option: u16, value: u32, window: &[u8]) -> i32 {
    use netipc::ring as r;
    let Net { udp, mcast, .. } = net;
    let Some(sock) = udp.entry_mut(k) else {
        return -1; // no such socket
    };
    // An IPv4 membership may name the interface by address: "any", or the
    // host's own. Joining on another has no device (ENODEV); leaving on one
    // finds no membership (EADDRNOTAVAIL), as Linux's search does.
    let iface = window.get(4..8).unwrap_or(&[0; 4]);
    if iface != [0; 4] && iface != me.ip {
        match option {
            r::UDP_OPT_MCAST_JOIN4 => return r::ERR_NO_DEVICE,
            r::UDP_OPT_MCAST_LEAVE4 => return r::ERR_ADDR_NOT_AVAIL,
            _ => {}
        }
    }
    let group = match option {
        r::UDP_OPT_MCAST_JOIN4 | r::UDP_OPT_MCAST_LEAVE4 => window
            .get(..4)
            .and_then(|g| <[u8; 4]>::try_from(g).ok())
            .map(mcast::Group::V4),
        r::UDP_OPT_MCAST_JOIN6 | r::UDP_OPT_MCAST_LEAVE6 => window
            .get(..16)
            .and_then(|g| <[u8; 16]>::try_from(g).ok())
            .map(mcast::Group::V6),
        _ => None,
    };
    let byte = u8::try_from(value).ok();
    let flag = match value {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    match option {
        r::UDP_OPT_MCAST_JOIN4 | r::UDP_OPT_MCAST_JOIN6 => {
            let Some(g) = group else {
                return r::ERR_INVALID;
            };
            if let Err(e) = sock.mcast.join(g) {
                return refused_errno(e);
            }
            match mcast.join(g) {
                Ok(a) => {
                    announce(me, a);
                    0
                }
                Err(e) => {
                    // The host has no room for it: the socket is not in it
                    // either. Its own leave cannot fail, it just joined.
                    let _ = sock.mcast.leave(&g);
                    refused_errno(e)
                }
            }
        }
        r::UDP_OPT_MCAST_LEAVE4 | r::UDP_OPT_MCAST_LEAVE6 => {
            let Some(g) = group else {
                return r::ERR_INVALID;
            };
            if let Err(e) = sock.mcast.leave(&g) {
                return refused_errno(e);
            }
            announce(me, mcast.leave(&g));
            0
        }
        r::UDP_OPT_MCAST_TTL => byte.map_or(r::ERR_INVALID, |v| {
            sock.mcast.ttl = v;
            0
        }),
        r::UDP_OPT_MCAST_HOPS => byte.map_or(r::ERR_INVALID, |v| {
            sock.mcast.hops = v;
            0
        }),
        r::UDP_OPT_MCAST_LOOP4 => flag.map_or(r::ERR_INVALID, |v| {
            sock.mcast.loop4 = v;
            0
        }),
        r::UDP_OPT_MCAST_LOOP6 => flag.map_or(r::ERR_INVALID, |v| {
            sock.mcast.loop6 = v;
            0
        }),
        _ => r::ERR_INVALID,
    }
}

/// Serve [`netipc::ring::OP_UDP_GETOPT`] for the socket at `k`.
fn udp_getopt(net: &mut Net, k: Key, option: u16) -> i32 {
    use netipc::ring as r;
    let Some(sock) = net.udp.entry_mut(k) else {
        return -1; // no such socket
    };
    match option {
        r::UDP_OPT_MCAST_TTL => i32::from(sock.mcast.ttl),
        r::UDP_OPT_MCAST_HOPS => i32::from(sock.mcast.hops),
        r::UDP_OPT_MCAST_LOOP4 => i32::from(sock.mcast.loop4),
        r::UDP_OPT_MCAST_LOOP6 => i32::from(sock.mcast.loop6),
        _ => r::ERR_INVALID,
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
        Self {
            port,
            backlog: core::array::from_fn(|_| None),
            ipid: seed_ipid,
        }
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
        self.backlog
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|c| {
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

    /// Whether a backlog connection is one `OP_ACCEPT` may hand out: its handshake
    /// completed and it did not fail. The one predicate behind both
    /// [`take_established`](Self::take_established) and
    /// [`has_established`](Self::has_established), so `poll` can never report a
    /// connection that `accept` would then refuse, or miss one it would take.
    fn acceptable(c: &TcpConn) -> bool {
        c.established && !c.connect_failed
    }

    /// Remove and return the first *established* backlog connection (ready to be
    /// handed to the caller by `OP_ACCEPT`), or `None` if none is ready yet.
    fn take_established(&mut self) -> Option<TcpConn> {
        for slot in &mut self.backlog {
            if slot.as_ref().is_some_and(Self::acceptable) {
                return slot.take();
            }
        }
        None
    }

    /// Whether [`take_established`](Self::take_established) would return a
    /// connection now, without taking it: what makes a listener readable to
    /// `poll`/`select`/`epoll` (`POLLIN` means "accept will not block").
    fn has_established(&self) -> bool {
        self.backlog.iter().flatten().any(Self::acceptable)
    }

    /// Close every backlog connection (listener teardown), without waiting:
    /// each lingers in `closing` to answer its peer.
    fn close_all(&mut self, me: &IfInfo, closing: &mut Closing) {
        for slot in &mut self.backlog {
            if let Some(c) = slot.take() {
                closing.retire(c, me);
            }
        }
    }
}

/// Table of active listeners, keyed by the `OP_LISTEN` `conn_id`.
struct Listeners {
    slots: [Option<(Key, Listener)>; MAX_LISTENERS],
}

impl Listeners {
    fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
        }
    }

    /// Register a listener `id` bound to `port`. Rejects a duplicate id, a port
    /// already bound by another listener, or a full table (`false`).
    fn add(&mut self, id: Key, port: u16, seed_ipid: u16) -> bool {
        if self
            .slots
            .iter()
            .flatten()
            .any(|(lid, l)| *lid == id || l.port == port)
        {
            return false; // duplicate id or port already bound
        }
        if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
            *slot = Some((id, Listener::new(port, seed_ipid)));
            true
        } else {
            false
        }
    }

    /// Unregister the listener `id`, closing every connection still waiting in
    /// its backlog. `false` if there is no such listener.
    fn remove(&mut self, id: Key, me: &IfInfo, closing: &mut Closing) -> bool {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|(lid, _)| *lid == id) {
                if let Some((_, mut l)) = slot.take() {
                    l.close_all(me, closing);
                }
                return true;
            }
        }
        false
    }

    /// Borrow the listener registered under `id`, if any.
    fn get_mut(&mut self, id: Key) -> Option<&mut Listener> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|(lid, _)| *lid == id)
            .map(|(_, l)| l)
    }

    /// Whether a listener is bound to `port`.
    fn port_in_use(&self, port: u16) -> bool {
        self.slots.iter().flatten().any(|(_, l)| l.port == port)
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
}

/// Everything the daemon knows about sockets, whichever ring created them: one
/// table each of TCP connections, listeners and datagram sockets, keyed by
/// ([`key`]) the session and id that created each entry.
///
/// One for the whole daemon, so [`pump`] can route any frame to its owner
/// whatever session is being served. Until 2026-09-27 each ring session had
/// tables of its own and only one session existed: a second ring reset the
/// first, and a frame for anything outside the session being served was lost.
struct Net {
    conns: RingConns,
    listeners: Listeners,
    udp: UdpSocks,
    /// The multicast groups the host is in on behalf of its UDP sockets, each
    /// with how many are in it (`OP_UDP_SETOPT`).
    mcast: mcast::HostGroups<MAX_HOST_GROUPS>,
    /// The multicast filter the kernel last took for the cards.
    filter: McastFilter,
    /// Connections their owner has closed, lingering to answer their peers.
    closing: Closing,
    /// The lease the daemon holds on a driver process's card, kept in the
    /// serving loop ([`lease_step`]); `None` over the kernel's NIC, whose
    /// address is the kernel's.
    dhcp: Option<dhcp_client::Client>,
    /// What a DHCP reply did to the lease, from [`pump`], for [`lease_step`]
    /// to act on ([`note_dhcp`]).
    dhcp_event: Option<dhcp_client::Event>,
    /// How many times the card's link came back, and how many times its
    /// lease was then confirmed by its server: the bounded service -- the
    /// boot test's -- ends failed when they differ.
    relinks: u32,
    confirmed: u32,
    /// The DHCP server's next hop's MAC address, once a renewal looked it up.
    dhcp_server_mac: ServerMac,
}

/// Most closed connections that linger at once.
const MAX_CLOSING: usize = 16;

/// How long a closed connection lingers for its peer's FIN.
const CLOSE_LINGER_NS: u64 = 2_000_000_000;

/// Connections closed by their owner, kept until the peer's FIN has been
/// acknowledged or [`CLOSE_LINGER_NS`] has passed. The pump offers them the
/// segments no live connection claims ([`TcpConn::closing_seg`]), so a close
/// never waits for its peer in the request that asked for it, and never reads
/// the wire itself. A full table drops the connection nearest its deadline:
/// its peer has had longest to answer.
struct Closing {
    slots: [Option<(TcpConn, u64)>; MAX_CLOSING],
}

impl Closing {
    fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
        }
    }

    /// Close `c` without waiting: send its FIN and keep it to answer the
    /// peer's. One whose peer has already sent a FIN (acknowledged when it
    /// arrived) has nothing left to answer, and is dropped.
    fn retire(&mut self, mut c: TcpConn, me: &IfInfo) {
        c.begin_close(me);
        if c.peer_fin {
            return;
        }
        let deadline = now_ns().saturating_add(CLOSE_LINGER_NS);
        let slot = match self.slots.iter().position(Option::is_none) {
            Some(i) => i,
            None => self
                .slots
                .iter()
                .enumerate()
                .min_by_key(|(_, s)| s.as_ref().map_or(0, |(_, d)| *d))
                .map_or(0, |(i, _)| i),
        };
        if let Some(s) = self.slots.get_mut(slot) {
            *s = Some((c, deadline));
        }
    }

    /// Offer an IPv4 segment to the lingering connection it belongs to.
    /// `true` if one claimed it.
    fn ingest(
        &mut self,
        me: &IfInfo,
        src_ip: &[u8; 4],
        src_port: u16,
        dst_port: u16,
        rx: &TcpRx,
    ) -> bool {
        self.ingest_where(me, rx, |c| {
            c.dst6.is_none()
                && c.dst_ip == *src_ip
                && c.dst_port == src_port
                && c.local_port == dst_port
        })
    }

    /// The IPv6 sibling of [`Self::ingest`].
    fn ingest6(
        &mut self,
        me: &IfInfo,
        src_ip6: &[u8; 16],
        src_port: u16,
        dst_port: u16,
        rx: &TcpRx,
    ) -> bool {
        self.ingest_where(me, rx, |c| {
            c.dst6 == Some(*src_ip6) && c.dst_port == src_port && c.local_port == dst_port
        })
    }

    fn ingest_where(&mut self, me: &IfInfo, rx: &TcpRx, owns: impl Fn(&TcpConn) -> bool) -> bool {
        for slot in &mut self.slots {
            let done = match slot.as_mut() {
                Some((c, _)) if owns(c) => c.closing_seg(me, rx),
                _ => continue,
            };
            if done {
                *slot = None;
            }
            return true;
        }
        false
    }

    /// Drop the connections whose linger has run out.
    fn expire(&mut self, now: u64) {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|(_, d)| now >= *d) {
                *slot = None;
            }
        }
    }
}

impl Net {
    fn new() -> Self {
        Self {
            conns: RingConns::new(),
            listeners: Listeners::new(),
            udp: UdpSocks::new(),
            mcast: mcast::HostGroups::new(),
            filter: McastFilter {
                addrs: [[0; 6]; MAX_FILTER],
                len: 0,
                pushed: false,
            },
            closing: Closing::new(),
            dhcp: None,
            dhcp_event: None,
            relinks: 0,
            confirmed: 0,
            dhcp_server_mac: None,
        }
    }

    /// Close every connection, listener and datagram socket `session` created:
    /// its ring's `OP_STOP`, or the daemon shutting down.
    fn close_session(&mut self, session: u32, me: &IfInfo) {
        let Self {
            conns,
            listeners,
            closing,
            ..
        } = self;
        for slot in &mut conns.slots {
            if slot
                .as_ref()
                .is_some_and(|(k, _)| key_session(*k) == session)
                && let Some((_, c)) = slot.take()
            {
                closing.retire(c, me);
            }
        }
        for slot in &mut listeners.slots {
            if slot
                .as_ref()
                .is_some_and(|(k, _)| key_session(*k) == session)
                && let Some((_, mut l)) = slot.take()
            {
                l.close_all(me, closing);
            }
        }
        let Self { udp, mcast, .. } = self;
        for slot in &mut udp.slots {
            if slot
                .as_ref()
                .is_some_and(|(k, _)| key_session(*k) == session)
                && let Some((_, sock)) = slot.take()
            {
                leave_all(mcast, &sock.mcast, me);
            }
        }
        sync_mcast_filter(self, me);
    }
}

/// Most rings the daemon keeps mapped at once. In design B of A-Q15 (a ring
/// per socket) this is the ceiling on sockets; in design A the kernel uses one.
const MAX_SESSIONS: usize = 64;

/// One mapped client ring.
struct Session {
    /// SHM handle of the ring.
    handle: u64,
    /// Where it is mapped.
    va: i64,
    /// Byte length of the mapping.
    size: u32,
    /// IPv4 identification seed for connections this session opens.
    ipid: u16,
}

/// Every ring the daemon has mapped, one session per client ring: its
/// `OP_RING_TCP` rounds are served against the one [`Net`].
///
/// This replaced a single `RingSession` that was torn down whenever a ring
/// with a different handle arrived, so a second ring destroyed the first ring's
/// connections, listeners and datagram sockets (A-Q15, design-decisions §972).
/// A session now ends only by its own ring's `OP_STOP`.
struct Sessions {
    slots: [Option<Session>; MAX_SESSIONS],
}

impl Sessions {
    fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
        }
    }

    /// The slot of `handle`'s session, mapping the ring if it is new. `None`
    /// if the ring cannot be mapped or every slot is taken -- the round fails
    /// rather than evicting a live session.
    fn open(&mut self, handle: u64, size: u32, seed_ipid: u16) -> Option<usize> {
        if let Some(i) = self
            .slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.handle == handle))
        {
            return Some(i);
        }
        let free = self.slots.iter().position(Option::is_none)?;
        let va = syscall2(SYS_SHM_MAP, handle, SHM_MAP_RW);
        if va < 0 {
            return None;
        }
        let slot = self.slots.get_mut(free)?;
        *slot = Some(Session {
            handle,
            va,
            size,
            ipid: seed_ipid,
        });
        Some(free)
    }

    /// End the session in `slot`: close everything it created and unmap its ring.
    fn close(&mut self, slot: usize, net: &mut Net, me: &IfInfo) {
        if let Some(s) = self.slots.get_mut(slot).and_then(Option::take) {
            #[allow(clippy::cast_possible_truncation)] // slot < MAX_SESSIONS
            net.close_session(slot as u32, me);
            syscall2(SYS_SHM_UNMAP, s.va as u64, u64::from(s.size));
        }
    }

    /// End every session (the daemon is shutting down).
    fn close_all(&mut self, net: &mut Net, me: &IfInfo) {
        for i in 0..MAX_SESSIONS {
            self.close(i, net, me);
        }
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
///   ([`pump`]) — so sibling connections' frames, on this ring or any other, are
///   routed to *them* rather than dropped — then copy the response into the SQE's
///   data window; completion `result` = bytes received, or `-1`.
/// - [`netipc::ring::OP_POLL`]: non-destructively report `sqe.conn_id`'s readiness
///   via [`ring_tcp_poll`]; completion `result` is a
///   [`POLL_READABLE`](netipc::ring::POLL_READABLE)/
///   [`POLL_WRITABLE`](netipc::ring::POLL_WRITABLE) bitmask, or `-1` (no such conn).
/// - [`netipc::ring::OP_CLOSE`]: look up and evict `sqe.conn_id`, graceful
///   teardown; completion `result = 0`, or `-1` if no such connection.
/// - [`netipc::ring::OP_NOP`]: complete with `result = 0`.
/// - [`netipc::ring::OP_STOP`]: complete with `result = 0` and request session
///   teardown (return flag) once the current batch is drained.
/// - [`netipc::ring::OP_UDP_BIND`]/[`OP_UDP_SEND`](netipc::ring::OP_UDP_SEND)/
///   [`OP_UDP_SEND6`](netipc::ring::OP_UDP_SEND6)/
///   [`OP_UDP_RECV`](netipc::ring::OP_UDP_RECV): connectionless UDP datagram
///   sockets in the daemon's [`UdpSocks`] table. Bind installs a socket under
///   `conn_id`; `OP_UDP_SEND` transmits from it to `aux`'s IPv4 endpoint, and
///   `OP_UDP_SEND6` to the IPv6 address at the front of the data window; recv
///   drains the NIC ([`pump`]) and dequeues one datagram of either family,
///   prepending the 24-byte source-address header. `OP_CLOSE` unbinds a UDP
///   socket too.
/// - any unknown opcode: `result = -1`.
///
/// `net` is every socket the daemon holds ([`Net`]) and `ipid` this ring's IPv4
/// identification seed ([`Session`]); both outlive the call, so connections
/// opened in an earlier call are still addressable here. `id_space` is this
/// ring's session number: what it creates is keyed by it ([`key`]), so two rings
/// using the same `conn_id` address different sockets. Returns
/// `(processed, stop)`: `processed` is true iff at least one SQE was handled and
/// the CQ never overflowed; `stop` is true iff an `OP_STOP` was seen (the caller
/// then closes the session).
fn ring_tcp_process(
    ring: &netring::Ring,
    net: &mut Net,
    id_space: u32,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    ipid: &mut u16,
) -> (bool, bool) {
    let mut processed = 0u32;
    let mut stop = false;
    while let Some(sqe) = ring.sq_pop() {
        // The completion's `flags`: 0 but for an op that reports more than
        // its result (`OP_POLL`'s count).
        let mut cqe_flags = 0u32;
        let result = match sqe.op {
            netipc::ring::OP_NOP => 0,
            netipc::ring::OP_STOP => {
                // Drain any remaining SQEs first, then the caller tears down.
                stop = true;
                0
            }
            netipc::ring::OP_CONNECT => 'connect: {
                let (ip, port) = netipc::ring::Sqe::unpack_endpoint(sqe.aux);
                // Advance the IPv4 ident seed past the handshake's own increments.
                *ipid = ipid.wrapping_add(0x10);
                let Some(local_port) = pick_local_port(&me.ip, &ip, port, |p| {
                    net.conns.port_in_use(p) || net.listeners.port_in_use(p)
                }) else {
                    break 'connect -1; // every ephemeral port is taken
                };
                if sqe.aux & netipc::ring::CONNECT_NONBLOCK != 0 {
                    // Non-blocking connect: transmit the SYN and install the conn in
                    // SYN_SENT without waiting for the SYN-ACK. Pump once so a SYN-ACK
                    // that has already arrived can complete the handshake immediately
                    // (a loopback/fast peer), letting us report success synchronously.
                    match TcpConn::connect_start(me, ip, port, *next_hop_mac, *ipid, local_port) {
                        Some(c) => match net.conns.reserve(key(id_space, sqe.conn_id)) {
                            Some(slot) => {
                                *slot = Some((key(id_space, sqe.conn_id), c));
                                pump(net, me, next_hop_mac);
                                match net.conns.get_mut(key(id_space, sqe.conn_id)) {
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
                    match TcpConn::connect(me, ip, port, *next_hop_mac, *ipid, local_port) {
                        Some(c) => match net.conns.reserve(key(id_space, sqe.conn_id)) {
                            Some(slot) => {
                                *slot = Some((key(id_space, sqe.conn_id), c));
                                0
                            }
                            None => {
                                // Duplicate id or table full: tear the fresh conn down
                                // gracefully rather than leaking the peer's half-open
                                // connection, then report failure.
                                net.closing.retire(c, me);
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
                let ok =
                    sqe.data_len as usize >= 16 && ring.read_data(sqe.data_off as usize, &mut addr);
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
                        Some(mac) => 'connect6: {
                            let Some(local_port) = pick_local_port(&me.ip6, &addr, port, |p| {
                                net.conns.port_in_use(p) || net.listeners.port_in_use(p)
                            }) else {
                                break 'connect6 -1; // every ephemeral port is taken
                            };
                            if sqe.aux & netipc::ring::CONNECT_NONBLOCK != 0 {
                                match TcpConn::connect_start6(
                                    me, addr, port, mac, *ipid, local_port,
                                ) {
                                    Some(c) => {
                                        match net.conns.reserve(key(id_space, sqe.conn_id)) {
                                            Some(slot) => {
                                                *slot = Some((key(id_space, sqe.conn_id), c));
                                                pump(net, me, next_hop_mac);
                                                match net.conns.get_mut(key(id_space, sqe.conn_id))
                                                {
                                                    Some(c) if c.established => 0,
                                                    Some(_) => netipc::ring::ERR_IN_PROGRESS,
                                                    None => -1,
                                                }
                                            }
                                            None => -1,
                                        }
                                    }
                                    None => -1,
                                }
                            } else {
                                match TcpConn::connect6(me, addr, port, mac, *ipid, local_port) {
                                    Some(c) => {
                                        match net.conns.reserve(key(id_space, sqe.conn_id)) {
                                            Some(slot) => {
                                                *slot = Some((key(id_space, sqe.conn_id), c));
                                                0
                                            }
                                            None => {
                                                net.closing.retire(c, me);
                                                -1
                                            }
                                        }
                                    }
                                    None => -1,
                                }
                            }
                        }
                    }
                }
            }
            netipc::ring::OP_SEND => ring_tcp_send(
                ring,
                net,
                key(id_space, sqe.conn_id),
                me,
                next_hop_mac,
                &sqe,
            ),
            netipc::ring::OP_RECV => ring_tcp_recv(
                ring,
                net,
                key(id_space, sqe.conn_id),
                me,
                next_hop_mac,
                &sqe,
            ),
            netipc::ring::OP_POLL => {
                // First try TCP -- a connection or a listener; `-1` means neither, so
                // fall through to a bound UDP datagram socket. A UDP socket is always
                // writable (a datagram send never blocks) and readable when a
                // datagram is queued.
                // Every answer that is not `-1` counts what a receive would
                // find, in the completion's `flags` (`POLL_COUNTED`).
                let (tcp, tcp_count) =
                    ring_tcp_poll(net, key(id_space, sqe.conn_id), me, next_hop_mac);
                if tcp >= 0 {
                    cqe_flags = tcp_count;
                    tcp | netipc::ring::POLL_COUNTED
                } else if net.udp.get_mut(key(id_space, sqe.conn_id)).is_some() {
                    pump(net, me, next_hop_mac);
                    let mut bits = netipc::ring::POLL_WRITABLE | netipc::ring::POLL_COUNTED;
                    if let Some(s) = net.udp.get_mut(key(id_space, sqe.conn_id)) {
                        if s.has_data() {
                            bits |= netipc::ring::POLL_READABLE;
                        }
                        cqe_flags = u32::try_from(s.next_len()).unwrap_or(u32::MAX);
                    }
                    bits
                } else {
                    -1
                }
            }
            netipc::ring::OP_CLOSE => match net.conns.remove(key(id_space, sqe.conn_id)) {
                Some(c) => {
                    net.closing.retire(c, me);
                    0
                }
                // Not a TCP connection — it may be a bound UDP datagram socket,
                // or a listener. A listener used to go only with the whole
                // session (OP_STOP); with one ring for every socket the session
                // is never stopped, so a closed listening socket must be
                // removable on its own, its unaccepted connections with it.
                None => {
                    let k = key(id_space, sqe.conn_id);
                    if let Some(sock) = net.udp.take(k) {
                        // Its groups go with it, announced as the last member
                        // of each leaves.
                        leave_all(&mut net.mcast, &sock.mcast, me);
                        sync_mcast_filter(net, me);
                        0
                    } else if net.listeners.remove(k, me, &mut net.closing) {
                        0
                    } else {
                        -1
                    }
                }
            },
            netipc::ring::OP_LISTEN => {
                // Bind a listener id to a local port (low 16 bits of aux).
                let port = (sqe.aux & 0xFFFF) as u16;
                *ipid = ipid.wrapping_add(0x10);
                if net.listeners.add(key(id_space, sqe.conn_id), port, *ipid) {
                    0
                } else {
                    -1
                }
            }
            netipc::ring::OP_ACCEPT => ring_tcp_accept(
                ring,
                net,
                key(id_space, sqe.conn_id),
                me,
                next_hop_mac,
                &sqe,
            ),
            netipc::ring::OP_LOCALADDR => {
                // getsockname: report this connection's local endpoint. The source IP
                // is the daemon's interface address (`me.ip`/`me.ip6`); the local port
                // is the ephemeral port the connection chose. Family is conveyed by the
                // written length (6 = v4, 18 = v6).
                match net.conns.get_mut(key(id_space, sqe.conn_id)) {
                    None => -1, // no such connection
                    Some(c) => {
                        if c.dst6.is_some() {
                            if sqe.data_len as usize >= 18 {
                                let mut addr = [0u8; 18];
                                addr[..16].copy_from_slice(&me.ip6);
                                addr[16..18].copy_from_slice(&c.local_port.to_be_bytes());
                                if ring.write_data(sqe.data_off as usize, &addr) {
                                    18
                                } else {
                                    -1
                                }
                            } else {
                                -1
                            }
                        } else if sqe.data_len as usize >= 6 {
                            let mut addr = [0u8; 6];
                            addr[..4].copy_from_slice(&me.ip);
                            addr[4..6].copy_from_slice(&c.local_port.to_be_bytes());
                            if ring.write_data(sqe.data_off as usize, &addr) {
                                6
                            } else {
                                -1
                            }
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
                match net.conns.get_mut(key(id_space, sqe.conn_id)) {
                    None => -1, // no such connection
                    Some(c) => {
                        c.shutdown(me, sqe.aux);
                        0
                    }
                }
            }
            netipc::ring::OP_UDP_BIND => {
                // Create/bind a datagram socket under conn_id. Low 16 bits of aux
                // are the requested local port (0 = ephemeral); OP_CLOSE unbinds it.
                // With UDP_BIND_UNBOUND it is created without a port, for the
                // options a program sets before bind(2).
                let port = (sqe.aux & 0xFFFF) as u16;
                let k = key(id_space, sqe.conn_id);
                if sqe.aux & netipc::ring::UDP_BIND_UNBOUND != 0 {
                    net.udp.open(k)
                } else {
                    net.udp.bind(k, port)
                }
            }
            netipc::ring::OP_UDP_SETOPT => {
                // A multicast option: the scalar value in aux, a join or leave's
                // group in the data window (struct ip_mreq / ipv6_mreq).
                let (option, value) = netipc::ring::Sqe::unpack_udp_opt(sqe.aux);
                let mut window = [0u8; netipc::ring::UDP_MREQ6_LEN];
                let want = (sqe.data_len as usize).min(window.len());
                if want > 0 && !ring.read_data(sqe.data_off as usize, &mut window[..want]) {
                    -1
                } else {
                    let r = udp_setopt(
                        net,
                        me,
                        key(id_space, sqe.conn_id),
                        option,
                        value,
                        &window[..want],
                    );
                    if r == 0 {
                        sync_mcast_filter(net, me);
                    }
                    r
                }
            }
            netipc::ring::OP_UDP_GETOPT => {
                let (option, _) = netipc::ring::Sqe::unpack_udp_opt(sqe.aux);
                udp_getopt(net, key(id_space, sqe.conn_id), option)
            }
            // A payload larger than one datagram is refused, never cut short.
            netipc::ring::OP_UDP_SEND if sqe.data_len as usize > UDP_DGRAM_MAX => {
                netipc::ring::ERR_MSG_SIZE
            }
            netipc::ring::OP_UDP_SEND6
                if (sqe.data_len as usize).saturating_sub(16) > UDP_DGRAM_MAX =>
            {
                netipc::ring::ERR_MSG_SIZE
            }
            netipc::ring::OP_UDP_SEND => {
                // Send one datagram from the bound socket to aux=[dst_ip:4][port_be:2].
                let (dst_ip, dst_port) = netipc::ring::Sqe::unpack_endpoint(sqe.aux);
                *ipid = ipid.wrapping_add(1);
                let id = *ipid;
                // Read the payload out of the ring data window first (immutable
                // borrow of the ring), then borrow the socket to send.
                let mut buf = [0u8; UDP_DGRAM_MAX];
                let want = (sqe.data_len as usize).min(UDP_DGRAM_MAX);
                let plen = if want == 0 {
                    0
                } else if ring.read_data(sqe.data_off as usize, &mut buf[..want]) {
                    want
                } else {
                    // fall through to error below
                    usize::MAX
                };
                if plen == usize::MAX {
                    -1
                } else {
                    let k = key(id_space, sqe.conn_id);
                    match net.udp.get_mut(k) {
                        None => -1, // not bound
                        Some(sock) if igmp::is_multicast(&dst_ip) => {
                            // To a group: the socket's TTL, the group's
                            // Ethernet address, and a copy for this machine's
                            // members unless the loop is off. TTL 0 stays here.
                            let (src_port, ttl, lp) =
                                (sock.local_port, sock.mcast.ttl, sock.mcast.loop4);
                            let sent = if ttl == 0 {
                                i32::try_from(plen).unwrap_or(i32::MAX)
                            } else {
                                udp_send_from(
                                    me,
                                    &igmp::multicast_mac(&dst_ip),
                                    src_port,
                                    &dst_ip,
                                    dst_port,
                                    &buf[..plen],
                                    id,
                                    ttl,
                                )
                            };
                            if lp && sent >= 0 {
                                let mut src = [0u8; 16];
                                src[..4].copy_from_slice(&me.ip);
                                deliver_locally(
                                    &mut net.udp,
                                    mcast::Group::V4(dst_ip),
                                    src,
                                    src_port,
                                    dst_port,
                                    &buf[..plen],
                                );
                            }
                            sent
                        }
                        Some(sock) => udp_sock_send(
                            me,
                            next_hop_mac,
                            sock,
                            &dst_ip,
                            dst_port,
                            &buf[..plen],
                            id,
                        ),
                    }
                }
            }
            netipc::ring::OP_UDP_SEND6 => {
                // Send one datagram over IPv6. The 16-byte destination address does
                // not fit in `aux`, so it rides at the front of the data window:
                // [dst_ip16:16][payload...]. Destination port is aux's low 16 bits.
                let dst_port = (sqe.aux & 0xFFFF) as u16;
                let want = sqe.data_len as usize;
                if want < 16 {
                    -1 // window too small to even hold the destination address
                } else {
                    let mut hdr = [0u8; 16];
                    let plen = (want - 16).min(UDP_DGRAM_MAX);
                    let mut buf = [0u8; UDP_DGRAM_MAX];
                    let off = sqe.data_off as usize;
                    if !ring.read_data(off, &mut hdr)
                        || (plen > 0 && !ring.read_data(off + 16, &mut buf[..plen]))
                    {
                        -1
                    } else {
                        match net.udp.get_mut(key(id_space, sqe.conn_id)) {
                            None => -1, // not bound
                            Some(sock) if mld::is_multicast(&hdr) => {
                                // As OP_UDP_SEND's group case, with the hop
                                // limit and IPv6's loop.
                                let (src_port, hops, lp) =
                                    (sock.local_port, sock.mcast.hops, sock.mcast.loop6);
                                let sent = if hops == 0 {
                                    i32::try_from(plen).unwrap_or(i32::MAX)
                                } else {
                                    udp_send6_from(
                                        me,
                                        &mld::multicast_mac(&hdr),
                                        src_port,
                                        &hdr,
                                        dst_port,
                                        &buf[..plen],
                                        hops,
                                    )
                                };
                                if lp && sent >= 0 {
                                    deliver_locally(
                                        &mut net.udp,
                                        mcast::Group::V6(hdr),
                                        me.ip6,
                                        src_port,
                                        dst_port,
                                        &buf[..plen],
                                    );
                                }
                                sent
                            }
                            Some(sock) => {
                                udp_sock_send6(me, next_hop_mac, sock, &hdr, dst_port, &buf[..plen])
                            }
                        }
                    }
                }
            }
            netipc::ring::OP_UDP_RECV => {
                // Drain the NIC (routing to all bound socks), then dequeue the
                // oldest datagram for this socket. The daemon prepends a 24-byte
                // source-address header (Sqe::pack_udp_addr) to the data window,
                // payload following; result = payload length (header excluded).
                let nonblock = sqe.aux & netipc::ring::RECV_NONBLOCK != 0;
                pump(net, me, next_hop_mac);
                let hdr_len = netipc::ring::UDP_ADDR_HDR_LEN;
                let cap = sqe.data_len as usize;
                if cap < hdr_len {
                    -1 // window too small even for the address header
                } else {
                    let mut payload = [0u8; UDP_DGRAM_MAX];
                    let room = (cap - hdr_len).min(UDP_DGRAM_MAX);
                    match net.udp.get_mut(key(id_space, sqe.conn_id)) {
                        None => -1, // not bound
                        Some(sock) => match sock.pop(&mut payload[..room]) {
                            Some((family, ip16, src_port, n)) => {
                                let hdr = netipc::ring::Sqe::pack_udp_addr(family, &ip16, src_port);
                                let off = sqe.data_off as usize;
                                if ring.write_data(off, &hdr)
                                    && (n == 0 || ring.write_data(off + hdr_len, &payload[..n]))
                                {
                                    i32::try_from(n).unwrap_or(i32::MAX)
                                } else {
                                    -1
                                }
                            }
                            None if nonblock => netipc::ring::ERR_WOULD_BLOCK,
                            // Blocking recv with nothing queued: would-block too. The
                            // daemon never holds a receive open (one client's wait
                            // would stall every other client's requests); the kernel
                            // waits instead -- net::socket::dgram_recv_from asks
                            // again with a backoff until a datagram or a signal
                            // arrives. (Until 2026-09-26 this comment said "the
                            // kernel client polls" and nothing did: known-issues.md
                            // A-BLOCKING-UDP-RECV-DID-NOT-BLOCK.)
                            None => netipc::ring::ERR_WOULD_BLOCK,
                        },
                    }
                }
            }
            _ => -1, // unsupported opcode: report failure, still complete
        };
        let cqe = netipc::ring::Cqe {
            user_data: sqe.user_data,
            result,
            flags: cqe_flags,
        };
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
/// ([`TcpConn::send_window_full`]). We drain pending ACKs via [`pump`] (which also
/// serves sibling connections, on every ring, so one blocked sender never starves
/// another).
/// - [`netipc::ring::SEND_NONBLOCK`] set: pump once; if the window is still full,
///   report `ERR_WOULD_BLOCK` (kernel → `EAGAIN`) rather than waiting.
/// - Blocking: pump/sleep up to the send deadline until the window drains, then
///   send. If the peer never ACKs within the deadline, report `ERR_WOULD_BLOCK`
///   so the caller can retry rather than us silently dropping the write.
fn ring_tcp_send(
    ring: &netring::Ring,
    net: &mut Net,
    target_id: Key,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    sqe: &netipc::ring::Sqe,
) -> i32 {
    match net.conns.get_mut(target_id) {
        None => return -1, // no such connection
        // A send after `shutdown(SHUT_WR)` is EPIPE, exactly like Linux: the write
        // side is closed and our FIN is already on the wire.
        Some(c) if c.write_shut => return netipc::ring::ERR_BROKEN_PIPE,
        // A send before the handshake completed is a protocol error on our minimal
        // client (no send buffering across connect): reject it so the kernel
        // surfaces ENOTCONN rather than us emitting a bad-seq segment.
        Some(c) if !c.established => return -1,
        // Every resend of the last segment went unanswered: ETIMEDOUT.
        Some(c) if c.timed_out => return netipc::ring::ERR_TIMED_OUT,
        Some(_) => {}
    }
    let nonblock = sqe.aux & netipc::ring::SEND_NONBLOCK != 0;
    // Drain any already-arrived ACKs so a window freed by the peer is visible.
    pump(net, me, next_hop_mac);
    if net
        .conns
        .get_mut(target_id)
        .is_some_and(|c| c.send_window_full())
    {
        if nonblock {
            return netipc::ring::ERR_WOULD_BLOCK; // kernel → EAGAIN
        }
        // Blocking send: wait for the outstanding segment to be ACKed, driving its
        // retransmit while we wait -- `pump` runs the retransmission timers.
        // Bounded by the send deadline.
        let mut heard = now_ns();
        loop {
            if now_ns().saturating_sub(heard) >= TCP_QUIET_NS {
                break;
            }
            if pump(net, me, next_hop_mac) {
                heard = now_ns();
            } else {
                nic::idle();
            }
            match net.conns.get_mut(target_id) {
                Some(c) if c.timed_out => return netipc::ring::ERR_TIMED_OUT,
                Some(c) if !c.send_window_full() => break, // window drained → send below
                Some(_) => {}
                None => return -1, // connection vanished mid-send
            }
        }
        if net
            .conns
            .get_mut(target_id)
            .is_some_and(|c| c.send_window_full())
        {
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
    match net.conns.get_mut(target_id) {
        Some(c) => match c.send(me, window) {
            Some(n) => n as i32,
            None => -1,
        },
        None => -1,
    }
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
/// [`pump`] rather than a 4-tuple-filtered read, so concurrent connections, on this
/// ring or any other, can all receive without starving one another
/// (D-NETSTACK-RX-DEMUX).
fn ring_tcp_recv(
    ring: &netring::Ring,
    net: &mut Net,
    target_id: Key,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    sqe: &netipc::ring::Sqe,
) -> i32 {
    match net.conns.get_mut(target_id) {
        None => return -1, // no such connection
        // A recv before the handshake completed cannot return stream data; reject so
        // the kernel surfaces ENOTCONN rather than us spinning the whole receive
        // deadline on a half-open connection.
        Some(c) if !c.established => return -1,
        Some(_) => {}
    }
    // A recv after `shutdown(SHUT_RD)` reports EOF immediately, like Linux — the
    // read side is closed, so we neither deliver buffered bytes nor wait.
    if net.conns.get_mut(target_id).is_some_and(|c| c.read_shut) {
        return 0;
    }
    if sqe.aux & netipc::ring::RECV_NONBLOCK != 0 {
        // Non-blocking receive (kernel honouring O_NONBLOCK): drain whatever has
        // already arrived exactly once, then decide immediately — never poll for
        // the full receive deadline. If nothing is buffered and the stream is
        // still open, report WOULD_BLOCK so the kernel returns EAGAIN; a buffered
        // segment (or EOF) falls through to the shared copy-out below.
        pump(net, me, next_hop_mac);
        match net.conns.get_mut(target_id) {
            Some(c) if c.rx_len == 0 && !c.peer_fin && c.timed_out => {
                return netipc::ring::ERR_TIMED_OUT;
            }
            Some(c) if c.rx_len == 0 && !c.peer_fin => return netipc::ring::ERR_WOULD_BLOCK,
            Some(_) => {}
            None => return -1,
        }
    } else {
        let mut heard = now_ns();
        loop {
            // Stop as soon as the target has in-order bytes to deliver (a blocking
            // recv returns available data promptly, like `read(2)` — it must not
            // spin the whole receive deadline once data has arrived), or its stream
            // has ended, or we've waited long enough. Without the `rx_len > 0` break
            // a recv burned the full `TCP_QUIET_NS` deadline even with data already
            // buffered — harmless for a fast path that stayed under the kernel's
            // control-channel timeout, but on the slower IPv6 loopback path it pushed
            // the round past `RECV_TIMEOUT_NS`, surfacing as a spurious `TimedOut`.
            match net.conns.get_mut(target_id) {
                Some(c) if c.rx_len > 0 || c.peer_fin || c.timed_out => break,
                Some(_) => {}
                None => break, // connection vanished (shouldn't happen mid-recv)
            }
            if now_ns().saturating_sub(heard) >= TCP_QUIET_NS {
                break;
            }
            if pump(net, me, next_hop_mac) {
                heard = now_ns();
            } else {
                nic::idle();
            }
        }
        // Nothing to deliver and the stream still open: say why, never `0`,
        // which means EOF and nothing else. Until 2026-09-26 a wait that
        // outlasted the deadline fell through to the copy-out and answered `0`,
        // and every blocking `recv(2)` on a connection quiet for two seconds
        // reported the peer closed (known-issues.md
        // `A-BLOCKING-TCP-RECV-REPORTS-EOF-AFTER-2S`). The kernel no longer asks
        // this way -- it waits itself, asking without blocking -- but the ring
        // tests in `proc/spawn.rs` do, and they read a short result as "no data
        // came back" whichever way it is said.
        match net.conns.get_mut(target_id) {
            Some(c) if c.rx_len == 0 && !c.peer_fin && c.timed_out => {
                return netipc::ring::ERR_TIMED_OUT;
            }
            Some(c) if c.rx_len == 0 && !c.peer_fin => return netipc::ring::ERR_WOULD_BLOCK,
            Some(_) => {}
            None => return -1,
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
    let n = match net.conns.get_mut(target_id) {
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
/// [`POLL_ERR`](netipc::ring::POLL_ERR)), or `-1` if there is no such
/// connection -- with the bytes a receive would find waiting, which the caller
/// reports in the completion's `flags` ([`netipc::ring::POLL_COUNTED`]): the
/// connection's buffered in-order bytes, 0 for a listener or a handshake still
/// in flight.
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
///
/// `target_id` may also name a **listener** (listener ids and connection ids
/// are separate session-local spaces, and the kernel's `poll_ready` sends the
/// listener's id for a listening socket). A listener is readable when a
/// connection has completed its handshake and waits in the backlog -- exactly
/// when `OP_ACCEPT` would dequeue one -- and never writable. Until 2026-09-26
/// this function looked only in `conns`, so a listener answered `-1`, which the
/// kernel reads as "not connected" and reports as no events at all: no
/// `poll`/`select`/`epoll` caller was ever woken by an incoming connection
/// (lane F's `requests/f-a-poll-never-reports-a-connection-waiting-on-a-listening-socket.md`).
fn ring_tcp_poll(net: &mut Net, target_id: Key, me: &IfInfo, next_hop_mac: &[u8; 6]) -> (i32, u32) {
    if net.conns.get_mut(target_id).is_none() {
        if net.listeners.get_mut(target_id).is_none() {
            return (-1, 0); // neither a connection nor a listener
        }
        // Route arrived frames first, as `ring_tcp_accept` does before it
        // dequeues: a handshake whose final ACK is waiting completes here, so
        // poll and accept judge the same backlog.
        pump(net, me, next_hop_mac);
        let bits = if net
            .listeners
            .get_mut(target_id)
            .is_some_and(|l| l.has_established())
        {
            netipc::ring::POLL_READABLE
        } else {
            0
        };
        return (bits, 0);
    }
    // Non-destructive: route any arrived frames into their owners (this completes a
    // pending handshake via `ingest_seg` if the SYN-ACK has arrived), then peek.
    pump(net, me, next_hop_mac);
    // Drive the SYN retransmit / connect timeout for a still-pending handshake.
    if let Some(c) = net.conns.get_mut(target_id) {
        c.poll_connect(me);
    }
    match net.conns.get_mut(target_id) {
        Some(c) => {
            let count = u32::try_from(c.rx_len).unwrap_or(u32::MAX);
            if c.connect_failed {
                // Failed connect: signal POLLOUT (so poll wakes) with the error bit;
                // the kernel then reports getsockopt(SO_ERROR) = ECONNREFUSED.
                return (netipc::ring::POLL_ERR | netipc::ring::POLL_WRITABLE, 0);
            }
            if !c.established {
                return (0, 0); // SYN_SENT: not yet writable, keep waiting for POLLOUT.
            }
            if c.timed_out {
                // Ready, in the sense that matters: a recv or a send now answers
                // ETIMEDOUT at once, so a poller must wake to hear it.
                return (
                    netipc::ring::POLL_ERR
                        | netipc::ring::POLL_READABLE
                        | netipc::ring::POLL_WRITABLE,
                    count,
                );
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
            (bits, count)
        }
        None => (-1, 0),
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
    net: &mut Net,
    target_id: Key,
    me: &IfInfo,
    next_hop_mac: &[u8; 6],
    sqe: &netipc::ring::Sqe,
) -> i32 {
    if net.listeners.get_mut(target_id).is_none() {
        return -1; // no such listener
    }
    // Validate the peer-address window up front so we never consume a backlog
    // connection only to fail after — the caller must supply at least 6 bytes.
    if (sqe.data_len as usize) < 6 {
        return -1;
    }
    // Drive pending passive-open handshakes (and any already-queued data) forward.
    pump(net, me, next_hop_mac);
    let conn = match net
        .listeners
        .get_mut(target_id)
        .and_then(Listener::take_established)
    {
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
        net.closing.retire(conn, me);
        return -1;
    }
    // Install under the caller-chosen id (low 32 bits of aux), in the
    // listener's session: the ring that asked.
    let new_id = key(key_session(target_id), (sqe.aux & 0xFFFF_FFFF) as u32);
    match net.conns.reserve(new_id) {
        Some(slot) => {
            *slot = Some((new_id, conn));
            0
        }
        None => {
            // Duplicate id or table full: close the accepted connection rather than
            // leak the peer's established half, then report failure.
            net.closing.retire(conn, me);
            -1
        }
    }
}

/// The process's argument block, fetched into `raw`: how many of its bytes
/// are there (0 for none, or none that fit). Native processes fetch argv
/// via `SYS_PROCESS_GET_ARGS` (the stack is bare), which hands the block
/// out once -- a second call answers 0 -- so it is fetched once and kept.
fn read_args(raw: &mut [u8; 256]) -> usize {
    let got = syscall2(
        SYS_PROCESS_GET_ARGS,
        raw.as_mut_ptr() as u64,
        raw.len() as u64,
    );
    usize::try_from(got).map_or(0, |n| n.min(raw.len()))
}

/// `argv[n]` in the argument block `raw` ([`read_args`]): empty when there
/// is none. The block is a `SpawnArgsHeader` (argc:u32, envc:u32,
/// argv_len:u32, envp_len:u32) followed by packed NUL-terminated argv
/// strings, then envp strings.
fn arg(raw: &[u8], n: u32) -> &[u8] {
    let Some(header) = raw.get(..16) else {
        return &[];
    };
    let argc = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    if argc <= n {
        return &[];
    }
    raw[16..].split(|&b| b == 0).nth(n as usize).unwrap_or(&[])
}

// ---------------------------------------------------------------------------
// `nic-check`: a card a driver process serves (design-decisions 1589)
// ---------------------------------------------------------------------------

/// The pings `nic-check` sends the gateway, how much each carries, and how
/// many go out before their answers are awaited: full-size frames, enough
/// for every one of the ring's slots to be used many times over, several in
/// flight at once each way.
const NIC_PINGS: usize = 400;
const NIC_PING_BYTES: usize = 1000;
const NIC_BURST: usize = 16;
/// The echo identifier `nic-check`'s pings carry.
const NIC_PING_ID: u16 = 0x5158;

/// What `nic-check` exits with: what failed (`netipc::nicport::check`,
/// which the kernel's self-test reads).
const NIC_OK: i64 = nic_exit(check::OK);
const NIC_NO_CARD: i64 = nic_exit(check::NO_CARD);
const NIC_GRANT: i64 = nic_exit(check::GRANT);
const NIC_NO_ADDRESS: i64 = nic_exit(check::NO_ADDRESS);
const NIC_NO_GATEWAY: i64 = nic_exit(check::NO_GATEWAY);
const NIC_PINGS_LOST: i64 = nic_exit(check::PINGS_LOST);
const NIC_TURNED_AWAY: i64 = nic_exit(check::TURNED_AWAY);
const NIC_LINK: i64 = nic_exit(check::LINK);
const NIC_MCAST: i64 = nic_exit(check::MCAST);
const NIC_PROTOCOL: i64 = nic_exit(check::PROTOCOL);
const NIC_USAGE: i64 = nic_exit(check::USAGE);
const NIC_LEASE: i64 = nic_exit(check::LEASE);

/// An exit code as `exit` takes it.
const fn nic_exit(code: i32) -> i64 {
    code as i64
}

use netipc::nicport::check;
/// SlateOS's `PermissionDenied`.
const PERMISSION_DENIED: i64 = -400;

/// `nic-check <service>`, the boot test's check of a card a driver process
/// serves: attached to by its service name; the kernel refusing to let the
/// daemon pass on a region it was never given; an address taken by DHCP
/// from the network beyond the card; the gateway resolved by ARP; then
/// [`NIC_PINGS`] pings of [`NIC_PING_BYTES`] bytes, [`NIC_BURST`] at a time,
/// each answered within a second -- sent again once if not -- with its
/// payload as sent; the card's link told up; its multicast filter as the
/// daemon lists it ([`nic_check_mcast`]) where the network is QEMU's; and,
/// with [`Beyond::QemuCable`], the link following the card's cable out and
/// back in ([`nic_check_cable`]). The exit code says what failed, 0 for
/// nothing.
fn nic_check(service: &[u8], beyond: Beyond) -> i64 {
    let cable = beyond.pulls_cable();
    let say = |what: &[u8]| {
        Line::new()
            .put(b"[netstack] nic-check: ")
            .put(what)
            .put(b"\n")
            .print();
    };
    let port = match attach_card(service, b"nic-check") {
        Ok(port) => port,
        Err(code) => return code,
    };
    let refused = port::grant_unheld();
    if refused != PERMISSION_DENIED {
        Line::new()
            .put(b"[netstack] nic-check: FAIL: passing on a region never granted answered ")
            .dec(refused.unsigned_abs())
            .put(if refused < 0 { b" (negated)\n" } else { b"\n" })
            .print();
        return NIC_GRANT;
    }
    let mut me = card_if_info(port.mac);
    let lease = match dhcp_lease(&mut me) {
        Ok(lease) => lease,
        Err(why) => {
            say(why.as_bytes());
            return NIC_NO_ADDRESS;
        }
    };
    Line::new()
        .put(b"[netstack] nic-check: DHCP gave ")
        .ip(&me.ip)
        .put(b" mask ")
        .ip(&me.mask)
        .put(b", gateway ")
        .ip(&me.gateway)
        .put(b"\n")
        .print();
    let Some(gateway_mac) = (0..5).find_map(|_| arp_resolve(&me.gateway, &me)) else {
        say(b"FAIL: the gateway did not answer ARP");
        return NIC_NO_GATEWAY;
    };
    Line::new()
        .put(b"[netstack] nic-check: the gateway is at ")
        .mac(&gateway_mac)
        .put(b"\n")
        .print();
    let seen = match ping_gateway(&me, &gateway_mac, beyond.lost_most()) {
        Ok(seen) => seen,
        Err(why) => {
            say(why.as_bytes());
            return NIC_PINGS_LOST;
        }
    };
    Line::new()
        .put(b"[netstack] nic-check: ")
        .dec(NIC_PINGS as u64)
        .put(b" pings of ")
        .dec(NIC_PING_BYTES as u64)
        .put(b" bytes answered, and ")
        .dec(NIC_COLD as u64)
        .put(b" sent to an idle driver, payloads as sent (")
        .dec(seen.lost)
        .put(b" lost, ")
        .dec(seen.resent)
        .put(b" sent again, ")
        .dec(port::send_errors())
        .put(b" send(s) failed, ")
        .dec(seen.slept)
        .put(b" wait(s) with no doorbell; ")
        .dec(seen.again)
        .put(b" answer(s) twice, ")
        .dec(seen.own)
        .put(b" of its own frames back, ")
        .dec(seen.other)
        .put(b" other(s))\n")
        .print();
    // A frame twice, or our own back, is the card or its driver sending
    // what it should not; a wait the driver's doorbell did not end, other
    // than for a ping lost, is a doorbell not rung; a ping sent again more
    // than a burst's worth is one the daemon's doorbell did not get sent.
    if seen.again != 0 || seen.own != 0 {
        say(b"FAIL: frames came that were never sent, or twice");
        return NIC_PINGS_LOST;
    }
    if port::send_errors() != 0 {
        say(b"FAIL: the driver failed sends");
        return NIC_PINGS_LOST;
    }
    // A wait that ran out is a doorbell not rung -- but for a ping sent
    // again, or one the network lost, each of which makes one.
    if seen.slept > seen.resent.saturating_add(seen.lost) || seen.resent > NIC_BURST as u64 {
        say(b"FAIL: answers waited on no doorbell, or pings had to be sent again too often");
        return NIC_PINGS_LOST;
    }
    // Told right after the attach message, and read in the waits since.
    if port::link() != Some(true) {
        say(b"FAIL: the card's driver did not say its link is up");
        return NIC_LINK;
    }
    if !beyond.checks_mcast() {
        say(b"a real network, whose router need not answer a solicitation to all nodes, which the multicast check counts: not checked");
    } else if let Err(why) = nic_check_mcast(&me, &gateway_mac) {
        say(why.as_bytes());
        return NIC_MCAST;
    }
    if !cable {
        say(b"the cable is not pulled: nothing here can pull it");
    } else if let Err(why) = nic_check_cable(&me, &gateway_mac, lease) {
        say(why.as_bytes());
        return NIC_LINK;
    }
    // Told on attaching and at each change, and no more (Beyond::links_told).
    let told = port::links_told();
    let allowed = beyond.links_told();
    if !allowed.contains(&told) {
        Line::new()
            .put(b"[netstack] nic-check: FAIL: the driver told the link ")
            .dec(u64::from(told))
            .put(b" time(s), where ")
            .dec(u64::from(*allowed.start()))
            .put(b" to ")
            .dec(u64::from(*allowed.end()))
            .put(b" are right on this network\n")
            .print();
        return NIC_LINK;
    }
    // Timed as `nic-bench` times the kernel's NIC, for the two to be read
    // side by side.
    if let Err(why) = nic_bench(b"linux_e1000e", &me, &gateway_mac) {
        say(why.as_bytes());
        return NIC_PINGS_LOST;
    }
    // Last, for it ends the attachment: a message the protocol has no place
    // for from a stack -- one only a driver sends -- and the driver lets the
    // stack go, within a second.
    port::send_out_of_place();
    let deadline = now_ns().saturating_add(1_000_000_000);
    while port::wait(deadline.saturating_sub(now_ns())) != port::Woke::Gone {
        let _advertisements = take_frames(Some(&me));
        if now_ns() >= deadline {
            say(b"FAIL: the card's driver kept a stack that broke the protocol");
            return NIC_PROTOCOL;
        }
    }
    port::detach();
    say(b"SUCCESS");
    NIC_OK
}

/// What `nic-check` may take of the network beyond the card, its third
/// argument: QEMU's -- which answers a router solicitation to all nodes,
/// the multicast check's signal -- with its cable left alone, or (`cable`)
/// pulled and plugged back from outside, when asked (the boot test, through
/// QEMU's monitor); or (`lan`) a real network, whose router need not answer
/// so: the multicast check is said and skipped, a few pings may be lost
/// ([`NIC_LAN_LOST_MOST`]), the link may renegotiate once, and the cable is
/// left alone. The words are `netipc::nicport::check::mode`'s.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Beyond {
    Qemu,
    QemuCable,
    Lan,
}

impl Beyond {
    /// How many pings this network may lose ([`ping_gateway`]): none on
    /// QEMU's, [`NIC_LAN_LOST_MOST`] on a real one, whose router may drop or
    /// delay an echo under load.
    fn lost_most(self) -> u64 {
        if self == Beyond::Lan {
            NIC_LAN_LOST_MOST
        } else {
            0
        }
    }

    /// Whether the multicast check can be made: the network's router
    /// answers a solicitation to all nodes, as QEMU's does.
    fn checks_mcast(self) -> bool {
        self != Beyond::Lan
    }

    /// Whether the card's cable is pulled and plugged back from outside.
    fn pulls_cable(self) -> bool {
        self == Beyond::QemuCable
    }

    /// How many times the driver may tell the link: on attaching; with the
    /// cable pulled, down and up again; on a real network, which may
    /// renegotiate, up to once more down and up. More is a driver telling it
    /// at every turn.
    fn links_told(self) -> core::ops::RangeInclusive<u32> {
        match self {
            Beyond::Qemu => 1..=1,
            Beyond::QemuCable => 3..=3,
            Beyond::Lan => 1..=3,
        }
    }

    /// `nic-check`'s third argument, `arg` (empty when there is none), or
    /// `None` for one it does not know.
    fn from_arg(arg: &[u8]) -> Option<Beyond> {
        match arg {
            b"" => Some(Beyond::Qemu),
            check::mode::CABLE => Some(Beyond::QemuCable),
            check::mode::LAN => Some(Beyond::Lan),
            _ => None,
        }
    }
}

/// How long `nic-check` gives the network to advertise a router that is
/// not to come through, and how long one that is.
const NIC_QUIET_NS: u64 = 1_500_000_000;
const NIC_ADVERT_NS: u64 = 3_000_000_000;
/// How often a solicitation is sent while advertisements are looked for.
const NIC_SOLICIT_EVERY_NS: u64 = 500_000_000;

/// The card's multicast filter as the daemon lists it, checked with router
/// advertisements -- which QEMU's network, beyond the card, sends to all
/// nodes (ff02::1, 33:33:00:00:00:01) whenever a router is solicited: none
/// passes while the list is empty, as when the card was opened; one does
/// once all-nodes is listed, and once every multicast frame is asked for
/// instead; none again once the list is emptied. The driver answers each
/// list before the card is tried, and before the last count everything the
/// card took under the lists before is read out ([`rx_barrier`]).
fn nic_check_mcast(me: &IfInfo, gateway_mac: &[u8; 6]) -> Result<(), &'static str> {
    let all_nodes = mld::multicast_mac(&icmpv6::ALL_NODES_LINK_LOCAL);
    let before = advertisements(me, NIC_QUIET_NS, false)?;
    mcast_in_force(me, false, &[all_nodes])?;
    let listed = advertisements(me, NIC_ADVERT_NS, true)?;
    mcast_in_force(me, true, &[])?;
    let every = advertisements(me, NIC_ADVERT_NS, true)?;
    mcast_in_force(me, false, &[])?;
    rx_barrier(me, gateway_mac)?;
    let after = advertisements(me, NIC_QUIET_NS, false)?;
    Line::new()
        .put(b"[netstack] nic-check: router advertisements to all nodes: ")
        .dec(before)
        .put(b" before the card listed the group, ")
        .dec(listed)
        .put(b" once it did, ")
        .dec(every)
        .put(b" once it asked for every multicast frame, ")
        .dec(after)
        .put(b" once it asked for none\n")
        .print();
    if before != 0 || after != 0 {
        return Err("FAIL: the card passed a multicast group it was not listed");
    }
    if listed == 0 || every == 0 {
        return Err("FAIL: the card did not pass a multicast group it was listed, or every one");
    }
    Ok(())
}

/// Every frame the card took before now read out: a ping sent to the
/// gateway, and the frames that come before its answer taken (any
/// advertisement among them uncounted). The card has one receive queue and
/// the driver copies its frames into the ring in order, so whatever came
/// before the answer came before the ping was sent -- under the multicast
/// list in force before. Three tries, a second each.
fn rx_barrier(me: &IfInfo, gateway_mac: &[u8; 6]) -> Result<(), &'static str> {
    let mut frame = [0u8; MAX_FRAME];
    for attempt in 0..3 {
        let seq = NIC_PINGS + NIC_COLD + attempt;
        send_ping(me, gateway_mac, seq)?;
        let deadline = now_ns().saturating_add(1_000_000_000);
        while now_ns() < deadline {
            if port::wait(deadline.saturating_sub(now_ns())) == port::Woke::Gone {
                return Err("FAIL: the card's driver went away");
            }
            while let Ok(n) = usize::try_from(nic::recv(&mut frame)) {
                let Some(bytes) = frame.get(..n) else {
                    continue;
                };
                if echo_answer(bytes, me).is_some_and(|(s, _)| s == seq) {
                    return Ok(());
                }
                if !is_advertisement_to_all_nodes(bytes) {
                    let _ = handle_frame(bytes, me);
                }
            }
        }
    }
    Err("FAIL: the gateway answered none of three pings sent as a barrier")
}

/// `nic-bench`'s pings: how many are timed one at a time, and how many are
/// sent [`NIC_BURST`] at a time for the rate; their sequence numbers start
/// past every other check's. More than [`BENCH_LOST_MOST`] lost and the
/// timing stops: a card that drops that many frames is not being measured.
const BENCH_ONE_AT_A_TIME: usize = 200;
const BENCH_BURSTS: usize = 25;
const BENCH_SEQ: usize = 20_000;
const BENCH_LOST_MOST: u64 = 5;

/// How long the daemon's pings to the gateway take through the card it
/// uses -- a card of the kernel's claimed alone, or a driver process's card
/// ([`port`]) -- reported as `name`: [`BENCH_ONE_AT_A_TIME`] pings of
/// [`NIC_PING_BYTES`] bytes one at a time, the answered ones' round trips'
/// median, 90th percentile and most; then [`BENCH_BURSTS`] bursts of
/// [`NIC_BURST`], how long each answer took on average over the bursts
/// answered whole. Printed as process benchmark lines ([`bench_score`]) and
/// in a line of prose. Each card is waited on as fast as it can be
/// ([`nic::wait_until`]): a driver process's by its doorbell, as the daemon's
/// serving loops wait on it; the kernel's by reading it again after
/// yielding once, sooner than those loops, which sleep [`POLL_SLEEP_NS`]
/// after reading nothing -- the kernel's card's best case. A ping
/// unanswered within a second is lost: counted, left out of the round
/// trips -- its burst out of the rate, which would otherwise read the
/// second waited for it as the card's slowness -- and past
/// [`BENCH_LOST_MOST`] the end of the timing. An error for that, a card
/// gone, a send failed -- at once or once the driver has it -- or an answer
/// with another payload.
fn nic_bench(name: &[u8], me: &IfInfo, gateway_mac: &[u8; 6]) -> Result<(), &'static str> {
    const TOO_MANY: &str = "FAIL: too many pings to the gateway were lost to time the card";
    let mut rtts = [0u64; BENCH_ONE_AT_A_TIME];
    let mut timed = 0usize;
    let mut lost = 0u64;
    let failed_before = port::send_errors();
    for i in 0..BENCH_ONE_AT_A_TIME {
        let seq = BENCH_SEQ + i;
        let sent = now_ns();
        send_ping(me, gateway_mac, seq)?;
        let mut answered = [false];
        if bench_wait(me, seq, &mut answered, sent.saturating_add(1_000_000_000))? {
            if let Some(rtt) = rtts.get_mut(timed) {
                *rtt = now_ns().saturating_sub(sent);
                timed += 1;
            }
        } else {
            lost += 1;
            if lost > BENCH_LOST_MOST {
                return Err(TOO_MANY);
            }
        }
    }
    let rtts = rtts.get_mut(..timed).unwrap_or(&mut []);
    rtts.sort_unstable();
    let at = |q: usize| {
        rtts.get(q * timed.saturating_sub(1) / 100)
            .copied()
            .unwrap_or(0)
    };
    let (median, p90, most) = (at(50), at(90), at(100));
    let mut answers = 0u64;
    let mut bursts_ns = 0u64;
    for burst in 0..BENCH_BURSTS {
        let first = BENCH_SEQ + BENCH_ONE_AT_A_TIME + burst * NIC_BURST;
        let began = now_ns();
        for seq in first..first + NIC_BURST {
            send_ping(me, gateway_mac, seq)?;
        }
        let mut answered = [false; NIC_BURST];
        let deadline = began.saturating_add(1_000_000_000);
        if bench_wait(me, first, &mut answered, deadline)? {
            bursts_ns = bursts_ns.saturating_add(now_ns().saturating_sub(began));
            answers += NIC_BURST as u64;
        } else {
            lost += answered.iter().filter(|&&a| !a).count() as u64;
            if lost > BENCH_LOST_MOST {
                return Err(TOO_MANY);
            }
        }
    }
    if port::send_errors() != failed_before {
        return Err("FAIL: the driver failed sends while the card was timed");
    }
    let per_answer = bursts_ns / answers.max(1);
    bench_score(
        name,
        |l| {
            l.put(b"rtt_median");
        },
        median,
    );
    bench_score(
        name,
        |l| {
            l.put(b"rtt_p90");
        },
        p90,
    );
    bench_score(
        name,
        |l| {
            l.put(b"burst").dec(NIC_BURST as u64).put(b"_per_answer");
        },
        per_answer,
    );
    Line::new()
        .put(b"[netstack] nic-bench, ")
        .put(name)
        .put(b": ")
        .dec(timed as u64)
        .put(b" pings of ")
        .dec(NIC_PING_BYTES as u64)
        .put(b" bytes answered one at a time, round trip median ")
        .dec(median / 1_000)
        .put(b" us, 90th percentile ")
        .dec(p90 / 1_000)
        .put(b" us, most ")
        .dec(most / 1_000)
        .put(b" us; ")
        .dec(answers)
        .put(b" in whole bursts of ")
        .dec(NIC_BURST as u64)
        .put(b", ")
        .dec(1_000_000_000 / per_answer.max(1))
        .put(b" answered a second; ")
        .dec(lost)
        .put(b" lost\n")
        .print();
    Ok(())
}

/// One of [`nic_bench`]'s figures as a process benchmark line --
/// `[bench] PROCESS nic_<name>_<what> <ns> - TRACK`, `what` written onto the
/// line by the caller, `ns` nanoseconds, no target. `scripts/bench-history.py`
/// records such lines beside the kernel's scorecard but out of its suite: a
/// process's figures are not the kernel's benchmarks, and printed before the
/// scorecard they must not take places in its order.
fn bench_score(name: &[u8], what: impl FnOnce(&mut Line), ns: u64) {
    let mut line = Line::new();
    line.put(b"[bench] PROCESS nic_").put(name).put(b"_");
    what(&mut line);
    line.put(b" ").dec(ns).put(b" - TRACK\n").print();
}

/// The answers to the pings from sequence number `first`, one for each of
/// `answered`, waited for until `deadline`, each marked there as it comes;
/// other frames handled as the daemon handles any. Whether every one came:
/// the card read until the last does, or until `deadline` -- at least once
/// past it, so that an answer in by then counts, and no further than one
/// frame past it, however many keep coming. An error for a card gone or an
/// answer with another payload.
fn bench_wait(
    me: &IfInfo,
    first: usize,
    answered: &mut [bool],
    deadline: u64,
) -> Result<bool, &'static str> {
    let count = answered.len();
    let mut left = answered.iter().filter(|&&a| !a).count();
    let mut frame = [0u8; MAX_FRAME];
    while left != 0 {
        while let Ok(n) = usize::try_from(nic::recv(&mut frame)) {
            let Some(bytes) = frame.get(..n) else {
                continue;
            };
            match echo_answer(bytes, me) {
                Some((_, false)) => return Err("FAIL: a ping was answered with another payload"),
                Some((seq, true)) if (first..first + count).contains(&seq) => {
                    if let Some(slot) = answered.get_mut(seq - first)
                        && !core::mem::replace(slot, true)
                    {
                        left -= 1;
                    }
                }
                // A ping of before, answered late.
                Some(_) => {}
                None => {
                    let _ = handle_frame(bytes, me);
                }
            }
            if left == 0 || now_ns() >= deadline {
                break;
            }
        }
        if left != 0 && now_ns() >= deadline {
            return Ok(false);
        }
        if left != 0 {
            nic::wait_until(deadline)?;
        }
    }
    Ok(true)
}

/// `nic-bench <mac> <name>`: the kernel's card whose MAC address is `mac`
/// (twelve hex digits) claimed alone -- the kernel's own stack keeps its
/// primary -- an address taken on it by DHCP, the gateway resolved, and
/// the card timed ([`nic_bench`]) as `nic-check` times a driver process's
/// card: a kernel driver's path, reported as `name`, for the two to be read
/// side by side. The exit code says what failed (`netipc::nicport::check`),
/// 0 for nothing.
fn nic_bench_card(mac_hex: &[u8], name: &[u8]) -> i64 {
    let say = |what: &str| {
        print("[netstack] nic-bench: ");
        print(what);
        print("\n");
    };
    let Some(mac) = parse_mac(mac_hex) else {
        say("FAIL: usage: nic-bench <twelve hex digits of a MAC address> <name>");
        return NIC_USAGE;
    };
    if syscall2(SYS_NET_RAW_OPEN, RAW_OPEN_BY_MAC, mac.as_ptr() as u64) < 0 {
        say("FAIL: no card of the kernel's with that MAC address could be claimed");
        return NIC_NO_CARD;
    }
    let mut me = card_if_info(mac);
    let code = if let Err(why) = dhcp_lease(&mut me) {
        say(why);
        NIC_NO_ADDRESS
    } else if let Some(gateway_mac) = (0..5).find_map(|_| arp_resolve(&me.gateway, &me)) {
        match nic_bench(name, &me, &gateway_mac) {
            Ok(()) => NIC_OK,
            Err(why) => {
                say(why);
                NIC_PINGS_LOST
            }
        }
    } else {
        say("FAIL: the gateway did not answer ARP");
        NIC_NO_GATEWAY
    };
    raw_close();
    code
}

/// `SYS_NET_RAW_OPEN`'s `arg0` for the card whose MAC address `arg1` points
/// at, claimed alone.
const RAW_OPEN_BY_MAC: u64 = 1;

/// The MAC address twelve hex digits spell, `None` for anything else.
fn parse_mac(hex: &[u8]) -> Option<[u8; 6]> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    if hex.len() != 12 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (byte, pair) in mac.iter_mut().zip(hex.chunks_exact(2)) {
        *byte = digit(*pair.first()?)? << 4 | digit(*pair.get(1)?)?;
    }
    Some(mac)
}

/// The card's multicast list made `addrs`, and every multicast frame with
/// `all`; the driver's answer that it is in force waited for, two seconds
/// at most -- frames that come meanwhile taken, advertisements not counted.
fn mcast_in_force(me: &IfInfo, all: bool, addrs: &[[u8; 6]]) -> Result<(), &'static str> {
    if !port::set_mcast(all, addrs) {
        return Err("FAIL: no card to give a multicast list");
    }
    let deadline = now_ns().saturating_add(2_000_000_000);
    loop {
        match port::mcast_settled() {
            Some(0) => return Ok(()),
            Some(_) => {
                return Err("FAIL: the card's driver could not put a multicast list in force");
            }
            None if now_ns() >= deadline => {
                return Err("FAIL: the card's driver did not answer a multicast list within 2 s");
            }
            None => {}
        }
        if port::wait(deadline.saturating_sub(now_ns())) == port::Woke::Gone {
            return Err("FAIL: the card's driver went away");
        }
        let _advertisements = take_frames(Some(me));
    }
}

/// Routers solicited from the card for `within_ns` -- a solicitation every
/// [`NIC_SOLICIT_EVERY_NS`] -- and the advertisements to all nodes that
/// came counted; with `first`, done at the first.
fn advertisements(me: &IfInfo, within_ns: u64, first: bool) -> Result<u64, &'static str> {
    let mut rs = [0u8; 16];
    let n = icmpv6::write_router_solicitation(
        &mut rs,
        &me.ip6,
        &icmpv6::ALL_ROUTERS_LINK_LOCAL,
        &me.mac,
    )
    .ok_or("FAIL: no room for a router solicitation")?;
    let to = mld::multicast_mac(&icmpv6::ALL_ROUTERS_LINK_LOCAL);
    let deadline = now_ns().saturating_add(within_ns);
    let mut next = 0;
    let mut seen = 0;
    while now_ns() < deadline && !(first && seen != 0) {
        if now_ns() >= next {
            // Neighbour discovery's messages carry a hop limit of 255
            // (RFC 4861 6.1.2): a router drops any other.
            let rs = rs.get(..n).unwrap_or(&[]);
            if !send_ipv6_hops(
                me,
                &to,
                &icmpv6::ALL_ROUTERS_LINK_LOCAL,
                icmpv6::NH_ICMPV6,
                rs,
                255,
            ) {
                return Err("FAIL: a router solicitation could not be sent");
            }
            next = now_ns().saturating_add(NIC_SOLICIT_EVERY_NS);
        }
        let until = deadline.min(next);
        if port::wait(until.saturating_sub(now_ns())) == port::Woke::Gone {
            return Err("FAIL: the card's driver went away");
        }
        seen += take_frames(Some(me));
    }
    Ok(seen)
}

/// The card's link following its cable: the harness asked to pull it --
/// the boot test does, through QEMU's monitor, at the line "cable: pull
/// it" -- the link told down, and a ping sent then dropped, as on a wire
/// with nothing at its end; asked to plug it back, told up, its coming back
/// noticed and acted on -- the `lease` confirmed by INIT-REBOOT, then our
/// addresses announced ([`announce_addresses`]), then the lease renewed as
/// asked, as the serving loop's client keeps it ([`dhcp_settle`]) -- the
/// gateway answering
/// ARP again, frames flowing once more, and the ping sent while the link
/// was down never answered: had it been queued, it would have gone out once
/// the link was back. 20 s for each.
fn nic_check_cable(
    me: &IfInfo,
    gateway_mac: &[u8; 6],
    lease: dhcp_client::Lease,
) -> Result<(), &'static str> {
    use dhcp_client::Event;
    const WITHIN_NS: u64 = 20_000_000_000;
    /// The ping sent with the cable out: past every other check's.
    const CABLE_SEQ: usize = 30_000;
    print("[netstack] nic-check: cable: pull it\n");
    let down = link_becomes(false, me, WITHIN_NS).map_err(|gone| {
        gone.unwrap_or(
            "FAIL: the card's link was not told down within 20 s of its cable being pulled",
        )
    })?;
    let dropped = port::dropped_while_down();
    send_ping(me, gateway_mac, CABLE_SEQ)?;
    if port::dropped_while_down() != dropped + 1 {
        return Err("FAIL: a frame sent with the card's link down was not dropped");
    }
    print("[netstack] nic-check: cable: plug it back\n");
    let up = link_becomes(true, me, WITHIN_NS).map_err(|gone| {
        gone.unwrap_or(
            "FAIL: the card's link was not told up within 20 s of its cable being plugged back",
        )
    })?;
    if !nic::take_link_back() {
        return Err("FAIL: the card's link coming back was not noticed");
    }
    let mut client = dhcp_client::Client::new(me.mac, lease, now_ns(), dhcp_xid(&me.mac));
    client.reboot(now_ns());
    let within = now_ns().saturating_add(6_000_000_000);
    if dhcp_settle(&mut client, me, within)? != Some(Event::Confirmed) {
        return Err(
            "FAIL: the DHCP server did not confirm the lease by INIT-REBOOT once the cable was back",
        );
    }
    // Announced once confirmed (RFC 2131 §4.4.1): not an address the server
    // may since have given another.
    announce_addresses(me)?;
    client.renew(now_ns());
    let within = now_ns().saturating_add(3_000_000_000);
    if dhcp_settle(&mut client, me, within)? != Some(Event::Renewed) {
        return Err("FAIL: the DHCP server did not renew the lease when asked");
    }
    // A lease the server did not give is refused: an address of our network
    // asked for by INIT-REBOOT -- outside QEMU's range -- answered with a
    // NAK, broadcast, as a lease lost is learnt.
    let mut elsewhere = me.ip;
    elsewhere[3] = if me.ip[3] == 99 { 98 } else { 99 };
    let stranger = dhcp_client::Lease {
        ip: elsewhere,
        ..lease
    };
    let mut stranger = dhcp_client::Client::new(me.mac, stranger, now_ns(), dhcp_xid(&me.mac));
    stranger.reboot(now_ns());
    let within = now_ns().saturating_add(6_000_000_000);
    if dhcp_settle(&mut stranger, me, within)? != Some(Event::Refused) {
        return Err("FAIL: the DHCP server did not refuse a lease it never gave");
    }
    if (0..5).find_map(|_| arp_resolve(&me.gateway, me)).is_none() {
        return Err("FAIL: the gateway did not answer ARP once the cable was back");
    }
    let mut stale = [false];
    if bench_wait(
        me,
        CABLE_SEQ,
        &mut stale,
        now_ns().saturating_add(1_000_000_000),
    )? {
        return Err("FAIL: a ping sent with the card's link down went out once it was back");
    }
    Line::new()
        .put(b"[netstack] nic-check: the link went down ")
        .dec(down / 1_000_000)
        .put(b" ms after the cable was pulled, a ping then dropped, and up ")
        .dec(up / 1_000_000)
        .put(b" ms after it was plugged back; the lease confirmed by INIT-REBOOT, the addresses announced, the lease renewed and one never given refused, the gateway answered again, and the dropped ping never went out\n")
        .print();
    Ok(())
}

/// Our addresses announced again, what the link's other end knew of us
/// perhaps gone with it -- once a card's link has come back and the lease
/// is confirmed or kept: a gratuitous ARP for the IPv4 one and an
/// unsolicited neighbour advertisement for the IPv6 one. An error when
/// either could not be sent.
fn announce_addresses(me: &IfInfo) -> Result<(), &'static str> {
    if nic::send(&arp::request(&me.mac, &me.ip, &me.ip)) < 0 {
        return Err("the gratuitous ARP could not be sent once the link came back");
    }
    if !announce_ip6(me) {
        return Err("the neighbour advertisement could not be sent once the link came back");
    }
    Ok(())
}

/// The UDP ports a DHCP client is answered at, and a server is asked at.
const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_SERVER_PORT: u16 = 67;

/// The DHCP server a renewal goes to, and its next hop's MAC address, kept
/// once looked up so that each renewal does not stop the card for an ARP.
type ServerMac = Option<([u8; 4], [u8; 6])>;

/// A DHCP client message `msg` sent as `out` says
/// ([`dhcp_client::Outgoing`]), IP identification `ident`: from our address,
/// or from 0.0.0.0; to every server in an Ethernet broadcast, or to one at
/// its next hop's MAC address -- kept in `server_mac` once looked up by ARP,
/// as a unicast IP datagram is not sent in a link broadcast (RFC 1122
/// §3.3.6). Whether it was sent: not when the next hop does not answer.
fn send_dhcp(
    me: &IfInfo,
    msg: &[u8],
    out: dhcp_client::Outgoing,
    ident: u16,
    server_mac: &mut ServerMac,
) -> bool {
    let unbound = card_if_info(me.mac);
    let from = if out.bound { me } else { &unbound };
    let (to, to_mac) = match out.to {
        Some(server) => match *server_mac {
            Some((known, mac)) if known == server => (server, mac),
            _ => match arp_resolve(&me.next_hop(&server), me) {
                Some(mac) => {
                    *server_mac = Some((server, mac));
                    (server, mac)
                }
                None => return false,
            },
        },
        None => ([255; 4], [0xff; 6]),
    };
    let mut datagram = [0u8; 400];
    let Some(len) = udp::write(
        &mut datagram,
        &from.ip,
        &to,
        DHCP_CLIENT_PORT,
        DHCP_SERVER_PORT,
        msg,
    ) else {
        return false;
    };
    datagram
        .get(..len)
        .is_some_and(|d| send_ipv4(from, &to_mac, &to, ipv4::PROTO_UDP, d, ident))
}

/// What time asks of `client` done -- a message due sent ([`send_dhcp`]) --
/// and the event, if a phase ended unanswered. An error when a message due
/// could not be sent: asked again at the client's next retransmission, but
/// said, for whoever waits on the answer. Nothing to do before
/// [`dhcp_client::Client::next_ns`], which is looked at first.
fn dhcp_poll(
    client: &mut dhcp_client::Client,
    me: &IfInfo,
    server_mac: &mut ServerMac,
) -> Result<Option<dhcp_client::Event>, &'static str> {
    let now = now_ns();
    if now < client.next_ns() {
        return Ok(None);
    }
    let mut msg = [0u8; 320];
    match client.poll(now, &mut msg) {
        dhcp_client::Polled::Send(out) => {
            if msg
                .get(..out.len)
                .is_some_and(|m| send_dhcp(me, m, out, 0, server_mac))
            {
                Ok(None)
            } else {
                Err("FAIL: a DHCP message could not be sent")
            }
        }
        dhcp_client::Polled::Event(event) => Ok(Some(event)),
        dhcp_client::Polled::Idle => Ok(None),
    }
}

/// The DHCP message `payload` given to `client`: what it did to the lease,
/// if it answered the transaction the client asked about.
fn dhcp_take(client: &mut dhcp_client::Client, payload: &[u8]) -> Option<dhcp_client::Event> {
    let msg = netproto::dhcp::Message::parse(payload)?;
    client.on_reply(now_ns(), &msg)
}

/// What a UDP datagram that came at our address -- `dg`, its payload at the
/// head of `payload` -- did to the lease, when it is a DHCP reply to the
/// client's transaction: an IPv4 datagram, not a group's, to the client
/// port, as a renewal's ACK comes to the address it renews. `None` for any
/// other, which is a socket's.
fn dhcp_at_our_address(
    client: &mut dhcp_client::Client,
    dg: &UdpRx,
    payload: &[u8],
) -> Option<dhcp_client::Event> {
    if dg.dst_port != DHCP_CLIENT_PORT
        || dg.family != netipc::ring::UDP_AF_INET
        || dg.group.is_some()
    {
        return None;
    }
    payload
        .get(..dg.payload_len.min(payload.len()))
        .and_then(|m| dhcp_take(client, m))
}

/// How the serving loop ended early: a fault, said, or the card's lease
/// lost -- refused, or run out -- which its caller may take anew.
enum Ended {
    Fault(&'static str),
    LeaseLost,
}

/// The card's lease kept, a step of the serving loop: a link come back
/// starting its confirmation (INIT-REBOOT); what time asks of the client
/// done ([`dhcp_poll`]); and what happened to the lease -- as time passed,
/// or as a reply came ([`pump`]) -- acted on: our addresses announced once
/// it is confirmed or kept (best effort, an announcement unsent said), a
/// renewal said, a lease refused or run out the end of the service
/// ([`Ended::LeaseLost`]). The link alone over the kernel's NIC, whose
/// address is the kernel's.
fn lease_step(net: &mut Net, me: &IfInfo) -> Result<(), Ended> {
    use dhcp_client::Event;
    // A link come back, once it is up: one gone down again first waits for
    // its next return.
    if nic::link_up() && nic::take_link_back() {
        if let Some(client) = net.dhcp.as_mut() {
            client.reboot(now_ns());
        }
        // The server's next hop may have changed with the link.
        net.dhcp_server_mac = None;
        net.relinks = net.relinks.saturating_add(1);
        print("[netstack] the card's link came back: its lease to be confirmed\n");
    }
    let Some(client) = net.dhcp.as_mut() else {
        return Ok(());
    };
    // An unsent message is asked again at its next retransmission.
    let timed = dhcp_poll(client, me, &mut net.dhcp_server_mac).unwrap_or(None);
    for event in [timed, net.dhcp_event.take()].into_iter().flatten() {
        match event {
            Event::Renewed => print("[netstack] the card's lease was renewed\n"),
            Event::Confirmed | Event::Kept => {
                if event == Event::Confirmed {
                    net.confirmed = net.confirmed.saturating_add(1);
                }
                print(if event == Event::Confirmed {
                    "[netstack] the card's lease was confirmed\n"
                } else {
                    "[netstack] no DHCP server answered: the card's lease kept\n"
                });
                if let Err(why) = announce_addresses(me) {
                    Line::new()
                        .put(b"[netstack] WARN: ")
                        .put(why.as_bytes())
                        .put(b"\n")
                        .print();
                }
            }
            Event::Refused => {
                print(
                    "[netstack] the DHCP server refused the card's lease: the address is no longer ours\n",
                );
                return Err(Ended::LeaseLost);
            }
            Event::Expired => {
                print("[netstack] the card's lease ran out unrenewed\n");
                return Err(Ended::LeaseLost);
            }
        }
    }
    Ok(())
}

/// `client` driven until something happens to the lease, `deadline` at
/// most, outside the serving loop: what time asks done ([`dhcp_poll`]), the
/// card read as the serving loop reads it ([`recv_any`]) so that a reply
/// reaches the lease here as it would there. The event, or `None` past the
/// deadline; an error when the card has gone or a message could not be sent.
fn dhcp_settle(
    client: &mut dhcp_client::Client,
    me: &IfInfo,
    deadline: u64,
) -> Result<Option<dhcp_client::Event>, &'static str> {
    let mut frame = [0u8; MAX_FRAME];
    let mut tcp = [0u8; MAX_FRAME];
    let mut udp = [0u8; UDP_DGRAM_MAX];
    let mut server_mac = None;
    while now_ns() < deadline {
        if let Some(event) = dhcp_poll(client, me, &mut server_mac)? {
            return Ok(Some(event));
        }
        loop {
            match recv_any(me, &mut frame, &mut tcp, &mut udp) {
                Rx::Drained => break,
                Rx::Dhcp(len) => {
                    if let Some(event) = udp.get(..len).and_then(|m| dhcp_take(client, m)) {
                        return Ok(Some(event));
                    }
                }
                Rx::Udp(dg) => {
                    if let Some(event) = dhcp_at_our_address(client, &dg, &udp) {
                        return Ok(Some(event));
                    }
                }
                _ => {}
            }
        }
        nic::wait_until(deadline.min(client.next_ns()))?;
    }
    Ok(None)
}

/// The card's lease taken anew, the last one lost: an address by DHCP for
/// `me` -- perhaps not the one it had -- waited for as the first one is
/// ([`lease_for_life`]), and the client to keep it. An exit code
/// (`netipc::nicport::check`) only when the card has gone.
fn retake_lease(me: &mut IfInfo, service: &[u8]) -> Result<dhcp_client::Client, i64> {
    me.ip = [0; 4];
    let lease = lease_for_life(me, service)?;
    loopback::set_my_ip(me.ip);
    Line::new()
        .put(b"[netstack] the card's lease was taken anew: ")
        .ip(&me.ip)
        .put(b"\n")
        .print();
    Ok(dhcp_client::Client::new(
        me.mac,
        lease,
        now_ns(),
        dhcp_xid(&me.mac),
    ))
}

/// Our IPv6 link-local address announced to every node on the link: an
/// unsolicited neighbour advertisement (RFC 4861 §7.2.6), the override flag
/// set and the solicited one clear. Whether it was sent.
fn announce_ip6(me: &IfInfo) -> bool {
    let mut na = [0u8; 64];
    let dst = icmpv6::ALL_NODES_LINK_LOCAL;
    let Some(len) = icmpv6::write_neighbor_advertisement(
        &mut na,
        &me.ip6,
        &dst,
        &me.ip6,
        icmpv6::NA_FLAG_OVERRIDE,
        &me.mac,
    ) else {
        return false;
    };
    na.get(..len).is_some_and(|l4| {
        send_ipv6_hops(
            me,
            &mld::multicast_mac(&dst),
            &dst,
            icmpv6::NH_ICMPV6,
            l4,
            icmpv6::NDP_HOP_LIMIT,
        )
    })
}

/// How long until the driver tells the card's link `up` (or down); or
/// `Err(None)` past `within_ns`, and `Err(Some(why))` when the card has
/// gone -- its driver died, or let the daemon go.
fn link_becomes(up: bool, me: &IfInfo, within_ns: u64) -> Result<u64, Option<&'static str>> {
    let began = now_ns();
    let deadline = began.saturating_add(within_ns);
    while port::link() != Some(up) {
        if now_ns() >= deadline {
            return Err(None);
        }
        if port::wait(deadline.saturating_sub(now_ns())) == port::Woke::Gone {
            return Err(Some(
                "FAIL: the card's driver went away while its link was waited for",
            ));
        }
        let _advertisements = take_frames(Some(me));
    }
    Ok(now_ns().saturating_sub(began))
}

/// Every frame received so far taken: how many were router advertisements
/// to all nodes. The rest are handled as the daemon handles any, when `me`
/// is given -- an ARP request for our address answered, say -- else
/// dropped. (`port::wait` returns at once while a frame waits unread, so a
/// loop waiting on the card takes them all.)
fn take_frames(me: Option<&IfInfo>) -> u64 {
    let mut frame = [0u8; MAX_FRAME];
    let mut advertisements = 0;
    loop {
        let Ok(n) = usize::try_from(nic::recv(&mut frame)) else {
            return advertisements;
        };
        let Some(bytes) = frame.get(..n) else {
            continue;
        };
        if is_advertisement_to_all_nodes(bytes) {
            advertisements += 1;
        } else if let Some(me) = me {
            let _ = handle_frame(bytes, me);
        }
    }
}

/// Whether `frame` is an ICMPv6 router advertisement to all nodes.
fn is_advertisement_to_all_nodes(frame: &[u8]) -> bool {
    let Some(eth) = ethernet::Frame::parse(frame) else {
        return false;
    };
    let ip = eth.payload;
    eth.ethertype == ethernet::ETHERTYPE_IPV6
        && ip.get(6) == Some(&icmpv6::NH_ICMPV6)
        && ip.get(24..40) == Some(&icmpv6::ALL_NODES_LINK_LOCAL[..])
        && ip.get(ipv6::HEADER_LEN) == Some(&icmpv6::TYPE_ROUTER_ADVERTISEMENT)
}

/// What a DHCP server answered: the address offered or acknowledged, and
/// what comes with it.
struct Lease {
    /// Its address, server, time, T1 and T2: what the client that keeps it
    /// needs ([`dhcp_client::Client`]). A server that gave no time gave it
    /// for ever.
    core: dhcp_client::Lease,
    mask: [u8; 4],
    router: [u8; 4],
    dns: [u8; 4],
}

/// An address for `me` (its MAC known, its addresses all zero) by DHCP:
/// DISCOVER until an OFFER, REQUEST until an ACK -- each sent up to five
/// times, a second apart -- and `me`'s addresses filled in from the ACK:
/// the lease, for its life after ([`dhcp_client::Client`]).
fn dhcp_lease(me: &mut IfInfo) -> Result<dhcp_client::Lease, &'static str> {
    use netproto::dhcp;
    let mac = me.mac;
    let xid = dhcp_xid(&mac);
    let mut msg = [0u8; 320];
    let n = dhcp::build_discover(&mut msg, xid, &mac).ok_or("FAIL: no room for a DHCPDISCOVER")?;
    // Holding no address yet, there is nothing to answer for meanwhile.
    let offer = dhcp_exchange(mac, &msg[..n], xid, dhcp::OFFER, |_| {}).map_err(|f| f.why())?;
    let n = dhcp::build_request(
        &mut msg,
        xid,
        &mac,
        &offer.core.ip,
        Some(&offer.core.server),
    )
    .ok_or("FAIL: no room for a DHCPREQUEST")?;
    let ack = dhcp_exchange(mac, &msg[..n], xid, dhcp::ACK, |_| {}).map_err(|f| f.why())?;
    me.ip = ack.core.ip;
    me.mask = ack.mask;
    me.gateway = ack.router;
    me.dns = ack.dns;
    Ok(ack.core)
}

/// A DHCP transaction's id for the client of MAC `mac`: the time mixed with
/// the MAC's last four bytes, so that two clients starting together differ.
fn dhcp_xid(mac: &[u8; 6]) -> u32 {
    (now_ns() as u32) ^ u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]])
}

/// Why a DHCP exchange ended without the answer it waited for.
#[derive(Clone, Copy)]
enum DhcpFailed {
    /// No server answered, the message sent five times a second apart.
    NoAnswer,
    /// The server refused it (DHCPNAK).
    Refused,
    /// It could not be done: a message not sent, an answer missing a part,
    /// or the card gone.
    Failed(&'static str),
}

impl DhcpFailed {
    /// What went wrong, as the daemon's FAIL lines say it.
    fn why(self) -> &'static str {
        match self {
            Self::NoAnswer => "FAIL: no answer from a DHCP server",
            Self::Refused => "FAIL: the DHCP server refused (DHCPNAK)",
            Self::Failed(why) => why,
        }
    }
}

/// `msg` broadcast from 0.0.0.0, by the client of MAC `mac`, until the
/// server answers transaction `xid` with a message of type `want`: its
/// lease. From 0.0.0.0 whatever address the client holds -- as RFC 2131 has
/// a client send until it is sure of one, INIT-REBOOT included -- which is
/// also the source the datagram's UDP checksum is written over. Every frame
/// that is no DHCP reply is given to `other`.
fn dhcp_exchange(
    mac: [u8; 6],
    msg: &[u8],
    xid: u32,
    want: u8,
    mut other: impl FnMut(&[u8]),
) -> Result<Lease, DhcpFailed> {
    use netproto::dhcp;
    let me = &card_if_info(mac);
    let out = dhcp_client::Outgoing {
        len: msg.len(),
        to: None,
        bound: false,
    };
    let mut frame = [0u8; MAX_FRAME];
    for attempt in 0..5u16 {
        if !send_dhcp(me, msg, out, attempt, &mut None) {
            return Err(DhcpFailed::Failed("FAIL: a DHCP message could not be sent"));
        }
        let deadline = now_ns().saturating_add(1_000_000_000);
        while now_ns() < deadline {
            nic::wait_until(deadline).map_err(DhcpFailed::Failed)?;
            loop {
                let n = nic::recv(&mut frame);
                let Ok(n) = usize::try_from(n) else { break };
                let Some(bytes) = frame.get(..n) else {
                    continue;
                };
                let Some(m) = dhcp_reply(bytes) else {
                    other(bytes);
                    continue;
                };
                if m.op != dhcp::OP_REPLY || m.xid != xid {
                    continue;
                }
                match m.msg_type() {
                    Some(t) if t == want => {
                        return Ok(Lease {
                            core: dhcp_client::Lease::from_ack(&m, m.siaddr),
                            mask: m.subnet_mask().unwrap_or([255, 255, 255, 0]),
                            router: m.router().ok_or(DhcpFailed::Failed(
                                "FAIL: the DHCP server named no router",
                            ))?,
                            dns: m.dns().unwrap_or([0; 4]),
                        });
                    }
                    Some(dhcp::NAK) => return Err(DhcpFailed::Refused),
                    _ => {}
                }
            }
        }
    }
    Err(DhcpFailed::NoAnswer)
}

/// The DHCP message `frame` carries to the client port, if it carries one:
/// its bytes, whatever address it is to -- a server answers at the address
/// offered, or broadcast.
fn dhcp_payload(frame: &[u8]) -> Option<&[u8]> {
    let eth = ethernet::Frame::parse(frame)?;
    if eth.ethertype != ethernet::ETHERTYPE_IPV4 {
        return None;
    }
    let ip = ipv4::Packet::parse(eth.payload)?;
    if ip.protocol != ipv4::PROTO_UDP {
        return None;
    }
    let datagram = udp::Datagram::parse(ip.payload, &ip.src, &ip.dst)?;
    if datagram.dst_port != DHCP_CLIENT_PORT {
        return None;
    }
    Some(datagram.payload)
}

/// [`dhcp_payload`], parsed.
fn dhcp_reply(frame: &[u8]) -> Option<netproto::dhcp::Message<'_>> {
    dhcp_payload(frame).and_then(netproto::dhcp::Message::parse)
}

/// Byte `i` of ping `seq`'s payload.
fn nic_payload(seq: usize, i: usize) -> u8 {
    (seq.wrapping_mul(7).wrapping_add(i)) as u8
}

/// What [`ping_gateway`] saw besides the answers it waited for: pings lost
/// on a real network, pings sent again, waits that ran to their deadline
/// with no doorbell, answers that came twice, the daemon's own frames come
/// back, and other frames.
#[derive(Default)]
struct Seen {
    lost: u64,
    resent: u64,
    slept: u64,
    again: u64,
    own: u64,
    other: u64,
}

/// The pings `nic-check` sends after the bursts, one at a time, each to a
/// driver left idle first: its doorbell must wake the driver to send it.
/// (In the bursts it never has to: answers read just before ring it awake,
/// and on one CPU the burst is in the ring by the time it runs.)
const NIC_COLD: usize = 8;
const NIC_COLD_QUIET_NS: u64 = 100_000_000;

/// How many pings `nic-check` may lose on a real network (`lan`), where a
/// router may drop or delay an echo under load: a burst's ping unanswered
/// twice, or an idle driver's unanswered once, counted, not failed -- 1% of
/// them. More is a card or driver losing frames.
const NIC_LAN_LOST_MOST: u64 = 4;

/// The failure for more pings lost than the network may lose: `lost` said,
/// against `lost_most`, when it may lose any -- a real network's, which a
/// QEMU doorbell failure must not read like.
fn too_many_lost(lost: u64, lost_most: u64) -> &'static str {
    if lost_most == 0 {
        return "FAIL: a ping went unanswered, sent twice";
    }
    Line::new()
        .put(b"[netstack] nic-check: ")
        .dec(lost)
        .put(b" pings lost, more than the ")
        .dec(lost_most)
        .put(b" this network may lose\n")
        .print();
    "FAIL: more pings lost than this network may lose"
}

/// The pings to the gateway: [`NIC_PINGS`] [`NIC_BURST`] at a time, each
/// answered within a second -- sent again once if not -- then [`NIC_COLD`]
/// to an idle driver, each answered within a second of its one sending;
/// `lost_most` of them may go unanswered ([`Beyond::lost_most`]). What else
/// was seen, or what failed. Frames that are not an answer -- an ARP request
/// for our address, say -- are handled as the daemon handles any.
fn ping_gateway(me: &IfInfo, gateway_mac: &[u8; 6], lost_most: u64) -> Result<Seen, &'static str> {
    let mut pings = Pings {
        answered: [false; NIC_PINGS + NIC_COLD],
        resent: [false; NIC_PINGS + NIC_COLD],
        seen: Seen::default(),
    };
    for first in (0..NIC_PINGS).step_by(NIC_BURST) {
        let burst = first..(first + NIC_BURST).min(NIC_PINGS);
        for attempt in 0..2 {
            for seq in burst.clone().filter(|&s| !pings.answered[s]) {
                send_ping(me, gateway_mac, seq)?;
                if attempt > 0 {
                    pings.resent[seq] = true;
                    pings.seen.resent += 1;
                }
            }
            let deadline = now_ns().saturating_add(1_000_000_000);
            while burst.clone().any(|s| !pings.answered[s]) && now_ns() < deadline {
                pings.wait(me, deadline)?;
            }
            if burst.clone().all(|s| pings.answered[s]) {
                break;
            }
        }
        let unanswered = burst.clone().filter(|&s| !pings.answered[s]).count() as u64;
        if unanswered != 0 {
            if pings.seen.lost.saturating_add(unanswered) > lost_most {
                return Err(too_many_lost(
                    pings.seen.lost.saturating_add(unanswered),
                    lost_most,
                ));
            }
            pings.seen.lost = pings.seen.lost.saturating_add(unanswered);
        }
    }
    for seq in NIC_PINGS..NIC_PINGS + NIC_COLD {
        sleep_ns(NIC_COLD_QUIET_NS);
        send_ping(me, gateway_mac, seq)?;
        let deadline = now_ns().saturating_add(1_000_000_000);
        while !pings.answered[seq] && now_ns() < deadline {
            pings.wait(me, deadline)?;
        }
        if !pings.answered[seq] {
            if pings.seen.lost < lost_most {
                pings.seen.lost = pings.seen.lost.saturating_add(1);
                continue;
            }
            if lost_most != 0 {
                return Err(too_many_lost(pings.seen.lost.saturating_add(1), lost_most));
            }
            return Err(
                "FAIL: a ping sent to an idle driver went unanswered for a second: the daemon's doorbell did not wake it",
            );
        }
    }
    Ok(pings.seen)
}

/// The pings' state: which were answered, which sent again, what else
/// was seen.
struct Pings {
    answered: [bool; NIC_PINGS + NIC_COLD],
    resent: [bool; NIC_PINGS + NIC_COLD],
    seen: Seen,
}

impl Pings {
    /// The driver's doorbell waited for, up to `deadline`, and every frame
    /// received taken: an answer marked -- one answered before counted as
    /// twice, unless it was sent twice -- anything else counted and handled.
    fn wait(&mut self, me: &IfInfo, deadline: u64) -> Result<(), &'static str> {
        match port::wait(deadline.saturating_sub(now_ns())) {
            port::Woke::Rang => {}
            port::Woke::Slept => self.seen.slept += 1,
            port::Woke::Gone => return Err("FAIL: the card's driver went away"),
        }
        let mut frame = [0u8; MAX_FRAME];
        loop {
            let n = nic::recv(&mut frame);
            let Ok(n) = usize::try_from(n) else {
                return Ok(());
            };
            let Some(bytes) = frame.get(..n) else {
                continue;
            };
            match echo_answer(bytes, me) {
                Some((seq, true)) if seq < self.answered.len() => {
                    if core::mem::replace(&mut self.answered[seq], true) && !self.resent[seq] {
                        self.seen.again += 1;
                    }
                }
                Some((_, false)) => return Err("FAIL: a ping was answered with another payload"),
                Some(_) => {}
                None => {
                    if bytes.get(6..12) == Some(&me.mac[..]) {
                        self.seen.own += 1;
                    } else {
                        self.seen.other += 1;
                    }
                    let _ = handle_frame(bytes, me);
                }
            }
        }
    }
}

/// Ping `seq` sent to the gateway, at `gateway_mac`.
fn send_ping(me: &IfInfo, gateway_mac: &[u8; 6], seq: usize) -> Result<(), &'static str> {
    let mut payload = [0u8; NIC_PING_BYTES];
    for (i, b) in payload.iter_mut().enumerate() {
        *b = nic_payload(seq, i);
    }
    let mut message = [0u8; icmp::HEADER_LEN + NIC_PING_BYTES];
    let n = icmp::write_echo(&mut message, true, NIC_PING_ID, seq as u16, &payload)
        .ok_or("FAIL: no room for a ping")?;
    if send_ipv4(
        me,
        gateway_mac,
        &me.gateway,
        ipv4::PROTO_ICMP,
        &message[..n],
        seq as u16,
    ) {
        Ok(())
    } else {
        Err("FAIL: a ping could not be sent")
    }
}

/// The ping `frame` answers, from the gateway to us -- its sequence number,
/// and whether its payload is the one sent -- if it is an answer.
fn echo_answer(frame: &[u8], me: &IfInfo) -> Option<(usize, bool)> {
    let eth = ethernet::Frame::parse(frame)?;
    if eth.ethertype != ethernet::ETHERTYPE_IPV4 {
        return None;
    }
    let ip = ipv4::Packet::parse(eth.payload)?;
    if ip.protocol != ipv4::PROTO_ICMP || ip.src != me.gateway || ip.dst != me.ip {
        return None;
    }
    let echo = icmp::Echo::parse(ip.payload)?;
    if echo.is_request || echo.id != NIC_PING_ID {
        return None;
    }
    let seq = usize::from(echo.seq);
    let intact = echo.data.len() == NIC_PING_BYTES
        && echo
            .data
            .iter()
            .enumerate()
            .all(|(i, &b)| b == nic_payload(seq, i));
    Some((seq, intact))
}

/// The card a serving mode was named (`serve-dns <service>`), attached
/// ([`attach_card`]) and given an address by DHCP: the daemon's interface on
/// it, and the client that keeps its lease. An exit code
/// (`netipc::nicport::check`) for what failed.
///
/// `for_life` (`serve-net`, the daemon a running system's sockets go
/// through): the lease is waited for rather than given up on
/// ([`lease_for_life`]). Only the card going ends it then.
fn serve_card(service: &[u8], for_life: bool) -> Result<(IfInfo, dhcp_client::Client), i64> {
    let port = attach_card(service, b"serve")?;
    let mut me = card_if_info(port.mac);
    let leased = if for_life {
        lease_for_life(&mut me, service)
    } else {
        // A bounded run is served only once its card's link is up, so it
        // asks at once, and once.
        dhcp_lease(&mut me).map_err(|why| {
            Line::new()
                .put(b"[netstack] serve: ")
                .put(why.as_bytes())
                .put(b"\n")
                .print();
            NIC_NO_ADDRESS
        })
    };
    let lease = match leased {
        Ok(lease) => lease,
        Err(code) => {
            port::detach();
            return Err(code);
        }
    };
    Line::new()
        .put(b"[netstack] serving over ")
        .put(service)
        .put(b": ")
        .mac(&me.mac)
        .put(b", DHCP gave ")
        .ip(&me.ip)
        .put(b" mask ")
        .ip(&me.mask)
        .put(b", gateway ")
        .ip(&me.gateway)
        .put(b", DNS ")
        .ip(&me.dns)
        .put(b"\n")
        .print();
    let client = dhcp_client::Client::new(me.mac, lease, now_ns(), dhcp_xid(&me.mac));
    Ok((me, client))
}

/// How long a driver may take to tell the card's link once attached: it
/// does so in the same turn as the attach message.
const LINK_TOLD_WITHIN_NS: u64 = 2_000_000_000;

/// A lease for `me` on the card `service` names, waited for for as long as
/// it takes -- for the first one, and for one lost (`retake_lease`): a card
/// with no link, or a network whose DHCP server does not answer, is asked
/// again later and later yet ([`RETAKE_FIRST_NS`], doubling to
/// [`RETAKE_MOST_NS`]), and at once when its link comes up -- a cable
/// plugged in, a WiFi network joined. Not asked while the card has no link,
/// a try that could only fail. Its link is judged once the driver has told
/// it ([`nic::link_known`]). The lease, `me` holding its addresses; an exit
/// code, `NIC_NO_CARD`, only when the card has gone.
fn lease_for_life(me: &mut IfInfo, service: &[u8]) -> Result<dhcp_client::Lease, i64> {
    let deadline = now_ns().saturating_add(LINK_TOLD_WITHIN_NS);
    while !nic::link_known() && now_ns() < deadline {
        drain_frames(me);
        if nic::wait_until(deadline).is_err() {
            break;
        }
    }
    let mut wait_ns = RETAKE_FIRST_NS;
    loop {
        let why = if nic::link_up() {
            match dhcp_lease(me) {
                Ok(lease) => return Ok(lease),
                Err(why) => why,
            }
        } else {
            "the card has no link"
        };
        Line::new()
            .put(b"[netstack] serve: ")
            .put(why.as_bytes())
            .put(b"\n")
            .print();
        Line::new()
            .put(b"[netstack] serve: no address over ")
            .put(service)
            .put(b" yet: asked again within ")
            .dec(wait_ns / 1_000_000_000)
            .put(b" s, or as soon as its link comes up\n")
            .print();
        if !rest_until_link_up(me, wait_ns) {
            print("[netstack] serve: FAIL: the card's driver went away\n");
            return Err(NIC_NO_CARD);
        }
        wait_ns = wait_ns.saturating_mul(2).min(RETAKE_MOST_NS);
    }
}

/// Every frame waiting on the card read and let go: none is for an address
/// not yet held, and one left unread would wake every wait at once.
fn drain_frames(me: &IfInfo) {
    let mut frame = [0u8; MAX_FRAME];
    let mut tcp = [0u8; MAX_FRAME];
    let mut udp = [0u8; UDP_DGRAM_MAX];
    while !matches!(recv_any(me, &mut frame, &mut tcp, &mut udp), Rx::Drained) {}
}

/// `ns` rested between two tries at a lease, cut short when the card's link
/// comes up, that being when a try can first succeed; the frames that come
/// meanwhile let go ([`drain_frames`]). False when the card has gone.
fn rest_until_link_up(me: &IfInfo, ns: u64) -> bool {
    let deadline = now_ns().saturating_add(ns);
    let was_up = nic::link_up();
    while now_ns() < deadline {
        drain_frames(me);
        // Up from down, or up at last when it was not yet told.
        if nic::take_link_back() || (!was_up && nic::link_up()) {
            return true;
        }
        if nic::wait_until(deadline).is_err() {
            return false;
        }
    }
    !nic::card_gone()
}

/// The card `service` names attached ([`port::attach`]), waiting up to 30 s
/// -- a driver probes its card before it serves it, and a bounded run waits
/// for the link too, seconds under emulation; attached says nothing of the
/// link, which the driver tells after ([`nic::link_known`]) -- and said so
/// under `who`, or why not, with the
/// exit code (`netipc::nicport::check`) for it: turned away for want of the
/// card's key, or no card.
fn attach_card(service: &[u8], who: &[u8]) -> Result<netipc::nicport::Port, i64> {
    let began = now_ns();
    match port::attach(service, 30_000_000_000) {
        Ok(port) => {
            Line::new()
                .put(b"[netstack] ")
                .put(who)
                .put(b": attached to ")
                .put(service)
                .put(b": ")
                .mac(&port.mac)
                .put(b", MTU ")
                .dec(u64::from(port.mtu))
                .put(b", a ring of ")
                .dec(u64::from(port.slots))
                .put(b" slots, after ")
                .dec(now_ns().saturating_sub(began) / 1_000_000)
                .put(b" ms\n")
                .print();
            Ok(port)
        }
        Err(e) => {
            let mut line = Line::new();
            line.put(b"[netstack] ")
                .put(who)
                .put(b": FAIL: ")
                .put(e.why.as_bytes())
                .put(b" (");
            if let Some(answer) = e.answer {
                line.put(b"the kernel answered ")
                    .put(if answer < 0 { b"-" } else { b"" })
                    .dec(answer.unsigned_abs())
                    .put(b", ");
            }
            line.put(b"after ")
                .dec(now_ns().saturating_sub(began) / 1_000_000)
                .put(b" ms)\n")
                .print();
            Err(if e.refused == Some(netipc::nicport::REFUSED_NO_KEY) {
                NIC_TURNED_AWAY
            } else {
                NIC_NO_CARD
            })
        }
    }
}

/// The daemon's interface on a card of MAC address `mac` before DHCP has
/// given it an address: its IPv6 link-local identity only.
fn card_if_info(mac: [u8; 6]) -> IfInfo {
    IfInfo {
        ip: [0; 4],
        mask: [0; 4],
        gateway: [0; 4],
        dns: [0; 4],
        mac,
        ip6: icmpv6::link_local_from_mac(&mac),
    }
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

    // `nic-check <service>` (design-decisions 1589) uses a driver process's
    // card alone: the kernel's NIC is neither queried nor claimed.
    let mut block = [0u8; 256];
    let fetched = read_args(&mut block);
    let args = &block[..fetched];
    let mode = arg(args, 1);
    if mode == b"nic-check" {
        // `nic-check <service> [cable|lan]` ([`Beyond`]).
        let Some(beyond) = Beyond::from_arg(arg(args, 3)) else {
            exit(NIC_USAGE);
        };
        exit(nic_check(arg(args, 2), beyond));
    }
    if mode == b"nic-bench" {
        // `nic-bench <mac> <name>`: a card of the kernel's timed, claimed
        // by its MAC alone, before the primary is claimed below.
        exit(nic_bench_card(arg(args, 2), arg(args, 3)));
    }

    // `serve-dns [<service>]`, `serve-net [<service>]`: with a card's service
    // named -- `nic.<name>`, a card a driver process serves (design-decisions
    // 1589) -- the daemon serves its sockets over that card, its address taken
    // there by DHCP, rather than over the kernel's NIC, which it then neither
    // queries nor claims; its lease is the daemon's own, kept in the serving
    // loop.
    let serving = mode == b"serve-dns" || mode == b"serve-net";
    let card = if serving { arg(args, 2) } else { b"" };
    // `serve-dns <service> relink`: the card's link will be taken away and
    // given back meanwhile (the boot test pulls its cable), and the lease
    // must be confirmed after.
    let expect_relink = mode == b"serve-dns" && arg(args, 3) == b"relink";
    let (mut me, dhcp) = if card.is_empty() {
        match query_if_info() {
            Some(i) => (i, None),
            None => {
                print("[netstack] FAIL: could not query interface info\n");
                exit(2);
            }
        }
    } else {
        // `serve-net` serves a running system's sockets for its life: it
        // waits for its card's network rather than give up on it.
        match serve_card(card, mode == b"serve-net") {
            Ok((me, client)) => (me, Some(client)),
            Err(code) => exit(code),
        }
    };

    // Arm the in-process loopback so frames the daemon addresses to its own IP
    // (server-socket self-connections) circulate internally instead of being
    // dropped by slirp. Inert for every other frame.
    loopback::set_my_ip(me.ip);
    // Same for the daemon's link-local IPv6 identity, so IPv6 self-connections
    // (the OP_CONNECT6 loopback self-test) circulate internally too.
    loopback::set_my_ip6(me.ip6);

    // Key TCP's port and ISN choices before any mode can open a connection.
    init_tcp_key(&me);

    if card.is_empty() {
        if !raw_open() {
            print("[netstack] FAIL: could not claim raw NIC (SYS_NET_RAW_OPEN)\n");
            exit(3);
        }
        print("[netstack] claimed raw NIC\n");
    }

    // Mode selection via argv[1]. Default = Phase-2 ARP round-trip self-test;
    // `serve-dns` = Phase-4 bounded DNS/TCP/UDP-over-IPC service (register
    // `net.stack`, serve kernel clients, then release the NIC at the idle
    // deadline); `serve-net` = Phase-5 persistent boot daemon (same service, but
    // owns the NIC for the system's lifetime and never idles out — used once the
    // kernel stack has stood down under the `net.userspace` switch).
    if mode == b"serve-dns" {
        print("[netstack] mode: serve-dns (Phase 4 bounded DNS/TCP/UDP-over-IPC)\n");
        let code = run_dns_service(&me, false, dhcp, expect_relink);
        nic::release(card);
        exit(code);
    }
    if mode == b"serve-net" {
        print("[netstack] mode: serve-net (Phase 5 persistent netstack daemon)\n");
        // Persistent: owns the NIC for the system's lifetime. Only returns on an
        // unrecoverable service fault, in which case release the NIC and exit
        // -- or, over a card, on its lease lost: a new one is taken (tried
        // again for as long as no server answers, as RFC 2131's INIT state
        // does) and the service begun again on it.
        let mut code = run_dns_service(&me, true, dhcp, false);
        while code == NIC_LEASE && nic::over_card() && !nic::card_gone() {
            code = match retake_lease(&mut me, card) {
                Ok(client) => run_dns_service(&me, true, Some(client), false),
                Err(gone) => gone,
            };
        }
        print("[netstack] persistent service returned\n");
        nic::release(card);
        exit(code);
    }

    // Signal readiness now that we own the NIC and can serve.
    syscall0(SYS_NOTIFY_READY);

    // Broadcast an ARP request for the gateway to prove TX + RX end-to-end.
    let arp_req = arp::request(&me.mac, &me.ip, &me.gateway);
    if nic::send(&arp_req) < 0 {
        print("[netstack] FAIL: raw TX of ARP request failed\n");
        raw_close();
        exit(4);
    }
    print("[netstack] sent ARP request for gateway\n");

    let mut buf = [0u8; MAX_FRAME];
    let mut resolved = false;
    for _ in 0..POLL_ITERS {
        loop {
            let n = nic::recv(&mut buf);
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
