//! Kernel Probes — dynamic tracing point management.
//!
//! Tracks registered kernel probes (kprobes, kretprobes),
//! hit counts, and probe overhead. Supports monitoring
//! the kernel's dynamic instrumentation subsystem.
//!
//! ## Architecture
//!
//! ```text
//! Kernel probes
//!   → kprobes::register(addr, name) → register probe
//!   → kprobes::unregister(id) → remove probe
//!   → kprobes::record_hit(id) → count probe hit
//!   → kprobes::list() → list all probes
//!
//! Integration:
//!   → tracemon (trace monitor)
//!   → perfmon (performance monitor)
//!   → sysdiag (diagnostics)
//!   → audit (audit logging)
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

/// Probe type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeType {
    Kprobe,
    Kretprobe,
    Tracepoint,
    Uprobe,
    Uretprobe,
}

impl ProbeType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Kprobe => "kprobe",
            Self::Kretprobe => "kretprobe",
            Self::Tracepoint => "tracepoint",
            Self::Uprobe => "uprobe",
            Self::Uretprobe => "uretprobe",
        }
    }
}

/// A registered probe.
#[derive(Debug, Clone)]
pub struct Probe {
    pub id: u32,
    pub probe_type: ProbeType,
    pub name: String,
    pub address: u64,
    pub hits: u64,
    pub misses: u64,
    pub enabled: bool,
    pub overhead_ns: u64,
    pub registered_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROBES: usize = 256;

struct State {
    probes: Vec<Probe>,
    next_id: u32,
    total_hits: u64,
    total_misses: u64,
    total_overhead_ns: u64,
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

/// Initialise the kernel-probes state.
///
/// Starts with no registered probes and zero hit/miss/overhead totals. The
/// `/proc/kprobes` generator and the `kprobes` kshell command surface this
/// list (and `by_type`) as if it reflects the real set of installed dynamic
/// instrumentation points, so seeding it with phantom probes would be
/// fabricated procfs data — it would claim probes are attached to kernel
/// functions that nothing actually instrumented. Probes are installed
/// through [`register`] and removed through [`unregister`]; hit/miss/
/// overhead counters advance only through real [`record_hit`] calls.
///
/// (Previously this seeded three fictional probes — a "do_page_fault"
/// kprobe (500k hits), a "sys_read" kretprobe (2M hits, 100 misses), and a
/// "sched:sched_switch" tracepoint (10M hits) — plus totals of 12.5M hits,
/// 100 misses, and 625ms of overhead.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        probes: Vec::new(),
        next_id: 1,
        total_hits: 0,
        total_misses: 0,
        total_overhead_ns: 0,
        ops: 0,
    });
}

/// Register a new probe.
pub fn register(probe_type: ProbeType, name: &str, address: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.probes.len() >= MAX_PROBES { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.probes.push(Probe {
            id, probe_type, name: String::from(name), address,
            hits: 0, misses: 0, enabled: true, overhead_ns: 0,
            registered_ns: now,
        });
        Ok(id)
    })
}

/// Unregister a probe.
pub fn unregister(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.probes.iter().position(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        state.probes.remove(idx);
        Ok(())
    })
}

/// Record a probe hit.
pub fn record_hit(id: u32, overhead_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = state.probes.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        if !p.enabled {
            p.misses += 1;
            state.total_misses += 1;
            return Ok(());
        }
        p.hits += 1;
        p.overhead_ns += overhead_ns;
        state.total_hits += 1;
        state.total_overhead_ns += overhead_ns;
        Ok(())
    })
}

/// Enable/disable a probe.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let p = state.probes.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        p.enabled = enabled;
        Ok(())
    })
}

/// List all probes.
pub fn list() -> Vec<Probe> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.probes.clone())
}

/// Get probes by type.
pub fn by_type(probe_type: ProbeType) -> Vec<Probe> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.probes.iter().filter(|p| p.probe_type == probe_type).cloned().collect()
    })
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
    crate::serial_println!("kprobes::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live probe list afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom probes, zero totals.
    assert_eq!(list().len(), 0);
    let (p0, h0, m0, o0, _) = stats();
    assert_eq!((p0, h0, m0, o0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — id starts at 1; probe begins enabled with zeroed counters.
    let id = register(ProbeType::Kprobe, "test_func", 0xDEAD_0000).expect("register");
    assert_eq!(id, 1);
    assert_eq!(list().len(), 1);
    let p = list().into_iter().find(|p| p.id == id).expect("find");
    assert!(p.enabled);
    assert_eq!((p.hits, p.misses, p.overhead_ns), (0, 0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record hit — enabled probe accrues hit + overhead; globals follow.
    record_hit(id, 50).expect("hit");
    let p = list().into_iter().find(|p| p.id == id).expect("p3");
    assert_eq!((p.hits, p.overhead_ns), (1, 50));
    let (_, hits, _, overhead, _) = stats();
    assert_eq!((hits, overhead), (1, 50));
    crate::serial_println!("  [3/8] hit: OK");

    // 4: Disabled probe counts a miss, not a hit; overhead unchanged.
    set_enabled(id, false).expect("disable");
    record_hit(id, 999).expect("hit_disabled");
    let p = list().into_iter().find(|p| p.id == id).expect("p4");
    assert_eq!((p.hits, p.misses, p.overhead_ns), (1, 1, 50));
    assert_eq!(stats().2, 1); // total_misses
    set_enabled(id, true).expect("enable");
    crate::serial_println!("  [4/8] enable/disable: OK");

    // 5: Register a second probe of a different type for the by_type test.
    let id2 = register(ProbeType::Tracepoint, "test_tp", 0).expect("register2");
    assert_eq!(id2, 2);
    assert_eq!(list().len(), 2);
    crate::serial_println!("  [5/8] register2: OK");

    // 6: by_type filters by probe type.
    assert_eq!(by_type(ProbeType::Kprobe).len(), 1);
    assert_eq!(by_type(ProbeType::Tracepoint).len(), 1);
    assert_eq!(by_type(ProbeType::Uprobe).len(), 0);
    crate::serial_println!("  [6/8] by type: OK");

    // 7: Unregister removes a probe; double/unknown ops are NotFound.
    unregister(id).expect("unregister");
    assert_eq!(list().len(), 1); // id2 remains
    assert!(unregister(id).is_err());
    assert!(record_hit(999, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above. (Global hit/miss/
    //    overhead totals are cumulative and not decremented on unregister.)
    let (probes, hits, misses, overhead, ops) = stats();
    assert_eq!((probes, hits, misses, overhead), (1, 1, 1, 50));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("kprobes::self_test() — all 8 tests passed");
}
