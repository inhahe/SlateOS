//! Block Trace — block I/O tracing and replay.
//!
//! Records block I/O operations (reads, writes, discards) per device
//! with timestamps, sector ranges, and latencies. Supports filtering
//! and trace export for analysis.
//!
//! ## Architecture
//!
//! ```text
//! Block tracing
//!   → blktrace::start(device) → start tracing a device
//!   → blktrace::stop(device) → stop tracing
//!   → blktrace::record(event) → record I/O event
//!   → blktrace::dump(device) → dump trace events
//!
//! Integration:
//!   → diskio (disk I/O stats)
//!   → iosched (I/O scheduler)
//!   → perfmon (performance monitor)
//!   → sysdiag (diagnostics)
//! ```

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Block I/O operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlkOp {
    Read,
    Write,
    Discard,
    Flush,
    Fua, // Force Unit Access.
}

impl BlkOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "R",
            Self::Write => "W",
            Self::Discard => "D",
            Self::Flush => "F",
            Self::Fua => "FUA",
        }
    }
}

/// A single block trace event.
#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub seq: u64,
    pub timestamp_ns: u64,
    pub device: String,
    pub op: BlkOp,
    pub sector: u64,
    pub size_bytes: u32,
    pub latency_us: u64,
    pub pid: u32,
    pub process_name: String,
}

/// Per-device trace state.
#[derive(Debug, Clone)]
pub struct DeviceTrace {
    pub device: String,
    pub active: bool,
    pub events: Vec<TraceEvent>,
    pub total_events: u64,
    pub total_bytes: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 16;
const MAX_EVENTS_PER_DEVICE: usize = 4096;

struct State {
    traces: Vec<DeviceTrace>,
    next_seq: u64,
    total_events: u64,
    total_bytes: u64,
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
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        traces: Vec::new(),
        next_seq: 1,
        total_events: 0,
        total_bytes: 0,
        ops: 0,
    });
}

/// Start tracing a device.
pub fn start(device: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(t) = state.traces.iter_mut().find(|t| t.device == device) {
            if t.active {
                return Err(KernelError::AlreadyExists);
            }
            t.active = true;
            return Ok(());
        }
        if state.traces.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        state.traces.push(DeviceTrace {
            device: String::from(device),
            active: true,
            events: Vec::new(),
            total_events: 0,
            total_bytes: 0,
        });
        Ok(())
    })
}

/// Stop tracing a device.
pub fn stop(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let trace = state
            .traces
            .iter_mut()
            .find(|t| t.device == device)
            .ok_or(KernelError::NotFound)?;
        if !trace.active {
            return Err(KernelError::InvalidArgument);
        }
        trace.active = false;
        Ok(())
    })
}

/// Record a trace event.
pub fn record(
    device: &str,
    op: BlkOp,
    sector: u64,
    size: u32,
    latency_us: u64,
    pid: u32,
    process: &str,
) -> KernelResult<u64> {
    with_state(|state| {
        let trace = state
            .traces
            .iter_mut()
            .find(|t| t.device == device)
            .ok_or(KernelError::NotFound)?;
        if !trace.active {
            return Err(KernelError::PermissionDenied);
        }
        let now = crate::hpet::elapsed_ns();
        let seq = state.next_seq;
        state.next_seq += 1;
        if trace.events.len() >= MAX_EVENTS_PER_DEVICE {
            trace.events.remove(0);
        }
        trace.events.push(TraceEvent {
            seq,
            timestamp_ns: now,
            device: String::from(device),
            op,
            sector,
            size_bytes: size,
            latency_us,
            pid,
            process_name: String::from(process),
        });
        trace.total_events += 1;
        trace.total_bytes += size as u64;
        state.total_events += 1;
        state.total_bytes += size as u64;
        Ok(seq)
    })
}

/// Dump trace events for a device.
pub fn dump(device: &str) -> Vec<TraceEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.traces
            .iter()
            .find(|t| t.device == device)
            .map_or(Vec::new(), |t| t.events.clone())
    })
}

/// List all traced devices.
pub fn list_devices() -> Vec<DeviceTrace> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.traces
            .iter()
            .map(|t| DeviceTrace {
                device: t.device.clone(),
                active: t.active,
                events: Vec::new(), // Don't clone events for listing.
                total_events: t.total_events,
                total_bytes: t.total_bytes,
            })
            .collect()
    })
}

/// Clear trace for a device.
pub fn clear(device: &str) -> KernelResult<u64> {
    with_state(|state| {
        let trace = state
            .traces
            .iter_mut()
            .find(|t| t.device == device)
            .ok_or(KernelError::NotFound)?;
        let count = trace.events.len() as u64;
        trace.events.clear();
        Ok(count)
    })
}

/// Filter events by operation type.
pub fn filter_op(device: &str, op: BlkOp) -> Vec<TraceEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.traces
            .iter()
            .find(|t| t.device == device)
            .map_or(Vec::new(), |t| {
                t.events.iter().filter(|e| e.op == op).cloned().collect()
            })
    })
}

/// Statistics: (device_count, total_events, total_bytes, active_count, ops).
pub fn stats() -> (usize, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let active = s.traces.iter().filter(|t| t.active).count();
            (s.traces.len(), s.total_events, s.total_bytes, active, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
    crate::serial_println!("blktrace::self_test() — running tests...");
    init_defaults();

    // 1: Empty.
    assert!(list_devices().is_empty());
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Start trace.
    start("sda").expect("start");
    assert_eq!(list_devices().len(), 1);
    assert!(list_devices()[0].active);
    crate::serial_println!("  [2/8] start: OK");

    // 3: Record events.
    record("sda", BlkOp::Read, 0, 4096, 100, 1, "init").expect("r1");
    record("sda", BlkOp::Write, 8, 8192, 250, 2, "journal").expect("r2");
    record("sda", BlkOp::Read, 16, 4096, 80, 1, "init").expect("r3");
    let events = dump("sda");
    assert_eq!(events.len(), 3);
    crate::serial_println!("  [3/8] record: OK");

    // 4: Filter.
    let reads = filter_op("sda", BlkOp::Read);
    assert_eq!(reads.len(), 2);
    let writes = filter_op("sda", BlkOp::Write);
    assert_eq!(writes.len(), 1);
    crate::serial_println!("  [4/8] filter: OK");

    // 5: Stop trace.
    stop("sda").expect("stop");
    assert!(record("sda", BlkOp::Read, 0, 4096, 100, 1, "x").is_err());
    crate::serial_println!("  [5/8] stop: OK");

    // 6: Restart.
    start("sda").expect("restart");
    record("sda", BlkOp::Discard, 100, 1024, 50, 3, "trim").expect("r4");
    assert_eq!(dump("sda").len(), 4);
    crate::serial_println!("  [6/8] restart: OK");

    // 7: Clear.
    let cleared = clear("sda").expect("clear");
    assert_eq!(cleared, 4);
    assert!(dump("sda").is_empty());
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats.
    let (devs, events_total, bytes, active, ops) = stats();
    assert_eq!(devs, 1);
    assert_eq!(events_total, 4);
    assert!(bytes > 0);
    assert_eq!(active, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("blktrace::self_test() — all 8 tests passed");
}
