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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        sockets: alloc::vec![
            Socket { id: 1, pid: 1, proto: SockProto::Tcp, local_addr: String::from("0.0.0.0"), local_port: 80, remote_addr: String::from("0.0.0.0"), remote_port: 0, state: TcpState::Listen, rx_bytes: 0, tx_bytes: 0, retransmits: 0, backlog: 128, opened_ns: now },
            Socket { id: 2, pid: 100, proto: SockProto::Tcp, local_addr: String::from("10.0.0.1"), local_port: 45000, remote_addr: String::from("93.184.216.34"), remote_port: 443, state: TcpState::Established, rx_bytes: 50_000_000, tx_bytes: 5_000_000, retransmits: 25, backlog: 0, opened_ns: now },
            Socket { id: 3, pid: 100, proto: SockProto::Udp, local_addr: String::from("10.0.0.1"), local_port: 55000, remote_addr: String::from("8.8.8.8"), remote_port: 53, state: TcpState::Established, rx_bytes: 10_000, tx_bytes: 5_000, retransmits: 0, backlog: 0, opened_ns: now },
        ],
        next_id: 4,
        total_opened: 5000,
        total_closed: 4997,
        total_rx: 500_000_000,
        total_tx: 100_000_000,
        total_retransmits: 1500,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Open socket.
    let id = open(200, SockProto::Tcp, "127.0.0.1", 8080).expect("open");
    assert!(id >= 4);
    assert_eq!(list().len(), 4);
    crate::serial_println!("  [2/8] open: OK");

    // 3: Set state.
    set_state(id, TcpState::Listen).expect("state");
    let s = list().iter().find(|s| s.id == id).cloned().unwrap();
    assert_eq!(s.state, TcpState::Listen);
    crate::serial_println!("  [3/8] state: OK");

    // 4: Record traffic.
    record_traffic(id, 1000, 500).expect("traffic");
    let s = list().iter().find(|s| s.id == id).cloned().unwrap();
    assert_eq!(s.rx_bytes, 1000);
    assert_eq!(s.tx_bytes, 500);
    crate::serial_println!("  [4/8] traffic: OK");

    // 5: Retransmit.
    record_retransmit(id).expect("retransmit");
    let s = list().iter().find(|s| s.id == id).cloned().unwrap();
    assert_eq!(s.retransmits, 1);
    crate::serial_println!("  [5/8] retransmit: OK");

    // 6: Filter by proto/state/pid.
    assert!(by_proto(SockProto::Tcp).len() >= 3);
    assert!(by_state(TcpState::Listen).len() >= 1);
    assert!(by_pid(200).len() >= 1);
    crate::serial_println!("  [6/8] filters: OK");

    // 7: Close socket.
    close(id).expect("close");
    assert_eq!(list().len(), 3);
    assert!(close(id).is_err());
    crate::serial_println!("  [7/8] close: OK");

    // 8: Stats.
    let (socks, opened, closed, rx, tx, retrans, ops) = stats();
    assert_eq!(socks, 3);
    assert!(opened > 5000);
    assert!(closed > 4997);
    assert!(rx > 500_000_000);
    assert!(tx > 100_000_000);
    assert!(retrans > 1500);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("netsock::self_test() — all 8 tests passed");
}
