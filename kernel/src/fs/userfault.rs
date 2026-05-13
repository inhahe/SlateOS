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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        handlers: alloc::vec![
            UffdStats { pid: 1, registered_ranges: 5, faults_missing: 100_000, faults_wp: 50_000, faults_minor: 10_000, resolves: 160_000, total_resolve_ns: 800_000_000, max_resolve_ns: 50_000, copy_pages: 100_000, zero_pages: 60_000 },
        ],
        total_faults: 160_000,
        total_resolves: 160_000,
        total_copies: 100_000,
        total_zeros: 60_000,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_process().len(), 1);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register(200).expect("register");
    assert!(register(200).is_err());
    assert_eq!(per_process().len(), 2);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Fault.
    record_fault(200, FaultType::Missing).expect("fault");
    let h = per_process().iter().find(|h| h.pid == 200).cloned().unwrap();
    assert_eq!(h.faults_missing, 1);
    crate::serial_println!("  [3/8] fault: OK");

    // 4: Resolve copy.
    record_resolve(200, 5000, true).expect("resolve_copy");
    let h = per_process().iter().find(|h| h.pid == 200).cloned().unwrap();
    assert_eq!(h.resolves, 1);
    assert_eq!(h.copy_pages, 1);
    crate::serial_println!("  [4/8] resolve copy: OK");

    // 5: Resolve zero.
    record_resolve(200, 2000, false).expect("resolve_zero");
    let h = per_process().iter().find(|h| h.pid == 200).cloned().unwrap();
    assert_eq!(h.zero_pages, 1);
    crate::serial_println!("  [5/8] resolve zero: OK");

    // 6: Max latency.
    let h = per_process().iter().find(|h| h.pid == 200).cloned().unwrap();
    assert_eq!(h.max_resolve_ns, 5000);
    crate::serial_println!("  [6/8] max latency: OK");

    // 7: Unregister.
    unregister(200).expect("unregister");
    assert_eq!(per_process().len(), 1);
    assert!(unregister(200).is_err());
    crate::serial_println!("  [7/8] unregister: OK");

    // 8: Stats.
    let (handlers, faults, resolves, copies, zeros, ops) = stats();
    assert_eq!(handlers, 1);
    assert!(faults > 160_000);
    assert!(resolves > 160_000);
    assert!(copies > 100_000);
    assert!(zeros > 60_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("userfault::self_test() — all 8 tests passed");
}
