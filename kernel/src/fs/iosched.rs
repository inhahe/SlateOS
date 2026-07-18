//! I/O Scheduler — block I/O scheduling configuration.
//!
//! Manages per-device I/O scheduling policies, queue depths,
//! priority boosting, and I/O merging settings. Supports multiple
//! scheduler algorithms.
//!
//! ## Architecture
//!
//! ```text
//! I/O scheduling
//!   → iosched::set_scheduler(dev, algo) → configure scheduler
//!   → iosched::get_scheduler(dev) → current algorithm
//!   → iosched::set_queue_depth(dev, depth) → set queue depth
//!   → iosched::stats() → global statistics
//!
//! Integration:
//!   → diskio (disk I/O stats)
//!   → diskhealth (disk health)
//!   → schedtune (scheduler tuning)
//!   → perfmon (performance monitor)
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

/// I/O scheduling algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoAlgorithm {
    None,        // No scheduling (direct dispatch).
    Fifo,        // First-in-first-out.
    Deadline,    // Deadline scheduler (read/write deadlines).
    Bfq,         // Budget Fair Queueing.
    Kyber,       // Kyber multiqueue scheduler.
    Mq,          // Simple multiqueue.
}

impl IoAlgorithm {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fifo => "fifo",
            Self::Deadline => "deadline",
            Self::Bfq => "bfq",
            Self::Kyber => "kyber",
            Self::Mq => "mq-deadline",
        }
    }
}

/// I/O priority class for BFQ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoPrioClass {
    RealTime,
    BestEffort,
    Idle,
}

impl IoPrioClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::RealTime => "RT",
            Self::BestEffort => "BE",
            Self::Idle => "Idle",
        }
    }
}

/// Per-device I/O scheduler configuration.
#[derive(Debug, Clone)]
pub struct DeviceScheduler {
    pub device_name: String,
    pub algorithm: IoAlgorithm,
    pub queue_depth: u32,
    pub read_expire_ms: u32,
    pub write_expire_ms: u32,
    pub merge_enabled: bool,
    pub front_merge: bool,
    pub nr_requests: u32,
    pub total_dispatched: u64,
    pub total_merged: u64,
    pub total_requeued: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 32;

struct State {
    devices: Vec<DeviceScheduler>,
    default_algo: IoAlgorithm,
    total_dispatched: u64,
    total_merged: u64,
    total_requeued: u64,
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
            DeviceScheduler {
                device_name: String::from("sda"),
                algorithm: IoAlgorithm::Bfq,
                queue_depth: 128, read_expire_ms: 500,
                write_expire_ms: 5000, merge_enabled: true,
                front_merge: true, nr_requests: 256,
                total_dispatched: 0, total_merged: 0, total_requeued: 0,
            },
            DeviceScheduler {
                device_name: String::from("nvme0n1"),
                algorithm: IoAlgorithm::Kyber,
                queue_depth: 1024, read_expire_ms: 250,
                write_expire_ms: 1000, merge_enabled: true,
                front_merge: false, nr_requests: 1024,
                total_dispatched: 0, total_merged: 0, total_requeued: 0,
            },
        ],
        default_algo: IoAlgorithm::Bfq,
        total_dispatched: 0,
        total_merged: 0,
        total_requeued: 0,
        ops: 0,
    });
}

/// List all device scheduler configs.
pub fn list_devices() -> Vec<DeviceScheduler> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get scheduler config for a device.
pub fn get_device(name: &str) -> Option<DeviceScheduler> {
    STATE.lock().as_ref().and_then(|s| s.devices.iter().find(|d| d.device_name == name).cloned())
}

/// Set I/O algorithm for a device.
pub fn set_scheduler(device: &str, algo: IoAlgorithm) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        dev.algorithm = algo;
        Ok(())
    })
}

/// Set queue depth for a device.
pub fn set_queue_depth(device: &str, depth: u32) -> KernelResult<()> {
    with_state(|state| {
        if depth == 0 || depth > 65536 {
            return Err(KernelError::InvalidArgument);
        }
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        dev.queue_depth = depth;
        Ok(())
    })
}

/// Set read/write expiry times.
pub fn set_expiry(device: &str, read_ms: u32, write_ms: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        dev.read_expire_ms = read_ms;
        dev.write_expire_ms = write_ms;
        Ok(())
    })
}

/// Enable/disable merge.
pub fn set_merge(device: &str, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        dev.merge_enabled = enabled;
        Ok(())
    })
}

/// Add a new device.
pub fn add_device(name: &str, algo: IoAlgorithm) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.devices.iter().any(|d| d.device_name == name) {
            return Err(KernelError::AlreadyExists);
        }
        state.devices.push(DeviceScheduler {
            device_name: String::from(name), algorithm: algo,
            queue_depth: 128, read_expire_ms: 500,
            write_expire_ms: 5000, merge_enabled: true,
            front_merge: true, nr_requests: 256,
            total_dispatched: 0, total_merged: 0, total_requeued: 0,
        });
        Ok(())
    })
}

/// Simulate dispatching I/O.
pub fn dispatch(device: &str, merged: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device_name == device)
            .ok_or(KernelError::NotFound)?;
        dev.total_dispatched += 1;
        state.total_dispatched += 1;
        if merged {
            dev.total_merged += 1;
            state.total_merged += 1;
        }
        Ok(())
    })
}

/// Set default algorithm.
pub fn set_default(algo: IoAlgorithm) -> KernelResult<()> {
    with_state(|state| {
        state.default_algo = algo;
        Ok(())
    })
}

/// Get default algorithm.
pub fn get_default() -> IoAlgorithm {
    STATE.lock().as_ref().map_or(IoAlgorithm::Bfq, |s| s.default_algo)
}

/// Statistics: (device_count, total_dispatched, total_merged, total_requeued, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_dispatched, s.total_merged, s.total_requeued, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("iosched::self_test() — running tests...");
    init_defaults();

    // 1: Default devices.
    assert_eq!(list_devices().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get device.
    let dev = get_device("sda").expect("get");
    assert_eq!(dev.algorithm, IoAlgorithm::Bfq);
    assert_eq!(dev.queue_depth, 128);
    crate::serial_println!("  [2/8] get: OK");

    // 3: Set scheduler.
    set_scheduler("sda", IoAlgorithm::Deadline).expect("set");
    let dev = get_device("sda").expect("get2");
    assert_eq!(dev.algorithm, IoAlgorithm::Deadline);
    crate::serial_println!("  [3/8] set scheduler: OK");

    // 4: Queue depth.
    set_queue_depth("nvme0n1", 512).expect("depth");
    let dev = get_device("nvme0n1").expect("get3");
    assert_eq!(dev.queue_depth, 512);
    assert!(set_queue_depth("nvme0n1", 0).is_err());
    crate::serial_println!("  [4/8] queue depth: OK");

    // 5: Add device.
    add_device("vdb", IoAlgorithm::Fifo).expect("add");
    assert_eq!(list_devices().len(), 3);
    assert!(add_device("vdb", IoAlgorithm::None).is_err());
    crate::serial_println!("  [5/8] add device: OK");

    // 6: Dispatch.
    dispatch("sda", false).expect("dispatch1");
    dispatch("sda", true).expect("dispatch2");
    let dev = get_device("sda").expect("get4");
    assert_eq!(dev.total_dispatched, 2);
    assert_eq!(dev.total_merged, 1);
    crate::serial_println!("  [6/8] dispatch: OK");

    // 7: Merge toggle.
    set_merge("vdb", false).expect("merge");
    let dev = get_device("vdb").expect("get5");
    assert!(!dev.merge_enabled);
    crate::serial_println!("  [7/8] merge: OK");

    // 8: Stats.
    let (count, dispatched, merged, requeued, ops) = stats();
    assert_eq!(count, 3);
    assert_eq!(dispatched, 2);
    assert_eq!(merged, 1);
    let _ = requeued;
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("iosched::self_test() — all 8 tests passed");
}
