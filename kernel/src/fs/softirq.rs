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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let mut cpu_states = Vec::new();
    for i in 0..4u32 {
        let mut counts = [0u64; 10];
        counts[1] = 500000 + i as u64 * 100000; // Timer.
        counts[3] = 200000 + i as u64 * 50000;  // NetRx.
        counts[4] = 100000 + i as u64 * 25000;  // Block.
        counts[9] = 300000 + i as u64 * 75000;  // RCU.
        let total: u64 = counts.iter().sum();
        cpu_states.push(CpuSoftirqState {
            cpu_id: i, total_softirqs: total, total_tasklets: 1000 + i as u64 * 200,
            ksoftirqd_wakeups: 50 + i as u64 * 10, type_counts: counts,
        });
    }
    let mut type_stats = Vec::new();
    for st in SoftirqType::all() {
        let base = match st {
            SoftirqType::Timer => 2_600_000,
            SoftirqType::NetRx => 1_000_000,
            SoftirqType::Block => 550_000,
            SoftirqType::Rcu => 1_500_000,
            _ => 10000,
        };
        type_stats.push(SoftirqTypeStats {
            softirq_type: *st, raised: base + 1000, executed: base, total_ns: base * 500,
        });
    }
    *guard = Some(State {
        cpu_states,
        type_stats,
        total_raised: 5_710_000,
        total_executed: 5_700_000,
        total_tasklets: 5200,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    assert_eq!(type_stats().len(), 10);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Raise.
    let before = type_stats().iter().find(|t| t.softirq_type == SoftirqType::Timer).unwrap().raised;
    raise(SoftirqType::Timer).expect("raise");
    let after = type_stats().iter().find(|t| t.softirq_type == SoftirqType::Timer).unwrap().raised;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] raise: OK");

    // 3: Run.
    let before = per_cpu()[0].total_softirqs;
    run(0, SoftirqType::NetRx, 1000).expect("run");
    let after = per_cpu()[0].total_softirqs;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [3/8] run: OK");

    // 4: Tasklet.
    let before = per_cpu()[1].total_tasklets;
    tasklet_run(1).expect("tasklet");
    let after = per_cpu()[1].total_tasklets;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] tasklet: OK");

    // 5: Ksoftirqd wakeup.
    let before = per_cpu()[2].ksoftirqd_wakeups;
    ksoftirqd_wakeup(2).expect("wakeup");
    let after = per_cpu()[2].ksoftirqd_wakeups;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [5/8] ksoftirqd: OK");

    // 6: Type stats ns.
    let ts = type_stats().iter().find(|t| t.softirq_type == SoftirqType::NetRx).cloned().unwrap();
    assert!(ts.total_ns > 0);
    crate::serial_println!("  [6/8] type ns: OK");

    // 7: CPU not found.
    assert!(run(99, SoftirqType::Timer, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (cpus, types, raised, executed, tasklets, ops) = stats();
    assert_eq!(cpus, 4);
    assert_eq!(types, 10);
    assert!(raised > 5_710_000);
    assert!(executed > 5_700_000);
    assert!(tasklets > 5200);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("softirq::self_test() — all 8 tests passed");
}
