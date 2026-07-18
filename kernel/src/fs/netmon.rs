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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** connection table.
///
/// Seeds NO connections and zero totals.  Real connections are tracked through
/// [`add_connection`] / [`close_connection`] / [`record_traffic`] as the network
/// stack opens and closes sockets; until then `/proc/netmon` and the `netmon`
/// kshell command report an empty table rather than fabricated connections —
/// the kernel's hard "never invent data in procfs" rule.
///
/// (Previously this seeded three fictional connections — sshd LISTEN on :22, a
/// browser ESTABLISHED to 93.184.216.34:443 with 4096/65536 bytes, and resolved
/// to 8.8.8.8:53 with 512/2048 bytes — plus invented aggregate totals
/// (total_created 3, total_bytes_sent 4608, total_bytes_recv 67584), which
/// `/proc/netmon` and the `netmon` command then displayed as if they were real
/// active connections.  The self-test now builds its own fixtures explicitly via
/// the real API — see [`self_test`].)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        connections: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_closed: 0,
        total_bytes_sent: 0,
        total_bytes_recv: 0,
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

/// Record traffic on a connection.
///
/// Called by the network stack as bytes flow on a tracked connection; updates
/// both the per-connection counters and the aggregate totals so `/proc/netmon`
/// reflects real throughput rather than seeded values.
pub fn record_traffic(id: u32, sent: u64, recv: u64) -> KernelResult<()> {
    with_state(|state| {
        let c = state.connections.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        c.bytes_sent += sent;
        c.bytes_recv += recv;
        state.total_bytes_sent += sent;
        state.total_bytes_recv += recv;
        Ok(())
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
    // Residue-free: begin from a clean EMPTY table and build every fixture via
    // the real API so the assertions are exact and no test connections leak into
    // the live /proc/netmon table (the kshell `netmon test` subcommand calls
    // this directly).
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated connections.
    assert_eq!(list_connections().len(), 0);
    let (a0, c0, cl0, s0, r0, _o0) = stats();
    assert_eq!((a0, c0, cl0, s0, r0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Add a listening TCP connection (ids start at 1).
    let listen_id = add_connection(Protocol::Tcp, ConnState::Listen,
        "0.0.0.0", 22, "0.0.0.0", 0, 100, "sshd").expect("add listen");
    assert_eq!(listen_id, 1);
    assert_eq!(list_connections().len(), 1);
    crate::serial_println!("  [2/8] add: OK");

    // 3: Add an established TCP and a UDP connection for the filters below.
    let tcp_id = add_connection(Protocol::Tcp, ConnState::Established,
        "10.0.2.15", 45678, "1.2.3.4", 443, 200, "browser").expect("add tcp");
    let _udp_id = add_connection(Protocol::Udp, ConnState::Established,
        "10.0.2.15", 53000, "8.8.8.8", 53, 50, "resolved").expect("add udp");
    assert_eq!(list_connections().len(), 3);
    crate::serial_println!("  [3/8] add more: OK");

    // 4: Get connection — exact fields.
    let conn = get_connection(listen_id).expect("get");
    assert_eq!((conn.protocol, conn.state, conn.local_port),
               (Protocol::Tcp, ConnState::Listen, 22));
    crate::serial_println!("  [4/8] get: OK");

    // 5: Record traffic (exact, from zero) on the established TCP connection.
    record_traffic(tcp_id, 4096, 65536).expect("traffic");
    let c = get_connection(tcp_id).expect("get tcp");
    assert_eq!((c.bytes_sent, c.bytes_recv), (4096, 65536));
    crate::serial_println!("  [5/8] traffic: OK");

    // 6: Filters reflect exact membership.
    assert_eq!(by_protocol(Protocol::Tcp).len(), 2);
    assert_eq!(by_protocol(Protocol::Udp).len(), 1);
    assert_eq!(by_state(ConnState::Listen).len(), 1);
    assert_eq!(per_process(200).len(), 1);
    crate::serial_println!("  [6/8] filters: OK");

    // 7: Close the listening connection; double-close fails.
    close_connection(listen_id).expect("close");
    assert_eq!(list_connections().len(), 2);
    assert!(close_connection(999).is_err());
    crate::serial_println!("  [7/8] close: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (active, created, closed, sent, recv, ops) = stats();
    assert_eq!((active, created, closed, sent, recv), (2, 3, 1, 4096, 65536));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/netmon table with its fixtures.  Reset to the uninitialised state so
    // production reads report an empty table until the network stack wires real
    // connection tracking.
    *STATE.lock() = None;
    crate::serial_println!("netmon::self_test() — all 8 tests passed");
}
