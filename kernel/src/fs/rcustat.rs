//! RCU Statistics — Read-Copy-Update subsystem monitoring.
//!
//! Tracks RCU grace periods, callbacks pending and completed,
//! per-CPU state, and stall detection. Essential for diagnosing
//! lock-free read-side performance.
//!
//! ## Architecture
//!
//! ```text
//! RCU statistics
//!   → rcustat::begin_gp() → start grace period
//!   → rcustat::end_gp() → complete grace period
//!   → rcustat::queue_callback() → register callback
//!   → rcustat::cpu_stats(cpu) → per-CPU RCU state
//!
//! Integration:
//!   → tracemon (trace monitor)
//!   → perfmon (performance monitor)
//!   → wqstat (workqueue stats)
//!   → sysdiag (diagnostics)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// RCU flavor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RcuFlavor {
    Preempt,
    Bh,
    Sched,
}

impl RcuFlavor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preempt => "rcu_preempt",
            Self::Bh => "rcu_bh",
            Self::Sched => "rcu_sched",
        }
    }
}

/// Per-CPU RCU state.
#[derive(Debug, Clone)]
pub struct CpuRcuState {
    pub cpu_id: u32,
    pub callbacks_pending: u64,
    pub callbacks_invoked: u64,
    pub quiescent_states: u64,
    pub in_critical_section: bool,
}

/// A grace period record.
#[derive(Debug, Clone)]
pub struct GracePeriod {
    pub id: u64,
    pub flavor: RcuFlavor,
    pub start_ns: u64,
    pub end_ns: u64,
    pub duration_ns: u64,
    pub callbacks_processed: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPU: usize = 64;
const MAX_GP_HISTORY: usize = 256;

struct State {
    cpu_states: Vec<CpuRcuState>,
    gp_history: Vec<GracePeriod>,
    current_gp_id: u64,
    current_gp_start: u64,
    total_gp: u64,
    total_callbacks: u64,
    total_stalls: u64,
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

/// Initialise an **empty** RCU-statistics table.
///
/// Seeds NO per-CPU rows, no grace-period history, and zero totals (current GP
/// id 0).  Real RCU accounting is wired through [`register_cpu`] (one
/// zero-counter row per online CPU, populated by the RCU core at bring-up),
/// [`begin_gp`]/[`end_gp`], [`queue_callback`]/[`process_callbacks`],
/// [`quiescent`], and [`report_stall`]; until those are called the tables are
/// genuinely empty, so the `/proc/rcustat` file and the `rcustat` kshell
/// command report zeros rather than fabricated numbers — the kernel's hard
/// "never invent data in procfs" rule.
///
/// NOTE: this previously seeded four fictional per-CPU rows (cpu0..3 with
/// callbacks_invoked 1000–2500 and quiescent_states 50_000–80_000) plus
/// invented aggregate state (current_gp_id 100, total_gp 100, total_callbacks
/// 6000), which `/proc/rcustat` then displayed as if they were real
/// grace-period/callback measurements.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The RCU core is expected to call [`register_cpu`] per online
/// CPU and the begin_gp/end_gp/queue_callback/etc. functions as RCU runs.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpu_states: Vec::new(),
        gp_history: Vec::new(),
        current_gp_id: 0,
        current_gp_start: 0,
        total_gp: 0,
        total_callbacks: 0,
        total_stalls: 0,
        ops: 0,
    });
}

/// Register a CPU for RCU tracking.
///
/// The RCU core calls this once per online CPU at bring-up so the per-CPU state
/// table reflects the real topology with zeroed counters.  Callback/quiescent
/// accounting ([`queue_callback`], [`process_callbacks`], [`quiescent`]) returns
/// `NotFound` for an unregistered CPU id.
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpu_states.iter().any(|c| c.cpu_id == cpu_id) { return Err(KernelError::AlreadyExists); }
        if state.cpu_states.len() >= MAX_CPU { return Err(KernelError::ResourceExhausted); }
        state.cpu_states.push(CpuRcuState {
            cpu_id, callbacks_pending: 0, callbacks_invoked: 0,
            quiescent_states: 0, in_critical_section: false,
        });
        Ok(())
    })
}

/// Begin a new grace period.
pub fn begin_gp(_flavor: RcuFlavor) -> KernelResult<u64> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        state.current_gp_id += 1;
        state.current_gp_start = now;
        state.total_gp += 1;
        Ok(state.current_gp_id)
    })
}

/// End current grace period.
pub fn end_gp(flavor: RcuFlavor, callbacks_processed: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let duration = now.saturating_sub(state.current_gp_start);
        if state.gp_history.len() >= MAX_GP_HISTORY { state.gp_history.remove(0); }
        state.gp_history.push(GracePeriod {
            id: state.current_gp_id, flavor, start_ns: state.current_gp_start,
            end_ns: now, duration_ns: duration, callbacks_processed,
        });
        state.total_callbacks += callbacks_processed;
        Ok(())
    })
}

/// Queue a callback on a CPU.
pub fn queue_callback(cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.callbacks_pending += 1;
        Ok(())
    })
}

/// Process callbacks on a CPU.
pub fn process_callbacks(cpu: u32, count: u64) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        let processed = count.min(cs.callbacks_pending);
        cs.callbacks_pending -= processed;
        cs.callbacks_invoked += processed;
        state.total_callbacks += processed;
        Ok(())
    })
}

/// Record a quiescent state.
pub fn quiescent(cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.quiescent_states += 1;
        Ok(())
    })
}

/// Report a stall.
pub fn report_stall(_cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        state.total_stalls += 1;
        Ok(())
    })
}

/// Get per-CPU state.
pub fn cpu_stats() -> Vec<CpuRcuState> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpu_states.clone())
}

/// Recent grace periods.
pub fn gp_history(n: usize) -> Vec<GracePeriod> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if n >= s.gp_history.len() { 0 } else { s.gp_history.len() - n };
        s.gp_history[start..].to_vec()
    })
}

/// Statistics: (cpu_count, current_gp, total_gp, total_callbacks, total_stalls, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpu_states.len(), s.current_gp_id, s.total_gp, s.total_callbacks, s.total_stalls, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("rcustat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/rcustat must never surface).
    // Resetting first clears any residue from a prior `rcustat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated CPUs, history, or grace periods.
    assert_eq!(cpu_stats().len(), 0);
    let (c0, gp0, tgp0, tcb0, st0, _o0) = stats();
    assert_eq!((c0, gp0, tgp0, tcb0, st0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register CPUs (zeroed); the grace-period id starts from 0.
    register_cpu(0).expect("cpu0");
    register_cpu(1).expect("cpu1");
    assert!(register_cpu(0).is_err());
    let gp_id = begin_gp(RcuFlavor::Preempt).expect("begin");
    assert_eq!(gp_id, 1);
    crate::serial_println!("  [2/8] begin gp: OK");

    // 3: End GP records exactly one history entry.
    end_gp(RcuFlavor::Preempt, 10).expect("end");
    let hist = gp_history(5);
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].callbacks_processed, 10);
    crate::serial_println!("  [3/8] end gp: OK");

    // 4: Queue callbacks (exact, from zero); unknown CPU fails.
    queue_callback(0).expect("queue");
    queue_callback(0).expect("queue2");
    let c = cpu_stats().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(c.callbacks_pending, 2);
    assert!(queue_callback(99).is_err());
    crate::serial_println!("  [4/8] queue callback: OK");

    // 5: Process callbacks moves pending → invoked.
    process_callbacks(0, 1).expect("process");
    let c = cpu_stats().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(c.callbacks_pending, 1);
    assert_eq!(c.callbacks_invoked, 1);
    crate::serial_println!("  [5/8] process callbacks: OK");

    // 6: Quiescent state increments from exactly zero.
    quiescent(0).expect("qs");
    let c = cpu_stats().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(c.quiescent_states, 1);
    crate::serial_println!("  [6/8] quiescent: OK");

    // 7: Stall counter increments exactly.
    report_stall(0).expect("stall");
    let (_, _, _, _, stalls, _) = stats();
    assert_eq!(stalls, 1);
    crate::serial_println!("  [7/8] stall: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    //    total_callbacks = 10 (end_gp) + 1 (process_callbacks) = 11.
    let (cpus, gp, total_gp, total_cb, _stalls, ops) = stats();
    assert_eq!(cpus, 2);
    assert_eq!(gp, 1);
    assert_eq!(total_gp, 1);
    assert_eq!(total_cb, 11);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/rcustat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the RCU core wires real
    // accounting.
    *STATE.lock() = None;

    crate::serial_println!("rcustat::self_test() — all 8 tests passed");
}
