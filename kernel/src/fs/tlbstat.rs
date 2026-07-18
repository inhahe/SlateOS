//! TLB Statistics — Translation Lookaside Buffer performance monitoring.
//!
//! Tracks TLB hits, misses, shootdowns, and flushes per CPU.
//! Essential for diagnosing address translation overhead and
//! tuning huge page configuration.
//!
//! ## Architecture
//!
//! ```text
//! TLB statistics
//!   → tlbstat::record_hit(cpu) → count TLB hit
//!   → tlbstat::record_miss(cpu) → count TLB miss
//!   → tlbstat::record_shootdown(cpu) → count shootdown IPI
//!   → tlbstat::record_flush(cpu) → count full TLB flush
//!
//! Integration:
//!   → pftrack (page fault tracking)
//!   → numastat (NUMA statistics)
//!   → memlayout (memory layout)
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

/// TLB flush reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushReason {
    ContextSwitch,
    MunMap,
    MProtect,
    PageMigration,
    KernelRequest,
}

impl FlushReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::ContextSwitch => "ctx_switch",
            Self::MunMap => "munmap",
            Self::MProtect => "mprotect",
            Self::PageMigration => "migration",
            Self::KernelRequest => "kernel",
        }
    }
}

/// Per-CPU TLB state.
#[derive(Debug, Clone)]
pub struct CpuTlbState {
    pub cpu_id: u32,
    pub hits: u64,
    pub misses: u64,
    pub shootdowns_sent: u64,
    pub shootdowns_recv: u64,
    pub flushes: u64,
    pub flush_all: u64,
    pub flush_range: u64,
    pub walk_cycles: u64,
}

/// A TLB shootdown event.
#[derive(Debug, Clone)]
pub struct ShootdownEvent {
    pub source_cpu: u32,
    pub target_cpus: u32,
    pub pages: u64,
    pub timestamp_ns: u64,
    pub reason: FlushReason,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPU: usize = 64;
const MAX_EVENTS: usize = 256;

struct State {
    cpu_states: Vec<CpuTlbState>,
    shootdown_log: Vec<ShootdownEvent>,
    total_hits: u64,
    total_misses: u64,
    total_shootdowns: u64,
    total_flushes: u64,
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

/// Initialise an **empty** TLB-statistics table.
///
/// Seeds NO per-CPU rows, no shootdown log, and zero totals.  Real TLB
/// accounting is wired through [`register_cpu`] (one zero-counter row per online
/// CPU, populated by the memory subsystem at bring-up) and the
/// `record_hit`/`record_miss`/`record_shootdown`/`record_flush` functions;
/// until those are called the tables are genuinely empty, so the
/// `/proc/tlbstat` file and the `tlbstat` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded four fictional per-CPU rows (cpu0..3 with hits
/// 5_000_000–8_000_000, misses 50_000–80_000, shootdowns/flushes/walk_cycles)
/// plus invented aggregate totals (total_hits 26_000_000, total_misses 200_000,
/// total_shootdowns 550, total_flushes 3400), which `/proc/tlbstat` then
/// displayed as if they were real TLB hit-rate/shootdown measurements.  That
/// demo data was removed; the self-test now builds its own fixtures explicitly
/// via the real API (see [`self_test`]).  The memory subsystem is expected to
/// call [`register_cpu`] per online CPU and the record_* functions as the TLB
/// is exercised.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpu_states: Vec::new(),
        shootdown_log: Vec::new(),
        total_hits: 0,
        total_misses: 0,
        total_shootdowns: 0,
        total_flushes: 0,
        ops: 0,
    });
}

/// Register a CPU for TLB tracking.
///
/// The memory subsystem calls this once per online CPU at bring-up so the
/// per-CPU TLB state table reflects the real topology with zeroed counters.
/// The `record_hit`/`record_miss`/`record_flush` functions return `NotFound`
/// for an unregistered CPU id.
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpu_states.iter().any(|c| c.cpu_id == cpu_id) { return Err(KernelError::AlreadyExists); }
        if state.cpu_states.len() >= MAX_CPU { return Err(KernelError::ResourceExhausted); }
        state.cpu_states.push(CpuTlbState {
            cpu_id, hits: 0, misses: 0, shootdowns_sent: 0, shootdowns_recv: 0,
            flushes: 0, flush_all: 0, flush_range: 0, walk_cycles: 0,
        });
        Ok(())
    })
}

/// Record a TLB hit.
pub fn record_hit(cpu: u32, count: u64) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.hits += count;
        state.total_hits += count;
        Ok(())
    })
}

/// Record a TLB miss.
pub fn record_miss(cpu: u32, count: u64, walk_cycles: u64) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.misses += count;
        cs.walk_cycles += walk_cycles;
        state.total_misses += count;
        Ok(())
    })
}

/// Record a TLB shootdown.
pub fn record_shootdown(source: u32, target_count: u32, pages: u64, reason: FlushReason) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(cs) = state.cpu_states.iter_mut().find(|c| c.cpu_id == source) {
            cs.shootdowns_sent += 1;
        }
        // Record in all target CPUs (simplified: increment all others).
        for cs in &mut state.cpu_states {
            if cs.cpu_id != source { cs.shootdowns_recv += 1; }
        }
        state.total_shootdowns += 1;
        if state.shootdown_log.len() >= MAX_EVENTS { state.shootdown_log.remove(0); }
        state.shootdown_log.push(ShootdownEvent {
            source_cpu: source, target_cpus: target_count,
            pages, timestamp_ns: now, reason,
        });
        Ok(())
    })
}

/// Record a TLB flush.
pub fn record_flush(cpu: u32, full: bool) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.flushes += 1;
        if full { cs.flush_all += 1; } else { cs.flush_range += 1; }
        state.total_flushes += 1;
        Ok(())
    })
}

/// Get per-CPU TLB state.
pub fn cpu_stats() -> Vec<CpuTlbState> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpu_states.clone())
}

/// Hit rate as integer percentage (0-100).
pub fn hit_rate() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let total = s.total_hits + s.total_misses;
            if total == 0 { 100 } else { s.total_hits * 100 / total }
        }
        None => 0,
    }
}

/// Recent shootdown events.
pub fn shootdown_log(n: usize) -> Vec<ShootdownEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if n >= s.shootdown_log.len() { 0 } else { s.shootdown_log.len() - n };
        s.shootdown_log[start..].to_vec()
    })
}

/// Statistics: (cpu_count, total_hits, total_misses, total_shootdowns, total_flushes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpu_states.len(), s.total_hits, s.total_misses, s.total_shootdowns, s.total_flushes, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("tlbstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/tlbstat must never surface).
    // Resetting first clears any residue from a prior `tlbstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated CPUs or totals; hit_rate is 100 when
    //    there are no samples yet.
    assert_eq!(cpu_stats().len(), 0);
    let (c0, h0, m0, s0, f0, _o0) = stats();
    assert_eq!((c0, h0, m0, s0, f0), (0, 0, 0, 0, 0));
    assert_eq!(hit_rate(), 100);
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register CPUs (zeroed); record a hit exactly from zero.
    register_cpu(0).expect("cpu0");
    register_cpu(1).expect("cpu1");
    register_cpu(2).expect("cpu2");
    assert!(register_cpu(0).is_err());
    record_hit(0, 100).expect("hit");
    let c = cpu_stats().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(c.hits, 100);
    crate::serial_println!("  [2/8] hit: OK");

    // 3: Record miss with page-walk cycles.
    record_miss(1, 5, 500).expect("miss");
    let c = cpu_stats().iter().find(|c| c.cpu_id == 1).cloned().expect("cpu1");
    assert_eq!(c.misses, 5);
    assert_eq!(c.walk_cycles, 500);
    crate::serial_println!("  [3/8] miss: OK");

    // 4: Hit rate is exact: 100 hits / (100 + 5) = 95%.
    let rate = hit_rate();
    assert_eq!(rate, 95);
    crate::serial_println!("  [4/8] hit rate: OK ({}%)", rate);

    // 5: Shootdown logs an event and bumps sent/recv counters exactly.
    record_shootdown(0, 3, 4, FlushReason::MunMap).expect("shootdown");
    let log = shootdown_log(5);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].source_cpu, 0);
    let c0 = cpu_stats().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    let c1 = cpu_stats().iter().find(|c| c.cpu_id == 1).cloned().expect("cpu1");
    assert_eq!(c0.shootdowns_sent, 1);
    assert_eq!(c1.shootdowns_recv, 1); // every non-source CPU receives
    crate::serial_println!("  [5/8] shootdown: OK");

    // 6: Flush (full + range) increments exactly from zero.
    record_flush(2, true).expect("flush_full");
    record_flush(2, false).expect("flush_range");
    let c = cpu_stats().iter().find(|c| c.cpu_id == 2).cloned().expect("cpu2");
    assert_eq!(c.flushes, 2);
    assert_eq!(c.flush_all, 1);
    assert_eq!(c.flush_range, 1);
    crate::serial_println!("  [6/8] flush: OK");

    // 7: Recording on an unregistered CPU fails with NotFound.
    assert!(record_hit(99, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (cpus, hits, misses, shootdowns, flushes, ops) = stats();
    assert_eq!(cpus, 3);
    assert_eq!(hits, 100);
    assert_eq!(misses, 5);
    assert_eq!(shootdowns, 1);
    assert_eq!(flushes, 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/tlbstat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the memory subsystem
    // wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("tlbstat::self_test() — all 8 tests passed");
}
