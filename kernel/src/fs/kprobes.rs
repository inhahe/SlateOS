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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        probes: alloc::vec![
            Probe { id: 1, probe_type: ProbeType::Kprobe, name: String::from("do_page_fault"), address: 0xFFFF_8000_0010_0000, hits: 500000, misses: 0, enabled: true, overhead_ns: 25_000_000, registered_ns: now },
            Probe { id: 2, probe_type: ProbeType::Kretprobe, name: String::from("sys_read"), address: 0xFFFF_8000_0020_0000, hits: 2_000_000, misses: 100, enabled: true, overhead_ns: 100_000_000, registered_ns: now },
            Probe { id: 3, probe_type: ProbeType::Tracepoint, name: String::from("sched:sched_switch"), address: 0, hits: 10_000_000, misses: 0, enabled: true, overhead_ns: 500_000_000, registered_ns: now },
        ],
        next_id: 4,
        total_hits: 12_500_000,
        total_misses: 100,
        total_overhead_ns: 625_000_000,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    let id = register(ProbeType::Kprobe, "test_func", 0xDEAD_0000).expect("register");
    assert!(id >= 4);
    assert_eq!(list().len(), 4);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record hit.
    record_hit(id, 50).expect("hit");
    let p = list().iter().find(|p| p.id == id).cloned().unwrap();
    assert_eq!(p.hits, 1);
    assert_eq!(p.overhead_ns, 50);
    crate::serial_println!("  [3/8] hit: OK");

    // 4: Disable/enable.
    set_enabled(id, false).expect("disable");
    record_hit(id, 0).expect("hit_disabled");
    let p = list().iter().find(|p| p.id == id).cloned().unwrap();
    assert_eq!(p.hits, 1); // Unchanged (disabled).
    assert_eq!(p.misses, 1);
    set_enabled(id, true).expect("enable");
    crate::serial_println!("  [4/8] enable/disable: OK");

    // 5: Unregister.
    unregister(id).expect("unregister");
    assert_eq!(list().len(), 3);
    assert!(unregister(id).is_err());
    crate::serial_println!("  [5/8] unregister: OK");

    // 6: By type.
    let kprobes = by_type(ProbeType::Kprobe);
    assert!(kprobes.len() >= 1);
    let tp = by_type(ProbeType::Tracepoint);
    assert_eq!(tp.len(), 1);
    crate::serial_println!("  [6/8] by type: OK");

    // 7: Not found.
    assert!(record_hit(999, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (probes, hits, misses, overhead, ops) = stats();
    assert_eq!(probes, 3);
    assert!(hits > 12_500_000);
    assert!(misses > 100);
    assert!(overhead > 625_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kprobes::self_test() — all 8 tests passed");
}
