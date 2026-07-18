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
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** KSM (Kernel Same-page Merging) statistics table.
///
/// Seeds NO processes and zero counters.  Real KSM accounting is wired through
/// [`register_process`] (one row per process the KSM scanner tracks) and the
/// `record_merge`/`record_unmerge`/`record_scan`/`update_process` functions;
/// until those are called the table is genuinely empty, so `/proc/ksmstat` and
/// the `ksmstat` kshell command report zeros rather than fabricated numbers —
/// the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional processes ("init" pid 1: shared
/// 500 / unshared 10k / volatile 200; "vm-worker" pid 100: shared 50k /
/// unshared 100k / volatile 5k) plus invented global counters (pages_shared
/// 50k, pages_sharing 150k, pages_unshared 110k, pages_volatile 5200,
/// full_scans 1000, pages_scanned 500M, merges 200k, unmerges 50k, bytes_saved
/// 50k×16KiB ≈ 800MB), which `/proc/ksmstat` then displayed as if they were
/// real measured page-deduplication activity.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The KSM scanner is expected to call [`register_process`]
/// when it begins tracking a process and the record functions as it merges,
/// unmerges, and scans pages.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        processes: Vec::new(),
        pages_shared: 0,
        pages_sharing: 0,
        pages_unshared: 0,
        pages_volatile: 0,
        full_scans: 0,
        pages_scanned: 0,
        merges: 0,
        unmerges: 0,
        bytes_saved: 0,
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
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/ksmstat must never surface).
    // Resetting first clears any residue from a prior `ksmstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated processes or counters.
    assert_eq!(per_process().len(), 0);
    let (s0, sh0, m0, u0, b0, _o0) = stats();
    assert_eq!((s0, sh0, m0, u0, b0), (0, 0, 0, 0, 0));
    let (fs0, ps0) = scan_stats();
    assert_eq!((fs0, ps0), (0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register processes — zeroed rows; dup pid fails.
    register_process(200, "test").expect("register");
    register_process(201, "other").expect("register2");
    assert!(register_process(200, "dup").is_err());
    assert_eq!(per_process().len(), 2);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Merge — shared/sharing/merges/bytes_saved increment exactly from zero.
    record_merge().expect("merge");
    record_merge().expect("merge2");
    let (shared, sharing, merges, _, saved, _) = stats();
    assert_eq!((shared, sharing, merges, saved), (2, 2, 2, 2 * 16384));
    crate::serial_println!("  [3/8] merge: OK");

    // 4: Unmerge — shared/bytes_saved decrement, unmerges increments.
    record_unmerge().expect("unmerge");
    let (shared, _, _, unmerges, saved, _) = stats();
    assert_eq!((shared, unmerges, saved), (1, 1, 16384));
    crate::serial_println!("  [4/8] unmerge: OK");

    // 5: Scan — pages_scanned accumulates, full_scans counts only full passes.
    record_scan(10_000, true).expect("scan");
    record_scan(5_000, false).expect("scan2");
    let (full_scans, pages_scanned) = scan_stats();
    assert_eq!((full_scans, pages_scanned), (1, 15_000));
    crate::serial_println!("  [5/8] scan: OK");

    // 6: Update process sets per-process sharing counts exactly.
    update_process(200, 100, 500, 10).expect("update");
    let p = per_process().iter().find(|p| p.pid == 200).cloned().expect("p200");
    assert_eq!((p.shared_pages, p.unshared_pages, p.volatile_pages), (100, 500, 10));
    crate::serial_println!("  [6/8] update: OK");

    // 7: Unregistered process → NotFound.
    assert!(update_process(999, 0, 0, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate stats equal the exact sums of the operations above.
    let (shared, sharing, merges, unmerges, saved, ops) = stats();
    assert_eq!((shared, sharing, merges, unmerges, saved), (1, 2, 2, 1, 16384));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/ksmstat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the KSM scanner wires real
    // accounting.
    *STATE.lock() = None;

    crate::serial_println!("ksmstat::self_test() — all 8 tests passed");
}
