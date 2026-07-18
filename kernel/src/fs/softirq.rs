//! Soft IRQ Statistics — deferred interrupt processing monitoring.
//!
//! Tracks softirq execution, per-type statistics, tasklet runs,
//! and ksoftirqd activity. Softirqs handle deferred work from
//! hardware interrupts (networking, block I/O, timers, RCU).
//!
//! ## Architecture
//!
//! ```text
//! Soft IRQ statistics
//!   → softirq::raise(type) → schedule softirq
//!   → softirq::run(cpu, type) → execute softirq handler
//!   → softirq::tasklet_run(cpu) → execute tasklet
//!   → softirq::per_cpu() → per-CPU softirq state
//!
//! Integration:
//!   → irqstat (hardware IRQ stats)
//!   → rcustat (RCU statistics)
//!   → wqstat (workqueue stats)
//!   → perfmon (performance monitor)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Softirq type (matching Linux softirq vectors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftirqType {
    HiPri,
    Timer,
    NetTx,
    NetRx,
    Block,
    IrqPoll,
    Tasklet,
    Sched,
    Hrtimer,
    Rcu,
}

impl SoftirqType {
    pub fn label(self) -> &'static str {
        match self {
            Self::HiPri => "HI",
            Self::Timer => "TIMER",
            Self::NetTx => "NET_TX",
            Self::NetRx => "NET_RX",
            Self::Block => "BLOCK",
            Self::IrqPoll => "IRQ_POLL",
            Self::Tasklet => "TASKLET",
            Self::Sched => "SCHED",
            Self::Hrtimer => "HRTIMER",
            Self::Rcu => "RCU",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::HiPri => 0,
            Self::Timer => 1,
            Self::NetTx => 2,
            Self::NetRx => 3,
            Self::Block => 4,
            Self::IrqPoll => 5,
            Self::Tasklet => 6,
            Self::Sched => 7,
            Self::Hrtimer => 8,
            Self::Rcu => 9,
        }
    }

    pub fn all() -> &'static [SoftirqType] {
        &[
            Self::HiPri, Self::Timer, Self::NetTx, Self::NetRx,
            Self::Block, Self::IrqPoll, Self::Tasklet, Self::Sched,
            Self::Hrtimer, Self::Rcu,
        ]
    }
}

/// Per-type softirq counters.
#[derive(Debug, Clone)]
pub struct SoftirqTypeStats {
    pub softirq_type: SoftirqType,
    pub raised: u64,
    pub executed: u64,
    pub total_ns: u64,
}

/// Per-CPU softirq state.
#[derive(Debug, Clone)]
pub struct CpuSoftirqState {
    pub cpu_id: u32,
    pub total_softirqs: u64,
    pub total_tasklets: u64,
    pub ksoftirqd_wakeups: u64,
    pub type_counts: [u64; 10],
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPU: usize = 64;

struct State {
    cpu_states: Vec<CpuSoftirqState>,
    type_stats: Vec<SoftirqTypeStats>,
    total_raised: u64,
    total_executed: u64,
    total_tasklets: u64,
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

/// Initialise the softirq statistics state.
///
/// Pre-populates the ten softirq-vector rows (HI/TIMER/NET_TX/NET_RX/
/// BLOCK/IRQ_POLL/TASKLET/SCHED/HRTIMER/RCU — a fixed kernel taxonomy)
/// with ZEROED counters, and starts with no per-CPU state and all totals
/// at zero. The `/proc/softirq` generator and the `softirq` kshell command
/// surface this table as if it reflects real deferred-interrupt activity,
/// so seeding it with invented per-CPU counts and execution totals would
/// be fabricated procfs data. Per-CPU state is created as each CPU comes
/// online through [`register_cpu`], and the counters advance only through
/// real [`raise`] / [`run`] / [`tasklet_run`] / [`ksoftirqd_wakeup`]
/// calls.
///
/// (Previously this seeded four fictional CPUs with invented per-type
/// counts — Timer 500k+, NetRx 200k+, Block 100k+, RCU 300k+ per CPU —
/// plus ten type rows with invented bases (Timer 2.6M executed, NetRx 1M,
/// Block 550k, RCU 1.5M) and totals (5.71M raised, 5.7M executed, 5200
/// tasklets).)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let mut type_stats = Vec::new();
    for st in SoftirqType::all() {
        type_stats.push(SoftirqTypeStats {
            softirq_type: *st, raised: 0, executed: 0, total_ns: 0,
        });
    }
    *guard = Some(State {
        cpu_states: Vec::new(),
        type_stats,
        total_raised: 0,
        total_executed: 0,
        total_tasklets: 0,
        ops: 0,
    });
}

/// Register a CPU's softirq state as it comes online.
///
/// Creates a zeroed per-CPU softirq state. Returns `AlreadyExists` if the
/// CPU is already registered and `ResourceExhausted` once `MAX_CPU` CPUs
/// are registered. The softirq subsystem calls this for each online CPU
/// so that [`run`], [`tasklet_run`], and [`ksoftirqd_wakeup`] can account
/// real activity against it.
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpu_states.len() >= MAX_CPU { return Err(KernelError::ResourceExhausted); }
        if state.cpu_states.iter().any(|c| c.cpu_id == cpu_id) {
            return Err(KernelError::AlreadyExists);
        }
        state.cpu_states.push(CpuSoftirqState {
            cpu_id, total_softirqs: 0, total_tasklets: 0,
            ksoftirqd_wakeups: 0, type_counts: [0; 10],
        });
        Ok(())
    })
}

/// Raise a softirq.
pub fn raise(softirq_type: SoftirqType) -> KernelResult<()> {
    with_state(|state| {
        if let Some(ts) = state.type_stats.iter_mut().find(|t| t.softirq_type == softirq_type) {
            ts.raised += 1;
        }
        state.total_raised += 1;
        Ok(())
    })
}

/// Execute a softirq on a CPU.
pub fn run(cpu: u32, softirq_type: SoftirqType, duration_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        let idx = softirq_type.index();
        cs.type_counts[idx] += 1;
        cs.total_softirqs += 1;
        if let Some(ts) = state.type_stats.iter_mut().find(|t| t.softirq_type == softirq_type) {
            ts.executed += 1;
            ts.total_ns += duration_ns;
        }
        state.total_executed += 1;
        Ok(())
    })
}

/// Execute a tasklet on a CPU.
pub fn tasklet_run(cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.total_tasklets += 1;
        cs.type_counts[SoftirqType::Tasklet.index()] += 1;
        cs.total_softirqs += 1;
        state.total_tasklets += 1;
        state.total_executed += 1;
        Ok(())
    })
}

/// Record ksoftirqd wakeup.
pub fn ksoftirqd_wakeup(cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.ksoftirqd_wakeups += 1;
        Ok(())
    })
}

/// Get per-CPU state.
pub fn per_cpu() -> Vec<CpuSoftirqState> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpu_states.clone())
}

/// Get per-type statistics.
pub fn type_stats() -> Vec<SoftirqTypeStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.type_stats.clone())
}

/// Statistics: (cpu_count, type_count, total_raised, total_executed, total_tasklets, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpu_states.len(), s.type_stats.len(), s.total_raised, s.total_executed, s.total_tasklets, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("softirq::self_test() — running tests...");
    // Start from a clean state so the assertions below are exact and no
    // fixtures leak into the live tables afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Defaults — all 10 zeroed type rows, no CPUs, all totals zero.
    assert_eq!(per_cpu().len(), 0);
    assert_eq!(type_stats().len(), 10);
    assert!(type_stats().iter().all(|t| t.raised == 0 && t.executed == 0 && t.total_ns == 0));
    let (cpus0, types0, raised0, executed0, tasklets0, _) = stats();
    assert_eq!((cpus0, types0, raised0, executed0, tasklets0), (0, 10, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register CPUs — duplicate registration errors.
    register_cpu(0).expect("reg0");
    register_cpu(1).expect("reg1");
    assert_eq!(per_cpu().len(), 2);
    assert!(register_cpu(0).is_err());
    crate::serial_println!("  [2/8] register cpu: OK");

    // 3: Raise increments the type's raised counter and the total exactly.
    raise(SoftirqType::Timer).expect("raise");
    let ts = type_stats().into_iter().find(|t| t.softirq_type == SoftirqType::Timer).expect("ts");
    assert_eq!(ts.raised, 1);
    assert_eq!(ts.executed, 0); // raised but not yet run
    crate::serial_println!("  [3/8] raise: OK");

    // 4: Run executes on a CPU and accrues duration into the type row.
    run(0, SoftirqType::NetRx, 1000).expect("run");
    let cpu0 = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("cpu0");
    assert_eq!(cpu0.total_softirqs, 1);
    assert_eq!(cpu0.type_counts[SoftirqType::NetRx.index()], 1);
    let ts = type_stats().into_iter().find(|t| t.softirq_type == SoftirqType::NetRx).expect("ts");
    assert_eq!(ts.executed, 1);
    assert_eq!(ts.total_ns, 1000);
    crate::serial_println!("  [4/8] run: OK");

    // 5: Tasklet runs count as a softirq and a tasklet on that CPU.
    tasklet_run(1).expect("tasklet");
    let cpu1 = per_cpu().into_iter().find(|c| c.cpu_id == 1).expect("cpu1");
    assert_eq!(cpu1.total_tasklets, 1);
    assert_eq!(cpu1.total_softirqs, 1);
    assert_eq!(cpu1.type_counts[SoftirqType::Tasklet.index()], 1);
    crate::serial_println!("  [5/8] tasklet: OK");

    // 6: ksoftirqd wakeup accounting.
    ksoftirqd_wakeup(1).expect("wakeup");
    assert_eq!(per_cpu().into_iter().find(|c| c.cpu_id == 1).expect("cpu1").ksoftirqd_wakeups, 1);
    crate::serial_println!("  [6/8] ksoftirqd: OK");

    // 7: Operations on an unregistered CPU error.
    assert!(run(99, SoftirqType::Timer, 0).is_err());
    assert!(tasklet_run(99).is_err());
    assert!(ksoftirqd_wakeup(99).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final totals reflect only the real activity above. total_raised
    //    counts both raise (1) and the implicit raise inside run/tasklet?
    //    No — only raise() bumps total_raised; run/tasklet bump
    //    total_executed. So raised=1, executed=2 (run + tasklet), tasklets=1.
    let (cpus, types, raised, executed, tasklets, ops) = stats();
    assert_eq!(cpus, 2);
    assert_eq!(types, 10);
    assert_eq!(raised, 1);
    assert_eq!(executed, 2);
    assert_eq!(tasklets, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("softirq::self_test() — all 8 tests passed");
}
