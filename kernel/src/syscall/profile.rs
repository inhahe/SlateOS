//! Syscall profiler — tracks invocation counts and latencies per syscall.
//!
//! Records how many times each syscall number is invoked and how long
//! each invocation takes.  This data answers:
//! - Which syscalls are the hottest? (optimization priority)
//! - Are any syscalls anomalously slow? (performance bugs)
//! - What's the syscall mix for the current workload?
//!
//! ## Design
//!
//! Flat array indexed by syscall number (0..MAX_TRACKED).  Each slot
//! stores an invocation count and a cumulative latency sum.  Overhead
//! is two atomic operations per syscall (one for count, one for latency).
//!
//! TSC-based timing for nanosecond precision.
//!
//! ## Integration
//!
//! The syscall dispatch function calls `profile_enter()` before dispatch
//! and `profile_exit()` after.  The kshell `syscallprof` command shows
//! the accumulated profile.
//!
//! ## References
//!
//! - Linux `arch/x86/entry/syscall_64.c` — syscall entry tracing
//! - Linux ftrace `sys_enter`/`sys_exit` tracepoints
//! - strace — per-syscall timing and statistics

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum syscall number to track.  Keep this smaller than MAX_SYSCALL_NR
/// (1000) to avoid wasting memory on unused high numbers.
const MAX_TRACKED: usize = 256;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Per-syscall invocation counts.
static COUNTS: [AtomicU64; MAX_TRACKED] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_TRACKED]
};

/// Per-syscall cumulative latency (TSC cycles).
static LATENCIES: [AtomicU64; MAX_TRACKED] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_TRACKED]
};

/// Per-syscall maximum latency (TSC cycles).
static MAX_LAT: [AtomicU64; MAX_TRACKED] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_TRACKED]
};

/// Per-syscall error counts.
static ERRORS: [AtomicU64; MAX_TRACKED] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_TRACKED]
};

/// Total syscalls recorded.
static TOTAL_CALLS: AtomicU64 = AtomicU64::new(0);

/// Total syscall errors recorded.
static TOTAL_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Whether profiling is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// TSC MHz for display conversion.
static TSC_MHZ: AtomicU64 = AtomicU64::new(3000);

// ---------------------------------------------------------------------------
// Public API — recording
// ---------------------------------------------------------------------------

/// Begin profiling a syscall.  Returns the TSC start value.
///
/// Called at syscall entry, before dispatch.
#[inline]
pub fn enter(nr: u64) -> u64 {
    if !ENABLED.load(Ordering::Relaxed) {
        return 0;
    }
    let idx = nr as usize;
    if idx < MAX_TRACKED {
        COUNTS[idx].fetch_add(1, Ordering::Relaxed);
    }
    TOTAL_CALLS.fetch_add(1, Ordering::Relaxed);
    rdtsc()
}

/// End profiling a syscall.  Records the elapsed time.
///
/// Called at syscall exit, after the handler returns.
/// `start` is the value from `enter()`.
/// `error` indicates whether the syscall returned an error.
#[inline]
pub fn exit(nr: u64, start: u64, error: bool) {
    if start == 0 {
        return;
    }
    let elapsed = rdtsc().saturating_sub(start);
    let idx = nr as usize;
    if idx < MAX_TRACKED {
        LATENCIES[idx].fetch_add(elapsed, Ordering::Relaxed);

        // Update max latency.
        let mut current = MAX_LAT[idx].load(Ordering::Relaxed);
        while elapsed > current {
            match MAX_LAT[idx].compare_exchange_weak(
                current, elapsed, Ordering::Relaxed, Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }

        if error {
            ERRORS[idx].fetch_add(1, Ordering::Relaxed);
            TOTAL_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — control
// ---------------------------------------------------------------------------

/// Enable syscall profiling.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable syscall profiling.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether profiling is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Set TSC frequency for display conversions.
pub fn set_tsc_mhz(mhz: u64) {
    TSC_MHZ.store(mhz, Ordering::Release);
}

/// Reset all profiling data.
pub fn reset() {
    for i in 0..MAX_TRACKED {
        COUNTS[i].store(0, Ordering::Relaxed);
        LATENCIES[i].store(0, Ordering::Relaxed);
        MAX_LAT[i].store(0, Ordering::Relaxed);
        ERRORS[i].store(0, Ordering::Relaxed);
    }
    TOTAL_CALLS.store(0, Ordering::Relaxed);
    TOTAL_ERRORS.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API — reporting
// ---------------------------------------------------------------------------

/// Per-syscall statistics.
#[derive(Debug, Clone, Copy)]
pub struct SyscallStat {
    /// Syscall number.
    pub nr: u64,
    /// Total invocations.
    pub count: u64,
    /// Total latency (cycles).
    pub total_cycles: u64,
    /// Average latency (cycles).
    pub avg_cycles: u64,
    /// Maximum latency (cycles).
    pub max_cycles: u64,
    /// Error count.
    pub errors: u64,
}

/// Get stats for a specific syscall number.
#[must_use]
pub fn stat(nr: u64) -> Option<SyscallStat> {
    let idx = nr as usize;
    if idx >= MAX_TRACKED {
        return None;
    }
    let count = COUNTS[idx].load(Ordering::Relaxed);
    if count == 0 {
        return None;
    }
    let total = LATENCIES[idx].load(Ordering::Relaxed);
    let max = MAX_LAT[idx].load(Ordering::Relaxed);
    let avg = total.checked_div(count).unwrap_or(0);
    let errors = ERRORS[idx].load(Ordering::Relaxed);

    Some(SyscallStat { nr, count, total_cycles: total, avg_cycles: avg, max_cycles: max, errors })
}

/// Get the top N syscalls by invocation count.
///
/// Returns up to `limit` entries, sorted by count descending.
pub fn top_by_count(buf: &mut [SyscallStat]) -> usize {
    let mut temp = [(0u64, 0u64); MAX_TRACKED]; // (count, nr)
    let mut valid = 0;

    for i in 0..MAX_TRACKED {
        let count = COUNTS[i].load(Ordering::Relaxed);
        if count > 0 {
            temp[valid] = (count, i as u64);
            valid += 1;
        }
    }

    // Sort by count descending (insertion sort, small N).
    for i in 1..valid {
        let mut j = i;
        while j > 0 && temp[j].0 > temp[j - 1].0 {
            temp.swap(j, j - 1);
            j -= 1;
        }
    }

    let to_return = buf.len().min(valid);
    for i in 0..to_return {
        let nr = temp[i].1;
        buf[i] = stat(nr).unwrap_or(SyscallStat {
            nr, count: 0, total_cycles: 0, avg_cycles: 0, max_cycles: 0, errors: 0,
        });
    }
    to_return
}

/// Overall statistics.
#[derive(Debug, Clone, Copy)]
pub struct OverallStats {
    /// Total syscalls profiled.
    pub total_calls: u64,
    /// Total errors.
    pub total_errors: u64,
    /// Number of distinct syscalls observed.
    pub distinct_syscalls: u32,
    /// Whether profiling is enabled.
    pub enabled: bool,
}

/// Get overall profiling stats.
#[must_use]
pub fn overall() -> OverallStats {
    let mut distinct: u32 = 0;
    for i in 0..MAX_TRACKED {
        if COUNTS[i].load(Ordering::Relaxed) > 0 {
            distinct += 1;
        }
    }

    OverallStats {
        total_calls: TOTAL_CALLS.load(Ordering::Relaxed),
        total_errors: TOTAL_ERRORS.load(Ordering::Relaxed),
        distinct_syscalls: distinct,
        enabled: ENABLED.load(Ordering::Relaxed),
    }
}

/// Convert cycles to nanoseconds using the configured TSC frequency.
pub fn cycles_to_ns(cycles: u64) -> u64 {
    let mhz = TSC_MHZ.load(Ordering::Relaxed);
    if mhz == 0 { return 0; }
    cycles.saturating_mul(1000).checked_div(mhz).unwrap_or(0)
}

/// Get a human-readable name for a syscall number.
#[must_use]
pub fn syscall_name(nr: u64) -> &'static str {
    match nr {
        0 => "yield",
        1 => "exit",
        2 => "task_id",
        10 => "clock_mono",
        11 => "sleep",
        12 => "timer_create",
        13 => "timer_cancel",
        20 => "mmap",
        21 => "munmap",
        30 => "irq_register",
        31 => "irq_wait",
        32 => "irq_release",
        40 => "port_read",
        41 => "port_write",
        50 => "sched_set_ts",
        51 => "sched_get_ts",
        52 => "sched_reconf",
        53 => "sched_set_prof",
        54 => "sched_get_prof",
        60 => "sysctl_get",
        61 => "sysctl_set",
        70 => "mm_set_prof",
        71 => "mm_get_prof",
        80 => "sys_set_prof",
        99 => "debug_print",
        100 => "con_write",
        101 => "con_read",
        102 => "log_read",
        103 => "con_try_read",
        200 => "chan_create",
        201 => "chan_send",
        202 => "chan_recv",
        203 => "chan_try_recv",
        204 => "chan_close",
        210 => "futex_wait",
        211 => "futex_wake",
        220 => "pipe_create",
        221 => "pipe_write",
        222 => "pipe_read",
        223 => "pipe_close",
        230 => "shm_create",
        231 => "shm_write",
        232 => "shm_read",
        233 => "shm_close",
        240 => "eventfd_create",
        241 => "eventfd_write",
        242 => "eventfd_read",
        243 => "eventfd_close",
        250 => "cp_create",
        251 => "cp_wait",
        252 => "cp_try_wait",
        253 => "cp_register",
        254 => "cp_unregister",
        255 => "cp_close",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

#[inline]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for syscall profiling.
pub fn self_test() {
    serial_println!("[syscall_prof] Running self-test...");

    // Test 1: Reset state.
    reset();
    let o = overall();
    assert_eq!(o.total_calls, 0);
    assert_eq!(o.distinct_syscalls, 0);
    serial_println!("[syscall_prof]   Reset: OK");

    // Test 2: Record a syscall entry/exit.
    let start = enter(0); // SYS_YIELD
    // Simulate some work.
    let mut dummy = 0u64;
    for i in 0..50u64 {
        dummy = dummy.wrapping_add(i);
    }
    core::hint::black_box(dummy);
    exit(0, start, false);

    let s = stat(0).expect("syscall 0 should have stats");
    assert_eq!(s.count, 1);
    assert!(s.avg_cycles > 0);
    assert_eq!(s.errors, 0);
    serial_println!("[syscall_prof]   Single syscall: OK (avg={}ns)", cycles_to_ns(s.avg_cycles));

    // Test 3: Multiple syscalls.
    for _ in 0..10 {
        let s = enter(2); // SYS_TASK_ID
        exit(2, s, false);
    }
    let s = stat(2).expect("syscall 2 should have stats");
    assert_eq!(s.count, 10);
    serial_println!("[syscall_prof]   Multiple invocations: OK (10 calls)");

    // Test 4: Error tracking.
    let start = enter(200); // SYS_CHANNEL_CREATE
    exit(200, start, true);
    let s = stat(200).expect("syscall 200 should have stats");
    assert_eq!(s.count, 1);
    assert_eq!(s.errors, 1);
    serial_println!("[syscall_prof]   Error tracking: OK");

    // Test 5: top_by_count.
    let mut top = [SyscallStat { nr: 0, count: 0, total_cycles: 0, avg_cycles: 0, max_cycles: 0, errors: 0 }; 8];
    let n = top_by_count(&mut top);
    assert!(n >= 3); // At least 3 distinct syscalls.
    assert_eq!(top[0].nr, 2); // SYS_TASK_ID has 10 calls (most).
    serial_println!("[syscall_prof]   Top by count: #{} is syscall {} ({} calls)",
        1, syscall_name(top[0].nr), top[0].count);

    // Test 6: Overall stats.
    let o = overall();
    assert_eq!(o.total_calls, 12); // 1 + 10 + 1
    assert_eq!(o.total_errors, 1);
    assert_eq!(o.distinct_syscalls, 3);
    serial_println!("[syscall_prof]   Overall: {} calls, {} distinct, {} errors",
        o.total_calls, o.distinct_syscalls, o.total_errors);

    // Test 7: Disable/enable.
    disable();
    let before = TOTAL_CALLS.load(Ordering::Relaxed);
    let s = enter(99);
    assert_eq!(s, 0); // Returns 0 when disabled.
    assert_eq!(TOTAL_CALLS.load(Ordering::Relaxed), before);
    enable();
    serial_println!("[syscall_prof]   Disable/enable: OK");

    // Test 8: syscall_name lookup.
    assert_eq!(syscall_name(0), "yield");
    assert_eq!(syscall_name(200), "chan_create");
    assert_eq!(syscall_name(999), "unknown");
    serial_println!("[syscall_prof]   Name lookup: OK");

    // Cleanup.
    reset();

    serial_println!("[syscall_prof] Self-test PASSED");
}
