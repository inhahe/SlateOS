//! Pidfd Statistics — process file descriptor monitoring.
//!
//! Tracks pidfd creation, polling, signal sending via pidfd,
//! and wait operations. Essential for understanding modern
//! process lifecycle management.
//!
//! ## Architecture
//!
//! ```text
//! Pidfd monitoring
//!   → pidfd::record_create(pid) → pidfd created
//!   → pidfd::record_poll(pid) → pidfd polled
//!   → pidfd::record_signal(pid) → signal sent via pidfd
//!   → pidfd::record_wait(pid) → pidfd_wait completed
//!
//! Integration:
//!   → fdtable (file descriptors)
//!   → procstat (process stats)
//!   → signalq (signal queue)
//!   → pidstat (PID stats)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-PID pidfd stats.
#[derive(Debug, Clone)]
pub struct PidfdStats {
    pub pid: u32,
    pub creates: u64,
    pub polls: u64,
    pub signals: u64,
    pub waits: u64,
    pub close_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TRACKED: usize = 512;

struct State {
    pids: Vec<PidfdStats>,
    total_creates: u64,
    total_polls: u64,
    total_signals: u64,
    total_waits: u64,
    total_closes: u64,
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

fn find_or_create(state: &mut State, pid: u32) -> KernelResult<&mut PidfdStats> {
    if !state.pids.iter().any(|p| p.pid == pid) {
        if state.pids.len() >= MAX_TRACKED { return Err(KernelError::ResourceExhausted); }
        state.pids.push(PidfdStats {
            pid, creates: 0, polls: 0, signals: 0, waits: 0, close_count: 0,
        });
    }
    state.pids.iter_mut().find(|p| p.pid == pid).ok_or(KernelError::InternalError)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        pids: alloc::vec![
            PidfdStats { pid: 1, creates: 100, polls: 50_000, signals: 200, waits: 10_000, close_count: 50 },
            PidfdStats { pid: 100, creates: 50, polls: 20_000, signals: 100, waits: 5_000, close_count: 30 },
        ],
        total_creates: 150,
        total_polls: 70_000,
        total_signals: 300,
        total_waits: 15_000,
        total_closes: 80,
        ops: 0,
    });
}

/// Record pidfd creation.
pub fn record_create(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = find_or_create(state, pid)?;
        p.creates += 1;
        state.total_creates += 1;
        Ok(())
    })
}

/// Record pidfd poll.
pub fn record_poll(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = find_or_create(state, pid)?;
        p.polls += 1;
        state.total_polls += 1;
        Ok(())
    })
}

/// Record signal sent via pidfd.
pub fn record_signal(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = find_or_create(state, pid)?;
        p.signals += 1;
        state.total_signals += 1;
        Ok(())
    })
}

/// Record pidfd wait.
pub fn record_wait(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = find_or_create(state, pid)?;
        p.waits += 1;
        state.total_waits += 1;
        Ok(())
    })
}

/// Record pidfd close.
pub fn record_close(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = state.pids.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        p.close_count += 1;
        state.total_closes += 1;
        Ok(())
    })
}

/// Per-PID stats.
pub fn per_pid() -> Vec<PidfdStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.pids.clone())
}

/// Statistics: (tracked_pids, creates, polls, signals, waits, closes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.pids.len(), s.total_creates, s.total_polls, s.total_signals, s.total_waits, s.total_closes, s.ops),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pidfd::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_pid().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create (auto-registers pid).
    record_create(500).expect("create");
    assert_eq!(per_pid().len(), 3);
    let p = per_pid().iter().find(|p| p.pid == 500).cloned().unwrap();
    assert_eq!(p.creates, 1);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Poll.
    record_poll(500).expect("poll");
    let p = per_pid().iter().find(|p| p.pid == 500).cloned().unwrap();
    assert_eq!(p.polls, 1);
    crate::serial_println!("  [3/8] poll: OK");

    // 4: Signal.
    record_signal(500).expect("signal");
    let p = per_pid().iter().find(|p| p.pid == 500).cloned().unwrap();
    assert_eq!(p.signals, 1);
    crate::serial_println!("  [4/8] signal: OK");

    // 5: Wait.
    record_wait(500).expect("wait");
    let p = per_pid().iter().find(|p| p.pid == 500).cloned().unwrap();
    assert_eq!(p.waits, 1);
    crate::serial_println!("  [5/8] wait: OK");

    // 6: Close.
    record_close(500).expect("close");
    let p = per_pid().iter().find(|p| p.pid == 500).cloned().unwrap();
    assert_eq!(p.close_count, 1);
    crate::serial_println!("  [6/8] close: OK");

    // 7: Close not-found.
    assert!(record_close(9999).is_err());
    crate::serial_println!("  [7/8] close not found: OK");

    // 8: Stats.
    let (pids, creates, polls, signals, waits, closes, ops) = stats();
    assert!(pids >= 3);
    assert!(creates > 150);
    assert!(polls > 70_000);
    assert!(signals > 300);
    assert!(waits > 15_000);
    assert!(closes > 80);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("pidfd::self_test() — all 8 tests passed");
}
