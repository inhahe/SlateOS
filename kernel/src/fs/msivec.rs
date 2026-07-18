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
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the MSI-vector statistics state.
///
/// Starts with no devices and zero vector/interrupt/alloc/free totals. The
/// `/proc/msivec` generator and the `msivec` kshell command surface this
/// per-device table (and `per_device`) as if it reflects the real set of
/// MSI/MSI-X vector allocations, so seeding it with phantom devices would
/// be fabricated procfs data — it would claim interrupt vectors are
/// allocated to PCIe devices that nothing actually programmed. Vectors are
/// allocated through [`alloc_vectors`] when a device's MSI capability is
/// configured and released through [`free_vectors`]; the interrupt counters
/// advance only through real [`record_interrupt`] calls.
///
/// (Previously this seeded four fictional devices — "nvme0" (8 MSI-X
/// vectors, 50M interrupts), "eth0" (4 MSI-X vectors, 100M interrupts),
/// "gpu0" (1 MSI vector, 5M interrupts), and "ahci0" (1 MSI vector, 10M
/// interrupts) — plus totals of 14 vectors, 165M interrupts, 100 allocs,
/// and 86 frees.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        total_vectors: 0,
        total_interrupts: 0,
        alloc_count: 0,
        free_count: 0,
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live MSI-vector table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom devices, zero totals.
    assert_eq!(per_device().len(), 0);
    let (d0, v0, i0, a0, f0, _) = stats();
    assert_eq!((d0, v0, i0, a0, f0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Alloc — device appears with its vectors; totals advance; duplicate is
    //    AlreadyExists.
    alloc_vectors("test_dev", MsiType::MsiX, 4, 0).expect("alloc");
    let devs = per_device();
    assert_eq!(devs.len(), 1);
    let d = devs.iter().find(|d| d.device == "test_dev").expect("find");
    assert_eq!((d.vectors_allocated, d.vectors_active, d.interrupts, d.target_cpu), (4, 4, 0, 0));
    assert_eq!(d.msi_type, MsiType::MsiX);
    let (_, vecs, _, allocs, _, _) = stats();
    assert_eq!((vecs, allocs), (4, 1));
    assert!(alloc_vectors("test_dev", MsiType::Msi, 1, 0).is_err());
    crate::serial_println!("  [2/8] alloc: OK");

    // 3: Interrupt — per-device and global interrupt counters advance.
    record_interrupt("test_dev").expect("interrupt");
    let d = per_device().into_iter().find(|d| d.device == "test_dev").expect("p3");
    assert_eq!(d.interrupts, 1);
    assert_eq!(stats().2, 1); // total_interrupts
    crate::serial_println!("  [3/8] interrupt: OK");

    // 4: Target CPU — retargeting updates the device's affinity.
    set_target_cpu("test_dev", 3).expect("target");
    let d = per_device().into_iter().find(|d| d.device == "test_dev").expect("p4");
    assert_eq!(d.target_cpu, 3);
    crate::serial_println!("  [4/8] target cpu: OK");

    // 5: Free — device disappears; total_vectors drops; free_count advances;
    //    double free is NotFound.
    free_vectors("test_dev").expect("free");
    assert_eq!(per_device().len(), 0);
    let (_, vecs, _, _, frees, _) = stats();
    assert_eq!((vecs, frees), (0, 1));
    assert!(free_vectors("test_dev").is_err());
    crate::serial_println!("  [5/8] free: OK");

    // 6: Vector count back to zero after the device is freed.
    assert_eq!(stats().1, 0);
    crate::serial_println!("  [6/8] vector count: OK");

    // 7: Not found — interrupt/retarget/free on an unknown device all error.
    assert!(record_interrupt("nonexist").is_err());
    assert!(set_target_cpu("nonexist", 0).is_err());
    assert!(free_vectors("nonexist").is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above. total_interrupts is
    //    cumulative (not decremented on free): 1 interrupt, 1 alloc, 1 free.
    let (devs, vecs, ints, allocs, frees, ops) = stats();
    assert_eq!((devs, vecs, ints, allocs, frees), (0, 0, 1, 1, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("msivec::self_test() — all 8 tests passed");
}
