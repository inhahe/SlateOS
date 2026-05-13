//! Mmap Statistics — memory mapping operation monitoring.
//!
//! Tracks mmap/munmap/mprotect operations, mapping types,
//! total mapped regions, and per-process mapping counts.
//! Essential for virtual memory diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! Memory mapping monitoring
//!   → mmapstat::record_map(pid, size, type) → track mmap
//!   → mmapstat::record_unmap(pid, size) → track munmap
//!   → mmapstat::record_protect(pid) → track mprotect
//!   → mmapstat::per_process() → per-process stats
//!
//! Integration:
//!   → vmmap (VM address space)
//!   → pagestat (page allocator)
//!   → memcg (memory cgroup)
//!   → pftrack (page fault tracking)
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

/// Mapping type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapType {
    Anonymous,
    File,
    SharedAnon,
    SharedFile,
    Stack,
    Vdso,
}

impl MapType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Anonymous => "anon",
            Self::File => "file",
            Self::SharedAnon => "shared_anon",
            Self::SharedFile => "shared_file",
            Self::Stack => "stack",
            Self::Vdso => "vdso",
        }
    }
}

/// Per-process mapping stats.
#[derive(Debug, Clone)]
pub struct ProcessMapStats {
    pub pid: u32,
    pub name: String,
    pub regions: u64,
    pub total_bytes: u64,
    pub maps: u64,
    pub unmaps: u64,
    pub protects: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROCESSES: usize = 256;

struct State {
    processes: Vec<ProcessMapStats>,
    type_counts: [u64; 6],
    total_maps: u64,
    total_unmaps: u64,
    total_protects: u64,
    total_bytes_mapped: u64,
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

fn type_index(t: MapType) -> usize {
    match t {
        MapType::Anonymous => 0,
        MapType::File => 1,
        MapType::SharedAnon => 2,
        MapType::SharedFile => 3,
        MapType::Stack => 4,
        MapType::Vdso => 5,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        processes: alloc::vec![
            ProcessMapStats { pid: 1, name: String::from("init"), regions: 50, total_bytes: 200_000_000, maps: 500_000, unmaps: 499_950, protects: 10_000 },
            ProcessMapStats { pid: 100, name: String::from("shell"), regions: 30, total_bytes: 100_000_000, maps: 100_000, unmaps: 99_970, protects: 5_000 },
        ],
        type_counts: [300_000, 200_000, 50_000, 30_000, 10_000, 2_000],
        total_maps: 600_000,
        total_unmaps: 599_920,
        total_protects: 15_000,
        total_bytes_mapped: 300_000_000,
        ops: 0,
    });
}

/// Record an mmap.
pub fn record_map(pid: u32, size: u64, map_type: MapType) -> KernelResult<()> {
    with_state(|state| {
        let p = state.processes.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        p.maps += 1;
        p.regions += 1;
        p.total_bytes += size;
        state.type_counts[type_index(map_type)] += 1;
        state.total_maps += 1;
        state.total_bytes_mapped += size;
        Ok(())
    })
}

/// Record an munmap.
pub fn record_unmap(pid: u32, size: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = state.processes.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        p.unmaps += 1;
        p.regions = p.regions.saturating_sub(1);
        p.total_bytes = p.total_bytes.saturating_sub(size);
        state.total_unmaps += 1;
        Ok(())
    })
}

/// Record an mprotect.
pub fn record_protect(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = state.processes.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        p.protects += 1;
        state.total_protects += 1;
        Ok(())
    })
}

/// Register a process.
pub fn register_process(pid: u32, name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.processes.iter().any(|p| p.pid == pid) { return Err(KernelError::AlreadyExists); }
        if state.processes.len() >= MAX_PROCESSES { return Err(KernelError::ResourceExhausted); }
        state.processes.push(ProcessMapStats {
            pid, name: String::from(name), regions: 0, total_bytes: 0,
            maps: 0, unmaps: 0, protects: 0,
        });
        Ok(())
    })
}

/// Per-process stats.
pub fn per_process() -> Vec<ProcessMapStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.processes.clone())
}

/// Type breakdown.
pub fn type_breakdown() -> [(MapType, u64); 6] {
    let guard = STATE.lock();
    let counts = guard.as_ref().map_or([0u64; 6], |s| s.type_counts);
    [
        (MapType::Anonymous, counts[0]),
        (MapType::File, counts[1]),
        (MapType::SharedAnon, counts[2]),
        (MapType::SharedFile, counts[3]),
        (MapType::Stack, counts[4]),
        (MapType::Vdso, counts[5]),
    ]
}

/// Statistics: (process_count, total_maps, total_unmaps, total_protects, total_bytes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.processes.len(), s.total_maps, s.total_unmaps, s.total_protects, s.total_bytes_mapped, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("mmapstat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_process().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register_process(200, "test").expect("register");
    assert!(register_process(200, "dup").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Map.
    record_map(200, 4096, MapType::Anonymous).expect("map");
    let p = per_process().iter().find(|p| p.pid == 200).cloned().unwrap();
    assert_eq!(p.maps, 1);
    assert_eq!(p.total_bytes, 4096);
    crate::serial_println!("  [3/8] map: OK");

    // 4: Unmap.
    record_unmap(200, 4096).expect("unmap");
    let p = per_process().iter().find(|p| p.pid == 200).cloned().unwrap();
    assert_eq!(p.unmaps, 1);
    assert_eq!(p.total_bytes, 0);
    crate::serial_println!("  [4/8] unmap: OK");

    // 5: Protect.
    record_protect(200).expect("protect");
    let p = per_process().iter().find(|p| p.pid == 200).cloned().unwrap();
    assert_eq!(p.protects, 1);
    crate::serial_println!("  [5/8] protect: OK");

    // 6: Type breakdown.
    let types = type_breakdown();
    assert!(types[0].1 > 300_000); // Anonymous.
    crate::serial_println!("  [6/8] types: OK");

    // 7: Not found.
    assert!(record_map(999, 0, MapType::Anonymous).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (procs, maps, unmaps, protects, bytes, ops) = stats();
    assert!(procs >= 3);
    assert!(maps > 600_000);
    assert!(unmaps > 599_920);
    assert!(protects > 15_000);
    assert!(bytes > 300_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("mmapstat::self_test() — all 8 tests passed");
}
