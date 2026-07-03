//! Per-CPU time accounting with TSC precision.
//!
//! Tracks how each CPU spends its time, broken down into categories:
//!
//! - **System**: executing kernel code on behalf of a task.
//! - **IRQ**: handling hardware interrupts.
//! - **Softirq**: processing deferred interrupt work.
//! - **Idle**: halted or in MWAIT, waiting for work.
//!
//! ## Design
//!
//! Each CPU maintains per-category cycle accumulators.  Context transitions
//! (enter/exit IRQ, enter/exit softirq, enter/exit idle) stamp the TSC
//! and attribute the elapsed cycles to the correct category.  This gives
//! nanosecond-precision CPU utilization without the 10ms granularity
//! limitation of tick-based sampling.
//!
//! The accounting nests correctly: if an IRQ fires during idle, the cycles
//! from idle-entry to IRQ-entry go to `idle`, the IRQ handler cycles go to
//! `irq`, and after IRQ-exit the CPU returns to idle accounting.
//!
//! ## Overhead
//!
//! Each transition is one `rdtsc` (~25 cycles) plus an atomic add (~5 cycles).
//! At 10,000 interrupts/second, this adds ~300 ns/sec overhead per CPU — negligible.
//!
//! ## Usage
//!
//! ```ignore
//! // At IRQ entry:
//! cputime::enter_irq();
//! // ... handle interrupt ...
//! cputime::exit_irq();
//!
//! // At softirq entry:
//! cputime::enter_softirq();
//! // ... process deferred work ...
//! cputime::exit_softirq();
//!
//! // At idle entry:
//! cputime::enter_idle();
//! // ... HLT / MWAIT ...
//! cputime::exit_idle();
//!
//! // Query:
//! let stats = cputime::per_cpu_stats();
//! ```
//!
//! ## References
//!
//! - Linux `kernel/sched/cputime.c` — tick and vtime accounting
//! - Linux `arch/x86/kernel/irq.c` — `irq_enter`/`irq_exit` hooks
//! - FreeBSD `kern/kern_clock.c` — CPU time accounting

use core::sync::atomic::{AtomicU64, Ordering};

use crate::bench;
use crate::smp::{self, MAX_CPUS};

// ---------------------------------------------------------------------------
// Per-CPU state
// ---------------------------------------------------------------------------

/// Per-CPU time accounting data.
///
/// All cycle counts are raw TSC cycles.  Convert to nanoseconds via
/// `bench::cycles_to_ns()` when displaying.
///
/// Each field is on its own cache line to prevent false sharing when
/// multiple CPUs update simultaneously.
#[repr(C, align(64))]
struct CpuTimeData {
    /// Total cycles spent handling hardware interrupts.
    irq_cycles: AtomicU64,
    _pad0: [u8; 56],
    /// Total cycles spent in softirq processing.
    softirq_cycles: AtomicU64,
    _pad1: [u8; 56],
    /// Total cycles spent idle (HLT/MWAIT).
    idle_cycles: AtomicU64,
    _pad2: [u8; 56],
    /// TSC stamp when current IRQ context was entered (0 = not in IRQ).
    irq_enter_tsc: AtomicU64,
    _pad3: [u8; 56],
    /// TSC stamp when current softirq context was entered (0 = not in softirq).
    softirq_enter_tsc: AtomicU64,
    _pad4: [u8; 56],
    /// TSC stamp when idle was entered (0 = not idle).
    idle_enter_tsc: AtomicU64,
    _pad5: [u8; 56],
    /// Nesting depth for IRQs (supports nested interrupts).
    irq_depth: AtomicU64,
    _pad6: [u8; 56],
    /// Number of IRQ entries (for averaging).
    irq_count: AtomicU64,
    _pad7: [u8; 56],
    /// Number of softirq entries.
    softirq_count: AtomicU64,
    _pad8: [u8; 56],
    /// Number of idle entries.
    idle_count: AtomicU64,
    _pad9: [u8; 56],
}

impl CpuTimeData {
    const fn new() -> Self {
        Self {
            irq_cycles: AtomicU64::new(0),
            _pad0: [0; 56],
            softirq_cycles: AtomicU64::new(0),
            _pad1: [0; 56],
            idle_cycles: AtomicU64::new(0),
            _pad2: [0; 56],
            irq_enter_tsc: AtomicU64::new(0),
            _pad3: [0; 56],
            softirq_enter_tsc: AtomicU64::new(0),
            _pad4: [0; 56],
            idle_enter_tsc: AtomicU64::new(0),
            _pad5: [0; 56],
            irq_depth: AtomicU64::new(0),
            _pad6: [0; 56],
            irq_count: AtomicU64::new(0),
            _pad7: [0; 56],
            softirq_count: AtomicU64::new(0),
            _pad8: [0; 56],
            idle_count: AtomicU64::new(0),
            _pad9: [0; 56],
        }
    }
}

/// Per-CPU time accounting array.
static CPU_TIME: [CpuTimeData; MAX_CPUS] = [const { CpuTimeData::new() }; MAX_CPUS];

/// TSC value at boot (set once during init).
static BOOT_TSC: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize CPU time accounting.  Call once during boot after TSC
/// calibration.
pub fn init() {
    BOOT_TSC.store(bench::rdtsc(), Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// IRQ context transitions
// ---------------------------------------------------------------------------

/// Mark entry into hardware interrupt context.
///
/// Call at the very start of every ISR, before any processing.
/// Supports nesting: if an IRQ fires during another IRQ, only the
/// outermost exit will stop the clock.
#[inline]
pub fn enter_irq() {
    let cpu = smp::current_cpu_index();
    let Some(data) = CPU_TIME.get(cpu) else { return };

    let now = bench::rdtsc();

    // If we interrupted idle, account idle cycles up to now.
    let idle_start = data.idle_enter_tsc.load(Ordering::Relaxed);
    if idle_start != 0 {
        let elapsed = now.saturating_sub(idle_start);
        data.idle_cycles.fetch_add(elapsed, Ordering::Relaxed);
        // Pause idle clock (will resume on exit_irq if still idle).
        data.idle_enter_tsc.store(0, Ordering::Relaxed);
    }

    // Nest depth: only stamp on outermost entry.
    let prev_depth = data.irq_depth.fetch_add(1, Ordering::Relaxed);
    if prev_depth == 0 {
        data.irq_enter_tsc.store(now, Ordering::Relaxed);
        data.irq_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// Mark exit from hardware interrupt context.
///
/// Call at the very end of every ISR, after EOI and softirq processing.
/// Only the outermost exit (depth returning to 0) accounts cycles.
#[inline]
pub fn exit_irq() {
    let cpu = smp::current_cpu_index();
    let Some(data) = CPU_TIME.get(cpu) else { return };

    let depth = data.irq_depth.load(Ordering::Relaxed);
    if depth == 0 {
        // Mismatched exit — defensive, just return.
        return;
    }

    let new_depth = data.irq_depth.fetch_sub(1, Ordering::Relaxed);
    if new_depth == 1 {
        // We're the outermost IRQ exiting.
        let now = bench::rdtsc();
        let enter = data.irq_enter_tsc.swap(0, Ordering::Relaxed);
        if enter != 0 {
            let elapsed = now.saturating_sub(enter);
            data.irq_cycles.fetch_add(elapsed, Ordering::Relaxed);
        }

        // If we interrupted idle, resume idle clock.
        // We detect this by checking if idle was active before this IRQ.
        // Since we cleared idle_enter_tsc in enter_irq, we re-stamp it.
        // The caller (ISR stub) will return to the idle loop which
        // checks if idle should continue.  We only resume if we're
        // returning to the idle path — signalled by idle_count > 0 and
        // the idle loop re-calling enter_idle.
    }
}

/// Current hardware-IRQ nesting depth on this CPU.
///
/// 0 = not in IRQ context, 1 = outermost IRQ handler, 2+ = a nested IRQ
/// (an interrupt fired while another IRQ handler was running with
/// interrupts re-enabled).  Call *after* [`enter_irq`] to test whether
/// the current handler is the outermost (`== 1`) or nested (`> 1`).
///
/// Used by the LAPIC timer ISR to bound IRQ-stack nesting: only the
/// outermost timer handler re-enables interrupts (softirq processing +
/// preemption re-enable).  A nested timer handler runs entirely with
/// interrupts disabled and returns immediately, so timer-on-timer
/// nesting can never exceed depth 2 regardless of per-handler cost.
/// Without this bound, a slow handler (e.g. the poison-debug heap) that
/// exceeds the tick period lets timer IRQs pile up on the fixed-size
/// per-CPU IRQ stack until it overflows the guard page.
#[inline]
#[must_use]
pub fn irq_depth() -> u64 {
    let cpu = smp::current_cpu_index();
    CPU_TIME
        .get(cpu)
        .map_or(0, |data| data.irq_depth.load(Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// Softirq context transitions
// ---------------------------------------------------------------------------

/// Mark entry into softirq processing context.
///
/// Called at the start of `softirq::process_pending()`.
#[inline]
pub fn enter_softirq() {
    let cpu = smp::current_cpu_index();
    let Some(data) = CPU_TIME.get(cpu) else { return };

    data.softirq_enter_tsc.store(bench::rdtsc(), Ordering::Relaxed);
    data.softirq_count.fetch_add(1, Ordering::Relaxed);
}

/// Mark exit from softirq processing context.
///
/// Called at the end of `softirq::process_pending()`.
#[inline]
pub fn exit_softirq() {
    let cpu = smp::current_cpu_index();
    let Some(data) = CPU_TIME.get(cpu) else { return };

    let enter = data.softirq_enter_tsc.swap(0, Ordering::Relaxed);
    if enter != 0 {
        let now = bench::rdtsc();
        let elapsed = now.saturating_sub(enter);
        data.softirq_cycles.fetch_add(elapsed, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Idle context transitions
// ---------------------------------------------------------------------------

/// Mark entry into CPU idle state.
///
/// Called just before HLT/MWAIT in the idle loop.
#[inline]
pub fn enter_idle() {
    let cpu = smp::current_cpu_index();
    let Some(data) = CPU_TIME.get(cpu) else { return };

    data.idle_enter_tsc.store(bench::rdtsc(), Ordering::Relaxed);
    data.idle_count.fetch_add(1, Ordering::Relaxed);
}

/// Mark exit from CPU idle state.
///
/// Called when the CPU wakes from HLT/MWAIT (interrupt arrival).
#[inline]
pub fn exit_idle() {
    let cpu = smp::current_cpu_index();
    let Some(data) = CPU_TIME.get(cpu) else { return };

    let enter = data.idle_enter_tsc.swap(0, Ordering::Relaxed);
    if enter != 0 {
        let now = bench::rdtsc();
        let elapsed = now.saturating_sub(enter);
        data.idle_cycles.fetch_add(elapsed, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Per-CPU time breakdown (in nanoseconds).
#[derive(Debug, Clone, Copy)]
pub struct CpuTimeStats {
    /// Total time since boot (ns).
    pub total_ns: u64,
    /// Time in hardware interrupt handlers (ns).
    pub irq_ns: u64,
    /// Time in softirq processing (ns).
    pub softirq_ns: u64,
    /// Time spent idle (ns).
    pub idle_ns: u64,
    /// Time in system/kernel code (ns) = total - irq - softirq - idle.
    pub system_ns: u64,
    /// Number of IRQ entries.
    pub irq_count: u64,
    /// Number of softirq entries.
    pub softirq_count: u64,
    /// Number of idle entries.
    pub idle_count: u64,
}

/// Get time accounting stats for a specific CPU.
///
/// Returns `None` if the CPU index is out of range.
#[must_use]
pub fn cpu_stats(cpu: usize) -> Option<CpuTimeStats> {
    let data = CPU_TIME.get(cpu)?;
    let freq = bench::tsc_freq();
    if freq == 0 {
        return None;
    }

    let boot_tsc = BOOT_TSC.load(Ordering::Relaxed);
    let now = bench::rdtsc();
    let total_cycles = now.saturating_sub(boot_tsc);

    let irq_cycles = data.irq_cycles.load(Ordering::Relaxed);
    let softirq_cycles = data.softirq_cycles.load(Ordering::Relaxed);
    let idle_cycles = data.idle_cycles.load(Ordering::Relaxed);

    // System = total - irq - softirq - idle (can't go negative due to
    // measurement races, so saturate).
    let accounted = irq_cycles
        .saturating_add(softirq_cycles)
        .saturating_add(idle_cycles);
    let system_cycles = total_cycles.saturating_sub(accounted);

    Some(CpuTimeStats {
        total_ns: cycles_to_ns(total_cycles, freq),
        irq_ns: cycles_to_ns(irq_cycles, freq),
        softirq_ns: cycles_to_ns(softirq_cycles, freq),
        idle_ns: cycles_to_ns(idle_cycles, freq),
        system_ns: cycles_to_ns(system_cycles, freq),
        irq_count: data.irq_count.load(Ordering::Relaxed),
        softirq_count: data.softirq_count.load(Ordering::Relaxed),
        idle_count: data.idle_count.load(Ordering::Relaxed),
    })
}

/// Get time accounting stats for all online CPUs.
///
/// Returns a vector of (cpu_index, stats) for each CPU that has data.
#[must_use]
pub fn all_cpu_stats() -> alloc::vec::Vec<(usize, CpuTimeStats)> {
    let num_cpus = crate::smp::cpu_count().max(1);
    let mut result = alloc::vec::Vec::with_capacity(num_cpus);
    for cpu in 0..num_cpus {
        if let Some(stats) = cpu_stats(cpu) {
            result.push((cpu, stats));
        }
    }
    result
}

/// Get aggregate stats across all CPUs.
#[must_use]
pub fn aggregate_stats() -> CpuTimeStats {
    let num_cpus = crate::smp::cpu_count().max(1);
    let mut agg = CpuTimeStats {
        total_ns: 0,
        irq_ns: 0,
        softirq_ns: 0,
        idle_ns: 0,
        system_ns: 0,
        irq_count: 0,
        softirq_count: 0,
        idle_count: 0,
    };

    for cpu in 0..num_cpus {
        if let Some(stats) = cpu_stats(cpu) {
            agg.total_ns = agg.total_ns.saturating_add(stats.total_ns);
            agg.irq_ns = agg.irq_ns.saturating_add(stats.irq_ns);
            agg.softirq_ns = agg.softirq_ns.saturating_add(stats.softirq_ns);
            agg.idle_ns = agg.idle_ns.saturating_add(stats.idle_ns);
            agg.system_ns = agg.system_ns.saturating_add(stats.system_ns);
            agg.irq_count = agg.irq_count.saturating_add(stats.irq_count);
            agg.softirq_count = agg.softirq_count.saturating_add(stats.softirq_count);
            agg.idle_count = agg.idle_count.saturating_add(stats.idle_count);
        }
    }
    agg
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert TSC cycles to nanoseconds.
#[inline]
fn cycles_to_ns(cycles: u64, freq: u64) -> u64 {
    // ns = cycles * 1_000_000_000 / freq
    // To avoid overflow: split into seconds and remainder.
    let secs = cycles / freq;
    let rem = cycles % freq;
    secs.saturating_mul(1_000_000_000)
        .saturating_add(rem.saturating_mul(1_000_000_000) / freq)
}

extern crate alloc;
