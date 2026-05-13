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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn prlimit64(
    pid: i32,
    resource: i32,
    new_limit: *const Rlimit,
    old_limit: *mut Rlimit,
) -> i32 {
    prlimit(pid, resource, new_limit, old_limit)
}
