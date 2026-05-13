//! Timer Queue — kernel timer management and scheduling.
//!
//! Manages one-shot and periodic timers. Tracks timer creation,
//! firing, cancellation, and overruns. Provides hierarchical
//! timer wheel statistics.
//!
//! ## Architecture
//!
//! ```text
//! Timer queue
//!   → timerq::add(callback, deadline) → schedule timer
//!   → timerq::cancel(id) → cancel timer
//!   → timerq::list_pending() → pending timers
//!   → timerq::fire_expired() → fire ready timers
//!
//! Integration:
//!   → schedtune (scheduler tuning)
//!   → tasksched (task scheduler)
//!   → perfmon (performance monitor)
//!   → wqstat (workqueue stats)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Timer type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerType {
    OneShot,
    Periodic,
    Deadline,    // Fires at exact time.
    Deferrable,  // Can be delayed to reduce wakeups.
}

impl TimerType {
    pub fn label(self) -> &'static str {
        match self {
            Self::OneShot => "one-shot",
            Self::Periodic => "periodic",
            Self::Deadline => "deadline",
            Self::Deferrable => "deferrable",
        }
    }
}

/// Timer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Pending,
    Active,     // Currently executing callback.
    Fired,
    Cancelled,
    Expired,    // Missed deadline.
}

impl TimerState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Fired => "fired",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }
}

/// A timer entry.
#[derive(Debug, Clone)]
pub struct Timer {
    pub id: u32,
    pub name: String,
    pub timer_type: TimerType,
    pub state: TimerState,
    pub deadline_ns: u64,
    pub interval_ns: u64,    // For periodic.
    pub fire_count: u64,
    pub overruns: u64,       // Missed fires for periodic.
    pub cpu: u32,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TIMERS: usize = 4096;

struct State {
    timers: Vec<Timer>,
    next_id: u32,
    total_created: u64,
    total_fired: u64,
    total_cancelled: u64,
    total_overruns: u64,
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
        timers: alloc::vec![
            Timer { id: 1, name: String::from("tick"), timer_type: TimerType::Periodic, state: TimerState::Pending, deadline_ns: now + 10_000_000, interval_ns: 10_000_000, fire_count: 0, overruns: 0, cpu: 0, created_ns: now },
            Timer { id: 2, name: String::from("watchdog"), timer_type: TimerType::Periodic, state: TimerState::Pending, deadline_ns: now + 1_000_000_000, interval_ns: 1_000_000_000, fire_count: 0, overruns: 0, cpu: 0, created_ns: now },
            Timer { id: 3, name: String::from("rcu_callback"), timer_type: TimerType::Deferrable, state: TimerState::Pending, deadline_ns: now + 50_000_000, interval_ns: 0, fire_count: 0, overruns: 0, cpu: 0, created_ns: now },
        ],
        next_id: 4,
        total_created: 3,
        total_fired: 0,
        total_cancelled: 0,
        total_overruns: 0,
        ops: 0,
    });
}

/// Add a timer.
pub fn add(name: &str, timer_type: TimerType, deadline_ns: u64, interval_ns: u64, cpu: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.timers.len() >= MAX_TIMERS { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.timers.push(Timer {
            id, name: String::from(name), timer_type, state: TimerState::Pending,
            deadline_ns, interval_ns, fire_count: 0, overruns: 0, cpu,
            created_ns: now,
        });
        state.total_created += 1;
        Ok(id)
    })
}

/// Cancel a timer.
pub fn cancel(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let t = state.timers.iter_mut().find(|t| t.id == id).ok_or(KernelError::NotFound)?;
        if t.state == TimerState::Cancelled { return Err(KernelError::AlreadyExists); }
        t.state = TimerState::Cancelled;
        state.total_cancelled += 1;
        Ok(())
    })
}

/// Fire a timer (simulate).
pub fn fire(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let t = state.timers.iter_mut().find(|t| t.id == id).ok_or(KernelError::NotFound)?;
        if t.state != TimerState::Pending { return Err(KernelError::InvalidArgument); }
        t.fire_count += 1;
        state.total_fired += 1;
        match t.timer_type {
            TimerType::Periodic => {
                // Reschedule.
                t.deadline_ns += t.interval_ns;
            }
            _ => {
                t.state = TimerState::Fired;
            }
        }
        Ok(())
    })
}

/// Check and fire all expired timers. Returns count fired.
pub fn fire_expired() -> KernelResult<u32> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let mut fired = 0u32;
        for t in &mut state.timers {
            if t.state != TimerState::Pending { continue; }
            if now >= t.deadline_ns {
                t.fire_count += 1;
                state.total_fired += 1;
                fired += 1;
                match t.timer_type {
                    TimerType::Periodic => {
                        // Check for overruns.
                        if t.interval_ns > 0 {
                            let missed = (now - t.deadline_ns) / t.interval_ns;
                            t.overruns += missed;
                            state.total_overruns += missed;
                        }
                        t.deadline_ns = now + t.interval_ns;
                    }
                    _ => {
                        t.state = TimerState::Fired;
                    }
                }
            }
        }
        Ok(fired)
    })
}

/// List pending timers.
pub fn list_pending() -> Vec<Timer> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.timers.iter().filter(|t| t.state == TimerState::Pending).cloned().collect()
    })
}

/// List all timers.
pub fn list_all() -> Vec<Timer> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.timers.clone())
}

/// Cleanup fired/cancelled timers.
pub fn cleanup() -> KernelResult<u32> {
    with_state(|state| {
        let before = state.timers.len();
        state.timers.retain(|t| t.state == TimerState::Pending || t.state == TimerState::Active);
        Ok((before - state.timers.len()) as u32)
    })
}

/// Statistics: (total_timers, pending, total_created, total_fired, total_cancelled, total_overruns, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let pending = s.timers.iter().filter(|t| t.state == TimerState::Pending).count();
            (s.timers.len(), pending, s.total_created, s.total_fired, s.total_cancelled, s.total_overruns, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("timerq::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_all().len(), 3);
    assert_eq!(list_pending().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add timer.
    let now = crate::hpet::elapsed_ns();
    let id = add("test_timer", TimerType::OneShot, now + 1000, 0, 0).expect("add");
    assert!(id >= 4);
    crate::serial_println!("  [2/8] add: OK");

    // 3: Fire.
    fire(id).expect("fire");
    let t = list_all().iter().find(|t| t.id == id).expect("find").clone();
    assert_eq!(t.state, TimerState::Fired);
    assert_eq!(t.fire_count, 1);
    crate::serial_println!("  [3/8] fire: OK");

    // 4: Cancel.
    let id2 = add("cancel_me", TimerType::OneShot, now + 999999, 0, 0).expect("add2");
    cancel(id2).expect("cancel");
    let t = list_all().iter().find(|t| t.id == id2).expect("find2").clone();
    assert_eq!(t.state, TimerState::Cancelled);
    crate::serial_println!("  [4/8] cancel: OK");

    // 5: Periodic fire.
    let pid = add("periodic_test", TimerType::Periodic, now, 1000, 0).expect("add3");
    fire(pid).expect("fire_p");
    let t = list_all().iter().find(|t| t.id == pid).expect("find3").clone();
    assert_eq!(t.state, TimerState::Pending); // Still pending (periodic).
    assert_eq!(t.fire_count, 1);
    crate::serial_println!("  [5/8] periodic: OK");

    // 6: Cleanup.
    let cleaned = cleanup().expect("cleanup");
    assert!(cleaned >= 2); // Fired + cancelled.
    crate::serial_println!("  [6/8] cleanup: OK");

    // 7: Fire expired.
    let fired = fire_expired().expect("expired");
    let _ = fired;
    crate::serial_println!("  [7/8] fire expired: OK");

    // 8: Stats.
    let (total, pending, created, fired_count, cancelled, overruns, ops) = stats();
    assert!(total >= 3);
    let _ = pending;
    assert!(created >= 6);
    assert!(fired_count >= 2);
    assert!(cancelled >= 1);
    let _ = overruns;
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("timerq::self_test() — all 8 tests passed");
}
