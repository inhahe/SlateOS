//! Syscall tracing — per-event capture for strace-like debugging.
//!
//! Records individual syscall entry/exit events in a ring buffer, capturing
//! the syscall number, arguments, return value, PID, and timestamp.  This
//! complements `syscall::profile` (aggregate stats) by providing a detailed
//! event log for debugging specific interactions.
//!
//! ## Design
//!
//! - Fixed-size ring buffer (64 entries) — minimal memory footprint.
//! - Per-PID filtering: trace only a specific process, or all.
//! - Enable/disable at runtime via kshell.
//! - Lock-free writes (atomic write position).
//! - Captures up to 4 syscall arguments (the most diagnostically useful).
//!
//! ## Usage
//!
//! ```text
//! kshell> strace on           — enable tracing for all PIDs
//! kshell> strace pid 3        — trace only PID 3
//! kshell> strace              — show recent traced events
//! kshell> strace off          — disable tracing
//! ```
//!
//! ## Overhead
//!
//! When disabled: zero overhead (single atomic load check).
//! When enabled: ~50ns per syscall (TSC read + ring write).
//!
//! ## References
//!
//! - Linux ptrace + strace — per-syscall argument/result tracing
//! - DTrace syscall provider — lightweight syscall event capture
//! - Windows ETW SystemCall events — per-event syscall logging

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Ring buffer capacity (power of 2).
const RING_SIZE: usize = 64;
const RING_MASK: usize = RING_SIZE - 1;

/// Maximum number of arguments captured per event.
const MAX_ARGS: usize = 4;

// ---------------------------------------------------------------------------
// Trace event
// ---------------------------------------------------------------------------

/// A single syscall trace event.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TraceEvent {
    /// TSC timestamp at syscall entry.
    pub timestamp: u64,
    /// Process ID.
    pub pid: u32,
    /// Syscall number.
    pub syscall_nr: u32,
    /// First 4 arguments (register values).
    pub args: [u64; MAX_ARGS],
    /// Return value (set on exit, 0 if entry-only).
    pub result: i64,
    /// Duration in TSC cycles (0 if entry-only).
    pub duration_cycles: u64,
    /// Whether this is a complete entry+exit event.
    pub complete: bool,
}

impl TraceEvent {
    pub const fn empty() -> Self {
        Self {
            timestamp: 0,
            pid: 0,
            syscall_nr: 0,
            args: [0; MAX_ARGS],
            result: 0,
            duration_cycles: 0,
            complete: false,
        }
    }

    /// Whether this event slot is occupied.
    pub fn is_valid(&self) -> bool {
        self.timestamp != 0
    }
}

// ---------------------------------------------------------------------------
// Ring buffer storage
// ---------------------------------------------------------------------------

struct TraceRing(core::cell::UnsafeCell<[TraceEvent; RING_SIZE]>);
unsafe impl Sync for TraceRing {}

static RING: TraceRing = TraceRing(core::cell::UnsafeCell::new(
    [TraceEvent::empty(); RING_SIZE]
));

/// Write position.
static WRITE_POS: AtomicU32 = AtomicU32::new(0);

/// Whether tracing is enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// PID filter (0 = trace all PIDs).
static FILTER_PID: AtomicU32 = AtomicU32::new(0);

/// Total events recorded.
static TOTAL_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Events dropped (filtered out).
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — recording
// ---------------------------------------------------------------------------

/// Record a complete syscall event (entry + exit).
///
/// Called by the syscall dispatch path after the handler returns.
/// Only records if tracing is enabled and PID matches filter.
#[inline]
pub fn record(
    pid: u32,
    syscall_nr: u32,
    args: &[u64; 6],
    result: i64,
    entry_tsc: u64,
    exit_tsc: u64,
) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // PID filter check.
    let filter = FILTER_PID.load(Ordering::Relaxed);
    if filter != 0 && filter != pid {
        DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    let event = TraceEvent {
        timestamp: entry_tsc,
        pid,
        syscall_nr,
        args: [args[0], args[1], args[2], args[3]],
        result,
        duration_cycles: exit_tsc.saturating_sub(entry_tsc),
        complete: true,
    };

    // Write to ring buffer.
    // SAFETY: slot is masked to RING_MASK (< RING_SIZE).
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed);
    let slot = (pos as usize) & RING_MASK;
    unsafe {
        let ptr = RING.0.get() as *mut TraceEvent;
        ptr.add(slot).write(event);
    }

    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);
}

/// Record a syscall entry only (for when you want to log before dispatch).
///
/// Less common — mainly useful for tracing syscalls that might not return
/// (e.g., exit, exec).
#[inline]
pub fn record_entry(pid: u32, syscall_nr: u32, args: &[u64; 6]) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let filter = FILTER_PID.load(Ordering::Relaxed);
    if filter != 0 && filter != pid {
        return;
    }

    let event = TraceEvent {
        timestamp: rdtsc(),
        pid,
        syscall_nr,
        args: [args[0], args[1], args[2], args[3]],
        result: 0,
        duration_cycles: 0,
        complete: false,
    };

    // SAFETY: slot is masked to RING_MASK (< RING_SIZE).
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed);
    let slot = (pos as usize) & RING_MASK;
    unsafe {
        let ptr = RING.0.get() as *mut TraceEvent;
        ptr.add(slot).write(event);
    }

    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API — control
// ---------------------------------------------------------------------------

/// Enable syscall tracing.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable syscall tracing.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether tracing is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Set PID filter (0 = trace all).
pub fn set_pid_filter(pid: u32) {
    FILTER_PID.store(pid, Ordering::Release);
}

/// Get current PID filter.
#[must_use]
pub fn pid_filter() -> u32 {
    FILTER_PID.load(Ordering::Relaxed)
}

/// Reset the trace log.
pub fn reset() {
    WRITE_POS.store(0, Ordering::Release);
    TOTAL_EVENTS.store(0, Ordering::Relaxed);
    DROPPED_EVENTS.store(0, Ordering::Relaxed);
    // SAFETY: i < RING_SIZE; RING uses UnsafeCell with atomic position guard.
    for i in 0..RING_SIZE {
        unsafe {
            let ptr = RING.0.get() as *mut TraceEvent;
            ptr.add(i).write(TraceEvent::empty());
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — querying
// ---------------------------------------------------------------------------

/// Trace statistics.
#[derive(Debug, Clone, Copy)]
pub struct TraceStats {
    pub enabled: bool,
    pub pid_filter: u32,
    pub total_events: u64,
    pub dropped_events: u64,
    pub ring_entries: usize,
}

/// Get trace statistics.
#[must_use]
pub fn stats() -> TraceStats {
    let write_pos = WRITE_POS.load(Ordering::Relaxed) as usize;
    let entries = write_pos.min(RING_SIZE);

    TraceStats {
        enabled: ENABLED.load(Ordering::Relaxed),
        pid_filter: FILTER_PID.load(Ordering::Relaxed),
        total_events: TOTAL_EVENTS.load(Ordering::Relaxed),
        dropped_events: DROPPED_EVENTS.load(Ordering::Relaxed),
        ring_entries: entries,
    }
}

/// Get the most recent N trace events (newest first).
pub fn recent(buf: &mut [TraceEvent]) -> usize {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let available = write_pos.min(RING_SIZE);
    let to_copy = buf.len().min(available);

    // SAFETY: idx is masked to RING_MASK (< RING_SIZE).
    for i in 0..to_copy {
        let idx = (write_pos.wrapping_sub(1).wrapping_sub(i)) & RING_MASK;
        unsafe {
            let ptr = RING.0.get() as *const TraceEvent;
            buf[i] = ptr.add(idx).read();
        }
    }

    to_copy
}

/// Get events for a specific PID from the ring buffer.
pub fn events_for_pid(pid: u32, buf: &mut [TraceEvent]) -> usize {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let count = write_pos.min(RING_SIZE);
    let start = if write_pos <= RING_SIZE { 0 } else { write_pos & RING_MASK };

    let mut found = 0;
    for i in 0..count {
        if found >= buf.len() {
            break;
        }
        // Read newest first.
        // SAFETY: idx is masked to RING_MASK (< RING_SIZE).
        let idx = (write_pos.wrapping_sub(1).wrapping_sub(i)) & RING_MASK;
        let event = unsafe {
            let ptr = RING.0.get() as *const TraceEvent;
            ptr.add(idx).read()
        };
        if event.is_valid() && event.pid == pid {
            buf[found] = event;
            found += 1;
        }
    }
    let _ = start; // suppress unused warning
    found
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read TSC (Time Stamp Counter).
#[inline]
fn rdtsc() -> u64 {
    // SAFETY: _rdtsc is always available on x86_64.
    unsafe {
        core::arch::x86_64::_rdtsc() as u64
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for syscall tracing.
pub fn self_test() {
    serial_println!("[syscall_trace] Running self-test...");

    // Test 1: Reset state.
    reset();
    let s = stats();
    assert_eq!(s.total_events, 0);
    assert!(!s.enabled);
    serial_println!("[syscall_trace]   Reset: OK");

    // Test 2: Recording while disabled does nothing.
    let args = [1u64, 2, 3, 4, 5, 6];
    record(1, 42, &args, 0, 1000, 2000);
    assert_eq!(stats().total_events, 0);
    serial_println!("[syscall_trace]   Disabled no-op: OK");

    // Test 3: Enable and record.
    enable();
    assert!(is_enabled());
    record(1, 42, &args, 0, 1000, 2000);
    record(1, 43, &args, -1, 3000, 3500);
    record(2, 44, &args, 99, 4000, 4200);

    let s = stats();
    assert_eq!(s.total_events, 3);
    assert_eq!(s.ring_entries, 3);
    serial_println!("[syscall_trace]   Record events: OK (3 events)");

    // Test 4: Recent entries (newest first).
    let mut buf = [TraceEvent::empty(); 8];
    let n = recent(&mut buf);
    assert_eq!(n, 3);
    // Newest first = PID 2, syscall 44.
    assert_eq!(buf[0].pid, 2);
    assert_eq!(buf[0].syscall_nr, 44);
    assert_eq!(buf[0].result, 99);
    assert_eq!(buf[0].duration_cycles, 200);
    assert!(buf[0].complete);
    // Second = PID 1, syscall 43.
    assert_eq!(buf[1].pid, 1);
    assert_eq!(buf[1].syscall_nr, 43);
    assert_eq!(buf[1].result, -1);
    serial_println!("[syscall_trace]   Recent (newest first): OK");

    // Test 5: PID filter.
    set_pid_filter(1);
    assert_eq!(pid_filter(), 1);
    record(1, 50, &args, 0, 5000, 5100); // Should be recorded.
    record(2, 51, &args, 0, 6000, 6100); // Should be filtered.
    let s = stats();
    assert_eq!(s.total_events, 4); // Only one more.
    assert_eq!(s.dropped_events, 1);
    serial_println!("[syscall_trace]   PID filter: OK (1 recorded, 1 dropped)");

    // Test 6: Per-PID query.
    set_pid_filter(0); // Reset filter.
    let mut pid_buf = [TraceEvent::empty(); 8];
    let n = events_for_pid(1, &mut pid_buf);
    // PID 1 events: syscall 42, 43, 50.
    assert_eq!(n, 3);
    serial_println!("[syscall_trace]   Per-PID query: OK ({} events for PID 1)", n);

    // Test 7: Arguments captured correctly.
    let test_args = [0xDEAD_u64, 0xBEEF, 0xCAFE, 0xF00D, 0x1234, 0x5678];
    record(5, 100, &test_args, 42, 7000, 7500);
    let mut buf = [TraceEvent::empty(); 1];
    let n = recent(&mut buf);
    assert_eq!(n, 1);
    assert_eq!(buf[0].args[0], 0xDEAD);
    assert_eq!(buf[0].args[1], 0xBEEF);
    assert_eq!(buf[0].args[2], 0xCAFE);
    assert_eq!(buf[0].args[3], 0xF00D);
    serial_println!("[syscall_trace]   Argument capture: OK");

    // Test 8: Entry-only recording.
    let entry_args = [10u64, 20, 30, 40, 50, 60];
    record_entry(7, 200, &entry_args);
    let mut buf = [TraceEvent::empty(); 1];
    let n = recent(&mut buf);
    assert_eq!(n, 1);
    assert_eq!(buf[0].pid, 7);
    assert_eq!(buf[0].syscall_nr, 200);
    assert!(!buf[0].complete);
    assert_eq!(buf[0].duration_cycles, 0);
    serial_println!("[syscall_trace]   Entry-only: OK");

    // Cleanup.
    disable();
    reset();

    serial_println!("[syscall_trace] Self-test PASSED");
}
