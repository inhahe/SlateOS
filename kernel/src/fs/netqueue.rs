//! Network Queue Statistics — per-queue and NAPI monitoring.
//!
//! Tracks network TX/RX queue depths, NAPI poll events,
//! budget exhaustion, and per-queue packet counts. Essential
//! for network I/O performance tuning.
//!
//! ## Architecture
//!
//! ```text
//! Network queue monitoring
//!   → netqueue::record_rx(queue, packets) → RX packets
//!   → netqueue::record_tx(queue, packets) → TX packets
//!   → netqueue::record_napi_poll(queue) → NAPI poll
//!   → netqueue::per_queue() → per-queue stats
//!
//! Integration:
//!   → netdev (NIC-level stats)
//!   → netsock (socket tracking)
//!   → irqstat (interrupt stats)
//!   → softirq (soft interrupts)
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

/// Queue direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueDir {
    Rx,
    Tx,
}

impl QueueDir {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rx => "rx",
            Self::Tx => "tx",
        }
    }
}

/// Per-queue stats.
#[derive(Debug, Clone)]
pub struct QueueStats {
    pub iface: String,
    pub queue_id: u32,
    pub direction: QueueDir,
    pub packets: u64,
    pub bytes: u64,
    pub drops: u64,
    pub napi_polls: u64,
    pub napi_budget_exhausted: u64,
    pub backlog_len: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_QUEUES: usize = 256;

struct State {
    queues: Vec<QueueStats>,
    total_rx_packets: u64,
    total_tx_packets: u64,
    total_napi_polls: u64,
    total_drops: u64,
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

/// Initialise an **empty** network-queue table.
///
/// Seeds NO queues and zero counters.  Real per-queue accounting is wired
/// through [`register_queue`] (one row per NIC TX/RX queue the network stack
/// brings online) and the `record_packets`/`record_drop`/`record_napi_poll`
/// functions; until those are called the table is genuinely empty, so
/// `/proc/netqueue` and the `netqueue` kshell command report zeros rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded four fictional queues (eth0 q0 RX: 100M packets
/// / 60GB / 1000 drops / 5M NAPI polls / 50k budget-exhausted; eth0 q0 TX: 80M /
/// 40GB / 500 drops; eth0 q1 RX: 90M / 55GB / 800 drops / 4.5M polls / 40k
/// exhausted; eth0 q1 TX: 70M / 35GB / 300 drops) plus invented aggregate totals
/// (total_rx_packets 190M, total_tx_packets 150M, total_napi_polls 9.5M,
/// total_drops 2600), which `/proc/netqueue` (and the `per_queue`/`for_iface`
/// views) then displayed as if they were real measured network queue traffic.
/// That demo data was removed; the self-test now builds its own fixtures
/// explicitly via the real API (see [`self_test`]).  The network stack is
/// expected to call [`register_queue`] when a queue is brought online and the
/// record functions on every packet/drop/NAPI-poll event.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        queues: Vec::new(),
        total_rx_packets: 0,
        total_tx_packets: 0,
        total_napi_polls: 0,
        total_drops: 0,
        ops: 0,
    });
}

/// Register a queue.
pub fn register_queue(iface: &str, queue_id: u32, direction: QueueDir) -> KernelResult<()> {
    with_state(|state| {
        if state.queues.len() >= MAX_QUEUES { return Err(KernelError::ResourceExhausted); }
        if state.queues.iter().any(|q| q.iface == iface && q.queue_id == queue_id && q.direction == direction) {
            return Err(KernelError::AlreadyExists);
        }
        state.queues.push(QueueStats {
            iface: String::from(iface), queue_id, direction,
            packets: 0, bytes: 0, drops: 0, napi_polls: 0,
            napi_budget_exhausted: 0, backlog_len: 0,
        });
        Ok(())
    })
}

/// Record packets on a queue.
pub fn record_packets(iface: &str, queue_id: u32, direction: QueueDir, packets: u64, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let q = state.queues.iter_mut().find(|q| q.iface == iface && q.queue_id == queue_id && q.direction == direction)
            .ok_or(KernelError::NotFound)?;
        q.packets += packets;
        q.bytes += bytes;
        match direction {
            QueueDir::Rx => state.total_rx_packets += packets,
            QueueDir::Tx => state.total_tx_packets += packets,
        }
        Ok(())
    })
}

/// Record a drop.
pub fn record_drop(iface: &str, queue_id: u32, direction: QueueDir) -> KernelResult<()> {
    with_state(|state| {
        let q = state.queues.iter_mut().find(|q| q.iface == iface && q.queue_id == queue_id && q.direction == direction)
            .ok_or(KernelError::NotFound)?;
        q.drops += 1;
        state.total_drops += 1;
        Ok(())
    })
}

/// Record a NAPI poll.
pub fn record_napi_poll(iface: &str, queue_id: u32, budget_exhausted: bool) -> KernelResult<()> {
    with_state(|state| {
        let q = state.queues.iter_mut().find(|q| q.iface == iface && q.queue_id == queue_id && q.direction == QueueDir::Rx)
            .ok_or(KernelError::NotFound)?;
        q.napi_polls += 1;
        if budget_exhausted { q.napi_budget_exhausted += 1; }
        state.total_napi_polls += 1;
        Ok(())
    })
}

/// Per-queue stats.
pub fn per_queue() -> Vec<QueueStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.queues.clone())
}

/// Queues for an interface.
pub fn for_iface(iface: &str) -> Vec<QueueStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.queues.iter().filter(|q| q.iface == iface).cloned().collect()
    })
}

/// Statistics: (queue_count, total_rx, total_tx, total_napi, total_drops, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.queues.len(), s.total_rx_packets, s.total_tx_packets, s.total_napi_polls, s.total_drops, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("netqueue::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/netqueue must never surface).  Resetting
    // first clears any residue from a prior `netqueue test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated queues or counters; record on an
    // unregistered queue fails.
    assert_eq!(per_queue().len(), 0);
    let (c0, rx0, tx0, napi0, drops0, _o0) = stats();
    assert_eq!((c0, rx0, tx0, napi0, drops0), (0, 0, 0, 0, 0));
    assert!(record_packets("eth0", 0, QueueDir::Rx, 1, 1).is_err()); // no phantom queue
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register — zeroed counters; same iface/id/dir twice fails, but the
    // opposite direction on the same id is a distinct queue.
    register_queue("eth0", 0, QueueDir::Rx).expect("register rx");
    register_queue("eth0", 0, QueueDir::Tx).expect("register tx");
    assert_eq!(per_queue().len(), 2);
    assert!(register_queue("eth0", 0, QueueDir::Rx).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Packets — per-queue packets/bytes rise; RX tally feeds total_rx.
    record_packets("eth0", 0, QueueDir::Rx, 100, 5000).expect("rx packets");
    record_packets("eth0", 0, QueueDir::Tx, 40, 2000).expect("tx packets");
    let rx = per_queue().into_iter().find(|q| q.queue_id == 0 && q.direction == QueueDir::Rx).expect("find rx");
    assert_eq!((rx.packets, rx.bytes), (100, 5000));
    crate::serial_println!("  [3/8] packets: OK");

    // 4: Drop — per-queue drops and total rise by one.
    record_drop("eth0", 0, QueueDir::Rx).expect("drop");
    assert_eq!(per_queue().into_iter().find(|q| q.direction == QueueDir::Rx).expect("find rx").drops, 1);
    crate::serial_println!("  [4/8] drop: OK");

    // 5: NAPI poll — only RX queues poll; budget-exhausted flag tallied.
    record_napi_poll("eth0", 0, true).expect("napi exhausted");
    record_napi_poll("eth0", 0, false).expect("napi ok");
    let rx = per_queue().into_iter().find(|q| q.direction == QueueDir::Rx).expect("find rx");
    assert_eq!(rx.napi_polls, 2);
    assert_eq!(rx.napi_budget_exhausted, 1); // only the first was exhausted
    crate::serial_println!("  [5/8] napi: OK");

    // 6: for_iface filters by interface name.
    register_queue("lo", 0, QueueDir::Rx).expect("register lo");
    assert_eq!(for_iface("eth0").len(), 2);
    assert_eq!(for_iface("lo").len(), 1);
    crate::serial_println!("  [6/8] for iface: OK");

    // 7: Unknown queue → NotFound on every record path.
    assert!(record_packets("nonexist", 0, QueueDir::Rx, 0, 0).is_err());
    assert!(record_drop("nonexist", 0, QueueDir::Rx).is_err());
    assert!(record_napi_poll("nonexist", 0, false).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals are exact: rx 100, tx 40, 2 NAPI polls, 1 drop.
    let (queues, rx_total, tx_total, napi, drops, ops) = stats();
    assert_eq!(queues, 3);
    assert_eq!(rx_total, 100);
    assert_eq!(tx_total, 40);
    assert_eq!(napi, 2);
    assert_eq!(drops, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/netqueue table.
    *STATE.lock() = None;

    crate::serial_println!("netqueue::self_test() — all 8 tests passed");
}
