//! BPF Program Statistics — eBPF program lifecycle monitoring.
//!
//! Tracks loaded BPF programs, map usage, verifier stats,
//! and execution counts. Essential for understanding
//! in-kernel programmable filtering and tracing.
//!
//! ## Architecture
//!
//! ```text
//! BPF monitoring
//!   → bpfstat::load_program(name, type) → track program load
//!   → bpfstat::unload_program(id) → track program unload
//!   → bpfstat::record_run(id) → execution event
//!   → bpfstat::list_programs() → list loaded programs
//!
//! Integration:
//!   → kprobes (dynamic probes)
//!   → netfilter (packet filtering)
//!   → tracemon (tracing)
//!   → secpolicy (security policy)
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

/// BPF program type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpfProgType {
    SocketFilter,
    Kprobe,
    TracePoint,
    Xdp,
    PerfEvent,
    CgroupSkb,
}

impl BpfProgType {
    pub fn label(self) -> &'static str {
        match self {
            Self::SocketFilter => "socket_filter",
            Self::Kprobe => "kprobe",
            Self::TracePoint => "tracepoint",
            Self::Xdp => "xdp",
            Self::PerfEvent => "perf_event",
            Self::CgroupSkb => "cgroup_skb",
        }
    }
}

/// BPF program info.
#[derive(Debug, Clone)]
pub struct BpfProgram {
    pub id: u32,
    pub name: String,
    pub prog_type: BpfProgType,
    pub insn_count: u32,
    pub run_count: u64,
    pub run_time_ns: u64,
    pub map_count: u32,
    pub loaded_ns: u64,
}

/// BPF map info.
#[derive(Debug, Clone)]
pub struct BpfMap {
    pub id: u32,
    pub name: String,
    pub max_entries: u32,
    pub key_size: u32,
    pub value_size: u32,
    pub used_entries: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROGRAMS: usize = 256;
const MAX_MAPS: usize = 512;

struct State {
    programs: Vec<BpfProgram>,
    maps: Vec<BpfMap>,
    next_prog_id: u32,
    next_map_id: u32,
    total_loaded: u64,
    total_unloaded: u64,
    total_runs: u64,
    verifier_errors: u64,
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

/// Initialise an **empty** BPF statistics table.
///
/// Seeds NO programs, NO maps, and zero totals.  Real BPF accounting is wired
/// through [`load_program`]/[`unload_program`]/[`record_run`]/[`create_map`]/
/// [`record_verifier_error`]; until those are called the table is genuinely
/// empty, so the `/proc/bpfstat` file and the `bpfstat` kshell command report
/// zeros rather than fabricated numbers — the kernel's hard "never invent data
/// in procfs" rule.
///
/// NOTE: this previously seeded three fictional programs ("tcp_retransmit"
/// kprobe run_count 50000; "xdp_filter" XDP run_count 10M; "sched_trace"
/// tracepoint run_count 1M), six fictional maps, and invented aggregate totals
/// (total_loaded 50, total_unloaded 47, total_runs 11_050_000, verifier_errors
/// 15), which `/proc/bpfstat` then displayed as if they were real loaded-program
/// and execution measurements.  That demo data was removed; the self-test now
/// builds its own fixtures explicitly via the real API (see [`self_test`]).
/// The BPF subsystem is expected to call [`load_program`]/[`create_map`] when
/// programs/maps are installed and the record_* functions as they execute.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        programs: Vec::new(),
        maps: Vec::new(),
        next_prog_id: 1,
        next_map_id: 1,
        total_loaded: 0,
        total_unloaded: 0,
        total_runs: 0,
        verifier_errors: 0,
        ops: 0,
    });
}

/// Load a BPF program.
pub fn load_program(name: &str, prog_type: BpfProgType, insn_count: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.programs.len() >= MAX_PROGRAMS { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_prog_id;
        state.next_prog_id += 1;
        state.total_loaded += 1;
        state.programs.push(BpfProgram {
            id, name: String::from(name), prog_type, insn_count,
            run_count: 0, run_time_ns: 0, map_count: 0, loaded_ns: now,
        });
        Ok(id)
    })
}

/// Unload a BPF program.
pub fn unload_program(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.programs.iter().position(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        state.programs.remove(idx);
        state.total_unloaded += 1;
        Ok(())
    })
}

/// Record a program execution.
pub fn record_run(id: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = state.programs.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        p.run_count += 1;
        p.run_time_ns += ns;
        state.total_runs += 1;
        Ok(())
    })
}

/// Record a verifier error.
pub fn record_verifier_error() -> KernelResult<()> {
    with_state(|state| {
        state.verifier_errors += 1;
        Ok(())
    })
}

/// Create a BPF map.
pub fn create_map(name: &str, max_entries: u32, key_size: u32, value_size: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.maps.len() >= MAX_MAPS { return Err(KernelError::ResourceExhausted); }
        let id = state.next_map_id;
        state.next_map_id += 1;
        state.maps.push(BpfMap {
            id, name: String::from(name), max_entries, key_size, value_size, used_entries: 0,
        });
        Ok(id)
    })
}

/// List loaded programs.
pub fn list_programs() -> Vec<BpfProgram> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.programs.clone())
}

/// List maps.
pub fn list_maps() -> Vec<BpfMap> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.maps.clone())
}

/// Programs by type.
pub fn by_type(prog_type: BpfProgType) -> Vec<BpfProgram> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.programs.iter().filter(|p| p.prog_type == prog_type).cloned().collect()
    })
}

/// Statistics: (program_count, map_count, total_loaded, total_runs, verifier_errors, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.programs.len(), s.maps.len(), s.total_loaded, s.total_runs, s.verifier_errors, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("bpfstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/bpfstat must never surface).
    // Resetting first clears any residue from a prior `bpfstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated programs, maps, or totals.
    assert_eq!(list_programs().len(), 0);
    assert_eq!(list_maps().len(), 0);
    let (p0, m0, l0, r0, v0, _o0) = stats();
    assert_eq!((p0, m0, l0, r0, v0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Load programs — ids start at 1 and increment.
    let id1 = load_program("prog_a", BpfProgType::SocketFilter, 32).expect("load1");
    let id2 = load_program("prog_b", BpfProgType::Xdp, 64).expect("load2");
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(list_programs().len(), 2);
    crate::serial_println!("  [2/8] load: OK");

    // 3: Record run increments count + time exactly from zero.
    record_run(id1, 500).expect("run");
    let p = list_programs().iter().find(|p| p.id == id1).cloned().expect("prog");
    assert_eq!(p.run_count, 1);
    assert_eq!(p.run_time_ns, 500);
    assert!(record_run(9999, 1).is_err()); // unknown id
    crate::serial_println!("  [3/8] run: OK");

    // 4: Create maps — ids start at 1, used_entries zeroed.
    let map_id = create_map("test_map", 1024, 4, 8).expect("create_map");
    assert_eq!(map_id, 1);
    assert_eq!(list_maps().len(), 1);
    let m = list_maps().iter().find(|m| m.id == map_id).cloned().expect("map");
    assert_eq!(m.used_entries, 0);
    crate::serial_println!("  [4/8] create map: OK");

    // 5: Verifier error increments exactly from zero.
    record_verifier_error().expect("verifier");
    let (_, _, _, _, ve, _) = stats();
    assert_eq!(ve, 1);
    crate::serial_println!("  [5/8] verifier error: OK");

    // 6: by_type filters correctly (one XDP program: prog_b).
    let xdp = by_type(BpfProgType::Xdp);
    assert_eq!(xdp.len(), 1);
    assert_eq!(xdp[0].id, id2);
    crate::serial_println!("  [6/8] by type: OK");

    // 7: Unload removes the program; unloading again fails.
    unload_program(id1).expect("unload");
    assert_eq!(list_programs().len(), 1);
    assert!(unload_program(id1).is_err());
    crate::serial_println!("  [7/8] unload: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (progs, maps, loaded, runs, verr, ops) = stats();
    assert_eq!(progs, 1);    // two loaded, one unloaded
    assert_eq!(maps, 1);
    assert_eq!(loaded, 2);   // two load_program calls
    assert_eq!(runs, 1);     // one successful record_run
    assert_eq!(verr, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/bpfstat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the BPF subsystem wires
    // real accounting.
    *STATE.lock() = None;

    crate::serial_println!("bpfstat::self_test() — all 8 tests passed");
}
