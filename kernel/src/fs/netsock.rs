//! Network Socket Statistics — TCP/UDP connection state monitoring.
//!
//! Tracks open sockets, connection states, backlog depths,
//! retransmits, and per-protocol counters. Essential for
//! diagnosing network performance and connection issues.
//!
//! ## Architecture
//!
//! ```text
//! Socket monitoring
//!   → netsock::open(proto, local, remote) → track new socket
//!   → netsock::close(id) → track socket close
//!   → netsock::set_state(id, state) → update connection state
//!   → netsock::record_traffic(id, rx, tx) → track bytes
//!
//! Integration:
//!   → netmon (network monitor)
//!   → netfilter (packet filtering)
//!   → epollstat (event polling)
//!   → taskstats (per-task accounting)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Socket protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SockProto {
    Tcp,
    Udp,
    Raw,
    Unix,
    Icmp,
}

impl SockProto {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Raw => "raw",
            Self::Unix => "unix",
            Self::Icmp => "icmp",
        }
    }
}

/// TCP connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Listen,
    SynSent,
    SynRecv,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
    Closed,
}

impl TcpState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Listen => "LISTEN",
            Self::SynSent => "SYN_SENT",
            Self::SynRecv => "SYN_RECV",
            Self::Established => "ESTABLISHED",
            Self::FinWait1 => "FIN_WAIT1",
            Self::FinWait2 => "FIN_WAIT2",
            Self::CloseWait => "CLOSE_WAIT",
            Self::Closing => "CLOSING",
            Self::LastAck => "LAST_ACK",
            Self::TimeWait => "TIME_WAIT",
            Self::Closed => "CLOSED",
        }
    }
}

/// A tracked socket.
#[derive(Debug, Clone)]
pub struct Socket {
    pub id: u32,
    pub pid: u32,
    pub proto: SockProto,
    pub local_addr: String,
    pub local_port: u16,
    pub remote_addr: String,
    pub remote_port: u16,
    pub state: TcpState,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub retransmits: u64,
    pub backlog: u32,
    pub opened_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SOCKETS: usize = 1024;

struct State {
    sockets: Vec<Socket>,
    next_id: u32,
    total_opened: u64,
    total_closed: u64,
    total_rx: u64,
    total_tx: u64,
    total_retransmits: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** socket table.
///
/// Seeds NO sockets and zero totals.  Real socket tracking is wired through
/// [`open`]/[`close`]/[`set_state`]/[`record_traffic`]/[`record_retransmit`];
/// until those are called the table is genuinely empty, so the
/// `/proc/netsock` file and the `netsock` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded three fictional sockets (e.g. pid 100 →
/// 93.184.216.34:443 ESTABLISHED with rx_bytes 50_000_000, retransmits 25)
/// plus invented aggregate totals (total_opened 5000, total_rx 500_000_000),
/// which `/proc/netsock` then displayed as if they were real connection
/// statistics.  That demo data was removed; the self-test now builds its own
/// fixtures explicitly via the real API (see [`self_test`]).  The network
/// stack is expected to call [`open`] when a socket is created and the
/// `record_*` functions as traffic flows.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        sockets: Vec::new(),
        next_id: 1,
        total_opened: 0,
        total_closed: 0,
        total_rx: 0,
        total_tx: 0,
        total_retransmits: 0,
        ops: 0,
    });
}

/// Open a new socket.
pub fn open(pid: u32, proto: SockProto, local_addr: &str, local_port: u16) -> KernelResult<u32> {
    with_state(|state| {
        if state.sockets.len() >= MAX_SOCKETS { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.total_opened += 1;
        state.sockets.push(Socket {
            id, pid, proto, local_addr: String::from(local_addr), local_port,
            remote_addr: String::from("0.0.0.0"), remote_port: 0,
            state: TcpState::Closed, rx_bytes: 0, tx_bytes: 0,
            retransmits: 0, backlog: 0, opened_ns: now,
        });
        Ok(id)
    })
}

/// Close a socket.
pub fn close(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.sockets.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        state.sockets.remove(idx);
        state.total_closed += 1;
        Ok(())
    })
}

/// Set TCP state.
pub fn set_state(id: u32, new_state: TcpState) -> KernelResult<()> {
    with_state(|state| {
        let s = state.sockets.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        s.state = new_state;
        Ok(())
    })
}

/// Record traffic on a socket.
pub fn record_traffic(id: u32, rx: u64, tx: u64) -> KernelResult<()> {
    with_state(|state| {
        let s = state.sockets.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        s.rx_bytes += rx;
        s.tx_bytes += tx;
        state.total_rx += rx;
        state.total_tx += tx;
        Ok(())
    })
}

/// Record a retransmission.
pub fn record_retransmit(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let s = state.sockets.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        s.retransmits += 1;
        state.total_retransmits += 1;
        Ok(())
    })
}

/// List all sockets.
pub fn list() -> Vec<Socket> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sockets.clone())
}

/// Sockets by protocol.
pub fn by_proto(proto: SockProto) -> Vec<Socket> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.sockets.iter().filter(|sk| sk.proto == proto).cloned().collect()
    })
}

/// Sockets by TCP state.
pub fn by_state(state_filter: TcpState) -> Vec<Socket> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.sockets.iter().filter(|sk| sk.state == state_filter).cloned().collect()
    })
}

/// Sockets for a given PID.
pub fn by_pid(pid: u32) -> Vec<Socket> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.sockets.iter().filter(|sk| sk.pid == pid).cloned().collect()
    })
}

/// Statistics: (socket_count, total_opened, total_closed, total_rx, total_tx, total_retransmits, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.sockets.len(), s.total_opened, s.total_closed, s.total_rx, s.total_tx, s.total_retransmits, s.ops),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netsock::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/netsock must never surface).
    // Resetting first clears any residue from a prior `netsock test` run so
    // the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated rows.
    assert_eq!(list().len(), 0);
    let (socks0, opened0, closed0, rx0, tx0, retr0, _o0) = stats();
    assert_eq!((socks0, opened0, closed0, rx0, tx0, retr0), (0, 0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Open a socket (ids start at 1; total_opened increments).
    let id = open(200, SockProto::Tcp, "127.0.0.1", 8080).expect("open");
    assert_eq!(id, 1);
    assert_eq!(list().len(), 1);
    crate::serial_println!("  [2/8] open: OK");

    // 3: Set state.
    set_state(id, TcpState::Listen).expect("state");
    let s = list().iter().find(|s| s.id == id).cloned().expect("sock");
    assert_eq!(s.state, TcpState::Listen);
    crate::serial_println!("  [3/8] state: OK");

    // 4: Record traffic (exact, from zero).
    record_traffic(id, 1000, 500).expect("traffic");
    let s = list().iter().find(|s| s.id == id).cloned().expect("sock");
    assert_eq!(s.rx_bytes, 1000);
    assert_eq!(s.tx_bytes, 500);
    crate::serial_println!("  [4/8] traffic: OK");

    // 5: Retransmit.
    record_retransmit(id).expect("retransmit");
    let s = list().iter().find(|s| s.id == id).cloned().expect("sock");
    assert_eq!(s.retransmits, 1);
    crate::serial_println!("  [5/8] retransmit: OK");

    // 6: Open a UDP socket for a second pid; filters reflect exact membership.
    let udp = open(201, SockProto::Udp, "0.0.0.0", 5353).expect("open udp");
    assert_eq!(by_proto(SockProto::Tcp).len(), 1);
    assert_eq!(by_proto(SockProto::Udp).len(), 1);
    assert_eq!(by_state(TcpState::Listen).len(), 1);
    assert_eq!(by_pid(200).len(), 1);
    assert_eq!(by_pid(201).len(), 1);
    crate::serial_println!("  [6/8] filters: OK");

    // 7: Close the TCP socket; total stays consistent, double-close fails.
    close(id).expect("close");
    assert_eq!(list().len(), 1); // only the UDP socket remains
    assert!(close(id).is_err());
    let _ = udp;
    crate::serial_println!("  [7/8] close: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (socks, opened, closed, rx, tx, retrans, ops) = stats();
    assert_eq!(socks, 1); // the UDP socket
    assert_eq!(opened, 2); // one TCP + one UDP opened
    assert_eq!(closed, 1); // one TCP closed
    assert_eq!(rx, 1000);
    assert_eq!(tx, 500);
    assert_eq!(retrans, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/netsock table with its fixtures.  Reset to the uninitialised
    // state so production reads report an empty table until the network stack
    // wires real socket tracking.
    *STATE.lock() = None;

    crate::serial_println!("netsock::self_test() — all 8 tests passed");
}
