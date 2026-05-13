//! MSI Vector Statistics — MSI/MSI-X interrupt vector monitoring.
//!
//! Tracks MSI and MSI-X vector allocations, interrupt delivery
//! counts, and per-device vector usage. Essential for PCIe
//! interrupt performance tuning.
//!
//! ## Architecture
//!
//! ```text
//! MSI vector monitoring
//!   → msivec::alloc_vector(device, count) → allocate vectors
//!   → msivec::free_vector(device) → free vectors
//!   → msivec::record_interrupt(vec) → interrupt delivered
//!   → msivec::per_device() → per-device stats
//!
//! Integration:
//!   → irqstat (interrupt stats)
//!   → irqbalance (IRQ balancing)
//!   → softirq (soft interrupts)
//!   → netqueue (network queues)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// MSI type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsiType {
    Msi,
    MsiX,
}

impl MsiType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Msi => "MSI",
            Self::MsiX => "MSI-X",
        }
    }
}

/// Per-device MSI info.
#[derive(Debug, Clone)]
pub struct DeviceMsi {
    pub device: String,
    pub msi_type: MsiType,
    pub vectors_allocated: u32,
    pub vectors_active: u32,
    pub interrupts: u64,
    pub target_cpu: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 256;

struct State {
    devices: Vec<DeviceMsi>,
    total_vectors: u32,
    total_interrupts: u64,
    alloc_count: u64,
    free_count: u64,
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
        devices: alloc::vec![
            DeviceMsi { device: String::from("nvme0"), msi_type: MsiType::MsiX, vectors_allocated: 8, vectors_active: 8, interrupts: 50_000_000, target_cpu: 0 },
            DeviceMsi { device: String::from("eth0"), msi_type: MsiType::MsiX, vectors_allocated: 4, vectors_active: 4, interrupts: 100_000_000, target_cpu: 1 },
            DeviceMsi { device: String::from("gpu0"), msi_type: MsiType::Msi, vectors_allocated: 1, vectors_active: 1, interrupts: 5_000_000, target_cpu: 0 },
            DeviceMsi { device: String::from("ahci0"), msi_type: MsiType::Msi, vectors_allocated: 1, vectors_active: 1, interrupts: 10_000_000, target_cpu: 2 },
        ],
        total_vectors: 14,
        total_interrupts: 165_000_000,
        alloc_count: 100,
        free_count: 86,
        ops: 0,
    });
}

/// Allocate vectors for a device.
pub fn alloc_vectors(device: &str, msi_type: MsiType, count: u32, cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES { return Err(KernelError::ResourceExhausted); }
        if state.devices.iter().any(|d| d.device == device) { return Err(KernelError::AlreadyExists); }
        state.devices.push(DeviceMsi {
            device: String::from(device), msi_type, vectors_allocated: count,
            vectors_active: count, interrupts: 0, target_cpu: cpu,
        });
        state.total_vectors += count;
        state.alloc_count += 1;
        Ok(())
    })
}

/// Free vectors for a device.
pub fn free_vectors(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.devices.iter().position(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        let count = state.devices[idx].vectors_allocated;
        state.devices.remove(idx);
        state.total_vectors = state.total_vectors.saturating_sub(count);
        state.free_count += 1;
        Ok(())
    })
}

/// Record an interrupt delivery.
pub fn record_interrupt(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        d.interrupts += 1;
        state.total_interrupts += 1;
        Ok(())
    })
}

/// Set target CPU for a device's vectors.
pub fn set_target_cpu(device: &str, cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        d.target_cpu = cpu;
        Ok(())
    })
}

/// Per-device stats.
pub fn per_device() -> Vec<DeviceMsi> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Statistics: (device_count, total_vectors, total_interrupts, allocs, frees, ops).
pub fn stats() -> (usize, u32, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_vectors, s.total_interrupts, s.alloc_count, s.free_count, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("msivec::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_device().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Alloc.
    alloc_vectors("test_dev", MsiType::MsiX, 4, 0).expect("alloc");
    assert_eq!(per_device().len(), 5);
    assert!(alloc_vectors("test_dev", MsiType::Msi, 1, 0).is_err());
    crate::serial_println!("  [2/8] alloc: OK");

    // 3: Interrupt.
    record_interrupt("test_dev").expect("interrupt");
    let d = per_device().iter().find(|d| d.device == "test_dev").cloned().unwrap();
    assert_eq!(d.interrupts, 1);
    crate::serial_println!("  [3/8] interrupt: OK");

    // 4: Target CPU.
    set_target_cpu("test_dev", 3).expect("target");
    let d = per_device().iter().find(|d| d.device == "test_dev").cloned().unwrap();
    assert_eq!(d.target_cpu, 3);
    crate::serial_println!("  [4/8] target cpu: OK");

    // 5: Free.
    free_vectors("test_dev").expect("free");
    assert_eq!(per_device().len(), 4);
    assert!(free_vectors("test_dev").is_err());
    crate::serial_println!("  [5/8] free: OK");

    // 6: Vector count.
    let (_, vecs, _, _, _, _) = stats();
    assert_eq!(vecs, 14); // back to default after free
    crate::serial_println!("  [6/8] vector count: OK");

    // 7: Not found.
    assert!(record_interrupt("nonexist").is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (devs, vecs, ints, allocs, frees, ops) = stats();
    assert_eq!(devs, 4);
    assert_eq!(vecs, 14);
    assert!(ints > 165_000_000);
    assert!(allocs > 100);
    assert!(frees > 86);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("msivec::self_test() — all 8 tests passed");
}
