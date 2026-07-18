//! IRQ Statistics — interrupt request monitoring.
//!
//! Tracks hardware interrupt delivery, per-CPU IRQ counts,
//! interrupt latency, and spurious interrupt detection.
//! Essential for diagnosing interrupt storms and driver issues.
//!
//! ## Architecture
//!
//! ```text
//! IRQ statistics
//!   → irqstat::record(cpu, irq) → count interrupt delivery
//!   → irqstat::record_latency(cpu, ns) → track ISR latency
//!   → irqstat::mark_spurious(cpu, irq) → count spurious IRQs
//!   → irqstat::per_cpu() → per-CPU interrupt state
//!
//! Integration:
//!   → softirq (soft interrupt stats)
//!   → perfmon (performance monitor)
//!   → tracemon (trace monitor)
//!   → sysdiag (diagnostics)
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

/// IRQ type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqType {
    Timer,
    Keyboard,
    Cascade,
    Serial,
    Disk,
    Network,
    Usb,
    Gpu,
    Ipi,
    Other(u32),
}

impl IrqType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Timer => "timer",
            Self::Keyboard => "kbd",
            Self::Cascade => "cascade",
            Self::Serial => "serial",
            Self::Disk => "disk",
            Self::Network => "net",
            Self::Usb => "usb",
            Self::Gpu => "gpu",
            Self::Ipi => "ipi",
            Self::Other(_) => "other",
        }
    }
}

/// Per-IRQ line statistics.
#[derive(Debug, Clone)]
pub struct IrqLine {
    pub irq_num: u32,
    pub irq_type: IrqType,
    pub name: String,
    pub count: u64,
    pub spurious: u64,
    pub affinity_mask: u64,
}

/// Per-CPU interrupt state.
#[derive(Debug, Clone)]
pub struct CpuIrqState {
    pub cpu_id: u32,
    pub total_irqs: u64,
    pub total_ipi: u64,
    pub total_timer: u64,
    pub total_spurious: u64,
    pub avg_latency_ns: u64,
    pub max_latency_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_IRQ_LINES: usize = 256;
const MAX_CPU: usize = 64;

struct State {
    irq_lines: Vec<IrqLine>,
    cpu_states: Vec<CpuIrqState>,
    total_irqs: u64,
    total_spurious: u64,
    total_latency_samples: u64,
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

/// Initialise an **empty** IRQ statistics table.
///
/// Seeds NO IRQ lines, NO per-CPU rows, and zero totals.  Real interrupt
/// accounting is wired through [`register_irq`] (one row per discovered IRQ
/// line), [`register_cpu`] (one zero-counter row per online CPU, populated by
/// the scheduler at bring-up), and the `record`/`record_latency`/`mark_spurious`
/// functions; until those are called the table is genuinely empty, so the
/// `/proc/irqstat` file and the `irqstat` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded five fictional IRQ lines (irq 0 "HPET/LAPIC"
/// count 10M; irq 1 "i8042" count 50000 spurious 5; irq 14 "ahci0" count
/// 500000; irq 19 "eth0" count 2M spurious 12; irq 23 "xhci0" count 100000
/// spurious 3) plus four fictional per-CPU rows (cpu0..3 with total_irqs
/// 2.15M–4M, total_ipi 35000–50000, total_timer 2.5M each, latency 750–900ns
/// avg / 12000–18000ns max) and invented aggregate totals (total_irqs
/// 12_650_000, total_spurious 20, total_latency_samples 12_650_000), which
/// `/proc/irqstat` then displayed as if they were real interrupt-delivery
/// measurements.  That demo data was removed; the self-test now builds its own
/// fixtures explicitly via the real API (see [`self_test`]).  The IRQ subsystem
/// is expected to call [`register_irq`] per discovered line, [`register_cpu`]
/// per online CPU, and the record_* functions on the interrupt path.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        irq_lines: Vec::new(),
        cpu_states: Vec::new(),
        total_irqs: 0,
        total_spurious: 0,
        total_latency_samples: 0,
        ops: 0,
    });
}

/// Record an interrupt delivery.
pub fn record(cpu: u32, irq_num: u32) -> KernelResult<()> {
    with_state(|state| {
        // Validate the CPU is registered BEFORE mutating any counter, so a
        // record for an unknown CPU fails cleanly rather than partially applying
        // (bumping the IRQ line count) and then erroring.
        if !state.cpu_states.iter().any(|c| c.cpu_id == cpu) {
            return Err(KernelError::NotFound);
        }
        if let Some(line) = state.irq_lines.iter_mut().find(|l| l.irq_num == irq_num) {
            line.count += 1;
        }
        if let Some(cs) = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu) {
            cs.total_irqs += 1;
        }
        state.total_irqs += 1;
        Ok(())
    })
}

/// Record ISR latency.
pub fn record_latency(cpu: u32, latency_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        if latency_ns > cs.max_latency_ns { cs.max_latency_ns = latency_ns; }
        // Running average (EWMA, weight 1/8).  Seed exactly on the first sample
        // rather than blending against a zero initial value, which would
        // underweight the first measurement and bias the average low at cold
        // start.  We treat "no samples yet on this CPU" as avg_latency_ns == 0,
        // which holds because register_cpu zeroes the row and only this path
        // ever raises it.
        cs.avg_latency_ns = if cs.avg_latency_ns == 0 {
            latency_ns
        } else {
            (cs.avg_latency_ns * 7 + latency_ns) / 8
        };
        state.total_latency_samples += 1;
        Ok(())
    })
}

/// Mark a spurious interrupt.
pub fn mark_spurious(cpu: u32, irq_num: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(line) = state.irq_lines.iter_mut().find(|l| l.irq_num == irq_num) {
            line.spurious += 1;
        }
        if let Some(cs) = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu) {
            cs.total_spurious += 1;
        }
        state.total_spurious += 1;
        Ok(())
    })
}

/// Register an IRQ line.
pub fn register_irq(irq_num: u32, irq_type: IrqType, name: &str, affinity: u64) -> KernelResult<()> {
    with_state(|state| {
        if state.irq_lines.len() >= MAX_IRQ_LINES { return Err(KernelError::ResourceExhausted); }
        if state.irq_lines.iter().any(|l| l.irq_num == irq_num) { return Err(KernelError::AlreadyExists); }
        state.irq_lines.push(IrqLine {
            irq_num, irq_type, name: String::from(name),
            count: 0, spurious: 0, affinity_mask: affinity,
        });
        Ok(())
    })
}

/// Register a CPU for per-CPU interrupt tracking.
///
/// The scheduler calls this once per online CPU at bring-up so the per-CPU
/// interrupt table reflects the real topology with all counters zeroed.  The
/// `record`/`record_latency` functions return `NotFound` for an unregistered
/// CPU id.
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpu_states.len() >= MAX_CPU { return Err(KernelError::ResourceExhausted); }
        if state.cpu_states.iter().any(|c| c.cpu_id == cpu_id) { return Err(KernelError::AlreadyExists); }
        state.cpu_states.push(CpuIrqState {
            cpu_id, total_irqs: 0, total_ipi: 0, total_timer: 0,
            total_spurious: 0, avg_latency_ns: 0, max_latency_ns: 0,
        });
        Ok(())
    })
}

/// Get all IRQ line stats.
pub fn irq_lines() -> Vec<IrqLine> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.irq_lines.clone())
}

/// Get per-CPU state.
pub fn per_cpu() -> Vec<CpuIrqState> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpu_states.clone())
}

/// Statistics: (irq_count, cpu_count, total_irqs, total_spurious, total_samples, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.irq_lines.len(), s.cpu_states.len(), s.total_irqs, s.total_spurious, s.total_latency_samples, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("irqstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/irqstat must never surface).
    // Resetting first clears any residue from a prior `irqstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated IRQ lines, per-CPU rows, or totals.
    assert_eq!(irq_lines().len(), 0);
    assert_eq!(per_cpu().len(), 0);
    let (l0, c0, t0, s0, sm0, _o0) = stats();
    assert_eq!((l0, c0, t0, s0, sm0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register an IRQ line (zeroed) and two CPUs (zeroed); duplicates fail.
    register_irq(1, IrqType::Keyboard, "i8042", 0x1).expect("reg irq");
    assert!(register_irq(1, IrqType::Keyboard, "dup", 0).is_err());
    register_cpu(0).expect("reg cpu0");
    register_cpu(1).expect("reg cpu1");
    assert!(register_cpu(0).is_err());
    assert_eq!(irq_lines().len(), 1);
    assert_eq!(per_cpu().len(), 2);
    let line = irq_lines().iter().find(|l| l.irq_num == 1).cloned().expect("irq1");
    assert_eq!(line.count, 0);
    assert_eq!(line.spurious, 0);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record increments the IRQ line count and the CPU's total exactly from
    //    zero.
    record(0, 1).expect("record");
    let line = irq_lines().iter().find(|l| l.irq_num == 1).cloned().expect("irq1");
    assert_eq!(line.count, 1);
    let cs0 = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(cs0.total_irqs, 1);
    crate::serial_println!("  [3/8] record: OK");

    // 4: Latency — first sample seeds the average exactly (no cold-start bias),
    //    and sets the max.
    record_latency(0, 800).expect("lat1");
    let cs0 = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(cs0.avg_latency_ns, 800); // seeded, not blended against 0
    assert_eq!(cs0.max_latency_ns, 800);
    // Second sample blends: (800*7 + 1600)/8 = (5600+1600)/8 = 900.
    record_latency(0, 1600).expect("lat2");
    let cs0 = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(cs0.avg_latency_ns, 900);
    assert_eq!(cs0.max_latency_ns, 1600);
    crate::serial_println!("  [4/8] latency: OK");

    // 5: Spurious — increments the line, the CPU, and the aggregate exactly.
    mark_spurious(0, 1).expect("spurious");
    let line = irq_lines().iter().find(|l| l.irq_num == 1).cloned().expect("irq1");
    assert_eq!(line.spurious, 1);
    let cs0 = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(cs0.total_spurious, 1);
    crate::serial_println!("  [5/8] spurious: OK");

    // 6: Recording on an unregistered CPU fails with NotFound; the IRQ line
    //    count is NOT bumped when the CPU lookup fails (record returns early).
    assert!(record(99, 1).is_err());
    let line = irq_lines().iter().find(|l| l.irq_num == 1).cloned().expect("irq1");
    assert_eq!(line.count, 1); // unchanged — still just the one valid record
    crate::serial_println!("  [6/8] not found: OK");

    // 7: Latency on an unregistered CPU also fails with NotFound.
    assert!(record_latency(99, 100).is_err());
    crate::serial_println!("  [7/8] latency not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (irqs, cpus, total, spurious, samples, ops) = stats();
    assert_eq!(irqs, 1);
    assert_eq!(cpus, 2);
    assert_eq!(total, 1);     // one successful record
    assert_eq!(spurious, 1);  // one mark_spurious
    assert_eq!(samples, 2);   // two record_latency calls
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/irqstat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the IRQ subsystem wires
    // real accounting.
    *STATE.lock() = None;

    crate::serial_println!("irqstat::self_test() — all 8 tests passed");
}
