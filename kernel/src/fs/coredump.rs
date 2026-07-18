//! Core Dump — crash dump management.
//!
//! Manages process core dumps: storage, listing, analysis metadata,
//! and cleanup. Configures dump patterns and size limits.
//!
//! ## Architecture
//!
//! ```text
//! Core dump management
//!   → coredump::write(pid, signal, data) → store dump
//!   → coredump::list() → list stored dumps
//!   → coredump::get(id) → read dump metadata
//!   → coredump::cleanup() → remove old dumps
//!
//! Integration:
//!   → crashreport (crash reporting)
//!   → dumpanalyzer (dump analysis)
//!   → diskclean (disk cleanup)
//!   → storageclean (storage cleanup)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Dump trigger reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpReason {
    Segfault,
    Abort,
    IllegalInstruction,
    BusError,
    FloatingPoint,
    UserRequest,
    Watchdog,
}

impl DumpReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Segfault => "SIGSEGV",
            Self::Abort => "SIGABRT",
            Self::IllegalInstruction => "SIGILL",
            Self::BusError => "SIGBUS",
            Self::FloatingPoint => "SIGFPE",
            Self::UserRequest => "User request",
            Self::Watchdog => "Watchdog",
        }
    }
}

/// A stored core dump record.
#[derive(Debug, Clone)]
pub struct CoreDumpRecord {
    pub id: u32,
    pub pid: u32,
    pub process_name: String,
    pub reason: DumpReason,
    pub timestamp_ns: u64,
    pub size_bytes: u64,
    pub path: String,
    pub signal_code: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DUMPS: usize = 100;

struct State {
    dumps: Vec<CoreDumpRecord>,
    next_id: u32,
    dump_pattern: String,
    max_size_bytes: u64,
    dumps_enabled: bool,
    total_dumps: u64,
    total_bytes: u64,
    total_cleaned: u64,
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
        dumps: Vec::new(),
        next_id: 1,
        dump_pattern: String::from("/var/crash/core.%p.%t"),
        max_size_bytes: 1_073_741_824, // 1 GiB
        dumps_enabled: true,
        total_dumps: 0,
        total_bytes: 0,
        total_cleaned: 0,
        ops: 0,
    });
}

/// Record a core dump.
pub fn record_dump(pid: u32, process_name: &str, reason: DumpReason, size_bytes: u64, signal_code: u32) -> KernelResult<u32> {
    with_state(|state| {
        if !state.dumps_enabled {
            return Err(KernelError::PermissionDenied);
        }
        if size_bytes > state.max_size_bytes {
            return Err(KernelError::FileTooLarge);
        }
        if state.dumps.len() >= MAX_DUMPS {
            // Remove oldest.
            state.dumps.remove(0);
            state.total_cleaned += 1;
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        let path = format!("/var/crash/core.{}.{}", pid, id);
        state.dumps.push(CoreDumpRecord {
            id, pid, process_name: String::from(process_name), reason,
            timestamp_ns: now, size_bytes, path, signal_code,
        });
        state.total_dumps += 1;
        state.total_bytes += size_bytes;
        Ok(id)
    })
}

/// Get dump record by ID.
pub fn get_dump(id: u32) -> Option<CoreDumpRecord> {
    STATE.lock().as_ref().and_then(|s| s.dumps.iter().find(|d| d.id == id).cloned())
}

/// List all dumps, newest first.
pub fn list_dumps() -> Vec<CoreDumpRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut dumps = s.dumps.clone();
        dumps.reverse();
        dumps
    })
}

/// Delete a dump.
pub fn delete_dump(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.dumps.len();
        state.dumps.retain(|d| d.id != id);
        if state.dumps.len() == before { return Err(KernelError::NotFound); }
        state.total_cleaned += 1;
        Ok(())
    })
}

/// Cleanup old dumps, keep only the newest N.
pub fn cleanup(keep: usize) -> KernelResult<u32> {
    with_state(|state| {
        if state.dumps.len() <= keep { return Ok(0); }
        let remove_count = state.dumps.len() - keep;
        state.dumps.drain(0..remove_count);
        state.total_cleaned += remove_count as u64;
        Ok(remove_count as u32)
    })
}

/// Enable/disable core dumps.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.dumps_enabled = enabled;
        Ok(())
    })
}

/// Set max dump size.
pub fn set_max_size(bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        state.max_size_bytes = bytes;
        Ok(())
    })
}

/// Check if dumps are enabled.
pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.dumps_enabled)
}

/// Get total bytes used by dumps.
pub fn total_size() -> u64 {
    STATE.lock().as_ref().map_or(0, |s| s.dumps.iter().map(|d| d.size_bytes).sum())
}

/// Statistics: (dump_count, total_dumps, total_bytes, total_cleaned, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.dumps.len(), s.total_dumps, s.total_bytes, s.total_cleaned, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("coredump::self_test() — running tests...");
    init_defaults();

    // 1: Empty.
    assert!(list_dumps().is_empty());
    assert!(is_enabled());
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record dump.
    let id = record_dump(1234, "test_app", DumpReason::Segfault, 1_000_000, 11).expect("record");
    assert_eq!(list_dumps().len(), 1);
    crate::serial_println!("  [2/8] record: OK");

    // 3: Get dump.
    let dump = get_dump(id).expect("get");
    assert_eq!(dump.pid, 1234);
    assert_eq!(dump.reason, DumpReason::Segfault);
    assert_eq!(dump.size_bytes, 1_000_000);
    crate::serial_println!("  [3/8] get: OK");

    // 4: Multiple dumps.
    record_dump(5678, "other_app", DumpReason::Abort, 500_000, 6).expect("record2");
    record_dump(9012, "crash_app", DumpReason::BusError, 2_000_000, 7).expect("record3");
    assert_eq!(list_dumps().len(), 3);
    crate::serial_println!("  [4/8] multiple: OK");

    // 5: Size limit.
    assert!(record_dump(1111, "big_app", DumpReason::Segfault, 2_000_000_000, 11).is_err());
    crate::serial_println!("  [5/8] size limit: OK");

    // 6: Delete dump.
    delete_dump(id).expect("delete");
    assert_eq!(list_dumps().len(), 2);
    assert!(get_dump(id).is_none());
    crate::serial_println!("  [6/8] delete: OK");

    // 7: Cleanup.
    let removed = cleanup(1).expect("cleanup");
    assert_eq!(removed, 1);
    assert_eq!(list_dumps().len(), 1);
    crate::serial_println!("  [7/8] cleanup: OK");

    // 8: Stats.
    let (count, total, bytes, cleaned, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(total, 3);
    assert!(bytes > 0);
    assert!(cleaned >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("coredump::self_test() — all 8 tests passed");
}
