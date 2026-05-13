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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        irq_lines: alloc::vec![
            IrqLine { irq_num: 0, irq_type: IrqType::Timer, name: String::from("HPET/LAPIC"), count: 10_000_000, spurious: 0, affinity_mask: 0xF },
            IrqLine { irq_num: 1, irq_type: IrqType::Keyboard, name: String::from("i8042"), count: 50000, spurious: 5, affinity_mask: 0x1 },
            IrqLine { irq_num: 14, irq_type: IrqType::Disk, name: String::from("ahci0"), count: 500000, spurious: 0, affinity_mask: 0x2 },
            IrqLine { irq_num: 19, irq_type: IrqType::Network, name: String::from("eth0"), count: 2_000_000, spurious: 12, affinity_mask: 0x4 },
            IrqLine { irq_num: 23, irq_type: IrqType::Usb, name: String::from("xhci0"), count: 100000, spurious: 3, affinity_mask: 0x8 },
        ],
        cpu_states: alloc::vec![
            CpuIrqState { cpu_id: 0, total_irqs: 4_000_000, total_ipi: 50000, total_timer: 2_500_000, total_spurious: 8, avg_latency_ns: 800, max_latency_ns: 15000 },
            CpuIrqState { cpu_id: 1, total_irqs: 3_500_000, total_ipi: 45000, total_timer: 2_500_000, total_spurious: 5, avg_latency_ns: 750, max_latency_ns: 12000 },
            CpuIrqState { cpu_id: 2, total_irqs: 3_000_000, total_ipi: 40000, total_timer: 2_500_000, total_spurious: 4, avg_latency_ns: 900, max_latency_ns: 18000 },
            CpuIrqState { cpu_id: 3, total_irqs: 2_150_000, total_ipi: 35000, total_timer: 2_500_000, total_spurious: 3, avg_latency_ns: 850, max_latency_ns: 14000 },
        ],
        total_irqs: 12_650_000,
        total_spurious: 20,
        total_latency_samples: 12_650_000,
        ops: 0,
    });
}

/// Record an interrupt delivery.
pub fn record(cpu: u32, irq_num: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(line) = state.irq_lines.iter_mut().find(|l| l.irq_num == irq_num) {
            line.count += 1;
        }
        let cs = state.cpu_states.iter_mut().find(|c| c.cpu_id == cpu)
            .ok_or(KernelError::NotFound)?;
        cs.total_irqs += 1;
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
        // Running average approximation.
        cs.avg_latency_ns = (cs.avg_latency_ns * 7 + latency_ns) / 8;
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(irq_lines().len(), 5);
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record IRQ.
    let before = irq_lines()[0].count;
    record(0, 0).expect("record");
    let after = irq_lines()[0].count;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] record: OK");

    // 3: Latency.
    record_latency(0, 5000).expect("latency");
    let cs = per_cpu();
    assert!(cs[0].max_latency_ns >= 5000);
    crate::serial_println!("  [3/8] latency: OK");

    // 4: Spurious.
    let before = per_cpu()[1].total_spurious;
    mark_spurious(1, 1).expect("spurious");
    let after = per_cpu()[1].total_spurious;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] spurious: OK");

    // 5: Register IRQ.
    register_irq(50, IrqType::Other(50), "test_irq", 0xF).expect("register");
    assert_eq!(irq_lines().len(), 6);
    assert!(register_irq(50, IrqType::Other(50), "dup", 0).is_err());
    crate::serial_println!("  [5/8] register: OK");

    // 6: Record on new IRQ.
    record(0, 50).expect("record_new");
    let line = irq_lines().iter().find(|l| l.irq_num == 50).cloned().unwrap();
    assert_eq!(line.count, 1);
    crate::serial_println!("  [6/8] new irq record: OK");

    // 7: CPU not found.
    assert!(record(99, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (irqs, cpus, total, spurious, samples, ops) = stats();
    assert_eq!(irqs, 6);
    assert_eq!(cpus, 4);
    assert!(total > 12_650_000);
    assert!(spurious > 20);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("irqstat::self_test() — all 8 tests passed");
}
