//! IRQ Balance — interrupt request affinity balancing.
//!
//! Distributes hardware interrupts across CPU cores to prevent
//! bottlenecks and improve system throughput. Tracks per-CPU
//! IRQ loads and rebalances periodically.
//!
//! ## Architecture
//!
//! ```text
//! IRQ balancing
//!   → irqbalance::set_affinity(irq, cpus) → pin IRQ to CPUs
//!   → irqbalance::balance() → auto-rebalance all IRQs
//!   → irqbalance::irq_stats() → per-IRQ counters
//!
//! Integration:
//!   → cputopo (CPU topology)
//!   → perfmon (performance monitor)
//!   → sysdiag (diagnostics)
//!   → schedtune (scheduler tuning)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// IRQ balancing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalancePolicy {
    /// Spread IRQs evenly across CPUs.
    RoundRobin,
    /// Pin to least-loaded CPU.
    LeastLoaded,
    /// Pin to specific CPUs (manual).
    Manual,
    /// Disable balancing (all go to CPU 0).
    Disabled,
}

impl BalancePolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::RoundRobin => "Round-Robin",
            Self::LeastLoaded => "Least-Loaded",
            Self::Manual => "Manual",
            Self::Disabled => "Disabled",
        }
    }
}

/// An IRQ entry.
#[derive(Debug, Clone)]
pub struct IrqEntry {
    pub irq: u32,
    pub name: String,
    pub affinity_mask: u64,
    pub count: u64,
    pub assigned_cpu: u32,
}

/// Per-CPU IRQ load.
#[derive(Debug, Clone)]
pub struct CpuIrqLoad {
    pub cpu_id: u32,
    pub total_irqs: u64,
    pub assigned_irqs: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_IRQS: usize = 256;
const MAX_CPUS: usize = 128;

struct State {
    irqs: Vec<IrqEntry>,
    cpu_loads: Vec<CpuIrqLoad>,
    policy: BalancePolicy,
    total_rebalances: u64,
    total_migrations: u64,
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
        irqs: alloc::vec![
            IrqEntry { irq: 0, name: String::from("timer"), affinity_mask: 0x01, count: 0, assigned_cpu: 0 },
            IrqEntry { irq: 1, name: String::from("keyboard"), affinity_mask: 0x01, count: 0, assigned_cpu: 0 },
            IrqEntry { irq: 8, name: String::from("rtc"), affinity_mask: 0x01, count: 0, assigned_cpu: 0 },
            IrqEntry { irq: 14, name: String::from("ata0"), affinity_mask: 0xFF, count: 0, assigned_cpu: 0 },
            IrqEntry { irq: 15, name: String::from("ata1"), affinity_mask: 0xFF, count: 0, assigned_cpu: 1 },
            IrqEntry { irq: 16, name: String::from("eth0"), affinity_mask: 0xFF, count: 0, assigned_cpu: 2 },
            IrqEntry { irq: 17, name: String::from("usb0"), affinity_mask: 0xFF, count: 0, assigned_cpu: 3 },
        ],
        cpu_loads: alloc::vec![
            CpuIrqLoad { cpu_id: 0, total_irqs: 0, assigned_irqs: 4 },
            CpuIrqLoad { cpu_id: 1, total_irqs: 0, assigned_irqs: 1 },
            CpuIrqLoad { cpu_id: 2, total_irqs: 0, assigned_irqs: 1 },
            CpuIrqLoad { cpu_id: 3, total_irqs: 0, assigned_irqs: 1 },
        ],
        policy: BalancePolicy::LeastLoaded,
        total_rebalances: 0,
        total_migrations: 0,
        ops: 0,
    });
}

/// Set balancing policy.
pub fn set_policy(policy: BalancePolicy) -> KernelResult<()> {
    with_state(|state| {
        state.policy = policy;
        Ok(())
    })
}

/// Get current policy.
pub fn get_policy() -> Option<BalancePolicy> {
    STATE.lock().as_ref().map(|s| s.policy)
}

/// Set affinity for an IRQ.
pub fn set_affinity(irq: u32, cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.irqs.iter_mut().find(|i| i.irq == irq)
            .ok_or(KernelError::NotFound)?;
        let old_cpu = entry.assigned_cpu;
        entry.assigned_cpu = cpu;
        entry.affinity_mask = 1u64 << cpu;
        // Update CPU load counts.
        if let Some(old_load) = state.cpu_loads.iter_mut().find(|c| c.cpu_id == old_cpu) {
            old_load.assigned_irqs = old_load.assigned_irqs.saturating_sub(1);
        }
        if let Some(new_load) = state.cpu_loads.iter_mut().find(|c| c.cpu_id == cpu) {
            new_load.assigned_irqs += 1;
        }
        if old_cpu != cpu {
            state.total_migrations += 1;
        }
        Ok(())
    })
}

/// Record an IRQ event.
pub fn record_irq(irq: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(entry) = state.irqs.iter_mut().find(|i| i.irq == irq) {
            entry.count += 1;
            let cpu = entry.assigned_cpu;
            if let Some(load) = state.cpu_loads.iter_mut().find(|c| c.cpu_id == cpu) {
                load.total_irqs += 1;
            }
        }
        Ok(())
    })
}

/// Register a new IRQ.
pub fn register_irq(irq: u32, name: &str, cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.irqs.len() >= MAX_IRQS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.irqs.iter().any(|i| i.irq == irq) {
            return Err(KernelError::AlreadyExists);
        }
        state.irqs.push(IrqEntry {
            irq, name: String::from(name), affinity_mask: 1u64 << cpu,
            count: 0, assigned_cpu: cpu,
        });
        if let Some(load) = state.cpu_loads.iter_mut().find(|c| c.cpu_id == cpu) {
            load.assigned_irqs += 1;
        }
        Ok(())
    })
}

/// Rebalance all IRQs across CPUs (simulated).
pub fn balance() -> KernelResult<u32> {
    with_state(|state| {
        if state.policy == BalancePolicy::Disabled || state.policy == BalancePolicy::Manual {
            return Ok(0);
        }
        let num_cpus = state.cpu_loads.len() as u32;
        if num_cpus == 0 { return Ok(0); }
        let mut migrations = 0u32;
        let irq_ids: Vec<u32> = state.irqs.iter().map(|i| i.irq).collect();
        for (idx, irq_id) in irq_ids.iter().enumerate() {
            if let Some(entry) = state.irqs.iter_mut().find(|i| i.irq == *irq_id) {
                let new_cpu = match state.policy {
                    BalancePolicy::RoundRobin => (idx as u32) % num_cpus,
                    BalancePolicy::LeastLoaded => {
                        state.cpu_loads.iter()
                            .min_by_key(|c| c.assigned_irqs)
                            .map(|c| c.cpu_id)
                            .unwrap_or(0)
                    }
                    _ => entry.assigned_cpu,
                };
                if entry.assigned_cpu != new_cpu {
                    if let Some(old_load) = state.cpu_loads.iter_mut().find(|c| c.cpu_id == entry.assigned_cpu) {
                        old_load.assigned_irqs = old_load.assigned_irqs.saturating_sub(1);
                    }
                    if let Some(new_load) = state.cpu_loads.iter_mut().find(|c| c.cpu_id == new_cpu) {
                        new_load.assigned_irqs += 1;
                    }
                    entry.assigned_cpu = new_cpu;
                    entry.affinity_mask = 1u64 << new_cpu;
                    migrations += 1;
                }
            }
        }
        state.total_rebalances += 1;
        state.total_migrations += migrations as u64;
        Ok(migrations)
    })
}

/// List all IRQs.
pub fn list_irqs() -> Vec<IrqEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.irqs.clone())
}

/// Get CPU loads.
pub fn cpu_loads() -> Vec<CpuIrqLoad> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpu_loads.clone())
}

/// Statistics: (irq_count, total_rebalances, total_migrations, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.irqs.len(), s.total_rebalances, s.total_migrations, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("irqbalance::self_test() — running tests...");
    init_defaults();

    // 1: Default IRQs.
    assert_eq!(list_irqs().len(), 7);
    assert_eq!(cpu_loads().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record IRQ.
    record_irq(1).expect("record");
    let irqs = list_irqs();
    let kb = irqs.iter().find(|i| i.irq == 1).expect("kb");
    assert_eq!(kb.count, 1);
    crate::serial_println!("  [2/8] record: OK");

    // 3: Set affinity.
    set_affinity(16, 1).expect("affinity");
    let irqs = list_irqs();
    let eth = irqs.iter().find(|i| i.irq == 16).expect("eth");
    assert_eq!(eth.assigned_cpu, 1);
    crate::serial_println!("  [3/8] affinity: OK");

    // 4: Register new IRQ.
    register_irq(32, "gpu0", 2).expect("register");
    assert_eq!(list_irqs().len(), 8);
    crate::serial_println!("  [4/8] register: OK");

    // 5: Policy.
    set_policy(BalancePolicy::RoundRobin).expect("policy");
    assert_eq!(get_policy(), Some(BalancePolicy::RoundRobin));
    crate::serial_println!("  [5/8] policy: OK");

    // 6: Balance.
    let migrations = balance().expect("balance");
    let _ = migrations;
    crate::serial_println!("  [6/8] balance: OK");

    // 7: CPU loads.
    let loads = cpu_loads();
    assert_eq!(loads.len(), 4);
    let total_assigned: u32 = loads.iter().map(|c| c.assigned_irqs).sum();
    assert!(total_assigned >= 8);
    crate::serial_println!("  [7/8] cpu loads: OK");

    // 8: Stats.
    let (irq_count, rebalances, _migrations, ops) = stats();
    assert_eq!(irq_count, 8);
    assert!(rebalances >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("irqbalance::self_test() — all 8 tests passed");
}
