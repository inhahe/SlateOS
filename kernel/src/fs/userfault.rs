//! Userfaultfd Statistics — user-space page fault handling monitoring.
//!
//! Tracks userfaultfd registrations, fault events, resolution
//! latency, and copy/zero page operations. Essential for
//! live migration and post-copy memory management.
//!
//! ## Architecture
//!
//! ```text
//! Userfaultfd monitoring
//!   → userfault::register(pid) → register uffd handler
//!   → userfault::record_fault(pid, type) → fault event
//!   → userfault::record_resolve(pid, ns) → fault resolved
//!   → userfault::per_process() → per-process stats
//!
//! Integration:
//!   → pftrack (page fault tracking)
//!   → mmapstat (mmap operations)
//!   → pagestat (page allocator)
//!   → migstat (process migration)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Fault type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultType {
    Missing,     // Page not present
    WriteProtect, // Write to read-only
    Minor,       // Minor fault (page present but needs update)
}

impl FaultType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::WriteProtect => "wp",
            Self::Minor => "minor",
        }
    }
}

/// Per-process uffd stats.
#[derive(Debug, Clone)]
pub struct UffdStats {
    pub pid: u32,
    pub registered_ranges: u32,
    pub faults_missing: u64,
    pub faults_wp: u64,
    pub faults_minor: u64,
    pub resolves: u64,
    pub total_resolve_ns: u64,
    pub max_resolve_ns: u64,
    pub copy_pages: u64,
    pub zero_pages: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HANDLERS: usize = 128;

struct State {
    handlers: Vec<UffdStats>,
    total_faults: u64,
    total_resolves: u64,
    total_copies: u64,
    total_zeros: u64,
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

/// Initialise the userfaultfd statistics state.
///
/// Starts with no registered handlers and zero fault/resolve/copy/zero
/// totals. A handler is added through [`register`] when a process actually
/// creates a userfaultfd, removed through [`unregister`], and its fault and
/// resolution counters advance only through real [`record_fault`] /
/// [`record_resolve`] calls. The `/proc/userfault` generator and the
/// `userfault` kshell command surface the per-process table (and
/// [`per_process`]) as if it reflects real userfaultfd registrations, so
/// seeding it with a phantom handler would be fabricated procfs data — it
/// would claim a process is handling page faults in userspace when nothing
/// registered a uffd.
///
/// (Previously this seeded one fictional handler — pid 1 with 5 registered
/// ranges, 100,000 missing / 50,000 write-protect / 10,000 minor faults,
/// 160,000 resolves, 100,000 copy pages and 60,000 zero pages — plus global
/// totals of 160,000 faults / 160,000 resolves / 100,000 copies / 60,000
/// zeros.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        handlers: Vec::new(),
        total_faults: 0,
        total_resolves: 0,
        total_copies: 0,
        total_zeros: 0,
        ops: 0,
    });
}

/// Register a uffd handler for a process.
pub fn register(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.handlers.iter().any(|h| h.pid == pid) { return Err(KernelError::AlreadyExists); }
        if state.handlers.len() >= MAX_HANDLERS { return Err(KernelError::ResourceExhausted); }
        state.handlers.push(UffdStats {
            pid, registered_ranges: 0, faults_missing: 0, faults_wp: 0,
            faults_minor: 0, resolves: 0, total_resolve_ns: 0, max_resolve_ns: 0,
            copy_pages: 0, zero_pages: 0,
        });
        Ok(())
    })
}

/// Unregister.
pub fn unregister(pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.handlers.iter().position(|h| h.pid == pid)
            .ok_or(KernelError::NotFound)?;
        state.handlers.remove(idx);
        Ok(())
    })
}

/// Record a fault.
pub fn record_fault(pid: u32, fault_type: FaultType) -> KernelResult<()> {
    with_state(|state| {
        let h = state.handlers.iter_mut().find(|h| h.pid == pid)
            .ok_or(KernelError::NotFound)?;
        match fault_type {
            FaultType::Missing => h.faults_missing += 1,
            FaultType::WriteProtect => h.faults_wp += 1,
            FaultType::Minor => h.faults_minor += 1,
        }
        state.total_faults += 1;
        Ok(())
    })
}

/// Record a fault resolution.
pub fn record_resolve(pid: u32, ns: u64, is_copy: bool) -> KernelResult<()> {
    with_state(|state| {
        let h = state.handlers.iter_mut().find(|h| h.pid == pid)
            .ok_or(KernelError::NotFound)?;
        h.resolves += 1;
        h.total_resolve_ns += ns;
        if ns > h.max_resolve_ns { h.max_resolve_ns = ns; }
        if is_copy { h.copy_pages += 1; state.total_copies += 1; }
        else { h.zero_pages += 1; state.total_zeros += 1; }
        state.total_resolves += 1;
        Ok(())
    })
}

/// Per-process stats.
pub fn per_process() -> Vec<UffdStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.handlers.clone())
}

/// Statistics: (handler_count, total_faults, total_resolves, total_copies, total_zeros, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.handlers.len(), s.total_faults, s.total_resolves, s.total_copies, s.total_zeros, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("userfault::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live handler table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom handlers, zero totals.
    assert_eq!(per_process().len(), 0);
    let (h0, f0, r0, c0, z0, _) = stats();
    assert_eq!((h0, f0, r0, c0, z0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — handler appears zeroed; duplicate is AlreadyExists.
    register(200).expect("register");
    let h = per_process().into_iter().find(|h| h.pid == 200).expect("find");
    assert_eq!((h.registered_ranges, h.faults_missing, h.faults_wp, h.faults_minor), (0, 0, 0, 0));
    assert_eq!((h.resolves, h.copy_pages, h.zero_pages), (0, 0, 0));
    assert_eq!(per_process().len(), 1);
    assert!(register(200).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Faults — each fault type lands in its own counter; global total tracks.
    record_fault(200, FaultType::Missing).expect("missing");
    record_fault(200, FaultType::WriteProtect).expect("wp");
    record_fault(200, FaultType::Minor).expect("minor");
    let h = per_process().into_iter().find(|h| h.pid == 200).expect("p3");
    assert_eq!((h.faults_missing, h.faults_wp, h.faults_minor), (1, 1, 1));
    assert_eq!(stats().1, 3); // total_faults
    crate::serial_println!("  [3/8] fault: OK");

    // 4: Resolve copy — resolves + copy_pages advance; global copy/resolve too.
    record_resolve(200, 5000, true).expect("resolve_copy");
    let h = per_process().into_iter().find(|h| h.pid == 200).expect("p4");
    assert_eq!((h.resolves, h.copy_pages, h.total_resolve_ns), (1, 1, 5000));
    let (_, _, resolves, copies, _, _) = stats();
    assert_eq!((resolves, copies), (1, 1));
    crate::serial_println!("  [4/8] resolve copy: OK");

    // 5: Resolve zero — zero_pages advance; global zero/resolve too.
    record_resolve(200, 2000, false).expect("resolve_zero");
    let h = per_process().into_iter().find(|h| h.pid == 200).expect("p5");
    assert_eq!((h.zero_pages, h.resolves), (1, 2));
    let (_, _, resolves, _, zeros, _) = stats();
    assert_eq!((resolves, zeros), (2, 1));
    crate::serial_println!("  [5/8] resolve zero: OK");

    // 6: Max latency holds the larger of the two resolve durations (5000 > 2000).
    let h = per_process().into_iter().find(|h| h.pid == 200).expect("p6");
    assert_eq!(h.max_resolve_ns, 5000);
    crate::serial_println!("  [6/8] max latency: OK");

    // 7: Unregister — list empties; double/unknown unregister + unknown
    //    fault/resolve all NotFound.
    unregister(200).expect("unregister");
    assert_eq!(per_process().len(), 0);
    assert!(unregister(200).is_err());
    assert!(record_fault(200, FaultType::Missing).is_err());
    assert!(record_resolve(200, 0, true).is_err());
    crate::serial_println!("  [7/8] unregister: OK");

    // 8: Final stats reflect only the real activity above. Global totals are
    //    cumulative and not decremented on unregister: 3 faults, 2 resolves,
    //    1 copy, 1 zero.
    let (handlers, faults, resolves, copies, zeros, ops) = stats();
    assert_eq!((handlers, faults, resolves, copies, zeros), (0, 3, 2, 1, 1));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("userfault::self_test() — all 8 tests passed");
}
