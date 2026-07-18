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
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** block-queue table.
///
/// Seeds NO devices and zero counters.  Real queue accounting is wired through
/// [`register_device`] (one row per block device the I/O layer brings online)
/// and the `submit`/`complete`/`merge`/`plug`/`unplug` functions; until those
/// are called the table is genuinely empty, so `/proc/blkqueue` and the
/// `blkqueue` kshell command report zeros rather than fabricated numbers — the
/// kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional block devices ("sda": queue_depth
/// 12 / max 128 / 10M submitted / 9,999,900 completed / 2M merged / 500k plug /
/// 500k unplug / 6M read / 4M write; "nvme0n1": queue_depth 64 / max 1024 / 50M
/// submitted / 49,999,500 completed / 10M merged / 2M plug / 2M unplug / 30M
/// read / 20M write) plus invented aggregate totals (total_submitted 60M,
/// total_completed 59,999,400, total_merged 12M, total_plugs 2.5M), which
/// `/proc/blkqueue` (and the `device_queues`/`queue_for` views) then displayed as
/// if they were real measured block I/O traffic.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The block layer is expected to call [`register_device`] when a
/// device queue is brought online and the record functions on every I/O event.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        total_submitted: 0,
        total_completed: 0,
        total_merged: 0,
        total_plugs: 0,
        ops: 0,
    });
}

/// Register a block device queue.
///
/// Creates a zeroed [`DeviceQueue`] row with the supplied `max_depth` (the
/// hardware/driver queue limit).  Duplicate device names return
/// [`KernelError::AlreadyExists`]; exceeding [`MAX_DEVICES`] returns
/// [`KernelError::ResourceExhausted`].
pub fn register_device(device: &str, max_depth: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES { return Err(KernelError::ResourceExhausted); }
        if state.devices.iter().any(|d| d.device == device) { return Err(KernelError::AlreadyExists); }
        state.devices.push(DeviceQueue {
            device: String::from(device), queue_depth: 0, max_depth,
            submitted: 0, completed: 0, merged: 0, plug_count: 0, unplug_count: 0,
            plugged: false, congested: false, read_submitted: 0, write_submitted: 0,
        });
        Ok(())
    })
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
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/blkqueue must never surface).  Resetting
    // first clears any residue from a prior `blkqueue test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated devices or counters; record on an
    // unregistered device fails.
    assert_eq!(device_queues().len(), 0);
    let (c0, sub0, comp0, mrg0, plg0, _o0) = stats();
    assert_eq!((c0, sub0, comp0, mrg0, plg0), (0, 0, 0, 0, 0));
    assert!(submit("sda", BlkOp::Read).is_err()); // no phantom device exists yet
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register — zeroed counters, max_depth preserved; dup fails.
    register_device("sda", 128).expect("register sda");
    let d = queue_for("sda").expect("find sda");
    assert_eq!(d.max_depth, 128);
    assert_eq!((d.queue_depth, d.submitted, d.completed, d.merged), (0, 0, 0, 0));
    assert!(register_device("sda", 128).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Submit — queue_depth and submitted rise; Read tallies read_submitted.
    submit("sda", BlkOp::Read).expect("submit");
    let d = queue_for("sda").expect("find sda");
    assert_eq!(d.queue_depth, 1);
    assert_eq!(d.submitted, 1);
    assert_eq!(d.read_submitted, 1);
    assert_eq!(d.write_submitted, 0);
    crate::serial_println!("  [3/8] submit: OK");

    // 4: Complete — queue_depth drops back, completed rises; underflow saturates.
    complete("sda").expect("complete");
    let d = queue_for("sda").expect("find sda");
    assert_eq!(d.queue_depth, 0);
    assert_eq!(d.completed, 1);
    complete("sda").expect("complete underflow");
    assert_eq!(queue_for("sda").expect("find sda").queue_depth, 0); // saturating_sub
    crate::serial_println!("  [4/8] complete: OK");

    // 5: Merge — merged count and total rise by exactly one.
    merge("sda").expect("merge");
    assert_eq!(queue_for("sda").expect("find sda").merged, 1);
    crate::serial_println!("  [5/8] merge: OK");

    // 6: Plug/unplug — idempotent: double plug counts once, double unplug once.
    plug("sda").expect("plug");
    plug("sda").expect("plug again");
    let d = queue_for("sda").expect("find sda");
    assert!(d.plugged);
    assert_eq!(d.plug_count, 1); // second plug is a no-op while already plugged
    unplug("sda").expect("unplug");
    unplug("sda").expect("unplug again");
    let d = queue_for("sda").expect("find sda");
    assert!(!d.plugged);
    assert_eq!(d.unplug_count, 1);
    crate::serial_println!("  [6/8] plug/unplug: OK");

    // 7: Unknown device → NotFound on every record path.
    assert!(submit("fake", BlkOp::Read).is_err());
    assert!(complete("fake").is_err());
    assert!(merge("fake").is_err());
    assert!(plug("fake").is_err());
    assert!(unplug("fake").is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals are exact: 1 submit, 2 completes, 1 merge, 1 plug.
    let (devs, submitted, completed, merged, plugs, ops) = stats();
    assert_eq!(devs, 1);
    assert_eq!(submitted, 1);
    assert_eq!(completed, 2);
    assert_eq!(merged, 1);
    assert_eq!(plugs, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/blkqueue table.
    *STATE.lock() = None;

    crate::serial_println!("blkqueue::self_test() — all 8 tests passed");
}
