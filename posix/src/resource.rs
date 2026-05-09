//! POSIX resource limits and usage.
//!
//! Implements `getrlimit`, `setrlimit`, `getrusage`, and related
//! structures and constants.
//!
//! ## Limitations
//!
//! - Resource limits are not actually enforced — values are stored
//!   in process-local statics and returned by getrlimit, but the
//!   kernel does not enforce them.
//! - getrusage returns zeroes for all fields except user/system time
//!   (which also return zero — no kernel support yet).

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
    /// User CPU time used.
    pub ru_utime: crate::stat::Timespec,
    /// System CPU time used.
    pub ru_stime: crate::stat::Timespec,
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
