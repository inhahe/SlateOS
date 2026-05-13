//! KSM Statistics — Kernel Same-page Merging monitoring.
//!
//! Tracks page merging/unmerging, memory savings, scan rates,
//! and per-process sharing. Essential for understanding memory
//! deduplication in virtualization and container workloads.
//!
//! ## Architecture
//!
//! ```text
//! KSM monitoring
//!   → ksmstat::record_merge() → pages merged
//!   → ksmstat::record_unmerge() → pages unmerged (CoW break)
//!   → ksmstat::record_scan(pages) → scanner progress
//!   → ksmstat::register_process(pid) → track per-process sharing
//!
//! Integration:
//!   → pagestat (page allocator)
//!   → mempress (memory pressure)
//!   → pftrack (page fault tracking)
//!   → mmapstat (mmap operations)
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

/// Per-process KSM stats.
#[derive(Debug, Clone)]
pub struct ProcessKsmStats {
    pub pid: u32,
    pub name: String,
    pub shared_pages: u64,
    pub unshared_pages: u64,
    pub volatile_pages: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROCESSES: usize = 256;

struct State {
    processes: Vec<ProcessKsmStats>,
    pages_shared: u64,
    pages_sharing: u64,
    pages_unshared: u64,
    pages_volatile: u64,
    full_scans: u64,
    pages_scanned: u64,
    merges: u64,
    unmerges: u64,
    bytes_saved: u64,
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
        processes: alloc::vec![
            ProcessKsmStats { pid: 1, name: String::from("init"), shared_pages: 500, unshared_pages: 10_000, volatile_pages: 200 },
            ProcessKsmStats { pid: 100, name: String::from("vm-worker"), shared_pages: 50_000, unshared_pages: 100_000, volatile_pages: 5_000 },
        ],
        pages_shared: 50_000,
        pages_sharing: 150_000,
        pages_unshared: 110_000,
        pages_volatile: 5_200,
        full_scans: 1_000,
        pages_scanned: 500_000_000,
        merges: 200_000,
        unmerges: 50_000,
        bytes_saved: 50_000 * 16384, // 50k pages × 16KiB
        ops: 0,
    });
}

/// Register a process for KSM tracking.
pub fn register_process(pid: u32, name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.processes.iter().any(|p| p.pid == pid) { return Err(KernelError::AlreadyExists); }
        if state.processes.len() >= MAX_PROCESSES { return Err(KernelError::ResourceExhausted); }
        state.processes.push(ProcessKsmStats {
            pid, name: String::from(name), shared_pages: 0, unshared_pages: 0, volatile_pages: 0,
        });
        Ok(())
    })
}

/// Record a page merge.
pub fn record_merge() -> KernelResult<()> {
    with_state(|state| {
        state.merges += 1;
        state.pages_shared += 1;
        state.pages_sharing += 1;
        state.bytes_saved += 16384; // 16 KiB page
        Ok(())
    })
}

/// Record a page unmerge (CoW break).
pub fn record_unmerge() -> KernelResult<()> {
    with_state(|state| {
        state.unmerges += 1;
        state.pages_shared = state.pages_shared.saturating_sub(1);
        state.bytes_saved = state.bytes_saved.saturating_sub(16384);
        Ok(())
    })
}

/// Record scan progress.
pub fn record_scan(pages: u64, full_scan: bool) -> KernelResult<()> {
    with_state(|state| {
        state.pages_scanned += pages;
        if full_scan { state.full_scans += 1; }
        Ok(())
    })
}

/// Update per-process sharing counts.
pub fn update_process(pid: u32, shared: u64, unshared: u64, volatile: u64) -> KernelResult<()> {
    with_state(|state| {
        let p = state.processes.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        p.shared_pages = shared;
        p.unshared_pages = unshared;
        p.volatile_pages = volatile;
        Ok(())
    })
}

/// Per-process stats.
pub fn per_process() -> Vec<ProcessKsmStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.processes.clone())
}

/// Statistics: (pages_shared, pages_sharing, merges, unmerges, bytes_saved, ops).
pub fn stats() -> (u64, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.pages_shared, s.pages_sharing, s.merges, s.unmerges, s.bytes_saved, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

/// Scan stats: (full_scans, pages_scanned).
pub fn scan_stats() -> (u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.full_scans, s.pages_scanned),
        None => (0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("ksmstat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_process().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register_process(200, "test").expect("register");
    assert!(register_process(200, "dup").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Merge.
    let (shared_before, _, _, _, _, _) = stats();
    record_merge().expect("merge");
    let (shared_after, _, _, _, _, _) = stats();
    assert_eq!(shared_after, shared_before + 1);
    crate::serial_println!("  [3/8] merge: OK");

    // 4: Unmerge.
    let (shared_before, _, _, _, _, _) = stats();
    record_unmerge().expect("unmerge");
    let (shared_after, _, _, _, _, _) = stats();
    assert_eq!(shared_after, shared_before - 1);
    crate::serial_println!("  [4/8] unmerge: OK");

    // 5: Scan.
    let (scans_before, _) = scan_stats();
    record_scan(10_000, true).expect("scan");
    let (scans_after, _) = scan_stats();
    assert_eq!(scans_after, scans_before + 1);
    crate::serial_println!("  [5/8] scan: OK");

    // 6: Update process.
    update_process(200, 100, 500, 10).expect("update");
    let p = per_process().iter().find(|p| p.pid == 200).cloned().unwrap();
    assert_eq!(p.shared_pages, 100);
    crate::serial_println!("  [6/8] update: OK");

    // 7: Not found.
    assert!(update_process(999, 0, 0, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (shared, sharing, merges, unmerges, saved, ops) = stats();
    assert!(shared > 49_000);
    assert!(sharing > 150_000);
    assert!(merges > 200_000);
    assert!(unmerges > 50_000);
    assert!(saved > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("ksmstat::self_test() — all 8 tests passed");
}
