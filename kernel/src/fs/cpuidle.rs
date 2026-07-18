//! CPU Idle — C-state and idle state monitoring.
//!
//! Tracks CPU idle states (C-states), residency times,
//! transitions, and power savings. Essential for diagnosing
//! power management and latency-sensitive workloads.
//!
//! ## Architecture
//!
//! ```text
//! CPU idle monitoring
//!   → cpuidle::enter_state(cpu, state) → record idle entry
//!   → cpuidle::exit_state(cpu) → record idle exit
//!   → cpuidle::per_cpu() → per-CPU idle stats
//!   → cpuidle::state_info() → C-state descriptions
//!
//! Integration:
//!   → cpufreq (CPU frequency)
//!   → thermal (thermal monitoring)
//!   → perfmon (performance monitor)
//!   → powerwake (power wake events)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// CPU idle state (C-state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CState {
    C0,   // Active.
    C1,   // Halt.
    C1E,  // Enhanced halt.
    C3,   // Sleep.
    C6,   // Deep sleep.
    C7,   // Package sleep.
    C10,  // Deepest sleep.
}

impl CState {
    pub fn label(self) -> &'static str {
        match self {
            Self::C0 => "C0",
            Self::C1 => "C1",
            Self::C1E => "C1E",
            Self::C3 => "C3",
            Self::C6 => "C6",
            Self::C7 => "C7",
            Self::C10 => "C10",
        }
    }

    pub fn exit_latency_us(self) -> u32 {
        match self {
            Self::C0 => 0,
            Self::C1 => 1,
            Self::C1E => 10,
            Self::C3 => 100,
            Self::C6 => 500,
            Self::C7 => 1000,
            Self::C10 => 5000,
        }
    }

    pub fn depth(self) -> u32 {
        match self {
            Self::C0 => 0,
            Self::C1 => 1,
            Self::C1E => 2,
            Self::C3 => 3,
            Self::C6 => 4,
            Self::C7 => 5,
            Self::C10 => 6,
        }
    }
}

/// Per-CPU idle state.
#[derive(Debug, Clone)]
pub struct CpuIdleState {
    pub cpu_id: u32,
    pub current_state: CState,
    pub entries: [u64; 7],       // Per C-state entry counts.
    pub residency_ns: [u64; 7],  // Per C-state total time.
    pub total_idle_ns: u64,
    pub total_active_ns: u64,
    pub last_entry_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPU: usize = 64;

struct State {
    cpu_states: Vec<CpuIdleState>,
    total_transitions: u64,
    total_idle_ns: u64,
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

/// Initialise the CPU-idle statistics state.
///
/// Starts with no CPUs and zero transition/idle totals. CPUs are discovered
/// hardware brought online at SMP startup; each online CPU is added through
/// [`register_cpu`], and its C-state entry/residency counters advance only
/// through real [`enter_state`] / [`exit_state`] calls. The `/proc/cpuidle`
/// generator and the `cpuidle` kshell command surface the per-CPU table (and
/// [`per_cpu`] / [`idle_pct`]) as if it reflects real C-state residency, so
/// seeding it with phantom CPUs would be fabricated procfs data — it would
/// claim idle-state residency on cores that nothing actually measured.
///
/// (Previously this seeded four fictional CPUs with invented C-state entry
/// counts — e.g. CPU0 with 1,000,000 C1 entries, 500,000 C1E, 100,000 C3,
/// 10,000 C6 — and multi-second residency times scaled per core, plus a
/// global total of 6,480,000 transitions.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpu_states: Vec::new(),
        total_transitions: 0,
        total_idle_ns: 0,
        ops: 0,
    });
}

/// Register a CPU as it is brought online at SMP startup.
///
/// The CPU begins in the active state (C0) with all per-C-state entry and
/// residency counters zeroed; they advance only through real
/// [`enter_state`] / [`exit_state`] calls. Returns [`KernelError::AlreadyExists`]
/// if the CPU id is already registered and [`KernelError::ResourceExhausted`]
/// if the maximum CPU count is reached.
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpu_states.len() >= MAX_CPU { return Err(KernelError::ResourceExhausted); }
        if state.cpu_states.iter().any(|c| c.cpu_id == cpu_id) {
            return Err(KernelError::AlreadyExists);
        }
        state.cpu_states.push(CpuIdleState {
            cpu_id, current_state: CState::C0,
            entries: [0u64; 7], residency_ns: [0u64; 7],
            total_idle_ns: 0, total_active_ns: 0, last_entry_ns: 0,
        });
        Ok(())
    })
}

/// Enter an idle state.
pub fn enter_state(cpu: u32, state_idx: CState) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.current_state = state_idx;
        cs.last_entry_ns = now;
        let depth = state_idx.depth() as usize;
        if depth < 7 { cs.entries[depth] += 1; }
        state.total_transitions += 1;
        Ok(())
    })
}

/// Exit idle state (return to C0).
pub fn exit_state(cpu: u32) -> KernelResult<u64> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        let duration = now.saturating_sub(cs.last_entry_ns);
        let depth = cs.current_state.depth() as usize;
        if depth < 7 { cs.residency_ns[depth] += duration; }
        cs.total_idle_ns += duration;
        cs.current_state = CState::C0;
        state.total_idle_ns += duration;
        state.total_transitions += 1;
        Ok(duration)
    })
}

/// Get per-CPU idle state.
pub fn per_cpu() -> Vec<CpuIdleState> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpu_states.clone())
}

/// Idle percentage for a CPU (0-100).
pub fn idle_pct(cpu: u32) -> u64 {
    STATE.lock().as_ref().map_or(0, |s| {
        s.cpu_states.iter().find(|c| c.cpu_id == cpu).map_or(0, |cs| {
            let total = cs.total_idle_ns + cs.total_active_ns;
            if total == 0 { 0 } else { cs.total_idle_ns * 100 / total }
        })
    })
}

/// Statistics: (cpu_count, total_transitions, total_idle_ns, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpu_states.len(), s.total_transitions, s.total_idle_ns, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cpuidle::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live per-CPU table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom CPUs, zero totals.
    assert_eq!(per_cpu().len(), 0);
    let (c0, t0, i0, _) = stats();
    assert_eq!((c0, t0, i0), (0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register CPU — appears in C0 with all counters zeroed; duplicate is
    //    AlreadyExists.
    register_cpu(0).expect("register0");
    let cs = per_cpu();
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].current_state, CState::C0);
    assert_eq!((cs[0].entries, cs[0].residency_ns), ([0u64; 7], [0u64; 7]));
    assert_eq!((cs[0].total_idle_ns, cs[0].total_active_ns), (0, 0));
    assert!(register_cpu(0).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Enter state — current_state updates; the C1 (depth 1) entry counter
    //    increments; a transition is recorded.
    enter_state(0, CState::C1).expect("enter");
    let cs = per_cpu();
    assert_eq!(cs[0].current_state, CState::C1);
    assert_eq!(cs[0].entries[CState::C1.depth() as usize], 1);
    crate::serial_println!("  [3/8] enter: OK");

    // 4: Exit state — returns to C0 and records a second transition. Residency
    //    duration depends on the HPET clock so it is not asserted exactly.
    let _dur = exit_state(0).expect("exit");
    let cs = per_cpu();
    assert_eq!(cs[0].current_state, CState::C0);
    assert_eq!(cs[0].entries[CState::C1.depth() as usize], 1); // unchanged by exit
    crate::serial_println!("  [4/8] exit: OK");

    // 5: Deep state on a second CPU — C6 (depth 4) entry counter increments.
    register_cpu(1).expect("register1");
    enter_state(1, CState::C6).expect("enter_deep");
    let cs = per_cpu();
    let c1 = cs.iter().find(|c| c.cpu_id == 1).expect("cpu1");
    assert_eq!(c1.current_state, CState::C6);
    assert_eq!(c1.entries[CState::C6.depth() as usize], 1);
    exit_state(1).expect("exit_deep");
    crate::serial_println!("  [5/8] deep state: OK");

    // 6: Idle percentage — unregistered CPU reports 0; a registered CPU reports
    //    a value within range (0..=100).
    assert_eq!(idle_pct(99), 0);
    assert!(idle_pct(0) <= 100);
    crate::serial_println!("  [6/8] idle pct: OK");

    // 7: Not found — enter/exit on an unregistered CPU both error.
    assert!(enter_state(99, CState::C1).is_err());
    assert!(exit_state(99).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above: 2 CPUs and 4
    //    transitions (2 enters + 2 exits).
    let (cpus, transitions, _idle_ns, ops) = stats();
    assert_eq!((cpus, transitions), (2, 4));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("cpuidle::self_test() — all 8 tests passed");
}
