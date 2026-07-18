//! Load Average — system load tracking.
//!
//! Tracks 1-minute, 5-minute, and 15-minute exponentially weighted
//! moving averages of system load, plus running/total process counts.
//!
//! ## Architecture
//!
//! ```text
//! Load average
//!   → loadavg::update(running, total) → update with current counts
//!   → loadavg::get() → (1min, 5min, 15min, running, total)
//!   → loadavg::history() → historical snapshots
//!
//! Integration:
//!   → perfmon (performance monitor)
//!   → sysinfo (system information)
//!   → taskmon (task monitor)
//!   → schedtune (scheduler tuning)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Load average snapshot.
#[derive(Debug, Clone)]
pub struct LoadSnapshot {
    pub timestamp_ns: u64,
    /// Load average × 1000 (fixed-point, e.g., 1500 = 1.500).
    pub avg_1m: u64,
    pub avg_5m: u64,
    pub avg_15m: u64,
    pub running: u32,
    pub total: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 500;

/// Exponential decay factors × 1000 for different intervals.
/// For 5-second sampling: exp(-5/60) ≈ 0.920, exp(-5/300) ≈ 0.983, exp(-5/900) ≈ 0.994
const DECAY_1M: u64 = 920;
const DECAY_5M: u64 = 983;
const DECAY_15M: u64 = 994;

struct State {
    avg_1m: u64,   // × 1000
    avg_5m: u64,   // × 1000
    avg_15m: u64,  // × 1000
    running: u32,
    total: u32,
    history: Vec<LoadSnapshot>,
    total_updates: u64,
    last_update_ns: u64,
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
        avg_1m: 0,
        avg_5m: 0,
        avg_15m: 0,
        running: 0,
        total: 0,
        history: Vec::new(),
        total_updates: 0,
        last_update_ns: 0,
        ops: 0,
    });
}

/// Update load averages with current running/total process counts.
/// Uses exponential moving average with integer math.
pub fn update(running: u32, total: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let load = running as u64 * 1000; // Fixed-point × 1000
        // EMA: new_avg = decay * old_avg + (1 - decay) * current
        state.avg_1m = (DECAY_1M * state.avg_1m + (1000 - DECAY_1M) * load) / 1000;
        state.avg_5m = (DECAY_5M * state.avg_5m + (1000 - DECAY_5M) * load) / 1000;
        state.avg_15m = (DECAY_15M * state.avg_15m + (1000 - DECAY_15M) * load) / 1000;
        state.running = running;
        state.total = total;
        state.total_updates += 1;
        state.last_update_ns = now;
        // Record snapshot.
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(LoadSnapshot {
            timestamp_ns: now,
            avg_1m: state.avg_1m, avg_5m: state.avg_5m, avg_15m: state.avg_15m,
            running, total,
        });
        Ok(())
    })
}

/// Get current load averages: (1m, 5m, 15m, running, total).
/// Averages are × 1000 (fixed-point).
pub fn get() -> (u64, u64, u64, u32, u32) {
    STATE.lock().as_ref().map_or((0, 0, 0, 0, 0), |s| {
        (s.avg_1m, s.avg_5m, s.avg_15m, s.running, s.total)
    })
}

/// Format a fixed-point × 1000 load average.
pub fn format_load(load_x1000: u64) -> String {
    format!("{}.{:02}", load_x1000 / 1000, (load_x1000 % 1000) / 10)
}

/// Get load average history.
pub fn history() -> Vec<LoadSnapshot> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.history.clone())
}

/// Statistics: (history_len, total_updates, ops).
pub fn stats() -> (usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.history.len(), s.total_updates, s.ops),
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("loadavg::self_test() — running tests...");
    init_defaults();

    // 1: Initial state.
    let (a1, a5, a15, r, t) = get();
    assert_eq!(a1, 0);
    assert_eq!(a5, 0);
    assert_eq!(a15, 0);
    assert_eq!(r, 0);
    assert_eq!(t, 0);
    crate::serial_println!("  [1/8] initial: OK");

    // 2: First update.
    update(3, 100).expect("update1");
    let (a1, _, _, r, t) = get();
    assert!(a1 > 0);
    assert_eq!(r, 3);
    assert_eq!(t, 100);
    crate::serial_println!("  [2/8] first update: OK");

    // 3: History recorded.
    assert_eq!(history().len(), 1);
    crate::serial_println!("  [3/8] history: OK");

    // 4: Multiple updates converge.
    for _ in 0..10 {
        update(5, 200).expect("update");
    }
    let (a1, _a5, a15, _, _) = get();
    // 1m average should be closer to 5000 (5.000) than the 15m.
    assert!(a1 > a15);
    crate::serial_println!("  [4/8] convergence: OK");

    // 5: Format.
    assert_eq!(format_load(1500), "1.50");
    assert_eq!(format_load(250), "0.25");
    assert_eq!(format_load(10_000), "10.00");
    crate::serial_println!("  [5/8] format: OK");

    // 6: History grows.
    assert_eq!(history().len(), 11);
    crate::serial_println!("  [6/8] history len: OK");

    // 7: Zero load decays.
    for _ in 0..20 {
        update(0, 50).expect("update_zero");
    }
    let (a1, _, _, r, _) = get();
    assert_eq!(r, 0);
    // Should be decaying toward 0.
    assert!(a1 < 5000);
    crate::serial_println!("  [7/8] decay: OK");

    // 8: Stats.
    let (hist_len, total_updates, ops) = stats();
    assert!(hist_len > 20);
    assert!(total_updates > 20);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("loadavg::self_test() — all 8 tests passed");
}
