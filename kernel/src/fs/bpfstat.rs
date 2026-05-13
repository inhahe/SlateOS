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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        programs: alloc::vec![
            BpfProgram { id: 1, name: String::from("tcp_retransmit"), prog_type: BpfProgType::Kprobe, insn_count: 128, run_count: 50_000, run_time_ns: 5_000_000, map_count: 2, loaded_ns: now },
            BpfProgram { id: 2, name: String::from("xdp_filter"), prog_type: BpfProgType::Xdp, insn_count: 256, run_count: 10_000_000, run_time_ns: 500_000_000, map_count: 3, loaded_ns: now },
            BpfProgram { id: 3, name: String::from("sched_trace"), prog_type: BpfProgType::TracePoint, insn_count: 64, run_count: 1_000_000, run_time_ns: 50_000_000, map_count: 1, loaded_ns: now },
        ],
        maps: alloc::vec![
            BpfMap { id: 1, name: String::from("retransmit_counts"), max_entries: 10000, key_size: 16, value_size: 8, used_entries: 500 },
            BpfMap { id: 2, name: String::from("retransmit_addrs"), max_entries: 10000, key_size: 4, value_size: 16, used_entries: 500 },
            BpfMap { id: 3, name: String::from("xdp_stats"), max_entries: 256, key_size: 4, value_size: 16, used_entries: 64 },
            BpfMap { id: 4, name: String::from("xdp_blacklist"), max_entries: 100000, key_size: 4, value_size: 1, used_entries: 1000 },
            BpfMap { id: 5, name: String::from("xdp_counters"), max_entries: 64, key_size: 4, value_size: 8, used_entries: 4 },
            BpfMap { id: 6, name: String::from("sched_hist"), max_entries: 1024, key_size: 4, value_size: 8, used_entries: 256 },
        ],
        next_prog_id: 4,
        next_map_id: 7,
        total_loaded: 50,
        total_unloaded: 47,
        total_runs: 11_050_000,
        verifier_errors: 15,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_programs().len(), 3);
    assert_eq!(list_maps().len(), 6);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Load program.
    let id = load_program("test_prog", BpfProgType::SocketFilter, 32).expect("load");
    assert!(id >= 4);
    assert_eq!(list_programs().len(), 4);
    crate::serial_println!("  [2/8] load: OK");

    // 3: Record run.
    record_run(id, 500).expect("run");
    let p = list_programs().iter().find(|p| p.id == id).cloned().unwrap();
    assert_eq!(p.run_count, 1);
    assert_eq!(p.run_time_ns, 500);
    crate::serial_println!("  [3/8] run: OK");

    // 4: Create map.
    let map_id = create_map("test_map", 1024, 4, 8).expect("create_map");
    assert!(map_id >= 7);
    assert_eq!(list_maps().len(), 7);
    crate::serial_println!("  [4/8] create map: OK");

    // 5: Verifier error.
    let (_, _, _, _, ve_before, _) = stats();
    record_verifier_error().expect("verifier");
    let (_, _, _, _, ve_after, _) = stats();
    assert_eq!(ve_after, ve_before + 1);
    crate::serial_println!("  [5/8] verifier error: OK");

    // 6: By type.
    let xdp = by_type(BpfProgType::Xdp);
    assert_eq!(xdp.len(), 1);
    crate::serial_println!("  [6/8] by type: OK");

    // 7: Unload.
    unload_program(id).expect("unload");
    assert_eq!(list_programs().len(), 3);
    assert!(unload_program(id).is_err());
    crate::serial_println!("  [7/8] unload: OK");

    // 8: Stats.
    let (progs, maps, loaded, runs, verr, ops) = stats();
    assert_eq!(progs, 3);
    assert!(maps >= 7);
    assert!(loaded > 50);
    assert!(runs > 11_050_000);
    assert!(verr > 15);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("bpfstat::self_test() — all 8 tests passed");
}
