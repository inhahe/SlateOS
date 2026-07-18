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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the timer-queue state.
///
/// Starts with no timers and all created/fired/cancelled/overrun totals at
/// zero. The `/proc/timerq` generator and the `timerq` kshell command
/// surface this table (and `list_pending`) as if it reflects the real set
/// of scheduled kernel timers, so seeding it with phantom timers would be
/// fabricated procfs data — it would claim timers are pending in the queue
/// that no subsystem actually scheduled. Timers are scheduled through
/// [`add`] and the counters advance only through real [`fire`] /
/// [`fire_expired`] / [`cancel`] calls.
///
/// (Previously this seeded three fictional pending timers — "tick"
/// (periodic 10ms), "watchdog" (periodic 1s), and "rcu_callback"
/// (deferrable 50ms) — and a total_created of 3.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        timers: Vec::new(),
        next_id: 1,
        total_created: 0,
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live timer queue afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom timers, all totals zero.
    assert_eq!(list_all().len(), 0);
    assert_eq!(list_pending().len(), 0);
    let (total0, pending0, created0, fired0, cancelled0, overruns0, _) = stats();
    assert_eq!((total0, pending0, created0, fired0, cancelled0, overruns0), (0, 0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Add + fire a one-shot — ids monotonic from 1; one-shot ends Fired.
    let id1 = add("oneshot", TimerType::OneShot, 1_000, 0, 0).expect("add");
    assert_eq!(id1, 1);
    fire(id1).expect("fire");
    let t = list_all().into_iter().find(|t| t.id == id1).expect("find");
    assert_eq!(t.state, TimerState::Fired);
    assert_eq!(t.fire_count, 1);
    crate::serial_println!("  [2/8] add + fire: OK");

    // 3: Cancel — state becomes Cancelled; double-cancel errors.
    let id2 = add("cancel_me", TimerType::OneShot, 999_999, 0, 0).expect("add2");
    cancel(id2).expect("cancel");
    assert_eq!(list_all().into_iter().find(|t| t.id == id2).expect("f2").state, TimerState::Cancelled);
    assert!(cancel(id2).is_err()); // already cancelled
    crate::serial_println!("  [3/8] cancel: OK");

    // 4: Periodic fire — stays Pending and the deadline advances by interval.
    //    Use a far-future deadline so fire_expired (test 6) never fires it.
    let pid = add("periodic", TimerType::Periodic, 1_000_000_000_000, 1_000, 0).expect("add3");
    fire(pid).expect("fire_p");
    let t = list_all().into_iter().find(|t| t.id == pid).expect("f3");
    assert_eq!(t.state, TimerState::Pending);
    assert_eq!(t.fire_count, 1);
    assert_eq!(t.deadline_ns, 1_000_000_000_000 + 1_000);
    crate::serial_println!("  [4/8] periodic: OK");

    // 5: Firing a non-pending timer errors; unknown ids are NotFound.
    assert!(fire(id1).is_err()); // id1 is Fired, not Pending
    assert!(fire(9999).is_err());
    assert!(cancel(9999).is_err());
    crate::serial_println!("  [5/8] invalid + not found: OK");

    // 6: fire_expired fires only the past-deadline pending one-shot.
    let id4 = add("expired", TimerType::OneShot, 0, 0, 0).expect("add4");
    let fired = fire_expired().expect("expired");
    assert_eq!(fired, 1); // only id4 (periodic pid is far-future)
    assert_eq!(list_all().into_iter().find(|t| t.id == id4).expect("f4").state, TimerState::Fired);
    crate::serial_println!("  [6/8] fire expired: OK");

    // 7: Cleanup retains only Pending/Active timers (removes id1+id2+id4).
    let cleaned = cleanup().expect("cleanup");
    assert_eq!(cleaned, 3);
    assert_eq!(list_all().len(), 1); // only the periodic remains
    crate::serial_println!("  [7/8] cleanup: OK");

    // 8: Final stats reflect only the real activity above. created: 4 adds;
    //    fired: id1 + periodic + id4 = 3; cancelled: 1; overruns: 0.
    let (total, pending, created, fired_count, cancelled, overruns, ops) = stats();
    assert_eq!(total, 1);
    assert_eq!(pending, 1);
    assert_eq!(created, 4);
    assert_eq!(fired_count, 3);
    assert_eq!(cancelled, 1);
    assert_eq!(overruns, 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("timerq::self_test() — all 8 tests passed");
}
