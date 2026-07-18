//! Trace Monitor — kernel tracing and profiling subsystem.
//!
//! Provides ftrace-style tracing for system calls, process events,
//! file access, and scheduling decisions. Supports multiple trace
//! points, per-CPU buffers, and filtering by process or subsystem.
//!
//! ## Architecture
//!
//! ```text
//! Trace monitoring
//!   → tracemon::enable(tracepoint) → enable a tracepoint
//!   → tracemon::disable(tracepoint) → disable a tracepoint
//!   → tracemon::read_buffer() → read trace events
//!   → tracemon::filter(pid) → filter by process
//!
//! Integration:
//!   → perfmon (performance monitor)
//!   → taskmon (task monitor)
//!   → sysprofiler (system profiler)
//!   → blktrace (block I/O trace)
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

/// Tracepoint category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceCategory {
    Syscall,
    Sched,
    Irq,
    FileIO,
    Memory,
    Network,
    Ipc,
    Custom,
}

impl TraceCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Syscall => "syscall",
            Self::Sched => "sched",
            Self::Irq => "irq",
            Self::FileIO => "fileio",
            Self::Memory => "memory",
            Self::Network => "network",
            Self::Ipc => "ipc",
            Self::Custom => "custom",
        }
    }
}

/// A tracepoint definition.
#[derive(Debug, Clone)]
pub struct Tracepoint {
    pub id: u32,
    pub name: String,
    pub category: TraceCategory,
    pub enabled: bool,
    pub hit_count: u64,
    pub description: String,
}

/// A trace event recorded when a tracepoint fires.
#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub tracepoint_id: u32,
    pub timestamp_ns: u64,
    pub cpu: u32,
    pub pid: u32,
    pub data: String,
}

/// Trace buffer mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferMode {
    Overwrite,   // Ring buffer, overwrites oldest.
    OneShot,     // Stop when full.
}

impl BufferMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Overwrite => "overwrite",
            Self::OneShot => "one-shot",
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TRACEPOINTS: usize = 256;
const MAX_EVENTS: usize = 4096;

struct State {
    tracepoints: Vec<Tracepoint>,
    events: Vec<TraceEvent>,
    next_tp_id: u32,
    buffer_mode: BufferMode,
    filter_pid: Option<u32>,
    global_enabled: bool,
    total_events: u64,
    total_dropped: u64,
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
        tracepoints: alloc::vec![
            Tracepoint {
                id: 1, name: String::from("sys_enter"), category: TraceCategory::Syscall,
                enabled: false, hit_count: 0, description: String::from("Syscall entry"),
            },
            Tracepoint {
                id: 2, name: String::from("sys_exit"), category: TraceCategory::Syscall,
                enabled: false, hit_count: 0, description: String::from("Syscall exit"),
            },
            Tracepoint {
                id: 3, name: String::from("sched_switch"), category: TraceCategory::Sched,
                enabled: false, hit_count: 0, description: String::from("Context switch"),
            },
            Tracepoint {
                id: 4, name: String::from("irq_handler"), category: TraceCategory::Irq,
                enabled: false, hit_count: 0, description: String::from("IRQ handler entry"),
            },
            Tracepoint {
                id: 5, name: String::from("vfs_read"), category: TraceCategory::FileIO,
                enabled: false, hit_count: 0, description: String::from("VFS read"),
            },
            Tracepoint {
                id: 6, name: String::from("page_fault"), category: TraceCategory::Memory,
                enabled: false, hit_count: 0, description: String::from("Page fault handler"),
            },
        ],
        events: Vec::new(),
        next_tp_id: 7,
        buffer_mode: BufferMode::Overwrite,
        filter_pid: None,
        global_enabled: false,
        total_events: 0,
        total_dropped: 0,
        ops: 0,
    });
}

/// Register a custom tracepoint.
pub fn register_tracepoint(name: &str, category: TraceCategory, desc: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.tracepoints.len() >= MAX_TRACEPOINTS { return Err(KernelError::ResourceExhausted); }
        if state.tracepoints.iter().any(|t| t.name == name) { return Err(KernelError::AlreadyExists); }
        let id = state.next_tp_id;
        state.next_tp_id += 1;
        state.tracepoints.push(Tracepoint {
            id, name: String::from(name), category, enabled: false,
            hit_count: 0, description: String::from(desc),
        });
        Ok(id)
    })
}

/// Enable a tracepoint by name.
pub fn enable(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let tp = state.tracepoints.iter_mut().find(|t| t.name == name)
            .ok_or(KernelError::NotFound)?;
        tp.enabled = true;
        Ok(())
    })
}

/// Disable a tracepoint by name.
pub fn disable(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let tp = state.tracepoints.iter_mut().find(|t| t.name == name)
            .ok_or(KernelError::NotFound)?;
        tp.enabled = false;
        Ok(())
    })
}

/// Enable/disable all tracepoints globally.
pub fn set_global(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// Record a trace event.
pub fn record(tracepoint_name: &str, cpu: u32, pid: u32, data: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.global_enabled { return Ok(()); }
        let tp = state.tracepoints.iter_mut().find(|t| t.name == tracepoint_name)
            .ok_or(KernelError::NotFound)?;
        if !tp.enabled { return Ok(()); }
        // Apply PID filter.
        if let Some(filter_pid) = state.filter_pid {
            if pid != filter_pid { return Ok(()); }
        }
        tp.hit_count += 1;
        let tp_id = tp.id;
        if state.events.len() >= MAX_EVENTS {
            match state.buffer_mode {
                BufferMode::Overwrite => { state.events.remove(0); }
                BufferMode::OneShot => { state.total_dropped += 1; return Ok(()); }
            }
        }
        let now = crate::hpet::elapsed_ns();
        state.events.push(TraceEvent {
            tracepoint_id: tp_id, timestamp_ns: now, cpu, pid,
            data: String::from(data),
        });
        state.total_events += 1;
        Ok(())
    })
}

/// Read trace buffer.
pub fn read_buffer(last_n: usize) -> Vec<TraceEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if last_n >= s.events.len() { 0 } else { s.events.len() - last_n };
        s.events[start..].to_vec()
    })
}

/// Clear trace buffer.
pub fn clear_buffer() -> KernelResult<()> {
    with_state(|state| {
        state.events.clear();
        Ok(())
    })
}

/// Set buffer mode.
pub fn set_buffer_mode(mode: BufferMode) -> KernelResult<()> {
    with_state(|state| { state.buffer_mode = mode; Ok(()) })
}

/// Set PID filter (None = no filter).
pub fn set_filter_pid(pid: Option<u32>) -> KernelResult<()> {
    with_state(|state| { state.filter_pid = pid; Ok(()) })
}

/// List tracepoints.
pub fn list_tracepoints() -> Vec<Tracepoint> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.tracepoints.clone())
}

/// Get tracepoint by name.
pub fn get_tracepoint(name: &str) -> Option<Tracepoint> {
    STATE.lock().as_ref().and_then(|s| s.tracepoints.iter().find(|t| t.name == name).cloned())
}

/// Statistics: (tracepoint_count, event_count, total_events, total_dropped, global_enabled, ops).
pub fn stats() -> (usize, usize, u64, u64, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tracepoints.len(), s.events.len(), s.total_events, s.total_dropped, s.global_enabled, s.ops),
        None => (0, 0, 0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("tracemon::self_test() — running tests...");
    // Start from a clean slate so the mutations below (enabling global tracing,
    // registering a custom_probe, switching to OneShot buffer mode, recording
    // events) can never leak into the live /proc/tracemon view.  tracemon is not
    // boot-wired — the kshell commands lazily init_defaults() on first use — so
    // the natural state is uninitialised; `tracemon test` must leave it that way
    // rather than permanently arming global tracing and a phantom tracepoint.
    *STATE.lock() = None;
    init_defaults();

    // 1: Defaults — six standard tracepoints, all disabled, zero hits.
    assert_eq!(list_tracepoints().len(), 6);
    let (_, _, _, _, enabled, _) = stats();
    assert!(!enabled);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Enable/disable.
    enable("sys_enter").expect("enable");
    let tp = get_tracepoint("sys_enter").expect("get");
    assert!(tp.enabled);
    disable("sys_enter").expect("disable");
    let tp = get_tracepoint("sys_enter").expect("get2");
    assert!(!tp.enabled);
    crate::serial_println!("  [2/8] enable/disable: OK");

    // 3: Global enable + record.
    set_global(true).expect("global");
    enable("sched_switch").expect("enable2");
    record("sched_switch", 0, 1, "prev=1 next=2").expect("record");
    let events = read_buffer(10);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].pid, 1);
    crate::serial_println!("  [3/8] record: OK");

    // 4: PID filter.
    set_filter_pid(Some(42)).expect("filter");
    record("sched_switch", 0, 1, "filtered out").expect("record2");
    assert_eq!(read_buffer(10).len(), 1); // Still 1, filtered.
    record("sched_switch", 0, 42, "passes filter").expect("record3");
    assert_eq!(read_buffer(10).len(), 2);
    set_filter_pid(None).expect("unfilter");
    crate::serial_println!("  [4/8] filter: OK");

    // 5: Register custom.
    let id = register_tracepoint("custom_probe", TraceCategory::Custom, "Test probe").expect("reg");
    assert!(id >= 7);
    assert!(register_tracepoint("custom_probe", TraceCategory::Custom, "dup").is_err());
    crate::serial_println!("  [5/8] register: OK");

    // 6: Buffer mode.
    set_buffer_mode(BufferMode::OneShot).expect("mode");
    crate::serial_println!("  [6/8] buffer mode: OK");

    // 7: Clear.
    clear_buffer().expect("clear");
    assert_eq!(read_buffer(10).len(), 0);
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats — exact totals: 7 tracepoints (6 default + custom_probe), buffer
    // cleared, exactly 2 events recorded (pid-1 and pid-42; the filtered pid-1
    // record returned before counting), nothing dropped, global tracing on.
    let (tp_count, ev_count, total, dropped, global, ops) = stats();
    assert_eq!(tp_count, 7);
    assert_eq!(ev_count, 0); // Cleared.
    assert_eq!(total, 2);
    assert_eq!(dropped, 0);
    assert!(global);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Reset so the test leaves no fixtures (custom_probe, global-enabled,
    // OneShot mode) behind in the live /proc/tracemon registry.
    *STATE.lock() = None;

    crate::serial_println!("tracemon::self_test() — all 8 tests passed");
}
