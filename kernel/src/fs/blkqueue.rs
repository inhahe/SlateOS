//! Block Queue — block device I/O request queue monitoring.
//!
//! Tracks per-device I/O queue depth, request merging,
//! plug/unplug events, and queue congestion. Essential
//! for diagnosing storage bottlenecks.
//!
//! ## Architecture
//!
//! ```text
//! Block queue monitoring
//!   → blkqueue::submit(device, op) → track I/O submission
//!   → blkqueue::complete(device, op) → track I/O completion
//!   → blkqueue::merge(device) → track request merge
//!   → blkqueue::plug/unplug(device) → track queue plugging
//!
//! Integration:
//!   → blktrace (block tracing)
//!   → iolatency (I/O latency)
//!   → iosched (I/O scheduler)
//!   → diskio (disk I/O stats)
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

/// Block I/O operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlkOp {
    Read,
    Write,
    Flush,
    Discard,
    SecureErase,
}

impl BlkOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Flush => "flush",
            Self::Discard => "discard",
            Self::SecureErase => "secure_erase",
        }
    }
}

/// Per-device queue statistics.
#[derive(Debug, Clone)]
pub struct DeviceQueue {
    pub device: String,
    pub queue_depth: u32,
    pub max_depth: u32,
    pub submitted: u64,
    pub completed: u64,
    pub merged: u64,
    pub plug_count: u64,
    pub unplug_count: u64,
    pub plugged: bool,
    pub congested: bool,
    pub read_submitted: u64,
    pub write_submitted: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 32;

struct State {
    devices: Vec<DeviceQueue>,
    total_submitted: u64,
    total_completed: u64,
    total_merged: u64,
    total_plugs: u64,
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
            DeviceQueue { device: String::from("sda"), queue_depth: 12, max_depth: 128, submitted: 10_000_000, completed: 9_999_900, merged: 2_000_000, plug_count: 500_000, unplug_count: 500_000, plugged: false, congested: false, read_submitted: 6_000_000, write_submitted: 4_000_000 },
            DeviceQueue { device: String::from("nvme0n1"), queue_depth: 64, max_depth: 1024, submitted: 50_000_000, completed: 49_999_500, merged: 10_000_000, plug_count: 2_000_000, unplug_count: 2_000_000, plugged: false, congested: false, read_submitted: 30_000_000, write_submitted: 20_000_000 },
        ],
        total_submitted: 60_000_000,
        total_completed: 59_999_400,
        total_merged: 12_000_000,
        total_plugs: 2_500_000,
        ops: 0,
    });
}

/// Submit an I/O request to a device queue.
pub fn submit(device: &str, op: BlkOp) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.queue_depth += 1;
        if dev.queue_depth > dev.max_depth {
            dev.max_depth = dev.queue_depth;
        }
        dev.submitted += 1;
        match op {
            BlkOp::Read => dev.read_submitted += 1,
            BlkOp::Write => dev.write_submitted += 1,
            _ => {}
        }
        state.total_submitted += 1;
        Ok(())
    })
}

/// Complete an I/O request.
pub fn complete(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.queue_depth = dev.queue_depth.saturating_sub(1);
        dev.completed += 1;
        state.total_completed += 1;
        Ok(())
    })
}

/// Record a request merge (two adjacent I/Os combined).
pub fn merge(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.merged += 1;
        state.total_merged += 1;
        Ok(())
    })
}

/// Plug the queue (batch requests before dispatch).
pub fn plug(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        if !dev.plugged {
            dev.plugged = true;
            dev.plug_count += 1;
            state.total_plugs += 1;
        }
        Ok(())
    })
}

/// Unplug the queue (dispatch batched requests).
pub fn unplug(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        if dev.plugged {
            dev.plugged = false;
            dev.unplug_count += 1;
        }
        Ok(())
    })
}

/// Get all device queue stats.
pub fn device_queues() -> Vec<DeviceQueue> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get queue for a specific device.
pub fn queue_for(device: &str) -> Option<DeviceQueue> {
    STATE.lock().as_ref().and_then(|s| {
        s.devices.iter().find(|d| d.device == device).cloned()
    })
}

/// Statistics: (device_count, total_submitted, total_completed, total_merged, total_plugs, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_submitted, s.total_completed, s.total_merged, s.total_plugs, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("blkqueue::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(device_queues().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Submit.
    let before = device_queues()[0].queue_depth;
    submit("sda", BlkOp::Read).expect("submit");
    let after = device_queues()[0].queue_depth;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] submit: OK");

    // 3: Complete.
    complete("sda").expect("complete");
    let after2 = device_queues()[0].queue_depth;
    assert_eq!(after2, before);
    crate::serial_println!("  [3/8] complete: OK");

    // 4: Merge.
    let before = device_queues()[0].merged;
    merge("sda").expect("merge");
    let after = device_queues()[0].merged;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] merge: OK");

    // 5: Plug/unplug.
    plug("sda").expect("plug");
    assert!(device_queues()[0].plugged);
    unplug("sda").expect("unplug");
    assert!(!device_queues()[0].plugged);
    crate::serial_println!("  [5/8] plug/unplug: OK");

    // 6: NVMe device.
    submit("nvme0n1", BlkOp::Write).expect("nvme_submit");
    complete("nvme0n1").expect("nvme_complete");
    let dev = queue_for("nvme0n1").expect("nvme_queue");
    assert!(dev.submitted > 50_000_000);
    crate::serial_println!("  [6/8] nvme: OK");

    // 7: Not found.
    assert!(submit("fake", BlkOp::Read).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (devs, submitted, completed, merged, plugs, ops) = stats();
    assert_eq!(devs, 2);
    assert!(submitted > 60_000_000);
    assert!(completed > 59_999_400);
    assert!(merged > 12_000_000);
    assert!(plugs > 2_500_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("blkqueue::self_test() — all 8 tests passed");
}
