//! POSIX resource limits and usage.
//!
//! Implements `getrlimit`, `setrlimit`, `getrusage`, `nice`,
//! `getpriority`, `setpriority`, and related structures/constants.
//!
//! ## Limitations
//!
//! - Resource limits are not actually enforced — values are stored
//!   in process-local statics and returned by getrlimit, but the
//!   kernel does not enforce them.
//! - getrusage returns zeroes for all fields except user/system time
//!   (which also return zero — no kernel support yet).
//! - nice/getpriority/setpriority are stubs — the kernel scheduler
//!   doesn't expose priority control to POSIX yet.

use crate::errno;

// ---------------------------------------------------------------------------
// Resource limit constants (RLIMIT_*)
// ---------------------------------------------------------------------------

/// Maximum size of the process's virtual memory (address space) in bytes.
pub const RLIMIT_AS: i32 = 9;
/// Maximum size of a core file.
pub const RLIMIT_CORE: i32 = 4;
/// Maximum CPU time in seconds.
pub const RLIMIT_CPU: i32 = 0;
/// Maximum size of the data segment.
pub const RLIMIT_DATA: i32 = 2;
/// Maximum size of files created by the process.
pub const RLIMIT_FSIZE: i32 = 1;
/// Maximum number of open file descriptors.
pub const RLIMIT_NOFILE: i32 = 7;
/// Maximum stack size.
pub const RLIMIT_STACK: i32 = 3;
/// Maximum number of threads.
pub const RLIMIT_NPROC: i32 = 6;
/// Maximum resident set size.
pub const RLIMIT_RSS: i32 = 5;
/// Maximum number of bytes of POSIX message queues.
pub const RLIMIT_MSGQUEUE: i32 = 12;
/// Maximum number of bytes that can be locked in memory.
pub const RLIMIT_MEMLOCK: i32 = 8;
/// Number of resource limit types.
const RLIMIT_NLIMITS: usize = 16;

/// Special value meaning "unlimited".
pub const RLIM_INFINITY: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Resource limit (soft + hard).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Rlimit {
    /// Soft limit (current enforcement level).
    pub rlim_cur: u64,
    /// Hard limit (maximum the soft limit can be raised to).
    pub rlim_max: u64,
}

/// Resource usage statistics.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Rusage {
    /// User CPU time used (POSIX: struct timeval, not timespec).
    pub ru_utime: crate::time::Timeval,
    /// System CPU time used (POSIX: struct timeval, not timespec).
    pub ru_stime: crate::time::Timeval,
    /// Maximum resident set size (in kilobytes).
    pub ru_maxrss: i64,
    // The remaining fields are rarely used but required by POSIX.
    pub ru_ixrss: i64,
    pub ru_idrss: i64,
    pub ru_isrss: i64,
    pub ru_minflt: i64,
    pub ru_majflt: i64,
    pub ru_nswap: i64,
    pub ru_inblock: i64,
    pub ru_oublock: i64,
    pub ru_msgsnd: i64,
    pub ru_msgrcv: i64,
    pub ru_nsignals: i64,
    pub ru_nvcsw: i64,
    pub ru_nivcsw: i64,
}

/// Who to query for getrusage.
pub const RUSAGE_SELF: i32 = 0;
pub const RUSAGE_CHILDREN: i32 = -1;

// ---------------------------------------------------------------------------
// Default limits
// ---------------------------------------------------------------------------

/// Process-local resource limits.
///
/// Initialized to sensible defaults; setrlimit can update them.
/// Not enforced by the kernel — purely advisory for programs that
/// query their own limits.
// Clippy's indexing_slicing lint fires in const context where .get_mut()
// is unavailable.  These indices are all compile-time constants into a
// fixed-size array, so the bounds are statically known to be safe.
#[allow(clippy::indexing_slicing)]
static mut RLIMITS: [Rlimit; RLIMIT_NLIMITS] = {
    const INF: Rlimit = Rlimit {
        rlim_cur: RLIM_INFINITY,
        rlim_max: RLIM_INFINITY,
    };
    let mut limits = [INF; RLIMIT_NLIMITS];

    // Stack: default 8 MiB (matches Linux default).
    limits[RLIMIT_STACK as usize] = Rlimit {
        rlim_cur: 8 * 1024 * 1024,
        rlim_max: RLIM_INFINITY,
    };

    // Open files: 256 (our fd table size).
    limits[RLIMIT_NOFILE as usize] = Rlimit {
        rlim_cur: 256,
        rlim_max: 256,
    };

    // Core dumps: 0 (disabled — we don't support them).
    limits[RLIMIT_CORE as usize] = Rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };

    limits
};

// ---------------------------------------------------------------------------
// getrlimit / setrlimit
// ---------------------------------------------------------------------------

/// Get resource limits.
///
/// Stores the soft and hard limits for `resource` in `*rlp`.
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getrlimit(resource: i32, rlp: *mut Rlimit) -> i32 {
    if rlp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    if resource < 0 || (resource as usize) >= RLIMIT_NLIMITS {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: Single-threaded access to RLIMITS.
    let limits = unsafe { core::ptr::addr_of!(RLIMITS).as_ref() };
    let Some(limits) = limits else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    if let Some(limit) = limits.get(resource as usize) {
        // SAFETY: Caller guarantees rlp is valid.
        unsafe { *rlp = *limit; }
        0
    } else {
        errno::set_errno(errno::EINVAL);
        -1
    }
}

/// Set resource limits.
///
/// Updates the soft and hard limits for `resource`.  The new soft
/// limit must not exceed the hard limit.
///
/// Note: limits are stored but not enforced by the kernel.
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setrlimit(resource: i32, rlp: *const Rlimit) -> i32 {
    if rlp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    if resource < 0 || (resource as usize) >= RLIMIT_NLIMITS {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: Caller guarantees rlp is valid.
    let new_limit = unsafe { *rlp };

    // Soft must not exceed hard.
    if new_limit.rlim_cur > new_limit.rlim_max {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: Single-threaded access to RLIMITS.
    let limits = unsafe { core::ptr::addr_of_mut!(RLIMITS).as_mut() };
    let Some(limits) = limits else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    if let Some(slot) = limits.get_mut(resource as usize) {
        *slot = new_limit;
        0
    } else {
        errno::set_errno(errno::EINVAL);
        -1
    }
}

// ---------------------------------------------------------------------------
// getrusage
// ---------------------------------------------------------------------------

/// Get resource usage.
///
/// Returns zeroed usage data (no kernel accounting support yet).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getrusage(who: i32, usage: *mut Rusage) -> i32 {
    if usage.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    if who != RUSAGE_SELF && who != RUSAGE_CHILDREN {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: Caller guarantees usage is valid.  We zero-fill since
    // we don't have kernel-level resource accounting yet.
    unsafe {
        core::ptr::write_bytes(usage, 0, 1);
    }

    0
}

// ---------------------------------------------------------------------------
// Process priority (nice / getpriority / setpriority)
// ---------------------------------------------------------------------------

/// Process priority target.
pub const PRIO_PROCESS: i32 = 0;
/// Process group priority target.
pub const PRIO_PGRP: i32 = 1;
/// User priority target.
pub const PRIO_USER: i32 = 2;

/// Process-local stored nice value.
///
/// Not enforced by the kernel — purely for programs that query or
/// set their own nice value and expect the call to succeed.
static mut NICE_VALUE: i32 = 0;

/// Adjust the nice value of the calling process.
///
/// Returns the new nice value on success.  Since our scheduler doesn't
/// use nice values, this just stores the value locally.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn nice(inc: i32) -> i32 {
    // SAFETY: Single-threaded access.
    let current = unsafe { core::ptr::addr_of!(NICE_VALUE).read() };
    // Clamp to [-20, 19] per POSIX.
    let new_val = current.saturating_add(inc).clamp(-20, 19);
    unsafe { core::ptr::addr_of_mut!(NICE_VALUE).write(new_val); }
    new_val
}

/// Get the scheduling priority of a process, process group, or user.
///
/// Returns the nice value (which can be negative), so callers must
/// clear errno before calling and check it after.  Returns 0 for
/// all queries (no kernel support).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpriority(which: i32, _who: u32) -> i32 {
    if which != PRIO_PROCESS && which != PRIO_PGRP && which != PRIO_USER {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    errno::set_errno(0); // Clear errno — return value can be negative.
    // SAFETY: Single-threaded access.
    unsafe { core::ptr::addr_of!(NICE_VALUE).read() }
}

/// Set the scheduling priority of a process, process group, or user.
///
/// Stores the value locally but does not affect kernel scheduling.
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setpriority(which: i32, _who: u32, prio: i32) -> i32 {
    if which != PRIO_PROCESS && which != PRIO_PGRP && which != PRIO_USER {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let val = prio.clamp(-20, 19);
    // SAFETY: Single-threaded access.
    unsafe { core::ptr::addr_of_mut!(NICE_VALUE).write(val); }
    0
}

/// Linux-specific: get and/or set resource limits for any process.
///
/// `pid` is the target process (0 = calling process).
/// If `new_limit` is non-null, sets the new limit.
/// If `old_limit` is non-null, stores the old limit.
///
/// Since our kernel doesn't track per-process resource limits, this
/// delegates to the global getrlimit/setrlimit.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn prlimit(
    _pid: i32,
    resource: i32,
    new_limit: *const Rlimit,
    old_limit: *mut Rlimit,
) -> i32 {
    // Get old limit first (if requested).
    if !old_limit.is_null() {
        let ret = getrlimit(resource, old_limit);
        if ret != 0 {
            return ret;
        }
    }

    // Set new limit (if requested).
    if !new_limit.is_null() {
        return setrlimit(resource, new_limit);
    }

    0
}

/// Alias: `prlimit64` — same as `prlimit` on 64-bit systems.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn prlimit64(
    pid: i32,
    resource: i32,
    new_limit: *const Rlimit,
    old_limit: *mut Rlimit,
) -> i32 {
    prlimit(pid, resource, new_limit, old_limit)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: reset RLIMITS to the compile-time defaults.
    ///
    /// Must be called at the start of any test that reads or writes
    /// RLIMITS or NICE_VALUE, because tests share the same process
    /// and the statics are mutable globals.  Run with
    /// `--test-threads=1` to avoid races between tests.
    #[allow(clippy::indexing_slicing)]
    fn reset_global_state() {
        unsafe {
            // Reset NICE_VALUE.
            core::ptr::addr_of_mut!(NICE_VALUE).write(0);

            // Reset RLIMITS to the compile-time defaults.
            let limits = core::ptr::addr_of_mut!(RLIMITS).as_mut().unwrap();
            // First fill everything with RLIM_INFINITY.
            for slot in limits.iter_mut() {
                *slot = Rlimit {
                    rlim_cur: RLIM_INFINITY,
                    rlim_max: RLIM_INFINITY,
                };
            }
            // Stack: 8 MiB soft, unlimited hard.
            limits[RLIMIT_STACK as usize] = Rlimit {
                rlim_cur: 8 * 1024 * 1024,
                rlim_max: RLIM_INFINITY,
            };
            // Open files: 256/256.
            limits[RLIMIT_NOFILE as usize] = Rlimit {
                rlim_cur: 256,
                rlim_max: 256,
            };
            // Core: 0/0.
            limits[RLIMIT_CORE as usize] = Rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
        }
    }

    // -----------------------------------------------------------------------
    // 1. RLIMIT_* constants match Linux values
    // -----------------------------------------------------------------------

    #[test]
    fn rlimit_constants_match_linux() {
        assert_eq!(RLIMIT_CPU, 0);
        assert_eq!(RLIMIT_FSIZE, 1);
        assert_eq!(RLIMIT_DATA, 2);
        assert_eq!(RLIMIT_STACK, 3);
        assert_eq!(RLIMIT_CORE, 4);
        assert_eq!(RLIMIT_RSS, 5);
        assert_eq!(RLIMIT_NPROC, 6);
        assert_eq!(RLIMIT_NOFILE, 7);
        assert_eq!(RLIMIT_MEMLOCK, 8);
        assert_eq!(RLIMIT_AS, 9);
        assert_eq!(RLIMIT_MSGQUEUE, 12);
    }

    // -----------------------------------------------------------------------
    // 2. RLIM_INFINITY equals u64::MAX
    // -----------------------------------------------------------------------

    #[test]
    fn rlim_infinity_is_u64_max() {
        assert_eq!(RLIM_INFINITY, u64::MAX);
    }

    // -----------------------------------------------------------------------
    // 3. PRIO_* constants
    // -----------------------------------------------------------------------

    #[test]
    fn prio_constants() {
        assert_eq!(PRIO_PROCESS, 0);
        assert_eq!(PRIO_PGRP, 1);
        assert_eq!(PRIO_USER, 2);
    }

    // -----------------------------------------------------------------------
    // 4. RUSAGE_* constants
    // -----------------------------------------------------------------------

    #[test]
    fn rusage_constants() {
        assert_eq!(RUSAGE_SELF, 0);
        assert_eq!(RUSAGE_CHILDREN, -1);
    }

    // -----------------------------------------------------------------------
    // 5. getrlimit: valid resource, null pointer, invalid resource
    // -----------------------------------------------------------------------

    #[test]
    fn getrlimit_valid_resource() {
        reset_global_state();
        let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
        let ret = getrlimit(RLIMIT_STACK, &mut rl);
        assert_eq!(ret, 0);
        assert_eq!(rl.rlim_cur, 8 * 1024 * 1024);
        assert_eq!(rl.rlim_max, RLIM_INFINITY);
    }

    #[test]
    fn getrlimit_null_pointer() {
        reset_global_state();
        let ret = getrlimit(RLIMIT_STACK, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn getrlimit_invalid_resource() {
        reset_global_state();
        let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
        let ret = getrlimit(-1, &mut rl);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let ret = getrlimit(9999, &mut rl);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // 6. Default limits
    // -----------------------------------------------------------------------

    #[test]
    fn default_limits_stack() {
        reset_global_state();
        let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_STACK, &mut rl), 0);
        assert_eq!(rl.rlim_cur, 8 * 1024 * 1024, "RLIMIT_STACK soft should be 8 MiB");
        assert_eq!(rl.rlim_max, RLIM_INFINITY, "RLIMIT_STACK hard should be RLIM_INFINITY");
    }

    #[test]
    fn default_limits_nofile() {
        reset_global_state();
        let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_NOFILE, &mut rl), 0);
        assert_eq!(rl.rlim_cur, 256);
        assert_eq!(rl.rlim_max, 256);
    }

    #[test]
    fn default_limits_core() {
        reset_global_state();
        let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_CORE, &mut rl), 0);
        assert_eq!(rl.rlim_cur, 0);
        assert_eq!(rl.rlim_max, 0);
    }

    #[test]
    fn default_limits_others_are_infinity() {
        reset_global_state();
        // Resources that default to RLIM_INFINITY for both soft and hard.
        let inf_resources = [
            RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_RSS,
            RLIMIT_NPROC, RLIMIT_MEMLOCK, RLIMIT_AS, RLIMIT_MSGQUEUE,
        ];
        for &res in &inf_resources {
            let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
            assert_eq!(getrlimit(res, &mut rl), 0, "getrlimit failed for resource {res}");
            assert_eq!(rl.rlim_cur, RLIM_INFINITY, "resource {res} soft should be RLIM_INFINITY");
            assert_eq!(rl.rlim_max, RLIM_INFINITY, "resource {res} hard should be RLIM_INFINITY");
        }
    }

    // -----------------------------------------------------------------------
    // 7. setrlimit: set-and-verify, soft > hard, null pointer, invalid
    // -----------------------------------------------------------------------

    #[test]
    fn setrlimit_set_and_verify() {
        reset_global_state();
        let new = Rlimit { rlim_cur: 1024, rlim_max: 4096 };
        let ret = setrlimit(RLIMIT_CPU, &new);
        assert_eq!(ret, 0);

        let mut readback = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_CPU, &mut readback), 0);
        assert_eq!(readback.rlim_cur, 1024);
        assert_eq!(readback.rlim_max, 4096);
    }

    #[test]
    fn setrlimit_soft_exceeds_hard_rejected() {
        reset_global_state();
        let bad = Rlimit { rlim_cur: 5000, rlim_max: 1000 };
        let ret = setrlimit(RLIMIT_CPU, &bad);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn setrlimit_null_pointer() {
        reset_global_state();
        let ret = setrlimit(RLIMIT_CPU, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn setrlimit_invalid_resource() {
        reset_global_state();
        let new = Rlimit { rlim_cur: 100, rlim_max: 200 };
        let ret = setrlimit(-1, &new);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let ret = setrlimit(9999, &new);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // 8. getrlimit + setrlimit round trip
    // -----------------------------------------------------------------------

    #[test]
    fn getrlimit_setrlimit_round_trip() {
        reset_global_state();
        let new = Rlimit { rlim_cur: 42, rlim_max: 1000 };
        assert_eq!(setrlimit(RLIMIT_DATA, &new), 0);

        let mut readback = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_DATA, &mut readback), 0);
        assert_eq!(readback.rlim_cur, 42);
        assert_eq!(readback.rlim_max, 1000);

        // Update again with different values.
        let new2 = Rlimit { rlim_cur: 500, rlim_max: 500 };
        assert_eq!(setrlimit(RLIMIT_DATA, &new2), 0);

        assert_eq!(getrlimit(RLIMIT_DATA, &mut readback), 0);
        assert_eq!(readback.rlim_cur, 500);
        assert_eq!(readback.rlim_max, 500);
    }

    // -----------------------------------------------------------------------
    // 9. nice: increments, clamping, return value
    // -----------------------------------------------------------------------

    #[test]
    fn nice_increments_stored_value() {
        reset_global_state();
        let val = nice(5);
        assert_eq!(val, 5);
        let val = nice(3);
        assert_eq!(val, 8);
    }

    #[test]
    fn nice_clamps_to_upper_bound() {
        reset_global_state();
        // Start at 0, add 100 => clamped to 19.
        let val = nice(100);
        assert_eq!(val, 19);
    }

    #[test]
    fn nice_clamps_to_lower_bound() {
        reset_global_state();
        // Start at 0, subtract 100 => clamped to -20.
        let val = nice(-100);
        assert_eq!(val, -20);
    }

    #[test]
    fn nice_returns_new_value() {
        reset_global_state();
        assert_eq!(nice(10), 10);
        assert_eq!(nice(-3), 7);
        assert_eq!(nice(0), 7);
    }

    // -----------------------------------------------------------------------
    // 10. getpriority: returns stored value, invalid which
    // -----------------------------------------------------------------------

    #[test]
    fn getpriority_returns_stored_nice() {
        reset_global_state();
        // Set nice to 5 first.
        nice(5);
        let val = getpriority(PRIO_PROCESS, 0);
        assert_eq!(val, 5);
        assert_eq!(errno::get_errno(), 0, "errno should be cleared on success");
    }

    #[test]
    fn getpriority_invalid_which() {
        reset_global_state();
        let val = getpriority(999, 0);
        assert_eq!(val, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn getpriority_all_valid_which_values() {
        reset_global_state();
        nice(3);
        for &which in &[PRIO_PROCESS, PRIO_PGRP, PRIO_USER] {
            let val = getpriority(which, 0);
            assert_eq!(val, 3, "getpriority with which={which} should return nice value");
            assert_eq!(errno::get_errno(), 0);
        }
    }

    // -----------------------------------------------------------------------
    // 11. setpriority: stores value, clamps, invalid which
    // -----------------------------------------------------------------------

    #[test]
    fn setpriority_stores_value() {
        reset_global_state();
        assert_eq!(setpriority(PRIO_PROCESS, 0, 10), 0);
        assert_eq!(getpriority(PRIO_PROCESS, 0), 10);
    }

    #[test]
    fn setpriority_clamps_to_range() {
        reset_global_state();
        assert_eq!(setpriority(PRIO_PROCESS, 0, 50), 0);
        assert_eq!(getpriority(PRIO_PROCESS, 0), 19);

        assert_eq!(setpriority(PRIO_PROCESS, 0, -50), 0);
        assert_eq!(getpriority(PRIO_PROCESS, 0), -20);
    }

    #[test]
    fn setpriority_invalid_which() {
        reset_global_state();
        let ret = setpriority(999, 0, 5);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // 12. getrusage: RUSAGE_SELF zeroed, null, invalid who
    // -----------------------------------------------------------------------

    #[test]
    fn getrusage_self_returns_zeroed() {
        // Pre-fill with garbage to verify it gets zeroed.
        let mut usage = unsafe { core::mem::MaybeUninit::<Rusage>::zeroed().assume_init() };
        usage.ru_maxrss = 999;
        usage.ru_minflt = 42;

        let ret = getrusage(RUSAGE_SELF, &mut usage);
        assert_eq!(ret, 0);
        assert_eq!(usage.ru_utime.tv_sec, 0);
        assert_eq!(usage.ru_utime.tv_usec, 0);
        assert_eq!(usage.ru_stime.tv_sec, 0);
        assert_eq!(usage.ru_stime.tv_usec, 0);
        assert_eq!(usage.ru_maxrss, 0);
        assert_eq!(usage.ru_minflt, 0);
        assert_eq!(usage.ru_majflt, 0);
        assert_eq!(usage.ru_nswap, 0);
        assert_eq!(usage.ru_inblock, 0);
        assert_eq!(usage.ru_oublock, 0);
        assert_eq!(usage.ru_msgsnd, 0);
        assert_eq!(usage.ru_msgrcv, 0);
        assert_eq!(usage.ru_nsignals, 0);
        assert_eq!(usage.ru_nvcsw, 0);
        assert_eq!(usage.ru_nivcsw, 0);
    }

    #[test]
    fn getrusage_children_returns_zeroed() {
        let mut usage = unsafe { core::mem::MaybeUninit::<Rusage>::zeroed().assume_init() };
        let ret = getrusage(RUSAGE_CHILDREN, &mut usage);
        assert_eq!(ret, 0);
        assert_eq!(usage.ru_maxrss, 0);
    }

    #[test]
    fn getrusage_null_pointer() {
        let ret = getrusage(RUSAGE_SELF, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn getrusage_invalid_who() {
        let mut usage = unsafe { core::mem::MaybeUninit::<Rusage>::zeroed().assume_init() };
        let ret = getrusage(99, &mut usage);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------------
    // 13. prlimit: get-only, set-only, get-and-set
    // -----------------------------------------------------------------------

    #[test]
    fn prlimit_get_only() {
        reset_global_state();
        let mut old = Rlimit { rlim_cur: 0, rlim_max: 0 };
        let ret = prlimit(0, RLIMIT_STACK, core::ptr::null(), &mut old);
        assert_eq!(ret, 0);
        assert_eq!(old.rlim_cur, 8 * 1024 * 1024);
        assert_eq!(old.rlim_max, RLIM_INFINITY);
    }

    #[test]
    fn prlimit_set_only() {
        reset_global_state();
        let new = Rlimit { rlim_cur: 100, rlim_max: 200 };
        let ret = prlimit(0, RLIMIT_CPU, &new, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // Verify via getrlimit.
        let mut readback = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_CPU, &mut readback), 0);
        assert_eq!(readback.rlim_cur, 100);
        assert_eq!(readback.rlim_max, 200);
    }

    #[test]
    fn prlimit_get_and_set() {
        reset_global_state();
        // Set an initial value.
        let init = Rlimit { rlim_cur: 10, rlim_max: 20 };
        assert_eq!(setrlimit(RLIMIT_FSIZE, &init), 0);

        // prlimit: get old, set new.
        let new = Rlimit { rlim_cur: 30, rlim_max: 40 };
        let mut old = Rlimit { rlim_cur: 0, rlim_max: 0 };
        let ret = prlimit(0, RLIMIT_FSIZE, &new, &mut old);
        assert_eq!(ret, 0);

        // Old should be what we set initially.
        assert_eq!(old.rlim_cur, 10);
        assert_eq!(old.rlim_max, 20);

        // Current should be the new values.
        let mut readback = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_FSIZE, &mut readback), 0);
        assert_eq!(readback.rlim_cur, 30);
        assert_eq!(readback.rlim_max, 40);
    }

    // -----------------------------------------------------------------------
    // 14. Rlimit struct layout: 16 bytes (two u64s)
    // -----------------------------------------------------------------------

    #[test]
    fn rlimit_struct_size() {
        assert_eq!(
            core::mem::size_of::<Rlimit>(),
            16,
            "Rlimit must be exactly 16 bytes (two u64 fields)"
        );
    }

    #[test]
    fn rlimit_struct_alignment() {
        assert_eq!(
            core::mem::align_of::<Rlimit>(),
            core::mem::align_of::<u64>(),
            "Rlimit alignment must match u64"
        );
    }

    // -----------------------------------------------------------------------
    // prlimit64 — alias for prlimit
    // -----------------------------------------------------------------------

    #[test]
    fn prlimit64_get_and_set() {
        reset_global_state();
        // Set an initial value via setrlimit.
        let init = Rlimit { rlim_cur: 100, rlim_max: 200 };
        assert_eq!(setrlimit(RLIMIT_FSIZE, &init), 0);

        // Use prlimit64 to get old and set new.
        let new = Rlimit { rlim_cur: 300, rlim_max: 400 };
        let mut old = Rlimit { rlim_cur: 0, rlim_max: 0 };
        let ret = prlimit64(0, RLIMIT_FSIZE, &new, &mut old);
        assert_eq!(ret, 0);

        // Old should match initial.
        assert_eq!(old.rlim_cur, 100);
        assert_eq!(old.rlim_max, 200);

        // Verify prlimit64 set the new values.
        let mut readback = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_FSIZE, &mut readback), 0);
        assert_eq!(readback.rlim_cur, 300);
        assert_eq!(readback.rlim_max, 400);
    }

    #[test]
    fn prlimit64_get_only() {
        reset_global_state();
        // prlimit64 with null new_limit should just get current.
        let mut old = Rlimit { rlim_cur: 0, rlim_max: 0 };
        let ret = prlimit64(0, RLIMIT_NOFILE, core::ptr::null(), &mut old);
        assert_eq!(ret, 0);
        assert_eq!(old.rlim_cur, 256);
        assert_eq!(old.rlim_max, 256);
    }
}
