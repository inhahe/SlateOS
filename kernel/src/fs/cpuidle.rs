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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let mut cpu_states = Vec::new();
    for i in 0..4u32 {
        let mut entries = [0u64; 7];
        entries[0] = 0; // C0 is active, not counted.
        entries[1] = 1_000_000 + i as u64 * 200_000;
        entries[2] = 500_000 + i as u64 * 100_000;
        entries[3] = 100_000 + i as u64 * 20_000;
        entries[4] = 10_000 + i as u64 * 2_000;
        let mut residency_ns = [0u64; 7];
        residency_ns[1] = 5_000_000_000 + i as u64 * 1_000_000_000;
        residency_ns[2] = 10_000_000_000 + i as u64 * 2_000_000_000;
        residency_ns[3] = 20_000_000_000 + i as u64 * 4_000_000_000;
        residency_ns[4] = 5_000_000_000 + i as u64 * 1_000_000_000;
        let total_idle: u64 = residency_ns.iter().sum();
        cpu_states.push(CpuIdleState {
            cpu_id: i, current_state: CState::C0,
            entries, residency_ns, total_idle_ns: total_idle,
            total_active_ns: 100_000_000_000 - total_idle,
            last_entry_ns: 0,
        });
    }
    let total_idle: u64 = cpu_states.iter().map(|c| c.total_idle_ns).sum();
    *guard = Some(State {
        cpu_states,
        total_transitions: 6_480_000,
        total_idle_ns: total_idle,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Enter state.
    enter_state(0, CState::C1).expect("enter");
    let cs = per_cpu();
    assert_eq!(cs[0].current_state, CState::C1);
    crate::serial_println!("  [2/8] enter: OK");

    // 3: Exit state.
    let dur = exit_state(0).expect("exit");
    let cs = per_cpu();
    assert_eq!(cs[0].current_state, CState::C0);
    let _ = dur; // Duration depends on timing.
    crate::serial_println!("  [3/8] exit: OK");

    // 4: Deep state.
    enter_state(1, CState::C6).expect("enter_deep");
    let cs = per_cpu();
    assert_eq!(cs[1].current_state, CState::C6);
    exit_state(1).expect("exit_deep");
    crate::serial_println!("  [4/8] deep state: OK");

    // 5: Idle percentage.
    let pct = idle_pct(0);
    assert!(pct > 0 && pct < 100);
    crate::serial_println!("  [5/8] idle pct: OK ({}%)", pct);

    // 6: Entry counts.
    let cs = per_cpu();
    assert!(cs[0].entries[1] > 1_000_000); // C1 entries from defaults + test.
    crate::serial_println!("  [6/8] entry counts: OK");

    // 7: Not found.
    assert!(enter_state(99, CState::C1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (cpus, transitions, idle_ns, ops) = stats();
    assert_eq!(cpus, 4);
    assert!(transitions > 6_480_000);
    assert!(idle_ns > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cpuidle::self_test() — all 8 tests passed");
}
