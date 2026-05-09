//! POSIX scheduling functions (`<sched.h>`).
//!
//! Provides scheduler policy and parameter functions.  Our OS uses
//! its own priority-based scheduler accessed via `SYS_SCHED_SET_PROFILE`,
//! not POSIX scheduling policies.  These stubs allow programs that
//! query or set scheduling parameters to link and run.
//!
//! Functions: `sched_getscheduler`, `sched_setscheduler`,
//! `sched_getparam`, `sched_setparam`, `sched_get_priority_min`,
//! `sched_get_priority_max`, `sched_rr_get_interval`.
//!
//! `sched_yield` is in `pthread.rs` (it's commonly grouped with
//! pthreads in POSIX implementations).

use crate::errno;

// ---------------------------------------------------------------------------
// Scheduling policies
// ---------------------------------------------------------------------------

/// Normal (time-sharing) scheduling policy.
pub const SCHED_OTHER: i32 = 0;
/// First-in first-out real-time policy.
pub const SCHED_FIFO: i32 = 1;
/// Round-robin real-time policy.
pub const SCHED_RR: i32 = 2;
/// Batch scheduling policy (Linux extension).
pub const SCHED_BATCH: i32 = 3;
/// Idle scheduling policy (Linux extension).
pub const SCHED_IDLE: i32 = 5;

// ---------------------------------------------------------------------------
// sched_param
// ---------------------------------------------------------------------------

/// Scheduling parameters.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SchedParam {
    /// Scheduling priority.
    pub sched_priority: i32,
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Get the scheduling policy of a process.
///
/// Returns `SCHED_OTHER` for all processes (our scheduler doesn't
/// use POSIX policies).
#[unsafe(no_mangle)]
pub extern "C" fn sched_getscheduler(_pid: i32) -> i32 {
    SCHED_OTHER
}

/// Set the scheduling policy and parameters of a process.
///
/// Stub: succeeds silently.  Our scheduler uses its own priority
/// system via `SYS_SCHED_SET_PROFILE`.
#[unsafe(no_mangle)]
pub extern "C" fn sched_setscheduler(
    _pid: i32,
    _policy: i32,
    _param: *const SchedParam,
) -> i32 {
    0
}

/// Get the scheduling parameters of a process.
///
/// Returns priority 0 (default).
#[unsafe(no_mangle)]
pub extern "C" fn sched_getparam(pid: i32, param: *mut SchedParam) -> i32 {
    let _ = pid;
    if param.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: param verified non-null.
    unsafe { (*param).sched_priority = 0; }
    0
}

/// Set the scheduling parameters of a process.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn sched_setparam(
    _pid: i32,
    _param: *const SchedParam,
) -> i32 {
    0
}

/// Get the minimum priority for a scheduling policy.
///
/// Returns 0 for all policies.
#[unsafe(no_mangle)]
pub extern "C" fn sched_get_priority_min(_policy: i32) -> i32 {
    0
}

/// Get the maximum priority for a scheduling policy.
///
/// Returns 99 for real-time policies, 0 for SCHED_OTHER.
#[unsafe(no_mangle)]
pub extern "C" fn sched_get_priority_max(policy: i32) -> i32 {
    match policy {
        SCHED_FIFO | SCHED_RR => 99,
        _ => 0,
    }
}

/// Get the round-robin time quantum.
///
/// Returns 100ms (a typical default) for all processes.
#[unsafe(no_mangle)]
pub extern "C" fn sched_rr_get_interval(
    _pid: i32,
    tp: *mut crate::stat::Timespec,
) -> i32 {
    if tp.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Return 100ms quantum.
    // SAFETY: tp verified non-null.
    unsafe {
        (*tp).tv_sec = 0;
        (*tp).tv_nsec = 100_000_000; // 100ms.
    }
    0
}
