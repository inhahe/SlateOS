//! Network Monitor — active network connection tracking.
//!
//! Tracks active TCP/UDP connections, their states, throughput,
//! and per-process network usage. Like `ss` or `netstat`.
//!
//! ## Architecture
//!
//! ```text
//! Network monitoring
//!   → netmon::list_connections() → active connections
//!   → netmon::add_connection(...) → register connection
//!   → netmon::close_connection(id) → close/remove
//!   → netmon::per_process(pid) → connections for a process
//!
//! Integration:
//!   → netsettings (network configuration)
//!   → netdiag (network diagnostics)
//!   → netusage (network usage)
//!   → fwsettings (firewall)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Connection protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Tcp6,
    Udp6,
    Unix,
    Raw,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
            Self::Tcp6 => "TCP6",
            Self::Udp6 => "UDP6",
            Self::Unix => "UNIX",
            Self::Raw => "RAW",
        }
    }
}

/// TCP connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Listen,
    Established,
    SynSent,
    SynRecv,
    FinWait1,
    FinWait2,
    TimeWait,
    CloseWait,
    LastAck,
    Closing,
    Closed,
}

impl ConnState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Listen => "LISTEN",
            Self::Established => "ESTABLISHED",
            Self::SynSent => "SYN_SENT",
            Self::SynRecv => "SYN_RECV",
            Self::FinWait1 => "FIN_WAIT1",
            Self::FinWait2 => "FIN_WAIT2",
            Self::TimeWait => "TIME_WAIT",
            Self::CloseWait => "CLOSE_WAIT",
            Self::LastAck => "LAST_ACK",
            Self::Closing => "CLOSING",
            Self::Closed => "CLOSED",
        }
    }
}

/// A network connection.
#[derive(Debug, Clone)]
pub struct Connection {
    pub id: u32,
    pub protocol: Protocol,
    pub state: ConnState,
    pub local_addr: String,
    pub local_port: u16,
    pub remote_addr: String,
    pub remote_port: u16,
    pub pid: u32,
    pub process_name: String,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CONNECTIONS: usize = 1024;

struct State {
    connections: Vec<Connection>,
    next_id: u32,
    total_created: u64,
    total_closed: u64,
    total_bytes_sent: u64,
    total_bytes_recv: u64,
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
        connections: alloc::vec![
            Connection {
                id: 1, protocol: Protocol::Tcp, state: ConnState::Listen,
                local_addr: String::from("0.0.0.0"), local_port: 22,
                remote_addr: String::from("0.0.0.0"), remote_port: 0,
                pid: 100, process_name: String::from("sshd"),
                bytes_sent: 0, bytes_recv: 0, created_ns: now,
            },
            Connection {
                id: 2, protocol: Protocol::Tcp, state: ConnState::Established,
                local_addr: String::from("10.0.2.15"), local_port: 45678,
                remote_addr: String::from("93.184.216.34"), remote_port: 443,
                pid: 200, process_name: String::from("browser"),
                bytes_sent: 4096, bytes_recv: 65536, created_ns: now,
            },
            Connection {
                id: 3, protocol: Protocol::Udp, state: ConnState::Established,
                local_addr: String::from("10.0.2.15"), local_port: 53000,
                remote_addr: String::from("8.8.8.8"), remote_port: 53,
                pid: 50, process_name: String::from("resolved"),
                bytes_sent: 512, bytes_recv: 2048, created_ns: now,
            },
        ],
        next_id: 4,
        total_created: 3,
        total_closed: 0,
        total_bytes_sent: 4608,
        total_bytes_recv: 67584,
        ops: 0,
    });
}

/// List all active connections.
pub fn list_connections() -> Vec<Connection> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.connections.clone())
}

/// Get connection by ID.
pub fn get_connection(id: u32) -> Option<Connection> {
    STATE.lock().as_ref().and_then(|s| s.connections.iter().find(|c| c.id == id).cloned())
}

/// Add a new connection.
pub fn add_connection(proto: Protocol, state: ConnState,
    local_addr: &str, local_port: u16,
    remote_addr: &str, remote_port: u16,
    pid: u32, process_name: &str) -> KernelResult<u32>
{
    with_state(|st| {
        if st.connections.len() >= MAX_CONNECTIONS {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = st.next_id;
        st.next_id += 1;
        st.connections.push(Connection {
            id, protocol: proto, state,
            local_addr: String::from(local_addr), local_port,
            remote_addr: String::from(remote_addr), remote_port,
            pid, process_name: String::from(process_name),
            bytes_sent: 0, bytes_recv: 0, created_ns: now,
        });
        st.total_created += 1;
        Ok(id)
    })
}

/// Close/remove a connection.
pub fn close_connection(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.connections.len();
        state.connections.retain(|c| c.id != id);
        if state.connections.len() == before { return Err(KernelError::NotFound); }
        state.total_closed += 1;
        Ok(())
    })
}

/// Get connections for a specific process.
pub fn per_process(pid: u32) -> Vec<Connection> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.connections.iter().filter(|c| c.pid == pid).cloned().collect()
    })
}

/// Filter by protocol.
pub fn by_protocol(proto: Protocol) -> Vec<Connection> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.connections.iter().filter(|c| c.protocol == proto).cloned().collect()
    })
}

/// Filter by state.
pub fn by_state(state: ConnState) -> Vec<Connection> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.connections.iter().filter(|c| c.state == state).cloned().collect()
    })
}

/// Statistics: (active_count, total_created, total_closed, bytes_sent, bytes_recv, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.connections.len(), s.total_created, s.total_closed,
                     s.total_bytes_sent, s.total_bytes_recv, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netmon::self_test() — running tests...");
    init_defaults();

    // 1: Default connections.
    assert_eq!(list_connections().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get connection.
    let conn = get_connection(1).expect("get");
    assert_eq!(conn.protocol, Protocol::Tcp);
    assert_eq!(conn.state, ConnState::Listen);
    assert_eq!(conn.local_port, 22);
    crate::serial_println!("  [2/8] get: OK");

    // 3: Add connection.
    let id = add_connection(Protocol::Tcp, ConnState::Established,
        "10.0.2.15", 12345, "1.2.3.4", 80, 300, "curl").expect("add");
    assert_eq!(list_connections().len(), 4);
    crate::serial_println!("  [3/8] add: OK");

    // 4: Per-process.
    let proc_conns = per_process(200);
    assert_eq!(proc_conns.len(), 1);
    assert_eq!(proc_conns[0].process_name, "browser");
    crate::serial_println!("  [4/8] per_process: OK");

    // 5: By protocol.
    let tcp = by_protocol(Protocol::Tcp);
    assert!(tcp.len() >= 3);
    let udp = by_protocol(Protocol::Udp);
    assert_eq!(udp.len(), 1);
    crate::serial_println!("  [5/8] by_protocol: OK");

    // 6: By state.
    let listening = by_state(ConnState::Listen);
    assert_eq!(listening.len(), 1);
    crate::serial_println!("  [6/8] by_state: OK");

    // 7: Close.
    close_connection(id).expect("close");
    assert_eq!(list_connections().len(), 3);
    assert!(close_connection(999).is_err());
    crate::serial_println!("  [7/8] close: OK");

    // 8: Stats.
    let (active, created, closed, sent, recv, ops) = stats();
    assert_eq!(active, 3);
    assert!(created >= 4);
    assert!(closed >= 1);
    let _ = (sent, recv);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("netmon::self_test() — all 8 tests passed");
}
