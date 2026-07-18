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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the function-trace statistics state.
///
/// Starts with no probes, zero hit/miss/overhead totals, and global
/// tracing OFF (the honest default — nothing is being traced until a
/// subsystem installs a probe and enables tracing). The `/proc/ftrace`
/// generator and the `ftrace` kshell command surface the probe list (and
/// `per_probe`) as if it reflects real installed function-trace probes, so
/// seeding it with phantom probes would be fabricated procfs data. Probes
/// are installed through [`add_probe`] and removed through [`remove_probe`];
/// the hit/miss/overhead counters advance only through real [`record_hit`]
/// calls.
///
/// (Previously this seeded four fictional probes — "schedule" (50M hits),
/// "do_page_fault" (10M hits, 100 misses), "sys_read" (30M hits), and
/// "tcp_sendmsg" (5M hits, 50 misses, disabled) — plus totals of 95M hits,
/// 150 misses, and 1.1s of overhead, with global tracing enabled.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        probes: Vec::new(),
        total_hits: 0,
        total_misses: 0,
        total_overhead_ns: 0,
        trace_enabled: false,
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
    STATE.lock().as_ref().is_some_and(|s| s.trace_enabled)
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live probe list afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom probes, zero totals, tracing off.
    assert_eq!(per_probe().len(), 0);
    let (p0, h0, m0, o0, _) = stats();
    assert_eq!((p0, h0, m0, o0), (0, 0, 0, 0));
    assert!(!is_enabled());
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Add probe — appears once; duplicate (name+kind) is AlreadyExists.
    add_probe("test_fn", ProbeKind::Function).expect("add");
    assert_eq!(per_probe().len(), 1);
    assert!(add_probe("test_fn", ProbeKind::Function).is_err());
    let p = per_probe().into_iter().find(|p| p.func_name == "test_fn").expect("find");
    assert_eq!((p.hits, p.misses, p.total_ns, p.max_ns), (0, 0, 0, 0));
    assert!(p.enabled);
    crate::serial_println!("  [2/8] add probe: OK");

    // 3: Hit — enabled probe accrues hit, total_ns, max_ns; globals follow.
    record_hit("test_fn", 100).expect("hit");
    let p = per_probe().into_iter().find(|p| p.func_name == "test_fn").expect("p3");
    assert_eq!((p.hits, p.total_ns, p.max_ns), (1, 100, 100));
    let (_, hits, _, overhead, _) = stats();
    assert_eq!((hits, overhead), (1, 100));
    crate::serial_println!("  [3/8] hit: OK");

    // 4: Disabled probe counts a miss, not a hit; total_ns unchanged.
    set_enabled("test_fn", false).expect("disable");
    record_hit("test_fn", 50).expect("miss");
    let p = per_probe().into_iter().find(|p| p.func_name == "test_fn").expect("p4");
    assert_eq!((p.hits, p.misses, p.total_ns), (1, 1, 100));
    assert_eq!(stats().2, 1); // total_misses
    crate::serial_println!("  [4/8] disable: OK");

    // 5: Re-enable — hit accrues again; max_ns rises to the larger duration.
    set_enabled("test_fn", true).expect("enable");
    record_hit("test_fn", 200).expect("hit2");
    let p = per_probe().into_iter().find(|p| p.func_name == "test_fn").expect("p5");
    assert_eq!((p.hits, p.max_ns, p.total_ns), (2, 200, 300));
    crate::serial_println!("  [5/8] re-enable: OK");

    // 6: Remove — list empties; double/unknown remove + unknown hit are NotFound.
    remove_probe("test_fn").expect("remove");
    assert_eq!(per_probe().len(), 0);
    assert!(remove_probe("test_fn").is_err());
    assert!(record_hit("nope", 0).is_err());
    crate::serial_println!("  [6/8] remove: OK");

    // 7: Global tracing toggles from the off default.
    assert!(!is_enabled());
    set_global_enabled(true).expect("global on");
    assert!(is_enabled());
    set_global_enabled(false).expect("global off");
    assert!(!is_enabled());
    crate::serial_println!("  [7/8] global toggle: OK");

    // 8: Final stats reflect only the real activity above. (Global hit/miss/
    //    overhead totals are cumulative and not decremented on remove.)
    let (probes, hits, misses, overhead, ops) = stats();
    assert_eq!((probes, hits, misses, overhead), (0, 2, 1, 300));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("ftrace::self_test() — all 8 tests passed");
}
