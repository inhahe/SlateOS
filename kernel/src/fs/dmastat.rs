//! DMA Statistics — Direct Memory Access and IOMMU monitoring.
//!
//! Tracks DMA operations, IOMMU translations, device memory
//! mappings, and security faults. Essential for diagnosing
//! device I/O performance and IOMMU protection.
//!
//! ## Architecture
//!
//! ```text
//! DMA statistics
//!   → dmastat::record_map(dev, addr, size) → track DMA mapping
//!   → dmastat::record_unmap(dev) → track DMA unmap
//!   → dmastat::record_fault(dev) → track IOMMU fault
//!   → dmastat::device_stats() → per-device DMA state
//!
//! Integration:
//!   → blktrace (block I/O tracing)
//!   → tlbstat (TLB statistics)
//!   → perfmon (performance monitor)
//!   → dmevent (device events)
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

/// DMA direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    ToDevice,
    FromDevice,
    Bidirectional,
    None,
}

impl DmaDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::ToDevice => "to_dev",
            Self::FromDevice => "from_dev",
            Self::Bidirectional => "bidir",
            Self::None => "none",
        }
    }
}

/// IOMMU fault type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuFaultType {
    ReadFault,
    WriteFault,
    DeviceAcs,
    TranslationFault,
    PermissionFault,
}

impl IommuFaultType {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadFault => "read",
            Self::WriteFault => "write",
            Self::DeviceAcs => "acs",
            Self::TranslationFault => "translation",
            Self::PermissionFault => "permission",
        }
    }
}

/// Per-device DMA statistics.
#[derive(Debug, Clone)]
pub struct DeviceDmaStats {
    pub device_id: u32,
    pub name: String,
    pub maps: u64,
    pub unmaps: u64,
    pub bytes_mapped: u64,
    pub bytes_transferred: u64,
    pub faults: u64,
    pub active_mappings: u32,
    pub iommu_enabled: bool,
}

/// An IOMMU fault record.
#[derive(Debug, Clone)]
pub struct IommuFault {
    pub device_id: u32,
    pub fault_type: IommuFaultType,
    pub address: u64,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 64;
const MAX_FAULTS: usize = 256;

struct State {
    devices: Vec<DeviceDmaStats>,
    fault_log: Vec<IommuFault>,
    total_maps: u64,
    total_unmaps: u64,
    total_bytes: u64,
    total_faults: u64,
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

/// Initialise an **empty** DMA statistics table.
///
/// Seeds NO devices, NO fault log, and zero totals.  Real DMA/IOMMU accounting
/// is wired through [`register_device`] (one zeroed row per device the driver
/// framework brings up) and the `record_map`/`record_unmap`/`record_transfer`/
/// `record_fault` functions; until those are called the table is genuinely
/// empty, so the `/proc/dmastat` file and the `dmastat` kshell command report
/// zeros rather than fabricated numbers — the kernel's hard "never invent data
/// in procfs" rule.
///
/// NOTE: this previously seeded three fictional devices ("nvme0" maps 500000 /
/// 2GB mapped / 100GB transferred; "eth0" maps 200000 / 512MB mapped / 20GB
/// transferred; "gpu0" maps 100000 / 4GB mapped / 40GB transferred / 1 fault)
/// plus invented aggregate totals (total_maps 800000, total_unmaps 793500,
/// total_bytes 171_798_691_840, total_faults 1), which `/proc/dmastat` then
/// displayed as if they were real per-device DMA measurements.  That demo data
/// was removed; the self-test now builds its own fixtures explicitly via the
/// real API (see [`self_test`]).  The driver framework is expected to call
/// [`register_device`] when a DMA-capable device is brought up and the record_*
/// functions as mappings and transfers occur.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        fault_log: Vec::new(),
        total_maps: 0,
        total_unmaps: 0,
        total_bytes: 0,
        total_faults: 0,
        ops: 0,
    });
}

/// Record a DMA mapping.
pub fn record_map(device_id: u32, size: u64, _direction: DmaDirection) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.maps += 1;
        dev.bytes_mapped += size;
        dev.active_mappings += 1;
        state.total_maps += 1;
        state.total_bytes += size;
        Ok(())
    })
}

/// Record a DMA unmap.
pub fn record_unmap(device_id: u32, _size: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.unmaps += 1;
        dev.active_mappings = dev.active_mappings.saturating_sub(1);
        state.total_unmaps += 1;
        Ok(())
    })
}

/// Record a DMA transfer completion.
pub fn record_transfer(device_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.bytes_transferred += bytes;
        Ok(())
    })
}

/// Record an IOMMU fault.
pub fn record_fault(device_id: u32, fault_type: IommuFaultType, address: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(dev) = state.devices.iter_mut().find(|d| d.device_id == device_id) {
            dev.faults += 1;
        }
        state.total_faults += 1;
        if state.fault_log.len() >= MAX_FAULTS { state.fault_log.remove(0); }
        state.fault_log.push(IommuFault {
            device_id, fault_type, address, timestamp_ns: now,
        });
        Ok(())
    })
}

/// Register a device for DMA tracking.
pub fn register_device(device_id: u32, name: &str, iommu: bool) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES { return Err(KernelError::ResourceExhausted); }
        if state.devices.iter().any(|d| d.device_id == device_id) { return Err(KernelError::AlreadyExists); }
        state.devices.push(DeviceDmaStats {
            device_id, name: String::from(name), maps: 0, unmaps: 0,
            bytes_mapped: 0, bytes_transferred: 0, faults: 0,
            active_mappings: 0, iommu_enabled: iommu,
        });
        Ok(())
    })
}

/// Get per-device statistics.
pub fn device_stats() -> Vec<DeviceDmaStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Recent IOMMU faults.
pub fn fault_log(n: usize) -> Vec<IommuFault> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if n >= s.fault_log.len() { 0 } else { s.fault_log.len() - n };
        s.fault_log[start..].to_vec()
    })
}

/// Statistics: (device_count, total_maps, total_unmaps, total_bytes, total_faults, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_maps, s.total_unmaps, s.total_bytes, s.total_faults, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("dmastat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/dmastat must never surface).
    // Resetting first clears any residue from a prior `dmastat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated devices, faults, or totals.
    assert_eq!(device_stats().len(), 0);
    assert_eq!(fault_log(5).len(), 0);
    let (d0, m0, u0, b0, f0, _o0) = stats();
    assert_eq!((d0, m0, u0, b0, f0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register devices (zeroed); duplicate id fails.
    register_device(1, "dev1", true).expect("reg1");
    register_device(2, "dev2", false).expect("reg2");
    assert!(register_device(1, "dup", true).is_err());
    assert_eq!(device_stats().len(), 2);
    let d = device_stats().iter().find(|d| d.device_id == 1).cloned().expect("dev");
    assert_eq!(d.maps, 0);
    assert!(d.iommu_enabled);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Map increments count + bytes + active mappings exactly from zero.
    record_map(1, 4096, DmaDirection::ToDevice).expect("map");
    let d = device_stats().iter().find(|d| d.device_id == 1).cloned().expect("dev");
    assert_eq!(d.maps, 1);
    assert_eq!(d.bytes_mapped, 4096);
    assert_eq!(d.active_mappings, 1);
    crate::serial_println!("  [3/8] map: OK");

    // 4: Unmap increments unmaps and drops active mappings back to zero.
    record_unmap(1, 4096).expect("unmap");
    let d = device_stats().iter().find(|d| d.device_id == 1).cloned().expect("dev");
    assert_eq!(d.unmaps, 1);
    assert_eq!(d.active_mappings, 0);
    crate::serial_println!("  [4/8] unmap: OK");

    // 5: Transfer accumulates bytes exactly from zero.
    record_transfer(2, 65536).expect("transfer");
    let d = device_stats().iter().find(|d| d.device_id == 2).cloned().expect("dev");
    assert_eq!(d.bytes_transferred, 65536);
    crate::serial_println!("  [5/8] transfer: OK");

    // 6: IOMMU fault logs an entry and increments the device + total counters.
    record_fault(1, IommuFaultType::WriteFault, 0xDEAD_0000).expect("fault");
    let faults = fault_log(5);
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].device_id, 1);
    let d = device_stats().iter().find(|d| d.device_id == 1).cloned().expect("dev");
    assert_eq!(d.faults, 1);
    crate::serial_println!("  [6/8] fault: OK");

    // 7: Operations on an unregistered device fail with NotFound.
    assert!(record_map(99, 4096, DmaDirection::None).is_err());
    assert!(record_transfer(99, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (devs, maps, unmaps, bytes, faults, ops) = stats();
    assert_eq!(devs, 2);
    assert_eq!(maps, 1);     // one record_map
    assert_eq!(unmaps, 1);   // one record_unmap
    assert_eq!(bytes, 4096); // total_bytes tracks mapped bytes only
    assert_eq!(faults, 1);   // one record_fault
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/dmastat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the driver framework
    // wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("dmastat::self_test() — all 8 tests passed");
}
