//! Function Trace Statistics — kernel function tracing monitoring.
//!
//! Tracks function entry/exit probes, trace events, per-function
//! hit counts, and tracing overhead. Essential for kernel
//! debugging and performance analysis.
//!
//! ## Architecture
//!
//! ```text
//! Function trace monitoring
//!   → ftrace::add_probe(func, kind) → register probe
//!   → ftrace::remove_probe(func) → unregister probe
//!   → ftrace::record_hit(func) → probe hit
//!   → ftrace::per_probe() → per-probe stats
//!
//! Integration:
//!   → kprobes (kprobe stats)
//!   → bpfstat (BPF program stats)
//!   → schedlat (scheduler latency)
//!   → tracemon (trace monitoring)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Probe kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    Function,    // Function entry
    ReturnProbe, // Function return
    TracePoint,  // Static tracepoint
    Dynamic,     // Dynamic probe
}

impl ProbeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Function => "func",
            Self::ReturnProbe => "ret",
            Self::TracePoint => "tp",
            Self::Dynamic => "dyn",
        }
    }
}

/// Per-probe stats.
#[derive(Debug, Clone)]
pub struct ProbeStats {
    pub func_name: String,
    pub kind: ProbeKind,
    pub hits: u64,
    pub misses: u64,
    pub total_ns: u64,
    pub max_ns: u64,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROBES: usize = 512;

struct State {
    probes: Vec<ProbeStats>,
    total_hits: u64,
    total_misses: u64,
    total_overhead_ns: u64,
    trace_enabled: bool,
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
        probes: alloc::vec![
            ProbeStats { func_name: String::from("schedule"), kind: ProbeKind::Function, hits: 50_000_000, misses: 0, total_ns: 500_000_000, max_ns: 10_000, enabled: true },
            ProbeStats { func_name: String::from("do_page_fault"), kind: ProbeKind::Function, hits: 10_000_000, misses: 100, total_ns: 200_000_000, max_ns: 50_000, enabled: true },
            ProbeStats { func_name: String::from("sys_read"), kind: ProbeKind::ReturnProbe, hits: 30_000_000, misses: 0, total_ns: 300_000_000, max_ns: 5_000, enabled: true },
            ProbeStats { func_name: String::from("tcp_sendmsg"), kind: ProbeKind::TracePoint, hits: 5_000_000, misses: 50, total_ns: 100_000_000, max_ns: 20_000, enabled: false },
        ],
        total_hits: 95_000_000,
        total_misses: 150,
        total_overhead_ns: 1_100_000_000,
        trace_enabled: true,
        ops: 0,
    });
}

/// Add a probe.
pub fn add_probe(func_name: &str, kind: ProbeKind) -> KernelResult<()> {
    with_state(|state| {
        if state.probes.len() >= MAX_PROBES { return Err(KernelError::ResourceExhausted); }
        if state.probes.iter().any(|p| p.func_name == func_name && p.kind == kind) {
            return Err(KernelError::AlreadyExists);
        }
        state.probes.push(ProbeStats {
            func_name: String::from(func_name), kind,
            hits: 0, misses: 0, total_ns: 0, max_ns: 0, enabled: true,
        });
        Ok(())
    })
}

/// Remove a probe.
pub fn remove_probe(func_name: &str) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.probes.iter().position(|p| p.func_name == func_name)
            .ok_or(KernelError::NotFound)?;
        state.probes.remove(idx);
        Ok(())
    })
}

/// Enable/disable a probe.
pub fn set_enabled(func_name: &str, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let p = state.probes.iter_mut().find(|p| p.func_name == func_name)
            .ok_or(KernelError::NotFound)?;
        p.enabled = enabled;
        Ok(())
    })
}

/// Record a probe hit.
pub fn record_hit(func_name: &str, duration_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = state.probes.iter_mut().find(|p| p.func_name == func_name)
            .ok_or(KernelError::NotFound)?;
        if !p.enabled {
            p.misses += 1;
            state.total_misses += 1;
            return Ok(());
        }
        p.hits += 1;
        p.total_ns += duration_ns;
        if duration_ns > p.max_ns { p.max_ns = duration_ns; }
        state.total_hits += 1;
        state.total_overhead_ns += duration_ns;
        Ok(())
    })
}

/// Toggle global tracing.
pub fn set_global_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.trace_enabled = enabled;
        Ok(())
    })
}

/// Per-probe stats.
pub fn per_probe() -> Vec<ProbeStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.probes.clone())
}

/// Global enabled state.
pub fn is_enabled() -> bool {
    STATE.lock().as_ref().map_or(false, |s| s.trace_enabled)
}

/// Statistics: (probe_count, total_hits, total_misses, total_overhead_ns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.probes.len(), s.total_hits, s.total_misses, s.total_overhead_ns, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("ftrace::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_probe().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add probe.
    add_probe("test_fn", ProbeKind::Function).expect("add");
    assert_eq!(per_probe().len(), 5);
    assert!(add_probe("test_fn", ProbeKind::Function).is_err());
    crate::serial_println!("  [2/8] add probe: OK");

    // 3: Hit.
    record_hit("test_fn", 100).expect("hit");
    let p = per_probe().iter().find(|p| p.func_name == "test_fn").cloned().unwrap();
    assert_eq!(p.hits, 1);
    assert_eq!(p.total_ns, 100);
    crate::serial_println!("  [3/8] hit: OK");

    // 4: Disable.
    set_enabled("test_fn", false).expect("disable");
    record_hit("test_fn", 50).expect("miss");
    let p = per_probe().iter().find(|p| p.func_name == "test_fn").cloned().unwrap();
    assert_eq!(p.hits, 1); // didn't increment
    assert_eq!(p.misses, 1);
    crate::serial_println!("  [4/8] disable: OK");

    // 5: Re-enable.
    set_enabled("test_fn", true).expect("enable");
    record_hit("test_fn", 200).expect("hit2");
    let p = per_probe().iter().find(|p| p.func_name == "test_fn").cloned().unwrap();
    assert_eq!(p.hits, 2);
    assert_eq!(p.max_ns, 200);
    crate::serial_println!("  [5/8] re-enable: OK");

    // 6: Remove.
    remove_probe("test_fn").expect("remove");
    assert_eq!(per_probe().len(), 4);
    assert!(remove_probe("test_fn").is_err());
    crate::serial_println!("  [6/8] remove: OK");

    // 7: Global toggle.
    assert!(is_enabled());
    set_global_enabled(false).expect("global off");
    assert!(!is_enabled());
    set_global_enabled(true).expect("global on");
    crate::serial_println!("  [7/8] global toggle: OK");

    // 8: Stats.
    let (probes, hits, misses, overhead, ops) = stats();
    assert_eq!(probes, 4);
    assert!(hits > 95_000_000);
    assert!(misses > 150);
    assert!(overhead > 1_100_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("ftrace::self_test() — all 8 tests passed");
}
