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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        processes: alloc::vec![
            ProcessSignalState {
                pid: 1,
                pending: Vec::new(),
                blocked_mask: 0,
                total_sent: 0,
                total_delivered: 5,
                total_blocked: 0
            },
            ProcessSignalState {
                pid: 100,
                pending: Vec::new(),
                blocked_mask: 0,
                total_sent: 0,
                total_delivered: 2,
                total_blocked: 0
            },
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
            if state.processes.len() >= MAX_PROCESSES {
                return Err(KernelError::ResourceExhausted);
            }
            state.processes.push(ProcessSignalState {
                pid: target,
                pending: Vec::new(),
                blocked_mask: 0,
                total_sent: 0,
                total_delivered: 0,
                total_blocked: 0,
            });
            state
                .processes
                .last_mut()
                .ok_or(KernelError::InternalError)?
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
            signal,
            sender_pid: sender,
            target_pid: target,
            timestamp_ns: now,
            data,
            delivered: false,
        });
        proc_state.total_sent += 1;
        state.total_sent += 1;
        Ok(())
    })
}

/// Deliver pending signals for a process. Returns count delivered.
pub fn deliver(pid: u32) -> KernelResult<u32> {
    with_state(|state| {
        let ps = state
            .processes
            .iter_mut()
            .find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let count = ps.pending.len() as u32;
        for s in &mut ps.pending {
            s.delivered = true;
        }
        ps.total_delivered += count as u64;
        ps.pending.clear();
        state.total_delivered += count as u64;
        Ok(count)
    })
}

/// Block a signal for a process.
pub fn block(pid: u32, signal: Signal) -> KernelResult<()> {
    with_state(|state| {
        let ps = state
            .processes
            .iter_mut()
            .find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let bit = 1u64 << (signal.number().min(63));
        ps.blocked_mask |= bit;
        Ok(())
    })
}

/// Unblock a signal.
pub fn unblock(pid: u32, signal: Signal) -> KernelResult<()> {
    with_state(|state| {
        let ps = state
            .processes
            .iter_mut()
            .find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let bit = 1u64 << (signal.number().min(63));
        ps.blocked_mask &= !bit;
        Ok(())
    })
}

/// Get pending signals for a process.
pub fn pending(pid: u32) -> Vec<QueuedSignal> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.processes
            .iter()
            .find(|p| p.pid == pid)
            .map_or(Vec::new(), |p| p.pending.clone())
    })
}

/// List process signal states.
pub fn list_processes() -> Vec<ProcessSignalState> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.processes.clone())
}

/// Statistics: (process_count, total_sent, total_delivered, total_dropped, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.processes.len(),
            s.total_sent,
            s.total_delivered,
            s.total_dropped,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
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
    //
    // `total_sent` counts signals *queued*, not `send` calls: the blocked
    // Breakpoint in test 4 returns `Ok(())` after bumping the target's
    // `total_blocked` and never reaches the increment.  So four calls to
    // `send` leave the counter at 3, which is what makes this assertion worth
    // stating exactly -- it asserted `>= 4` and failed the first time it ran.
    //
    // `total_delivered` is 8 because `init_defaults` seeds it at 7 (matching
    // the 5 + 2 it seeds on the two processes) and test 3 delivers one more.
    let (procs, sent, delivered, dropped, ops) = stats();
    assert_eq!(procs, 3);
    assert_eq!(sent, 3, "four sends, one of them blocked before queueing");
    assert_eq!(delivered, 8, "seeded 7, plus the one test 3 delivered");
    assert_eq!(dropped, 0, "nothing reached MAX_PENDING");
    // The send that did not count as sent counted as blocked. Asserted here
    // rather than left implicit: it is the whole reason `sent` is 3, and
    // without it a `send` that silently dropped blocked signals on the floor
    // would look identical from `stats()`.
    let p1 = list_processes()
        .into_iter()
        .find(|p| p.pid == 1)
        .expect("pid 1");
    assert_eq!(p1.total_blocked, 1, "the blocked Breakpoint was counted");
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("signalq::self_test() — all 8 tests passed");
}
