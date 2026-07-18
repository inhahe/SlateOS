//! Signal Queue — process signal delivery tracking.
//!
//! Tracks signal delivery between processes and the kernel.
//! Records pending, delivered, and blocked signals per process.
//! Note: our OS uses IPC for process control, not Unix signals,
//! but this tracks hardware exceptions mapped to SEH-style events.
//!
//! ## Architecture
//!
//! ```text
//! Signal queue
//!   → signalq::send(pid, signal) → queue a signal
//!   → signalq::deliver(pid) → deliver pending signals
//!   → signalq::block(pid, signal) → block signal
//!   → signalq::pending(pid) → list pending signals
//!
//! Integration:
//!   → procstat (process statistics)
//!   → tracemon (trace monitor)
//!   → audit (audit logging)
//!   → coredump (core dump)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Signal/exception type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    DivideError,
    Breakpoint,
    Overflow,
    BoundRange,
    InvalidOpcode,
    DeviceNotAvail,
    DoubleFault,
    SegmentFault,
    PageFault,
    FloatingPoint,
    AlignmentCheck,
    MachineCheck,
    UserDefined(u32),
}

impl Signal {
    pub fn label(self) -> &'static str {
        match self {
            Self::DivideError => "DIV0",
            Self::Breakpoint => "BRK",
            Self::Overflow => "OF",
            Self::BoundRange => "BR",
            Self::InvalidOpcode => "UD",
            Self::DeviceNotAvail => "NM",
            Self::DoubleFault => "DF",
            Self::SegmentFault => "GP",
            Self::PageFault => "PF",
            Self::FloatingPoint => "MF",
            Self::AlignmentCheck => "AC",
            Self::MachineCheck => "MC",
            Self::UserDefined(_) => "USR",
        }
    }

    pub fn number(self) -> u32 {
        match self {
            Self::DivideError => 0,
            Self::Breakpoint => 3,
            Self::Overflow => 4,
            Self::BoundRange => 5,
            Self::InvalidOpcode => 6,
            Self::DeviceNotAvail => 7,
            Self::DoubleFault => 8,
            Self::SegmentFault => 13,
            Self::PageFault => 14,
            Self::FloatingPoint => 16,
            Self::AlignmentCheck => 17,
            Self::MachineCheck => 18,
            Self::UserDefined(n) => 32 + n,
        }
    }
}

/// A queued signal.
#[derive(Debug, Clone)]
pub struct QueuedSignal {
    pub signal: Signal,
    pub sender_pid: u32,
    pub target_pid: u32,
    pub timestamp_ns: u64,
    pub data: u64,
    pub delivered: bool,
}

/// Per-process signal state.
#[derive(Debug, Clone)]
pub struct ProcessSignalState {
    pub pid: u32,
    pub pending: Vec<QueuedSignal>,
    pub blocked_mask: u64,
    pub total_sent: u64,
    pub total_delivered: u64,
    pub total_blocked: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROCESSES: usize = 256;
const MAX_PENDING: usize = 64;

struct State {
    processes: Vec<ProcessSignalState>,
    total_sent: u64,
    total_delivered: u64,
    total_dropped: u64,
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
    *guard = Some(State {
        processes: alloc::vec![
            ProcessSignalState { pid: 1, pending: Vec::new(), blocked_mask: 0, total_sent: 0, total_delivered: 5, total_blocked: 0 },
            ProcessSignalState { pid: 100, pending: Vec::new(), blocked_mask: 0, total_sent: 0, total_delivered: 2, total_blocked: 0 },
        ],
        total_sent: 0,
        total_delivered: 7,
        total_dropped: 0,
        ops: 0,
    });
}

/// Send a signal to a process.
pub fn send(sender: u32, target: u32, signal: Signal, data: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let proc_state = if let Some(ps) = state.processes.iter_mut().find(|p| p.pid == target) {
            ps
        } else {
            if state.processes.len() >= MAX_PROCESSES { return Err(KernelError::ResourceExhausted); }
            state.processes.push(ProcessSignalState {
                pid: target, pending: Vec::new(), blocked_mask: 0,
                total_sent: 0, total_delivered: 0, total_blocked: 0,
            });
            state.processes.last_mut().ok_or(KernelError::InternalError)?
        };
        // Check if blocked.
        let sig_bit = 1u64 << (signal.number().min(63));
        if proc_state.blocked_mask & sig_bit != 0 {
            proc_state.total_blocked += 1;
            return Ok(());
        }
        if proc_state.pending.len() >= MAX_PENDING {
            state.total_dropped += 1;
            return Err(KernelError::ResourceExhausted);
        }
        proc_state.pending.push(QueuedSignal {
            signal, sender_pid: sender, target_pid: target,
            timestamp_ns: now, data, delivered: false,
        });
        proc_state.total_sent += 1;
        state.total_sent += 1;
        Ok(())
    })
}

/// Deliver pending signals for a process. Returns count delivered.
pub fn deliver(pid: u32) -> KernelResult<u32> {
    with_state(|state| {
        let ps = state.processes.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let count = ps.pending.len() as u32;
        for s in &mut ps.pending { s.delivered = true; }
        ps.total_delivered += count as u64;
        ps.pending.clear();
        state.total_delivered += count as u64;
        Ok(count)
    })
}

/// Block a signal for a process.
pub fn block(pid: u32, signal: Signal) -> KernelResult<()> {
    with_state(|state| {
        let ps = state.processes.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let bit = 1u64 << (signal.number().min(63));
        ps.blocked_mask |= bit;
        Ok(())
    })
}

/// Unblock a signal.
pub fn unblock(pid: u32, signal: Signal) -> KernelResult<()> {
    with_state(|state| {
        let ps = state.processes.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let bit = 1u64 << (signal.number().min(63));
        ps.blocked_mask &= !bit;
        Ok(())
    })
}

/// Get pending signals for a process.
pub fn pending(pid: u32) -> Vec<QueuedSignal> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.processes.iter().find(|p| p.pid == pid)
            .map_or(Vec::new(), |p| p.pending.clone())
    })
}

/// List process signal states.
pub fn list_processes() -> Vec<ProcessSignalState> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.processes.clone())
}

/// Statistics: (process_count, total_sent, total_delivered, total_dropped, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.processes.len(), s.total_sent, s.total_delivered, s.total_dropped, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("signalq::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_processes().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Send signal.
    send(0, 1, Signal::PageFault, 0x1000).expect("send");
    let p = pending(1);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].signal.label(), "PF");
    crate::serial_println!("  [2/8] send: OK");

    // 3: Deliver.
    let count = deliver(1).expect("deliver");
    assert_eq!(count, 1);
    assert_eq!(pending(1).len(), 0);
    crate::serial_println!("  [3/8] deliver: OK");

    // 4: Block signal.
    block(1, Signal::Breakpoint).expect("block");
    send(0, 1, Signal::Breakpoint, 0).expect("send_blocked");
    assert_eq!(pending(1).len(), 0); // Blocked.
    crate::serial_println!("  [4/8] block: OK");

    // 5: Unblock.
    unblock(1, Signal::Breakpoint).expect("unblock");
    send(0, 1, Signal::Breakpoint, 0).expect("send2");
    assert_eq!(pending(1).len(), 1);
    crate::serial_println!("  [5/8] unblock: OK");

    // 6: Auto-create process.
    send(1, 999, Signal::DivideError, 0).expect("send3");
    assert_eq!(list_processes().len(), 3);
    crate::serial_println!("  [6/8] auto-create: OK");

    // 7: Signal numbers.
    assert_eq!(Signal::DivideError.number(), 0);
    assert_eq!(Signal::PageFault.number(), 14);
    assert_eq!(Signal::UserDefined(5).number(), 37);
    crate::serial_println!("  [7/8] numbers: OK");

    // 8: Stats.
    let (procs, sent, delivered, dropped, ops) = stats();
    assert_eq!(procs, 3);
    assert!(sent >= 4);
    assert!(delivered >= 8);
    let _ = dropped;
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("signalq::self_test() — all 8 tests passed");
}
