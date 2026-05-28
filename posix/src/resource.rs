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
/// Maximum number of file locks.
pub const RLIMIT_LOCKS: i32 = 10;
/// Maximum number of pending signals.
pub const RLIMIT_SIGPENDING: i32 = 11;
/// Ceiling for the process nice value (since Linux 2.6.12).
pub const RLIMIT_NICE: i32 = 13;
/// Ceiling on real-time scheduling priority (since Linux 2.6.12).
pub const RLIMIT_RTPRIO: i32 = 14;
/// Limit on CPU time for real-time processes (microseconds, since Linux 2.6.25).
pub const RLIMIT_RTTIME: i32 = 15;
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
/// Linux extension: resource usage of the calling thread.
pub const RUSAGE_THREAD: i32 = 1;

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

    // Open files: matches fd table size.
    limits[RLIMIT_NOFILE as usize] = Rlimit {
        rlim_cur: crate::fdtable::MAX_FDS as u64,
        rlim_max: crate::fdtable::MAX_FDS as u64,
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
///
/// Validation order matches Linux's `SYSCALL_DEFINE2(getrlimit)`
/// (`kernel/sys.c`): the kernel calls `do_prlimit(current, resource,
/// NULL, &value)` *first* — which validates `resource >= RLIM_NLIMITS`
/// → `-EINVAL` — and only then does `copy_to_user(rlim, &value, ...)`,
/// which can fail with `-EFAULT`. A buggy caller passing both a bad
/// resource ordinal and a NULL pointer therefore observes `EINVAL`
/// on Linux, not `EFAULT`. We pin that same order so userspace
/// (libc's `getrlimit(3)` wrapper, Python's `resource.getrlimit`)
/// sees identical errno on either malformed-input combination.
///
/// `setrlimit` uses the opposite order (EFAULT before EINVAL)
/// because Linux's `SYSCALL_DEFINE2(setrlimit)` does
/// `copy_from_user(&new_rlim, ...)` before `do_prlimit` — so the
/// asymmetry in this file mirrors Linux's asymmetric kernel code.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getrlimit(resource: i32, rlp: *mut Rlimit) -> i32 {
    // Linux validates the resource ordinal inside do_prlimit BEFORE
    // ever touching the userspace pointer (copy_to_user is the last
    // step, on the success path). Match that ordering.
    if resource < 0 || (resource as usize) >= RLIMIT_NLIMITS {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    if rlp.is_null() {
        errno::set_errno(errno::EFAULT);
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
///
/// ## Capability and ceiling checks (Phase 179)
///
/// Linux's `kernel/sys.c::do_prlimit` enforces two post-validation
/// guards:
///
/// ```text
/// if (resource == RLIMIT_NOFILE && new_rlim->rlim_max > sysctl_nr_open)
///     retval = -EPERM;
/// else if (new_rlim->rlim_max > old_rlim->rlim_max &&
///          !capable(CAP_SYS_RESOURCE))
///     retval = -EPERM;
/// ```
///
/// - The **`RLIMIT_NOFILE` ceiling** is absolute: even with
///   `CAP_SYS_RESOURCE`, you cannot raise the hard fd cap above the
///   kernel-wide upper bound (our `fdtable::MAX_FDS`).  This protects
///   the fd table from a privileged process asking for a value it
///   could never actually use.
/// - **Raising any other resource's hard limit** above its current
///   value requires `CAP_SYS_RESOURCE`.  Lowering the hard limit, or
///   leaving it unchanged while editing the soft limit, is always
///   permitted.  Pre-Phase-179 we silently accepted hard-limit raises
///   from any caller, which let unprivileged code claim impossible
///   resources and broke `prlimit`-based privilege separation.
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

    let Some(slot) = limits.get_mut(resource as usize) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let old = *slot;

    // Phase 179: RLIMIT_NOFILE absolute ceiling.  Linux's
    // `do_prlimit` rejects any rlim_max above sysctl_nr_open
    // unconditionally — CAP_SYS_RESOURCE does NOT lift this cap.
    // Our equivalent of sysctl_nr_open is `fdtable::MAX_FDS`.
    if resource == RLIMIT_NOFILE
        && new_limit.rlim_max > crate::fdtable::MAX_FDS as u64
    {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    // Phase 179: raising rlim_max above its current value requires
    // CAP_SYS_RESOURCE (matches Linux's `do_prlimit`).  Lowering or
    // holding equal — and any soft-only change — is always allowed.
    if new_limit.rlim_max > old.rlim_max
        && !crate::sys_capability::has_capability(
            crate::sys_capability::CAP_SYS_RESOURCE,
        )
    {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    *slot = new_limit;
    0
}

// ---------------------------------------------------------------------------
// getrusage
// ---------------------------------------------------------------------------

/// Get resource usage.
///
/// On the kernel target, `ru_utime` is filled from the aggregate kernel
/// "system" cycles (kernel + user code that's not IRQ/softirq/idle) and
/// `ru_stime` is filled from aggregate IRQ + softirq cycles, both via
/// `SYS_CPU_TIMES`.  All other fields are zeroed — we don't yet track
/// per-process page-fault counts, RSS, I/O bytes, etc.
///
/// For `RUSAGE_CHILDREN` we return all-zero (no terminated-children
/// accounting yet) so callers can still distinguish "no children" from
/// EINVAL.  This matches glibc's behavior on systems without child
/// tracking.
///
/// On host builds, the buffer is zero-filled (preserves existing test
/// behavior).
///
/// Validation order matches Linux's `kernel/sys.c::sys_getrusage`:
///
/// ```text
/// if (who != RUSAGE_SELF && who != RUSAGE_CHILDREN &&
///     who != RUSAGE_THREAD)
///     return -EINVAL;
/// getrusage(current, who, &r);
/// return copy_to_user(ru, &r, sizeof(r)) ? -EFAULT : 0;
/// ```
///
/// `who` is validated BEFORE the user pointer is ever touched, so a
/// caller passing both an invalid `who` and a NULL `usage` pointer
/// observes EINVAL — not EFAULT.  Pre-Phase 140 we checked the
/// pointer first and returned EFAULT for that combination, which
/// misdirected callers at the pointer when the real bug was the
/// `who` argument.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getrusage(who: i32, usage: *mut Rusage) -> i32 {
    // Phase 140: `who` validation precedes the user-pointer check
    // because Linux's `sys_getrusage` rejects bad `who` before any
    // copy_to_user can fault.  This gives diagnostic priority to
    // the value-domain error over the pointer error.
    if who != RUSAGE_SELF && who != RUSAGE_CHILDREN && who != RUSAGE_THREAD {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    if usage.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: Caller guarantees usage is valid for one Rusage.
    unsafe {
        core::ptr::write_bytes(usage, 0, 1);
    }

    // On the kernel target, populate user/system CPU times from kernel
    // aggregate stats.  RUSAGE_CHILDREN stays all-zero (no child tracking).
    #[cfg(target_os = "none")]
    {
        if who == RUSAGE_SELF || who == RUSAGE_THREAD {
            let system_ns = read_cpu_time_field_ns(0);
            let irq_ns = read_cpu_time_field_ns(1);
            let softirq_ns = read_cpu_time_field_ns(2);

            // SAFETY: We just zero-filled the buffer above; pointer is valid.
            unsafe {
                (*usage).ru_utime = ns_to_timeval(system_ns);
                (*usage).ru_stime = ns_to_timeval(irq_ns.saturating_add(softirq_ns));
            }
        }
    }

    0
}

/// Read one aggregate-CPU-time field from the kernel.
///
/// Returns 0 on any negative return (out-of-range selector, etc.) so
/// callers can use the value directly as a saturating-zero monotonic
/// counter.
#[cfg(target_os = "none")]
#[allow(clippy::cast_sign_loss)]
fn read_cpu_time_field_ns(which: u64) -> u64 {
    let raw = crate::syscall::syscall1(crate::syscall::SYS_CPU_TIMES, which);
    if raw < 0 { 0 } else { raw as u64 }
}

/// Convert a nanosecond duration into a POSIX `Timeval` (seconds + microseconds).
#[cfg(target_os = "none")]
#[allow(clippy::cast_possible_wrap)]
fn ns_to_timeval(ns: u64) -> crate::time::Timeval {
    const NS_PER_SEC: u64 = 1_000_000_000;
    const NS_PER_USEC: u64 = 1_000;
    let secs = ns / NS_PER_SEC;
    let usec = (ns % NS_PER_SEC) / NS_PER_USEC;
    crate::time::Timeval {
        tv_sec: if secs > i64::MAX as u64 { i64::MAX } else { secs as i64 },
        tv_usec: usec as i64,
    }
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
///
/// Phase 168: Linux's `kernel/sys.c::sys_nice` gates negative
/// increments on `CAP_SYS_NICE` (via `can_nice` /
/// `task_rlimit(RLIMIT_NICE)`).  Positive increments (lowering
/// priority) are always allowed; negative increments (raising
/// priority) require CAP_SYS_NICE under the default RLIMIT_NICE = 0.
/// On EPERM we return `-1` and set errno — callers must clear
/// errno before calling and re-check it after, since `-1` is also
/// a legitimate nice value.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn nice(inc: i32) -> i32 {
    // Phase 168: any negative increment is a priority-raise, which
    // requires CAP_SYS_NICE.  Linux performs this check after
    // computing the clamped target nice and consulting can_nice;
    // with our flat (no per-task rlimit) model the test collapses
    // to a pure cap probe.
    if inc < 0 && !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_NICE,
    ) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
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
///
/// Phase 169: Linux's `sys_setpriority` calls `set_one_prio` on each
/// task in scope.  After clamping `niceval` to `[MIN_NICE, MAX_NICE]`,
/// `set_one_prio` does:
///   - cross-uid permission check → `EPERM` (collapses in our
///     single-user model);
///   - `if (niceval < task_nice(p) && !can_nice(p, niceval)) error =
///     -EACCES;` — i.e. lowering the nice value (raising priority)
///     requires `CAP_SYS_NICE` under the default `RLIMIT_NICE = 0`.
/// Note the errno is `EACCES`, not `EPERM` — that distinction is
/// observable by callers that switch on errno.  Equivalent or higher
/// nice values (lowering priority) are always allowed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setpriority(which: i32, _who: u32, prio: i32) -> i32 {
    if which != PRIO_PROCESS && which != PRIO_PGRP && which != PRIO_USER {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let val = prio.clamp(-20, 19);
    // SAFETY: Single-threaded access.
    let current = unsafe { core::ptr::addr_of!(NICE_VALUE).read() };
    // Phase 169: priority-raise (new nice < current nice) requires
    // CAP_SYS_NICE.  Linux returns EACCES from set_one_prio in this
    // case (distinct from the cross-uid EPERM path).
    if val < current && !crate::sys_capability::has_capability(
        crate::sys_capability::CAP_SYS_NICE,
    ) {
        errno::set_errno(errno::EACCES);
        return -1;
    }
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
/// Argument-domain validation (Linux-matching):
///   - `pid < 0` → `-1` with `ESRCH`.  Linux's `find_get_task_by_vpid`
///     can't resolve a negative pid; the syscall surface reports it as
///     ESRCH (no such process), not EINVAL.
///   - `resource < 0 || resource >= RLIMIT_NLIMITS` → `-1` with
///     `EINVAL`, even when both limit pointers are NULL.  This matches
///     Linux's `do_prlimit`, which validates the resource ordinal
///     before doing any pointer work — a bare `prlimit(0, 9999, NULL,
///     NULL)` is a malformed call and must report it.
///   - If `new_limit` is non-NULL: `setrlimit` enforces `rlim_cur <=
///     rlim_max` and returns `EINVAL` on violation.
///
/// Since our kernel doesn't track per-process resource limits, valid
/// requests delegate to the global getrlimit/setrlimit; `pid` is
/// otherwise ignored (single-user, single-process resource view).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn prlimit(
    pid: i32,
    resource: i32,
    new_limit: *const Rlimit,
    old_limit: *mut Rlimit,
) -> i32 {
    // pid: 0 means "self", positive means a real pid.  Negative is
    // never a valid pid value — report it as ESRCH ("no such process").
    if pid < 0 {
        errno::set_errno(errno::ESRCH);
        return -1;
    }

    // Validate resource ordinal up front so a malformed call with both
    // pointers NULL still fails loudly.
    if resource < 0 || (resource as usize) >= RLIMIT_NLIMITS {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

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
            // Open files: matches fd table size.
            limits[RLIMIT_NOFILE as usize] = Rlimit {
                rlim_cur: crate::fdtable::MAX_FDS as u64,
                rlim_max: crate::fdtable::MAX_FDS as u64,
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
        assert_eq!(RLIMIT_LOCKS, 10);
        assert_eq!(RLIMIT_SIGPENDING, 11);
        assert_eq!(RLIMIT_MSGQUEUE, 12);
        assert_eq!(RLIMIT_NICE, 13);
        assert_eq!(RLIMIT_RTPRIO, 14);
        assert_eq!(RLIMIT_RTTIME, 15);
    }

    #[test]
    fn rlimit_constants_all_distinct() {
        let vals = [
            RLIMIT_CPU, RLIMIT_FSIZE, RLIMIT_DATA, RLIMIT_STACK,
            RLIMIT_CORE, RLIMIT_RSS, RLIMIT_NPROC, RLIMIT_NOFILE,
            RLIMIT_MEMLOCK, RLIMIT_AS, RLIMIT_LOCKS, RLIMIT_SIGPENDING,
            RLIMIT_MSGQUEUE, RLIMIT_NICE, RLIMIT_RTPRIO, RLIMIT_RTTIME,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j], "RLIMIT constants must be distinct");
            }
        }
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
        assert_eq!(RUSAGE_THREAD, 1);
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
        assert_eq!(rl.rlim_cur, crate::fdtable::MAX_FDS as u64);
        assert_eq!(rl.rlim_max, crate::fdtable::MAX_FDS as u64);
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
            RLIMIT_NPROC, RLIMIT_MEMLOCK, RLIMIT_AS, RLIMIT_LOCKS,
            RLIMIT_SIGPENDING, RLIMIT_MSGQUEUE, RLIMIT_NICE,
            RLIMIT_RTPRIO, RLIMIT_RTTIME,
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
    fn getrusage_thread_returns_zeroed() {
        let mut usage = unsafe { core::mem::MaybeUninit::<Rusage>::zeroed().assume_init() };
        usage.ru_maxrss = 123;
        let ret = getrusage(RUSAGE_THREAD, &mut usage);
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

    // ------------------------------------------------------------------
    // Phase 140 — getrusage validates `who` before touching the user
    // pointer, matching Linux's `kernel/sys.c::sys_getrusage`.
    //
    // Pre-Phase 140:
    //     usage NULL → EFAULT (first)
    //     invalid who → EINVAL (second)
    // Linux / Phase 140+:
    //     invalid who → EINVAL (first)
    //     usage NULL → EFAULT (second; copy_to_user happens last)
    //
    // Observable when both arguments are bad at once: pre-Phase 140
    // returned EFAULT and misdirected callers at the pointer when
    // the `who` argument was the real bug.
    // ------------------------------------------------------------------

    // --- per-error-class smoke tests under the new ordering ---------

    #[test]
    fn getrusage_phase140_invalid_who_alone_einval() {
        // Sanity: invalid who with a valid pointer still EINVAL.
        let mut usage = unsafe { core::mem::MaybeUninit::<Rusage>::zeroed().assume_init() };
        errno::set_errno(0);
        let ret = getrusage(99, &mut usage);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn getrusage_phase140_null_pointer_alone_efault() {
        // Sanity: valid who + NULL pointer still EFAULT.
        errno::set_errno(0);
        let ret = getrusage(RUSAGE_SELF, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // --- core regression: ordering matrix ----------------------------

    #[test]
    fn getrusage_phase140_invalid_who_beats_null_pointer() {
        // CORE: invalid who + NULL pointer.  Pre-Phase 140 returned
        // EFAULT; Linux / Phase 140+ returns EINVAL.
        errno::set_errno(0);
        let ret = getrusage(99, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn getrusage_phase140_negative_who_beats_null_pointer() {
        // Negative `who` (e.g. -2 from an integer underflow in the
        // caller) + NULL pointer.  EINVAL must still win.  We use
        // -2 rather than -1 because RUSAGE_CHILDREN == -1 is a
        // valid `who` value on Linux and on us.
        errno::set_errno(0);
        let ret = getrusage(-2, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn getrusage_phase140_extremely_large_who_beats_null_pointer() {
        // i32::MAX `who` + NULL.  EINVAL.
        errno::set_errno(0);
        let ret = getrusage(i32::MAX, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn getrusage_phase140_i32_min_who_beats_null_pointer() {
        // i32::MIN `who` (sign-bit set) + NULL.  EINVAL.
        errno::set_errno(0);
        let ret = getrusage(i32::MIN, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // --- "near-miss" who values ---------------------------------------

    #[test]
    fn getrusage_phase140_one_past_self_beats_null() {
        // RUSAGE_SELF is typically 0; 1 is between SELF (0) and
        // CHILDREN (-1) etc. — invalid.
        let near = RUSAGE_SELF.wrapping_add(1);
        if near == RUSAGE_CHILDREN || near == RUSAGE_THREAD {
            // Skip: the near value happens to coincide with another
            // valid constant on this build; pick a different test.
            return;
        }
        errno::set_errno(0);
        let ret = getrusage(near, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // --- buggy callers ------------------------------------------------

    #[test]
    fn getrusage_phase140_buggy_caller_uninit_who_with_null() {
        // Caller does `int who; getrusage(who, NULL);` — stack
        // garbage `who` is very likely outside the SELF/CHILDREN/
        // THREAD set.  EINVAL points at the real bug (uninit who).
        let bad_values = [42i32, 0x4242_4242i32, -42i32, 1234567i32];
        for &who in &bad_values {
            if who == RUSAGE_SELF || who == RUSAGE_CHILDREN || who == RUSAGE_THREAD {
                continue;
            }
            errno::set_errno(0);
            let ret = getrusage(who, core::ptr::null_mut());
            assert_eq!(ret, -1, "who={who}");
            assert_eq!(errno::get_errno(), errno::EINVAL, "who={who}");
        }
    }

    #[test]
    fn getrusage_phase140_buggy_caller_uses_pid_as_who_with_null() {
        // Real bug: caller confuses getrusage(2) arguments and
        // passes a pid as `who`.  Pid is positive and not in the
        // valid set.  Plus NULL pointer (forgot to allocate).
        // EINVAL.
        errno::set_errno(0);
        let ret = getrusage(1234, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // --- workflow + recovery -----------------------------------------

    #[test]
    fn getrusage_phase140_recovery_after_einval_fix_who() {
        // Caller corrects `who`, retries with valid pointer.  No
        // stale errno from the previous failure.
        errno::set_errno(0);
        assert_eq!(getrusage(99, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let mut usage = unsafe { core::mem::MaybeUninit::<Rusage>::zeroed().assume_init() };
        errno::set_errno(0);
        let ret = getrusage(RUSAGE_SELF, &mut usage);
        assert_eq!(ret, 0);
        // errno is not set on success path.
    }

    #[test]
    fn getrusage_phase140_recovery_after_einval_fix_pointer() {
        // Caller fixes the pointer but didn't realise the `who`
        // value was also wrong — the second call still returns
        // EINVAL (because the `who` is still wrong), proving the
        // value-domain check is independent of the pointer.
        errno::set_errno(0);
        assert_eq!(getrusage(99, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let mut usage = unsafe { core::mem::MaybeUninit::<Rusage>::zeroed().assume_init() };
        errno::set_errno(0);
        let ret = getrusage(99, &mut usage);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn getrusage_phase140_workflow_glibc_rusage_probe() {
        // glibc's wrapper is straightforward, but a userspace
        // sanity probe ("does my libc map who correctly?") will
        // pass garbage `who` to check the EINVAL surface.  Such
        // probes must see EINVAL even if they forgot to bind a
        // pointer — pre-Phase 140 they got EFAULT and concluded
        // their pointer handling was broken.
        errno::set_errno(0);
        let ret = getrusage(0xDEAD_BEEFu32 as i32, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn getrusage_phase140_no_side_effect_on_einval_loop() {
        // 100 invalid-who rejections must not corrupt anything.
        // The follow-up valid call must succeed.
        for _ in 0..100 {
            errno::set_errno(0);
            assert_eq!(getrusage(99, core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }
        let mut usage = unsafe { core::mem::MaybeUninit::<Rusage>::zeroed().assume_init() };
        errno::set_errno(0);
        let ret = getrusage(RUSAGE_SELF, &mut usage);
        assert_eq!(ret, 0);
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
        assert_eq!(old.rlim_cur, crate::fdtable::MAX_FDS as u64);
        assert_eq!(old.rlim_max, crate::fdtable::MAX_FDS as u64);
    }

    // -----------------------------------------------------------------------
    // Phase 86 — prlimit/prlimit64 argument-domain validation
    //
    // Linux semantics being validated:
    //   - pid < 0 → -1, ESRCH (no such process)
    //   - resource out of range → -1, EINVAL (even when both pointers NULL)
    //   - new_limit with rlim_cur > rlim_max → -1, EINVAL (via setrlimit)
    // -----------------------------------------------------------------------

    #[test]
    fn test_prlimit_phase86_negative_pid_is_esrch() {
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit(-1, RLIMIT_STACK, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    #[test]
    fn test_prlimit_phase86_intmin_pid_is_esrch() {
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit(i32::MIN, RLIMIT_CPU, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    #[test]
    fn test_prlimit_phase86_zero_pid_means_self() {
        // pid == 0 is "self" and must succeed for a valid resource.
        reset_global_state();
        let mut old = Rlimit { rlim_cur: 0, rlim_max: 0 };
        errno::set_errno(0);
        let ret = prlimit(0, RLIMIT_CPU, core::ptr::null(), &mut old);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_prlimit_phase86_positive_pid_accepted() {
        // We don't track other processes, so a positive pid behaves like
        // self.  Just verify it doesn't fall into the ESRCH path.
        reset_global_state();
        let mut old = Rlimit { rlim_cur: 0, rlim_max: 0 };
        errno::set_errno(0);
        let ret = prlimit(1234, RLIMIT_CPU, core::ptr::null(), &mut old);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_prlimit_phase86_invalid_resource_with_null_ptrs_einval() {
        // The bug being fixed: a malformed call with both pointers NULL
        // used to return 0 silently.  It must now report EINVAL.
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit(0, 99, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prlimit_phase86_negative_resource_with_null_ptrs_einval() {
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit(0, -1, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prlimit_phase86_resource_at_nlimits_einval() {
        // The first invalid index is RLIMIT_NLIMITS itself (16).
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit(
            0,
            RLIMIT_NLIMITS as i32,
            core::ptr::null(),
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prlimit_phase86_esrch_precedes_einval() {
        // pid < 0 with an invalid resource: ESRCH wins (pid check first).
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit(-5, 9999, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    #[test]
    fn test_prlimit_phase86_invalid_resource_with_get_buf_einval() {
        // Bad resource with non-NULL old_limit still EINVAL, and the
        // buffer must not be written.
        reset_global_state();
        let mut old = Rlimit { rlim_cur: 0xDEAD, rlim_max: 0xBEEF };
        errno::set_errno(0);
        let ret = prlimit(0, 100, core::ptr::null(), &mut old);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(old.rlim_cur, 0xDEAD, "old must not be overwritten on EINVAL");
        assert_eq!(old.rlim_max, 0xBEEF);
    }

    #[test]
    fn test_prlimit_phase86_invalid_resource_with_set_buf_einval() {
        reset_global_state();
        let new = Rlimit { rlim_cur: 1, rlim_max: 2 };
        errno::set_errno(0);
        let ret = prlimit(0, 100, &new, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prlimit_phase86_set_rlim_cur_above_max_einval() {
        // Inverted rlim_cur/rlim_max via prlimit's setrlimit delegate.
        reset_global_state();
        let new = Rlimit { rlim_cur: 200, rlim_max: 100 };
        errno::set_errno(0);
        let ret = prlimit(0, RLIMIT_CPU, &new, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prlimit_phase86_both_ptrs_null_valid_resource_ok() {
        // A degenerate but well-formed call: pid==0, valid resource,
        // both pointers NULL — succeeds.  Linux returns 0 here too.
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit(0, RLIMIT_CPU, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_prlimit64_phase86_negative_pid_is_esrch() {
        // Verify prlimit64 inherits the new validation.
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit64(-1, RLIMIT_STACK, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ESRCH);
    }

    #[test]
    fn test_prlimit64_phase86_invalid_resource_einval() {
        reset_global_state();
        errno::set_errno(0);
        let ret = prlimit64(0, 50, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_prlimit_phase86_does_not_mutate_state_on_einval() {
        // A bad-resource EINVAL must not touch the global RLIMITS array.
        reset_global_state();
        let original = Rlimit { rlim_cur: 8 * 1024 * 1024, rlim_max: RLIM_INFINITY };
        let mut sentinel = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_STACK, &mut sentinel), 0);
        assert_eq!(sentinel.rlim_cur, original.rlim_cur);

        // Bogus call.
        let new = Rlimit { rlim_cur: 1, rlim_max: 2 };
        errno::set_errno(0);
        let ret = prlimit(0, 9999, &new, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // Stack limit unchanged.
        let mut after = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_STACK, &mut after), 0);
        assert_eq!(after.rlim_cur, original.rlim_cur);
        assert_eq!(after.rlim_max, original.rlim_max);
    }

    #[test]
    fn test_prlimit_phase86_esrch_then_valid_call_progression() {
        reset_global_state();
        errno::set_errno(0);
        assert_eq!(
            prlimit(-1, RLIMIT_CPU, core::ptr::null(), core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::ESRCH);

        // Subsequent valid call succeeds and reports no error.
        let mut old = Rlimit { rlim_cur: 0, rlim_max: 0 };
        errno::set_errno(0);
        assert_eq!(prlimit(0, RLIMIT_CPU, core::ptr::null(), &mut old), 0);
    }

    // -----------------------------------------------------------------------
    // New RLIMIT_* constants: getrlimit/setrlimit round trip
    // -----------------------------------------------------------------------

    #[test]
    fn new_rlimit_constants_getrlimit_works() {
        reset_global_state();
        // Verify all new constants are accessible via getrlimit.
        let new_resources = [
            RLIMIT_LOCKS, RLIMIT_SIGPENDING, RLIMIT_NICE,
            RLIMIT_RTPRIO, RLIMIT_RTTIME,
        ];
        for &res in &new_resources {
            let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
            let ret = getrlimit(res, &mut rl);
            assert_eq!(ret, 0, "getrlimit failed for resource {res}");
            // All default to RLIM_INFINITY.
            assert_eq!(rl.rlim_cur, RLIM_INFINITY);
            assert_eq!(rl.rlim_max, RLIM_INFINITY);
        }
    }

    #[test]
    fn new_rlimit_constants_setrlimit_round_trip() {
        reset_global_state();
        let new = Rlimit { rlim_cur: 128, rlim_max: 256 };
        assert_eq!(setrlimit(RLIMIT_SIGPENDING, &new), 0);

        let mut readback = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_SIGPENDING, &mut readback), 0);
        assert_eq!(readback.rlim_cur, 128);
        assert_eq!(readback.rlim_max, 256);
    }

    // -----------------------------------------------------------------------
    // RLIMIT_NOFILE matches fd table size
    // -----------------------------------------------------------------------

    #[test]
    fn rlimit_nofile_matches_fdtable_max() {
        reset_global_state();
        let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_NOFILE, &mut rl), 0);
        assert_eq!(
            rl.rlim_cur,
            crate::fdtable::MAX_FDS as u64,
            "RLIMIT_NOFILE soft must match fdtable::MAX_FDS"
        );
        assert_eq!(
            rl.rlim_max,
            crate::fdtable::MAX_FDS as u64,
            "RLIMIT_NOFILE hard must match fdtable::MAX_FDS"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 112 — getrlimit validation order parity with Linux
    //
    // Linux's `SYSCALL_DEFINE2(getrlimit)` calls `do_prlimit` (which
    // returns -EINVAL on a bad resource ordinal) BEFORE `copy_to_user`
    // (which is the only path to -EFAULT). A buggy caller passing both
    // a bad resource and a NULL pointer therefore observes EINVAL on
    // Linux, not EFAULT.
    //
    // setrlimit is intentionally NOT reordered: Linux's
    // `SYSCALL_DEFINE2(setrlimit)` does `copy_from_user` first, so
    // EFAULT-before-EINVAL is the correct asymmetry for that syscall.
    // -----------------------------------------------------------------------

    #[test]
    fn test_getrlimit_phase112_einval_wins_over_efault_neg_resource() {
        // Negative resource + NULL pointer: Linux's do_prlimit returns
        // EINVAL before any copy_to_user. We now do too.
        reset_global_state();
        errno::set_errno(0);
        let ret = getrlimit(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getrlimit_phase112_einval_wins_over_efault_high_resource() {
        // Resource above RLIMIT_NLIMITS + NULL pointer -> EINVAL.
        reset_global_state();
        errno::set_errno(0);
        let ret = getrlimit(9999, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getrlimit_phase112_einval_wins_at_nlimits_with_null() {
        // The first invalid index is exactly RLIMIT_NLIMITS (16).
        reset_global_state();
        errno::set_errno(0);
        let ret = getrlimit(RLIMIT_NLIMITS as i32, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getrlimit_phase112_efault_only_when_resource_valid() {
        // Valid resource + NULL pointer: resource check passes -> EFAULT.
        reset_global_state();
        errno::set_errno(0);
        let ret = getrlimit(RLIMIT_STACK, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getrlimit_phase112_intmin_resource_with_null_is_einval() {
        // Catastrophic resource value: must report EINVAL, not EFAULT
        // and not panic on the `as usize` cast (we check `< 0` first).
        reset_global_state();
        errno::set_errno(0);
        let ret = getrlimit(i32::MIN, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getrlimit_phase112_intmax_resource_with_null_is_einval() {
        // i32::MAX is > RLIMIT_NLIMITS, so resource check fails first.
        reset_global_state();
        errno::set_errno(0);
        let ret = getrlimit(i32::MAX, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getrlimit_phase112_does_not_dereference_null_on_einval() {
        // Sanity check: passing NULL with a bad resource must NOT
        // attempt to write through the NULL pointer. The reorder pins
        // this: the resource check returns before the write.
        reset_global_state();
        errno::set_errno(0);
        // If the implementation regressed to "check pointer last" or
        // forgot to early-return, this would dereference NULL and
        // segfault on a hosted target — which the test runner would
        // catch as an abort.
        let ret = getrlimit(-42, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getrlimit_phase112_setrlimit_keeps_efault_before_einval() {
        // setrlimit MUST keep the inverted order (EFAULT before EINVAL)
        // because Linux's setrlimit does copy_from_user before
        // do_prlimit. Pin that: bad resource + NULL pointer -> EFAULT.
        reset_global_state();
        errno::set_errno(0);
        let ret = setrlimit(-1, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getrlimit_phase112_setrlimit_efault_at_nlimits_too() {
        // Same asymmetry confirmation: invalid resource at the boundary
        // + NULL pointer -> EFAULT (not EINVAL) for setrlimit.
        reset_global_state();
        errno::set_errno(0);
        let ret = setrlimit(RLIMIT_NLIMITS as i32, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getrlimit_phase112_recovery_after_einval() {
        // After a bad-resource EINVAL, a valid call still returns 0
        // and rewrites errno (errno is not sticky on success).
        reset_global_state();
        errno::set_errno(0);
        let r1 = getrlimit(9999, core::ptr::null_mut());
        assert_eq!(r1, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let mut rl = Rlimit { rlim_cur: 0, rlim_max: 0 };
        let r2 = getrlimit(RLIMIT_CPU, &mut rl);
        assert_eq!(r2, 0);
        // Don't assert errno here — POSIX permits successful calls to
        // leave errno alone; only the negative-return path must set
        // errno to a meaningful value.
    }

    #[test]
    fn test_getrlimit_phase112_does_not_mutate_state_on_einval() {
        // Bad resource must not perturb the global RLIMITS array, even
        // when the pointer is also NULL.
        reset_global_state();
        let mut before = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_STACK, &mut before), 0);

        errno::set_errno(0);
        let ret = getrlimit(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let mut after = Rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(getrlimit(RLIMIT_STACK, &mut after), 0);
        assert_eq!(after.rlim_cur, before.rlim_cur);
        assert_eq!(after.rlim_max, before.rlim_max);
    }

    #[test]
    fn test_getrlimit_phase112_python_resource_module_workflow() {
        // CPython's `resource.getrlimit(resource)` calls
        // `getrlimit(resource, &rl)` and raises ValueError on EINVAL,
        // OSError on other errnos. A buggy script passing
        // `resource.getrlimit(-1)` against a freshly-zeroed pointer
        // would, pre-reorder, surface as OSError(EFAULT) — opaque.
        // Post-reorder it surfaces as ValueError(EINVAL), matching
        // CPython's behaviour against the real Linux kernel.
        reset_global_state();
        errno::set_errno(0);
        let ret = getrlimit(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getrlimit_phase112_buggy_caller_passes_huge_unsigned() {
        // A C caller that does `getrlimit((int)0xFFFFFFFFu, NULL)`
        // (which sign-extends to -1) hits the resource check first and
        // sees EINVAL, the same as on Linux. Confirms we don't trip
        // the EFAULT branch on this common typo.
        reset_global_state();
        errno::set_errno(0);
        #[allow(clippy::cast_possible_wrap)]
        let bogus_resource = 0xFFFF_FFFF_u32 as i32; // = -1
        let ret = getrlimit(bogus_resource, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ----------------------------------------------------------------------
    // Phase 168: nice() — CAP_SYS_NICE gate on negative increments.
    //
    // Pre-Phase-168 behaviour: any caller could raise its own priority
    // by passing a negative `inc`; the stub merely clamped to -20.  This
    // ignores Linux's `sys_nice` guard
    //   if (increment < 0 && !can_nice(current, nice)) return -EPERM;
    // which (under the default `RLIMIT_NICE = 0`) requires
    // `CAP_SYS_NICE` for any negative increment.
    //
    // Implementation: cap-probe at the top of `nice()` for negative
    // inputs; positive increments are unaffected.  These tests exercise
    // the guard via the CapGuard / drop-cap pattern shared with the
    // unistd / process Phase-16x suites.
    // ----------------------------------------------------------------------

    mod nice_cap_phase168 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase 77 /
        /// 164 / 165 / 166 / 167.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_nice() {
            use crate::sys_capability::CAP_SYS_NICE;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_NICE < 32 {
                (lo & !(1u32 << CAP_SYS_NICE), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_NICE - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYS_NICE");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_NICE));
        }

        // -- Per-error-class ----------------------------------------------

        /// `nice(-1)` without CAP_SYS_NICE returns -1 and sets EPERM,
        /// matching Linux's `if (increment < 0 && !can_nice(...))`
        /// branch.
        #[test]
        fn test_nice_phase168_negative_one_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(-1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// `nice(-20)` (the most aggressive raise) without
        /// CAP_SYS_NICE is also EPERM.
        #[test]
        fn test_nice_phase168_negative_twenty_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(-20), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix ----------------------------------------------

        /// Positive increments (lower priority) are *always* allowed,
        /// even without CAP_SYS_NICE.
        #[test]
        fn test_nice_phase168_positive_no_cap_still_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(5), 5);
            assert_eq!(errno::get_errno(), 0,
                "positive nice must not set errno");
        }

        /// `nice(0)` is a query and must succeed without cap (the
        /// guard is gated on `inc < 0`, not `inc <= 0`).
        #[test]
        fn test_nice_phase168_zero_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(0), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        /// EPERM must beat the clamp: a no-cap caller that asks for
        /// `nice(-100)` (would clamp to -20) gets EPERM, not -20.
        #[test]
        fn test_nice_phase168_eperm_beats_clamp() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(-100), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Workflow -----------------------------------------------------

        /// Realtime-audio-daemon-style workflow: start with all caps,
        /// raise priority to -10 (succeeds), then drop CAP_SYS_NICE
        /// and try to raise further (EPERM).  Models a daemon that
        /// drops capabilities after initialisation.
        #[test]
        fn test_nice_phase168_workflow_raise_then_drop_cap_then_raise() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            errno::set_errno(0);
            assert_eq!(nice(-10), -10);
            assert_eq!(errno::get_errno(), 0);
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(-5), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy caller -------------------------------------------------

        /// A buggy unprivileged caller passing `i32::MIN` must hit
        /// EPERM, not crash / wrap / clamp.  Confirms the cap check
        /// fires before any arithmetic on the increment.
        #[test]
        fn test_nice_phase168_buggy_caller_i32_min_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(i32::MIN), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// A no-cap caller asking for `nice(-100)` (the same value
        /// that the pre-Phase-168 stub silently clamped to -20) must
        /// now fail.  Sentinel for the old clamp-first behaviour.
        #[test]
        fn test_nice_phase168_buggy_caller_minus_100_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(-100), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery -----------------------------------------------------

        /// After an EPERM rejection, restoring CAP_SYS_NICE lets the
        /// same call succeed.  Confirms the guard is dynamic (cap
        /// state checked per-call, not cached).
        #[test]
        fn test_nice_phase168_recovery_restore_cap_lets_raise_succeed() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(-5), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Restore via the guard's restore path (handled on drop)
            // would happen at scope end — for explicit recovery we
            // reset caps to the default holding-all state via capset.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0);
            errno::set_errno(0);
            assert_eq!(nice(-5), -5);
            assert_eq!(errno::get_errno(), 0);
        }

        // -- No-side-effect ----------------------------------------------

        /// An EPERM rejection must leave the stored NICE_VALUE
        /// unchanged.  Linux returns from `sys_nice` before writing
        /// the new nice; we mirror that.
        #[test]
        fn test_nice_phase168_eperm_does_not_modify_stored_value() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            // Seed a known value first (under default caps).
            assert_eq!(nice(7), 7);
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(nice(-5), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Read back via getpriority — must still be 7.
            errno::set_errno(0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), 7);
        }

        // -- Sentinel -----------------------------------------------------

        /// With CAP_SYS_NICE *held* (the default) the negative-inc
        /// path still works — confirms the guard didn't break the
        /// privileged case.  Mirrors the old
        /// `nice_clamps_to_lower_bound` test but explicit about caps.
        #[test]
        fn test_nice_phase168_sentinel_with_cap_negative_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_NICE,
            ));
            errno::set_errno(0);
            assert_eq!(nice(-3), -3);
            assert_eq!(errno::get_errno(), 0);
        }

        // -- Cross-checks -------------------------------------------------

        /// Default-cap `nice(-100)` still clamps to -20 — the
        /// pre-existing clamp behaviour is preserved for the
        /// privileged path.
        #[test]
        fn test_nice_phase168_default_cap_minus_100_still_clamps() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            assert_eq!(nice(-100), -20);
        }

        /// Dropping CAP_SYS_NICE must not affect other caps —
        /// CAP_SYS_ADMIN remains held, so other syscalls that gate
        /// on it are unaffected.  Defends against a stray bit-clear
        /// regression in `drop_cap_sys_nice`.
        #[test]
        fn test_nice_phase168_drop_sys_nice_leaves_other_caps() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_nice();
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_NICE,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_BOOT,
            ));
        }
    }

    // ----------------------------------------------------------------------
    // Phase 169: setpriority() — CAP_SYS_NICE gate on priority-raise.
    //
    // Pre-Phase-169 behaviour: setpriority clamped to [-20, 19] and
    // wrote the value with no capability check.  An unprivileged
    // caller could lower its nice value (raise priority) by any
    // amount.
    //
    // Linux semantics (kernel/sys.c::set_one_prio):
    //   if (niceval < task_nice(p) && !can_nice(p, niceval))
    //       error = -EACCES;
    // Note: EACCES, not EPERM.  The check is per-task; in our
    // single-process single-user model it collapses to a comparison
    // of the new clamped niceval against the stored NICE_VALUE.
    //
    // Implementation: read current NICE_VALUE, clamp prio, and if the
    // clamped value is strictly less than current (a raise) without
    // CAP_SYS_NICE return -1 / EACCES.  Equal or higher values
    // (lowering priority or no change) always succeed.
    // ----------------------------------------------------------------------

    mod setpriority_cap_phase169 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase
        /// 77 / 164 / 165 / 166 / 167 / 168.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_nice() {
            use crate::sys_capability::CAP_SYS_NICE;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_NICE < 32 {
                (lo & !(1u32 << CAP_SYS_NICE), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_NICE - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYS_NICE");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_NICE));
        }

        // -- Per-error-class ----------------------------------------------

        /// From the default nice=0, asking for -1 without CAP_SYS_NICE
        /// returns -1 with EACCES (not EPERM).
        #[test]
        fn test_setpriority_phase169_no_cap_raise_returns_eaccess() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -1), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        /// Errno must be EACCES specifically, not EPERM.  Distinguishes
        /// Linux's set_one_prio EACCES branch from the cross-uid
        /// EPERM branch (which collapses in our single-user model).
        #[test]
        fn test_setpriority_phase169_errno_is_eacces_not_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -5), -1);
            assert_ne!(errno::get_errno(), errno::EPERM);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        // -- Ordering matrix ----------------------------------------------

        /// EINVAL on `which` must beat EACCES — Linux validates
        /// `which` in sys_setpriority before entering the per-task
        /// loop that runs set_one_prio.
        #[test]
        fn test_setpriority_phase169_einval_which_beats_eacces() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            // Bogus which + raise attempt — must see EINVAL, not EACCES.
            assert_eq!(setpriority(999, 0, -10), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// Lowering priority (higher nice) without cap always works.
        #[test]
        fn test_setpriority_phase169_lower_priority_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, 10), 0);
            assert_eq!(errno::get_errno(), 0);
            errno::set_errno(0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), 10);
        }

        /// Setting the same value (no change) without cap succeeds —
        /// Linux uses `<` (strict less-than), not `<=`.
        #[test]
        fn test_setpriority_phase169_same_value_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            // Default NICE_VALUE = 0; setting prio=0 is a no-op.
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, 0), 0);
            assert_eq!(errno::get_errno(), 0);
        }

        /// EACCES must beat the clamp: an unprivileged caller asking
        /// for -100 (would clamp to -20) gets EACCES because clamped
        /// -20 < current 0.  Confirms we don't silently clamp-then-
        /// store before the cap check.
        #[test]
        fn test_setpriority_phase169_eacces_beats_clamp() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -100), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
            // And the stored value must NOT have been updated.
            errno::set_errno(0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), 0);
        }

        // -- Workflow -----------------------------------------------------

        /// Daemon workflow: setpriority to -10 under default caps
        /// (succeeds), drop CAP_SYS_NICE, then ask for -15 (raise →
        /// EACCES), then ask for -5 (lower → succeeds).
        #[test]
        fn test_setpriority_phase169_workflow_raise_drop_cap_raise_lower()
        {
            let _g = CapGuard::snapshot();
            reset_global_state();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -10), 0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), -10);
            drop_cap_sys_nice();
            // Further raise: EACCES.
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -15), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
            // Lower (nice value increases): still allowed without cap.
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -5), 0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), -5);
        }

        // -- Buggy caller -------------------------------------------------

        /// Passing i32::MIN with no cap must yield EACCES (the
        /// clamped target -20 is below the default 0), not crash or
        /// store.
        #[test]
        fn test_setpriority_phase169_buggy_caller_i32_min_no_cap_eacces()
        {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, i32::MIN), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
            errno::set_errno(0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), 0);
        }

        /// i32::MAX clamps to 19 and is a lower-priority request —
        /// must succeed without cap.
        #[test]
        fn test_setpriority_phase169_i32_max_no_cap_clamps_and_succeeds()
        {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, i32::MAX), 0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), 19);
        }

        // -- Recovery -----------------------------------------------------

        /// After EACCES, restoring CAP_SYS_NICE lets the same call
        /// succeed.  Confirms dynamic cap evaluation.
        #[test]
        fn test_setpriority_phase169_recovery_restore_cap_lets_raise() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -8), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
            // Restore caps to default-all.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -8), 0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), -8);
        }

        // -- No-side-effect ----------------------------------------------

        /// EACCES rejection must leave NICE_VALUE untouched, observable
        /// via getpriority.
        #[test]
        fn test_setpriority_phase169_eacces_does_not_modify_value() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            // Seed value to 4 under default caps.
            assert_eq!(setpriority(PRIO_PROCESS, 0, 4), 0);
            drop_cap_sys_nice();
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -2), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
            errno::set_errno(0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), 4);
        }

        // -- Sentinel -----------------------------------------------------

        /// With CAP_SYS_NICE held, raising priority works — confirms
        /// privileged path is unbroken.
        #[test]
        fn test_setpriority_phase169_sentinel_with_cap_raise_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_NICE,
            ));
            errno::set_errno(0);
            assert_eq!(setpriority(PRIO_PROCESS, 0, -10), 0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), -10);
        }

        // -- Cross-checks -------------------------------------------------

        /// Default-cap setpriority(-100) still clamps to -20 — the
        /// privileged clamp behaviour is preserved.
        #[test]
        fn test_setpriority_phase169_default_cap_minus_100_clamps() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            assert_eq!(setpriority(PRIO_PROCESS, 0, -100), 0);
            assert_eq!(getpriority(PRIO_PROCESS, 0), -20);
        }

        /// `setpriority` rejection should not perturb `nice()`'s
        /// negative-inc gate (both consult the same cap but via
        /// different errnos — EACCES vs EPERM).  Confirms the two
        /// gates remain independent.
        #[test]
        fn test_setpriority_phase169_distinct_from_nice_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_nice();
            errno::set_errno(0);
            // setpriority raise → EACCES.
            assert_eq!(setpriority(PRIO_PROCESS, 0, -3), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
            // nice raise → EPERM.
            errno::set_errno(0);
            assert_eq!(nice(-3), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }
    }

    // ----------------------------------------------------------------------
    // Phase 179: setrlimit/prlimit — CAP_SYS_RESOURCE gate on
    // hard-limit raise + unconditional RLIMIT_NOFILE ceiling.
    //
    // Pre-Phase-179 behaviour: setrlimit accepted any rlim_max from
    // any caller — including raises above the existing hard limit and
    // raises of RLIMIT_NOFILE above fdtable::MAX_FDS.  That diverges
    // from Linux's `do_prlimit` (`kernel/sys.c`):
    //
    //     if (resource == RLIMIT_NOFILE &&
    //         new_rlim->rlim_max > sysctl_nr_open)
    //         retval = -EPERM;
    //     else if (new_rlim->rlim_max > old_rlim->rlim_max &&
    //              !capable(CAP_SYS_RESOURCE))
    //         retval = -EPERM;
    //
    // Implementation: after the existing EFAULT / EINVAL(resource) /
    // EINVAL(cur>max) checks, read the current limit; reject NOFILE
    // hard-raises above MAX_FDS unconditionally; reject other
    // hard-raises without CAP_SYS_RESOURCE.  Errno is EPERM in both
    // cases — Linux uses EPERM, not EACCES, here.
    // ----------------------------------------------------------------------

    mod setrlimit_cap_phase179 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase 168
        /// / 169 et al.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) =
                    crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version:
                        crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ =
                    crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_resource() {
            use crate::sys_capability::CAP_SYS_RESOURCE;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_RESOURCE < 32 {
                (lo & !(1u32 << CAP_SYS_RESOURCE), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_RESOURCE - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc =
                crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0,
                "capset must succeed when dropping CAP_SYS_RESOURCE");
            assert!(!crate::sys_capability::has_capability(
                CAP_SYS_RESOURCE,
            ));
        }

        /// Helper: seed a known starting hard limit on a resource so
        /// tests can then attempt to raise it.  Runs under default
        /// caps so the seed itself isn't blocked.
        fn seed_limit(res: i32, cur: u64, max: u64) {
            let rl = Rlimit { rlim_cur: cur, rlim_max: max };
            assert_eq!(setrlimit(res, &rl), 0,
                "seed setrlimit for resource {res} must succeed under \
                 default caps");
        }

        // -- Per-error-class ----------------------------------------------

        /// Without CAP_SYS_RESOURCE, raising the hard limit above its
        /// current value returns -1 with EPERM.  Mirrors Linux's
        /// `do_prlimit` "new_rlim->rlim_max > old_rlim->rlim_max &&
        /// !capable(CAP_SYS_RESOURCE)" branch.
        #[test]
        fn test_setrlimit_phase179_raise_hard_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            // Lower the hard limit to a finite value first so the
            // subsequent raise is observable (default is INFINITY).
            seed_limit(RLIMIT_CPU, 100, 200);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit { rlim_cur: 100, rlim_max: 500 };
            assert_eq!(setrlimit(RLIMIT_CPU, &new), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Errno is EPERM specifically — not EACCES (which the
        /// setpriority gate uses) and not EINVAL (which inverted
        /// soft/hard yields).  Locks the Linux choice.
        #[test]
        fn test_setrlimit_phase179_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 0, 10);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit { rlim_cur: 0, rlim_max: 20 };
            assert_eq!(setrlimit(RLIMIT_CPU, &new), -1);
            assert_ne!(errno::get_errno(), errno::EACCES);
            assert_ne!(errno::get_errno(), errno::EINVAL);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// RLIMIT_NOFILE rlim_max above fdtable::MAX_FDS is rejected
        /// EVEN when CAP_SYS_RESOURCE is held.  Linux makes the
        /// sysctl_nr_open ceiling absolute.
        #[test]
        fn test_setrlimit_phase179_nofile_above_max_fds_eperm_with_cap()
        {
            let _g = CapGuard::snapshot();
            reset_global_state();
            // Default RLIMIT_NOFILE is (MAX_FDS, MAX_FDS).
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_RESOURCE,
            ));
            errno::set_errno(0);
            let new = Rlimit {
                rlim_cur: crate::fdtable::MAX_FDS as u64,
                rlim_max: crate::fdtable::MAX_FDS as u64 + 1,
            };
            assert_eq!(setrlimit(RLIMIT_NOFILE, &new), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// And without CAP_SYS_RESOURCE: same EPERM (the NOFILE
        /// branch fires before the cap-gated branch in our impl, but
        /// the externally-visible result is identical).
        #[test]
        fn test_setrlimit_phase179_nofile_above_max_fds_eperm_no_cap() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit {
                rlim_cur: crate::fdtable::MAX_FDS as u64,
                rlim_max: crate::fdtable::MAX_FDS as u64 + 1,
            };
            assert_eq!(setrlimit(RLIMIT_NOFILE, &new), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix ----------------------------------------------

        /// EFAULT on a NULL pointer must beat the cap probe — Linux
        /// does `copy_from_user` before any cap check.
        #[test]
        fn test_setrlimit_phase179_efault_beats_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_resource();
            errno::set_errno(0);
            // NULL pointer + would-be-raise intent: NULL wins → EFAULT.
            assert_eq!(setrlimit(RLIMIT_CPU, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        /// EINVAL on a bad resource ordinal must beat the cap probe.
        #[test]
        fn test_setrlimit_phase179_einval_resource_beats_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit { rlim_cur: 1, rlim_max: u64::MAX };
            assert_eq!(setrlimit(9999, &new), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// EINVAL on rlim_cur > rlim_max must also beat the cap probe.
        #[test]
        fn test_setrlimit_phase179_einval_inverted_beats_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 0, 10);
            drop_cap_sys_resource();
            errno::set_errno(0);
            // Inverted soft/hard AND a hard-raise intent.  EINVAL wins.
            let new = Rlimit { rlim_cur: 50, rlim_max: 20 };
            assert_eq!(setrlimit(RLIMIT_CPU, &new), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// Lowering the hard limit is allowed without cap.  Linux's
        /// gate fires only on `new > old`.
        #[test]
        fn test_setrlimit_phase179_lower_hard_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 100, 1000);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit { rlim_cur: 50, rlim_max: 500 };
            assert_eq!(setrlimit(RLIMIT_CPU, &new), 0);
        }

        /// Holding hard equal (re-asserting the same hard) is allowed
        /// without cap — `new == old` is NOT `new > old`.
        #[test]
        fn test_setrlimit_phase179_equal_hard_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 100, 200);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit { rlim_cur: 50, rlim_max: 200 };
            assert_eq!(setrlimit(RLIMIT_CPU, &new), 0);
        }

        /// Soft-only change (hard kept identical) is allowed without
        /// cap, even when soft is raised — only the *hard* gate matters.
        #[test]
        fn test_setrlimit_phase179_soft_only_raise_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 10, 1000);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit { rlim_cur: 900, rlim_max: 1000 };
            assert_eq!(setrlimit(RLIMIT_CPU, &new), 0);
            let mut rb = Rlimit { rlim_cur: 0, rlim_max: 0 };
            assert_eq!(getrlimit(RLIMIT_CPU, &mut rb), 0);
            assert_eq!(rb.rlim_cur, 900);
            assert_eq!(rb.rlim_max, 1000);
        }

        // -- Workflow -----------------------------------------------------

        /// Privilege-separation workflow: a daemon starts with all
        /// caps, sets RLIMIT_CPU=(100,200), drops CAP_SYS_RESOURCE,
        /// then a child or post-init code path tries to raise the
        /// hard limit — must be EPERM.  After re-acquiring caps (via
        /// the CapGuard's drop or explicit restore) the raise works.
        #[test]
        fn test_setrlimit_phase179_workflow_seed_drop_raise_then_restore()
        {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 100, 200);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let raise = Rlimit { rlim_cur: 100, rlim_max: 400 };
            assert_eq!(setrlimit(RLIMIT_CPU, &raise), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Restore caps and try again.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            errno::set_errno(0);
            assert_eq!(setrlimit(RLIMIT_CPU, &raise), 0);
        }

        /// prlimit delegates to setrlimit: a raise via prlimit
        /// without cap must also EPERM.
        #[test]
        fn test_setrlimit_phase179_workflow_prlimit_raise_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_FSIZE, 0, 100);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit { rlim_cur: 0, rlim_max: 200 };
            assert_eq!(
                prlimit(0, RLIMIT_FSIZE, &new, core::ptr::null_mut()),
                -1,
            );
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy caller -------------------------------------------------

        /// A buggy unprivileged caller asking for RLIM_INFINITY hard
        /// after a lowered seed must hit EPERM, not silently raise
        /// back to unlimited.
        #[test]
        fn test_setrlimit_phase179_buggy_caller_raise_to_infinity_eperm()
        {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_AS, 0, 1 << 20);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let new = Rlimit {
                rlim_cur: 0,
                rlim_max: RLIM_INFINITY,
            };
            assert_eq!(setrlimit(RLIMIT_AS, &new), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// A privileged caller asking for MAX_FDS exactly on NOFILE
        /// is at the ceiling (not above) and must succeed.  Boundary
        /// check on the strict-greater-than NOFILE guard.
        #[test]
        fn test_setrlimit_phase179_nofile_exactly_max_fds_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            errno::set_errno(0);
            let new = Rlimit {
                rlim_cur: crate::fdtable::MAX_FDS as u64,
                rlim_max: crate::fdtable::MAX_FDS as u64,
            };
            assert_eq!(setrlimit(RLIMIT_NOFILE, &new), 0);
        }

        // -- Recovery -----------------------------------------------------

        /// After an EPERM rejection, restoring CAP_SYS_RESOURCE lets
        /// the same call succeed.  Confirms dynamic cap evaluation
        /// (not cached).
        #[test]
        fn test_setrlimit_phase179_recovery_restore_cap_lets_raise() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 0, 50);
            drop_cap_sys_resource();
            let raise = Rlimit { rlim_cur: 0, rlim_max: 75 };
            errno::set_errno(0);
            assert_eq!(setrlimit(RLIMIT_CPU, &raise), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Restore caps.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version:
                    crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(
                crate::sys_capability::capset(&mut hdr, data.as_ptr()),
                0,
            );
            errno::set_errno(0);
            assert_eq!(setrlimit(RLIMIT_CPU, &raise), 0);
        }

        // -- No-side-effect ----------------------------------------------

        /// An EPERM rejection must leave the stored Rlimit unchanged
        /// — Linux returns from `do_prlimit` before the slot write.
        #[test]
        fn test_setrlimit_phase179_eperm_does_not_mutate_state() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 100, 200);
            drop_cap_sys_resource();
            errno::set_errno(0);
            let raise = Rlimit { rlim_cur: 100, rlim_max: 500 };
            assert_eq!(setrlimit(RLIMIT_CPU, &raise), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Read back: still (100, 200).
            let mut rb = Rlimit { rlim_cur: 0, rlim_max: 0 };
            assert_eq!(getrlimit(RLIMIT_CPU, &mut rb), 0);
            assert_eq!(rb.rlim_cur, 100);
            assert_eq!(rb.rlim_max, 200);
        }

        /// NOFILE EPERM rejection (the unconditional branch) likewise
        /// leaves the stored limit unchanged.
        #[test]
        fn test_setrlimit_phase179_nofile_eperm_does_not_mutate_state() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            let before_cur = crate::fdtable::MAX_FDS as u64;
            let before_max = crate::fdtable::MAX_FDS as u64;
            errno::set_errno(0);
            let new = Rlimit {
                rlim_cur: before_cur,
                rlim_max: before_max + 1,
            };
            assert_eq!(setrlimit(RLIMIT_NOFILE, &new), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            let mut rb = Rlimit { rlim_cur: 0, rlim_max: 0 };
            assert_eq!(getrlimit(RLIMIT_NOFILE, &mut rb), 0);
            assert_eq!(rb.rlim_cur, before_cur);
            assert_eq!(rb.rlim_max, before_max);
        }

        // -- Sentinel -----------------------------------------------------

        /// With CAP_SYS_RESOURCE held (default), raising the hard
        /// limit works — confirms the privileged path is unbroken.
        #[test]
        fn test_setrlimit_phase179_sentinel_with_cap_raise_succeeds() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 0, 100);
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_RESOURCE,
            ));
            errno::set_errno(0);
            let new = Rlimit { rlim_cur: 0, rlim_max: 1000 };
            assert_eq!(setrlimit(RLIMIT_CPU, &new), 0);
            let mut rb = Rlimit { rlim_cur: 0, rlim_max: 0 };
            assert_eq!(getrlimit(RLIMIT_CPU, &mut rb), 0);
            assert_eq!(rb.rlim_max, 1000);
        }

        // -- Cross-checks -------------------------------------------------

        /// Dropping CAP_SYS_RESOURCE must not affect other caps —
        /// CAP_SYS_NICE / CAP_SYS_ADMIN remain held, so other syscalls
        /// that gate on those are unaffected.  Defends against a
        /// stray bit-clear regression in `drop_cap_sys_resource`.
        #[test]
        fn test_setrlimit_phase179_drop_sys_resource_leaves_other_caps() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_resource();
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_RESOURCE,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_NICE,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_TIME,
            ));
        }

        /// Raising a non-NOFILE resource above MAX_FDS is fine — the
        /// MAX_FDS ceiling is RLIMIT_NOFILE-only.  Sentinel against
        /// the NOFILE branch accidentally applying to other
        /// resources.
        #[test]
        fn test_setrlimit_phase179_non_nofile_above_max_fds_ok_with_cap()
        {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_CPU, 0, 100);
            errno::set_errno(0);
            let new = Rlimit {
                rlim_cur: 0,
                rlim_max: crate::fdtable::MAX_FDS as u64 * 10,
            };
            assert_eq!(setrlimit(RLIMIT_CPU, &new), 0);
        }

        /// prlimit's get-old + set-new path: when the set fails with
        /// EPERM, the old buffer should still have been populated
        /// (Linux fills the old buffer before attempting the set).
        #[test]
        fn test_setrlimit_phase179_prlimit_get_old_succeeds_set_eperm() {
            let _g = CapGuard::snapshot();
            reset_global_state();
            seed_limit(RLIMIT_FSIZE, 50, 100);
            drop_cap_sys_resource();
            let new = Rlimit { rlim_cur: 50, rlim_max: 999 };
            let mut old = Rlimit { rlim_cur: 0, rlim_max: 0 };
            errno::set_errno(0);
            assert_eq!(prlimit(0, RLIMIT_FSIZE, &new, &mut old), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // old should have been populated with the pre-call value.
            assert_eq!(old.rlim_cur, 50);
            assert_eq!(old.rlim_max, 100);
        }
    }
}
