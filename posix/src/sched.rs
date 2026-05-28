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
/// Deadline scheduling policy (Linux extension).
pub const SCHED_DEADLINE: i32 = 6;

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

// ---------------------------------------------------------------------------
// Policy / priority validation helpers
// ---------------------------------------------------------------------------

/// Recognised scheduling policies.  Anything outside this set is
/// rejected by `sched_setscheduler` / `sched_get_priority_min` /
/// `sched_get_priority_max` with `EINVAL`, matching the Linux kernel's
/// `kernel/sched/syscalls.c` validator: a `default:` arm in the policy
/// switch yields `-EINVAL`.
fn is_valid_policy(policy: i32) -> bool {
    matches!(
        policy,
        SCHED_OTHER | SCHED_FIFO | SCHED_RR | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE,
    )
}

/// Priority range for a given policy.  Returns `(min, max)` for the
/// six recognised policies and is used both by
/// `sched_get_priority_min`/`max` (to report) and by
/// `sched_setscheduler`/`sched_setparam` (to validate `sched_priority`).
fn priority_range(policy: i32) -> Option<(i32, i32)> {
    match policy {
        SCHED_FIFO | SCHED_RR => Some((1, 99)),
        SCHED_OTHER | SCHED_BATCH | SCHED_IDLE | SCHED_DEADLINE => Some((0, 0)),
        _ => None,
    }
}

/// Get the scheduling policy of a process.
///
/// Returns `SCHED_OTHER` for all processes (our scheduler doesn't
/// use POSIX policies).  A negative pid is rejected with `EINVAL`
/// to match Linux's prologue.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_getscheduler(pid: i32) -> i32 {
    if pid < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    SCHED_OTHER
}

/// Set the scheduling policy and parameters of a process.
///
/// Linux validation order (`kernel/sched/syscalls.c::sched_setscheduler`):
///   1. `pid < 0` → `EINVAL`.
///   2. Unknown policy → `EINVAL`.
///   3. `param == NULL` → `EINVAL` (we keep `EINVAL` for ABI
///      stability with the rest of this module; Linux uses `EFAULT`
///      via `copy_from_user`).
///   4. `sched_priority` outside `[min(policy), max(policy)]` → `EINVAL`.
///
/// After validation we have no real scheduler hookup, so we report
/// success without altering any task state.  Tests that wanted a
/// silent accept for an arbitrary policy must now pass a recognised
/// `SCHED_*` constant.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_setscheduler(
    pid: i32,
    policy: i32,
    param: *const SchedParam,
) -> i32 {
    if pid < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if !is_valid_policy(policy) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if param.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: param non-null per the check above; SchedParam is repr(C)
    // with a single i32, so the read is well-defined as long as the
    // caller supplied a properly aligned pointer (the public ABI
    // contract).
    let prio = unsafe { (*param).sched_priority };
    let Some((lo, hi)) = priority_range(policy) else {
        // Unreachable: is_valid_policy passed.
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if prio < lo || prio > hi {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Get the scheduling parameters of a process.
///
/// Returns priority 0 (default).  A negative pid is rejected with
/// `EINVAL` to match Linux's prologue.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_getparam(pid: i32, param: *mut SchedParam) -> i32 {
    if pid < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
/// Linux's `sched_setparam` keeps the current policy and adjusts the
/// priority.  Because we report every task as `SCHED_OTHER`, the
/// priority must be 0 (the only valid value for that policy).
/// A negative pid is rejected with `EINVAL`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_setparam(
    pid: i32,
    param: *const SchedParam,
) -> i32 {
    if pid < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if param.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: param non-null per the check above.
    let prio = unsafe { (*param).sched_priority };
    // Current policy is always SCHED_OTHER → only priority 0 is legal.
    if prio != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Get the minimum priority for a scheduling policy.
///
/// Returns 1 for real-time policies (`SCHED_FIFO`, `SCHED_RR`),
/// 0 for the other recognised policies.  Unknown policies are
/// rejected with `-1/EINVAL`, matching Linux behaviour.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_get_priority_min(policy: i32) -> i32 {
    match priority_range(policy) {
        Some((lo, _)) => lo,
        None => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Get the maximum priority for a scheduling policy.
///
/// Returns 99 for real-time policies, 0 for `SCHED_OTHER` /
/// `SCHED_BATCH` / `SCHED_IDLE` / `SCHED_DEADLINE`.  Unknown
/// policies are rejected with `-1/EINVAL`, matching Linux.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_get_priority_max(policy: i32) -> i32 {
    match priority_range(policy) {
        Some((_, hi)) => hi,
        None => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Default round-robin time quantum in nanoseconds (100 ms).
///
/// Typical Linux default for `SCHED_RR`.  Used by `sched_rr_get_interval`.
const RR_QUANTUM_NS: i64 = 100_000_000;

/// Get the round-robin time quantum.
///
/// Returns 100ms (a typical default) for all processes.  A negative
/// pid is rejected with `EINVAL`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_rr_get_interval(
    pid: i32,
    tp: *mut crate::stat::Timespec,
) -> i32 {
    if pid < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if tp.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: tp verified non-null.
    unsafe {
        (*tp).tv_sec = 0;
        (*tp).tv_nsec = RR_QUANTUM_NS;
    }
    0
}

// ---------------------------------------------------------------------------
// CPU affinity (Linux extensions — stubs)
// ---------------------------------------------------------------------------

/// CPU set size constant (matches Linux for x86_64).
pub const CPU_SETSIZE: usize = 1024;

/// CPU set type: bitmask of CPUs.
///
/// Stores CPU_SETSIZE bits in an array of u64s.  Each bit represents
/// one CPU.  Bit N = 1 means CPU N is in the set.
#[repr(C)]
pub struct CpuSetT {
    /// Bitmask storage (1024 bits = 128 bytes = 16 x u64).
    pub bits: [u64; 16],
}

/// Number of CPUs reportable by `sched_getaffinity` / fitting in a `CpuSetT`.
const CPU_SETSIZE_BITS: usize = 1024;

/// Query the kernel's online CPU count.  Falls back to 1 in test builds where
/// our SYSCALL ABI isn't valid.  Always returns ≥ 1.
fn online_cpu_count() -> usize {
    #[cfg(target_os = "none")]
    {
        let n = crate::syscall::syscall0(crate::syscall::SYS_CPU_COUNT);
        if n >= 1 { n as usize } else { 1 }
    }
    #[cfg(not(target_os = "none"))]
    {
        1
    }
}

/// Get the CPU affinity mask for a process.
///
/// Populates `mask` with bits 0..N set, where N is the number of online CPUs
/// (capped at `CPU_SETSIZE`).  Our scheduler doesn't yet support per-thread
/// affinity restriction, so every thread can be dispatched to any online CPU.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_getaffinity(
    pid: i32,
    cpusetsize: usize,
    mask: *mut CpuSetT,
) -> i32 {
    if pid < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if mask.is_null() || cpusetsize < core::mem::size_of::<CpuSetT>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let ncpus = online_cpu_count().min(CPU_SETSIZE_BITS);

    // SAFETY: mask is non-null and cpusetsize is large enough.
    unsafe {
        // Zero the mask first.
        let bytes = mask.cast::<u8>();
        let mut i: usize = 0;
        while i < core::mem::size_of::<CpuSetT>() {
            *bytes.add(i) = 0;
            i = i.wrapping_add(1);
        }
        // Set bits 0..ncpus.
        let mut cpu: usize = 0;
        while cpu < ncpus {
            let word = cpu / 64;
            let bit = cpu % 64;
            (*mask).bits[word] |= 1u64 << bit;
            cpu = cpu.wrapping_add(1);
        }
    }

    0
}

/// Set the CPU affinity mask for a process.
///
/// Validates the mask (non-NULL, sufficient size, at least one valid CPU bit
/// set) but does not actually constrain scheduling — our scheduler treats all
/// online CPUs as eligible.  Returns 0 on success, -1 with errno on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_setaffinity(
    pid: i32,
    cpusetsize: usize,
    mask: *const CpuSetT,
) -> i32 {
    if pid < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if mask.is_null() || cpusetsize < core::mem::size_of::<CpuSetT>() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let ncpus = online_cpu_count().min(CPU_SETSIZE_BITS);

    // SAFETY: mask is non-null and large enough.
    let any_valid = unsafe {
        let mut found = false;
        let mut cpu: usize = 0;
        while cpu < ncpus {
            let word = cpu / 64;
            let bit = cpu % 64;
            if (*mask).bits[word] & (1u64 << bit) != 0 {
                found = true;
                break;
            }
            cpu = cpu.wrapping_add(1);
        }
        found
    };

    if !any_valid {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    0
}

// ---------------------------------------------------------------------------
// CPU set manipulation functions
// ---------------------------------------------------------------------------
//
// glibc provides these as macros; we export them as `extern "C"` functions
// for our libc.  Programs compiled against our headers will call these.

/// Zero out a CPU set (clear all CPUs).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_zero(set: *mut CpuSetT) {
    if set.is_null() {
        return;
    }
    // SAFETY: set is non-null.
    unsafe {
        let mut i: usize = 0;
        while i < 16 {
            (*set).bits[i] = 0;
            i = i.wrapping_add(1);
        }
    }
}

/// Add a CPU to a CPU set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_set(cpu: i32, set: *mut CpuSetT) {
    if set.is_null() || cpu < 0 || cpu as usize >= CPU_SETSIZE {
        return;
    }
    let word = cpu as usize / 64;
    let bit = cpu as usize % 64;
    // SAFETY: set is non-null, word < 16 (cpu < 1024, 1024/64 = 16).
    unsafe { (*set).bits[word] |= 1u64 << bit; }
}

/// Remove a CPU from a CPU set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_clr(cpu: i32, set: *mut CpuSetT) {
    if set.is_null() || cpu < 0 || cpu as usize >= CPU_SETSIZE {
        return;
    }
    let word = cpu as usize / 64;
    let bit = cpu as usize % 64;
    // SAFETY: set is non-null, word < 16.
    unsafe { (*set).bits[word] &= !(1u64 << bit); }
}

/// Test if a CPU is in a CPU set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_isset(cpu: i32, set: *const CpuSetT) -> i32 {
    if set.is_null() || cpu < 0 || cpu as usize >= CPU_SETSIZE {
        return 0;
    }
    let word = cpu as usize / 64;
    let bit = cpu as usize % 64;
    // SAFETY: set is non-null, word < 16.
    let val = unsafe { (*set).bits[word] };
    if val & (1u64 << bit) != 0 { 1 } else { 0 }
}

/// Count the number of CPUs in a CPU set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_count(set: *const CpuSetT) -> i32 {
    if set.is_null() {
        return 0;
    }
    let mut count: u32 = 0;
    let mut i: usize = 0;
    // SAFETY: set is non-null.
    while i < 16 {
        let val = unsafe { (*set).bits[i] };
        count = count.wrapping_add(val.count_ones());
        i = i.wrapping_add(1);
    }
    count as i32
}

/// Compute the bitwise AND of two CPU sets (intersection).
///
/// `destset = srcset1 & srcset2`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_and(destset: *mut CpuSetT, srcset1: *const CpuSetT, srcset2: *const CpuSetT) {
    if destset.is_null() || srcset1.is_null() || srcset2.is_null() {
        return;
    }
    // SAFETY: all pointers verified non-null.
    let mut i: usize = 0;
    while i < 16 {
        unsafe { (*destset).bits[i] = (*srcset1).bits[i] & (*srcset2).bits[i]; }
        i = i.wrapping_add(1);
    }
}

/// Compute the bitwise OR of two CPU sets (union).
///
/// `destset = srcset1 | srcset2`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_or(destset: *mut CpuSetT, srcset1: *const CpuSetT, srcset2: *const CpuSetT) {
    if destset.is_null() || srcset1.is_null() || srcset2.is_null() {
        return;
    }
    let mut i: usize = 0;
    while i < 16 {
        unsafe { (*destset).bits[i] = (*srcset1).bits[i] | (*srcset2).bits[i]; }
        i = i.wrapping_add(1);
    }
}

/// Compute the bitwise XOR of two CPU sets (symmetric difference).
///
/// `destset = srcset1 ^ srcset2`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_xor(destset: *mut CpuSetT, srcset1: *const CpuSetT, srcset2: *const CpuSetT) {
    if destset.is_null() || srcset1.is_null() || srcset2.is_null() {
        return;
    }
    let mut i: usize = 0;
    while i < 16 {
        unsafe { (*destset).bits[i] = (*srcset1).bits[i] ^ (*srcset2).bits[i]; }
        i = i.wrapping_add(1);
    }
}

/// Test if two CPU sets are equal.
///
/// Returns 1 if the sets are identical, 0 otherwise.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpu_equal(set1: *const CpuSetT, set2: *const CpuSetT) -> i32 {
    if set1.is_null() || set2.is_null() {
        return 0;
    }
    let mut i: usize = 0;
    while i < 16 {
        // SAFETY: both pointers verified non-null.
        if unsafe { (*set1).bits[i] != (*set2).bits[i] } {
            return 0;
        }
        i = i.wrapping_add(1);
    }
    1
}

/// Get the CPU number on which the calling thread is running.
///
/// Stub: always returns 0 (single-CPU assumption until SMP is
/// implemented in the kernel).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_getcpu() -> i32 {
    0
}

/// Get CPU and NUMA node (Linux vDSO interface).
///
/// Stub: returns 0 for both CPU and node.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getcpu(cpu: *mut u32, node: *mut u32) -> i32 {
    if !cpu.is_null() {
        // SAFETY: Caller guarantees pointer validity.
        unsafe { *cpu = 0; }
    }
    if !node.is_null() {
        unsafe { *node = 0; }
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Policy constants match Linux --

    #[test]
    fn test_sched_policy_values() {
        assert_eq!(SCHED_OTHER, 0);
        assert_eq!(SCHED_FIFO, 1);
        assert_eq!(SCHED_RR, 2);
        assert_eq!(SCHED_BATCH, 3);
        assert_eq!(SCHED_IDLE, 5);
        assert_eq!(SCHED_DEADLINE, 6);
    }

    // -- sched_getscheduler --

    #[test]
    fn test_sched_getscheduler_returns_other() {
        // Linux rejects pid<0 with EINVAL — see test_sched_getscheduler_negative_pid_einval.
        assert_eq!(sched_getscheduler(0), SCHED_OTHER);
        assert_eq!(sched_getscheduler(1), SCHED_OTHER);
        assert_eq!(sched_getscheduler(i32::MAX), SCHED_OTHER);
    }

    // -- sched_setscheduler --

    #[test]
    fn test_sched_setscheduler_succeeds() {
        let param = SchedParam { sched_priority: 50 };
        assert_eq!(sched_setscheduler(0, SCHED_RR, &raw const param), 0);
    }

    #[test]
    fn test_sched_setscheduler_null_param() {
        assert_eq!(sched_setscheduler(0, SCHED_RR, core::ptr::null()), -1);
    }

    #[test]
    fn test_sched_setparam_null_param() {
        assert_eq!(sched_setparam(0, core::ptr::null()), -1);
    }

    // -- sched_getparam --

    #[test]
    fn test_sched_getparam_fills_zero_priority() {
        let mut param = SchedParam { sched_priority: 99 };
        let ret = sched_getparam(0, &raw mut param);
        assert_eq!(ret, 0);
        assert_eq!(param.sched_priority, 0);
    }

    #[test]
    fn test_sched_getparam_null() {
        let ret = sched_getparam(0, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -- sched_setparam --

    #[test]
    fn test_sched_setparam_succeeds() {
        // sched_setparam adjusts priority within the *current* policy.
        // We report every task as SCHED_OTHER, so priority must be 0.
        let param = SchedParam { sched_priority: 0 };
        assert_eq!(sched_setparam(0, &raw const param), 0);
    }

    // -- Priority range --

    #[test]
    fn test_sched_priority_min_other() {
        assert_eq!(sched_get_priority_min(SCHED_OTHER), 0);
        assert_eq!(sched_get_priority_min(SCHED_BATCH), 0);
        assert_eq!(sched_get_priority_min(SCHED_IDLE), 0);
    }

    #[test]
    fn test_sched_priority_min_realtime() {
        // Real-time policies have min priority 1 (matching Linux).
        assert_eq!(sched_get_priority_min(SCHED_FIFO), 1);
        assert_eq!(sched_get_priority_min(SCHED_RR), 1);
    }

    #[test]
    fn test_sched_priority_max_realtime() {
        assert_eq!(sched_get_priority_max(SCHED_FIFO), 99);
        assert_eq!(sched_get_priority_max(SCHED_RR), 99);
    }

    #[test]
    fn test_sched_priority_max_other() {
        assert_eq!(sched_get_priority_max(SCHED_OTHER), 0);
        assert_eq!(sched_get_priority_max(SCHED_BATCH), 0);
        assert_eq!(sched_get_priority_max(SCHED_IDLE), 0);
    }

    // -- sched_rr_get_interval --

    #[test]
    fn test_sched_rr_get_interval_100ms() {
        let mut tp = crate::stat::Timespec { tv_sec: 99, tv_nsec: 99 };
        let ret = sched_rr_get_interval(0, &raw mut tp);
        assert_eq!(ret, 0);
        assert_eq!(tp.tv_sec, 0);
        assert_eq!(tp.tv_nsec, 100_000_000); // 100ms
    }

    #[test]
    fn test_sched_rr_get_interval_null() {
        let ret = sched_rr_get_interval(0, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -- CPU affinity --

    #[test]
    fn test_cpu_setsize() {
        assert_eq!(CPU_SETSIZE, 1024);
    }

    #[test]
    fn test_cpuset_size() {
        // 1024 bits = 128 bytes = 16 * 8
        assert_eq!(core::mem::size_of::<CpuSetT>(), 128);
    }

    #[test]
    fn test_sched_getaffinity_sets_cpu0() {
        let mut cpuset = CpuSetT { bits: [0xFF; 16] };
        let ret = sched_getaffinity(
            0,
            core::mem::size_of::<CpuSetT>(),
            &raw mut cpuset,
        );
        assert_eq!(ret, 0);
        assert_eq!(cpuset.bits[0], 1); // Only CPU 0
        for i in 1..16 {
            assert_eq!(cpuset.bits[i], 0, "bits[{i}] should be 0");
        }
    }

    #[test]
    fn test_sched_getaffinity_null() {
        let ret = sched_getaffinity(0, 128, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sched_getaffinity_too_small() {
        let mut cpuset = CpuSetT { bits: [0; 16] };
        let ret = sched_getaffinity(0, 1, &raw mut cpuset); // Too small
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sched_setaffinity_succeeds() {
        let cpuset = CpuSetT { bits: [1; 16] };
        let ret = sched_setaffinity(0, 128, &raw const cpuset);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_sched_setaffinity_null_einval() {
        let ret = sched_setaffinity(0, 128, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setaffinity_too_small_einval() {
        let cpuset = CpuSetT { bits: [1; 16] };
        let ret = sched_setaffinity(0, 1, &raw const cpuset);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setaffinity_empty_mask_einval() {
        // An all-zero mask has no valid CPU bits set -> EINVAL.
        let cpuset = CpuSetT { bits: [0; 16] };
        let ret = sched_setaffinity(0, 128, &raw const cpuset);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setaffinity_unreachable_cpu_einval() {
        // Bit only set for CPU 100, but in host test build only 1 CPU is
        // online, so no valid bit -> EINVAL.
        let mut cpuset = CpuSetT { bits: [0; 16] };
        cpuset.bits[1] = 1u64 << (100 - 64); // bit 100
        let ret = sched_setaffinity(0, 128, &raw const cpuset);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_getaffinity_roundtrip() {
        // sched_getaffinity should give back a mask that sched_setaffinity
        // accepts — the kernel's reported affinity is always usable.
        let mut cpuset = CpuSetT { bits: [0; 16] };
        let g = sched_getaffinity(0, 128, &raw mut cpuset);
        assert_eq!(g, 0);
        let s = sched_setaffinity(0, 128, &raw const cpuset);
        assert_eq!(s, 0);
    }

    // -- sched_getcpu / getcpu --

    #[test]
    fn test_sched_getcpu_returns_zero() {
        assert_eq!(sched_getcpu(), 0);
    }

    #[test]
    fn test_getcpu_returns_zero() {
        let mut cpu: u32 = 99;
        let mut node: u32 = 99;
        let ret = getcpu(&raw mut cpu, &raw mut node);
        assert_eq!(ret, 0);
        assert_eq!(cpu, 0);
        assert_eq!(node, 0);
    }

    #[test]
    fn test_getcpu_null_args() {
        let ret = getcpu(core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    // -- SchedParam layout --

    #[test]
    fn test_sched_param_size() {
        assert_eq!(core::mem::size_of::<SchedParam>(), 4);
    }

    // -- CPU set manipulation --

    #[test]
    fn test_cpu_zero_clears_all() {
        let mut set = CpuSetT { bits: [0xFFFF_FFFF_FFFF_FFFF; 16] };
        cpu_zero(&raw mut set);
        for i in 0..16 {
            assert_eq!(set.bits[i], 0, "bits[{i}] not zeroed");
        }
    }

    #[test]
    fn test_cpu_set_and_isset() {
        let mut set = CpuSetT { bits: [0; 16] };
        cpu_set(0, &raw mut set);
        assert_eq!(cpu_isset(0, &raw const set), 1);
        assert_eq!(cpu_isset(1, &raw const set), 0);

        cpu_set(63, &raw mut set);
        assert_eq!(cpu_isset(63, &raw const set), 1);
        assert_eq!(cpu_isset(62, &raw const set), 0);

        cpu_set(64, &raw mut set);
        assert_eq!(cpu_isset(64, &raw const set), 1);
        assert_eq!(set.bits[1], 1); // bit 0 of word 1
    }

    #[test]
    fn test_cpu_clr() {
        let mut set = CpuSetT { bits: [0; 16] };
        cpu_set(5, &raw mut set);
        assert_eq!(cpu_isset(5, &raw const set), 1);
        cpu_clr(5, &raw mut set);
        assert_eq!(cpu_isset(5, &raw const set), 0);
    }

    #[test]
    fn test_cpu_count() {
        let mut set = CpuSetT { bits: [0; 16] };
        assert_eq!(cpu_count(&raw const set), 0);
        cpu_set(0, &raw mut set);
        assert_eq!(cpu_count(&raw const set), 1);
        cpu_set(100, &raw mut set);
        assert_eq!(cpu_count(&raw const set), 2);
        cpu_set(1023, &raw mut set);
        assert_eq!(cpu_count(&raw const set), 3);
    }

    #[test]
    fn test_cpu_set_out_of_range() {
        let mut set = CpuSetT { bits: [0; 16] };
        // These should be no-ops (not crash).
        cpu_set(-1, &raw mut set);
        cpu_set(1024, &raw mut set);
        cpu_set(i32::MAX, &raw mut set);
        assert_eq!(cpu_count(&raw const set), 0);
    }

    #[test]
    fn test_cpu_isset_out_of_range() {
        let set = CpuSetT { bits: [0xFF; 16] };
        assert_eq!(cpu_isset(-1, &raw const set), 0);
        assert_eq!(cpu_isset(1024, &raw const set), 0);
    }

    // -- cpu_and --

    #[test]
    fn test_cpu_and_basic() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0xFF; 16] };
        cpu_set(0, &raw mut a);
        cpu_set(1, &raw mut a);
        cpu_set(2, &raw mut a);
        cpu_set(1, &raw mut b);
        cpu_set(2, &raw mut b);
        cpu_set(3, &raw mut b);
        cpu_and(&raw mut dest, &raw const a, &raw const b);
        // Intersection: CPUs 1 and 2.
        assert_eq!(cpu_isset(0, &raw const dest), 0);
        assert_eq!(cpu_isset(1, &raw const dest), 1);
        assert_eq!(cpu_isset(2, &raw const dest), 1);
        assert_eq!(cpu_isset(3, &raw const dest), 0);
        assert_eq!(cpu_count(&raw const dest), 2);
    }

    #[test]
    fn test_cpu_and_disjoint() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0xFF; 16] };
        cpu_set(0, &raw mut a);
        cpu_set(1, &raw mut b);
        cpu_and(&raw mut dest, &raw const a, &raw const b);
        assert_eq!(cpu_count(&raw const dest), 0);
    }

    #[test]
    fn test_cpu_and_null_safety() {
        let set = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0xFF; 16] };
        // Should not crash.
        cpu_and(core::ptr::null_mut(), &raw const set, &raw const set);
        cpu_and(&raw mut dest, core::ptr::null(), &raw const set);
        cpu_and(&raw mut dest, &raw const set, core::ptr::null());
    }

    // -- cpu_or --

    #[test]
    fn test_cpu_or_basic() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0; 16] };
        cpu_set(0, &raw mut a);
        cpu_set(1, &raw mut b);
        cpu_or(&raw mut dest, &raw const a, &raw const b);
        assert_eq!(cpu_isset(0, &raw const dest), 1);
        assert_eq!(cpu_isset(1, &raw const dest), 1);
        assert_eq!(cpu_count(&raw const dest), 2);
    }

    #[test]
    fn test_cpu_or_overlapping() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0; 16] };
        cpu_set(5, &raw mut a);
        cpu_set(5, &raw mut b);
        cpu_set(10, &raw mut b);
        cpu_or(&raw mut dest, &raw const a, &raw const b);
        assert_eq!(cpu_isset(5, &raw const dest), 1);
        assert_eq!(cpu_isset(10, &raw const dest), 1);
        assert_eq!(cpu_count(&raw const dest), 2);
    }

    // -- cpu_xor --

    #[test]
    fn test_cpu_xor_basic() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0; 16] };
        cpu_set(0, &raw mut a);
        cpu_set(1, &raw mut a);
        cpu_set(1, &raw mut b);
        cpu_set(2, &raw mut b);
        cpu_xor(&raw mut dest, &raw const a, &raw const b);
        // Symmetric difference: CPUs 0 and 2.
        assert_eq!(cpu_isset(0, &raw const dest), 1);
        assert_eq!(cpu_isset(1, &raw const dest), 0);
        assert_eq!(cpu_isset(2, &raw const dest), 1);
        assert_eq!(cpu_count(&raw const dest), 2);
    }

    #[test]
    fn test_cpu_xor_same_sets() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0xFF; 16] };
        cpu_set(0, &raw mut a);
        cpu_set(5, &raw mut a);
        cpu_xor(&raw mut dest, &raw const a, &raw const a);
        // XOR of a set with itself is empty.
        assert_eq!(cpu_count(&raw const dest), 0);
    }

    // -- cpu_equal --

    #[test]
    fn test_cpu_equal_identical() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        cpu_set(3, &raw mut a);
        cpu_set(3, &raw mut b);
        assert_eq!(cpu_equal(&raw const a, &raw const b), 1);
    }

    #[test]
    fn test_cpu_equal_different() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        cpu_set(3, &raw mut a);
        cpu_set(4, &raw mut b);
        assert_eq!(cpu_equal(&raw const a, &raw const b), 0);
    }

    #[test]
    fn test_cpu_equal_both_empty() {
        let a = CpuSetT { bits: [0; 16] };
        let b = CpuSetT { bits: [0; 16] };
        assert_eq!(cpu_equal(&raw const a, &raw const b), 1);
    }

    #[test]
    fn test_cpu_equal_null_returns_zero() {
        let a = CpuSetT { bits: [0; 16] };
        assert_eq!(cpu_equal(core::ptr::null(), &raw const a), 0);
        assert_eq!(cpu_equal(&raw const a, core::ptr::null()), 0);
        assert_eq!(cpu_equal(core::ptr::null(), core::ptr::null()), 0);
    }

    #[test]
    fn test_cpu_equal_high_cpus() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        cpu_set(1023, &raw mut a);
        cpu_set(1023, &raw mut b);
        assert_eq!(cpu_equal(&raw const a, &raw const b), 1);

        cpu_set(0, &raw mut a);
        assert_eq!(cpu_equal(&raw const a, &raw const b), 0);
    }

    // -- CPU set word boundary tests --

    #[test]
    fn test_cpu_set_word_boundary_63() {
        let mut set = CpuSetT { bits: [0; 16] };
        cpu_set(63, &raw mut set);
        assert_eq!(cpu_isset(63, &raw const set), 1);
        assert_eq!(set.bits[0], 1u64 << 63);
        assert_eq!(set.bits[1], 0);
    }

    #[test]
    fn test_cpu_set_word_boundary_64() {
        let mut set = CpuSetT { bits: [0; 16] };
        cpu_set(64, &raw mut set);
        assert_eq!(cpu_isset(64, &raw const set), 1);
        assert_eq!(set.bits[0], 0);
        assert_eq!(set.bits[1], 1);
    }

    #[test]
    fn test_cpu_set_word_boundary_127() {
        let mut set = CpuSetT { bits: [0; 16] };
        cpu_set(127, &raw mut set);
        assert_eq!(cpu_isset(127, &raw const set), 1);
        assert_eq!(set.bits[1], 1u64 << 63);
        assert_eq!(set.bits[2], 0);
    }

    #[test]
    fn test_cpu_set_word_boundary_128() {
        let mut set = CpuSetT { bits: [0; 16] };
        cpu_set(128, &raw mut set);
        assert_eq!(cpu_isset(128, &raw const set), 1);
        assert_eq!(set.bits[1], 0);
        assert_eq!(set.bits[2], 1);
    }

    #[test]
    fn test_cpu_set_last_valid_1023() {
        let mut set = CpuSetT { bits: [0; 16] };
        cpu_set(1023, &raw mut set);
        assert_eq!(cpu_isset(1023, &raw const set), 1);
        // 1023 = word 15, bit 63
        assert_eq!(set.bits[15], 1u64 << 63);
    }

    #[test]
    fn test_cpu_clr_word_boundary() {
        let mut set = CpuSetT { bits: [0; 16] };
        cpu_set(63, &raw mut set);
        cpu_set(64, &raw mut set);
        assert_eq!(cpu_count(&raw const set), 2);
        cpu_clr(63, &raw mut set);
        assert_eq!(cpu_isset(63, &raw const set), 0);
        assert_eq!(cpu_isset(64, &raw const set), 1);
        assert_eq!(cpu_count(&raw const set), 1);
    }

    // -- CPU set all bits in a word --

    #[test]
    fn test_cpu_set_fill_first_word() {
        let mut set = CpuSetT { bits: [0; 16] };
        for i in 0..64 {
            cpu_set(i, &raw mut set);
        }
        assert_eq!(set.bits[0], u64::MAX);
        assert_eq!(set.bits[1], 0);
        assert_eq!(cpu_count(&raw const set), 64);
    }

    #[test]
    fn test_cpu_set_fill_second_word() {
        let mut set = CpuSetT { bits: [0; 16] };
        for i in 64..128 {
            cpu_set(i, &raw mut set);
        }
        assert_eq!(set.bits[0], 0);
        assert_eq!(set.bits[1], u64::MAX);
        assert_eq!(cpu_count(&raw const set), 64);
    }

    #[test]
    fn test_cpu_count_all_bits_set() {
        let set = CpuSetT { bits: [u64::MAX; 16] };
        assert_eq!(cpu_count(&raw const set), 1024);
    }

    // -- CPU set operations across words --

    #[test]
    fn test_cpu_and_cross_word() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0; 16] };

        // Set bits in different words
        cpu_set(63, &raw mut a);  // word 0
        cpu_set(64, &raw mut a);  // word 1
        cpu_set(64, &raw mut b);  // word 1
        cpu_set(128, &raw mut b); // word 2

        cpu_and(&raw mut dest, &raw const a, &raw const b);
        // Only 64 is in both
        assert_eq!(cpu_isset(63, &raw const dest), 0);
        assert_eq!(cpu_isset(64, &raw const dest), 1);
        assert_eq!(cpu_isset(128, &raw const dest), 0);
        assert_eq!(cpu_count(&raw const dest), 1);
    }

    #[test]
    fn test_cpu_or_cross_word() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0; 16] };

        cpu_set(63, &raw mut a);   // word 0
        cpu_set(128, &raw mut b);  // word 2
        cpu_set(511, &raw mut b);  // word 7

        cpu_or(&raw mut dest, &raw const a, &raw const b);
        assert_eq!(cpu_isset(63, &raw const dest), 1);
        assert_eq!(cpu_isset(128, &raw const dest), 1);
        assert_eq!(cpu_isset(511, &raw const dest), 1);
        assert_eq!(cpu_count(&raw const dest), 3);
    }

    #[test]
    fn test_cpu_xor_cross_word() {
        let mut a = CpuSetT { bits: [0; 16] };
        let mut b = CpuSetT { bits: [0; 16] };
        let mut dest = CpuSetT { bits: [0; 16] };

        cpu_set(0, &raw mut a);
        cpu_set(0, &raw mut b);    // same — cancels
        cpu_set(64, &raw mut a);   // only in a
        cpu_set(128, &raw mut b);  // only in b

        cpu_xor(&raw mut dest, &raw const a, &raw const b);
        assert_eq!(cpu_isset(0, &raw const dest), 0);   // cancelled
        assert_eq!(cpu_isset(64, &raw const dest), 1);
        assert_eq!(cpu_isset(128, &raw const dest), 1);
        assert_eq!(cpu_count(&raw const dest), 2);
    }

    // -- CpuSetT layout --

    #[test]
    fn test_cpu_set_size() {
        // 16 × 8 bytes = 128 bytes
        assert_eq!(core::mem::size_of::<CpuSetT>(), 128);
    }

    #[test]
    fn test_cpu_set_alignment() {
        assert_eq!(core::mem::align_of::<CpuSetT>(), 8);
    }

    // -- sched_setscheduler errno --

    #[test]
    fn test_sched_setscheduler_recognised_policies_accepted() {
        // Phase 74: only the six SCHED_* constants are accepted.  Unknown
        // policies (e.g. 99) now yield EINVAL — see
        // test_sched_setscheduler_unknown_policy_einval.
        let param = SchedParam { sched_priority: 0 };
        assert_eq!(sched_setscheduler(0, SCHED_OTHER, &raw const param), 0);
        assert_eq!(sched_setscheduler(0, SCHED_BATCH, &raw const param), 0);
        assert_eq!(sched_setscheduler(0, SCHED_IDLE, &raw const param), 0);
        let rt = SchedParam { sched_priority: 50 };
        assert_eq!(sched_setscheduler(0, SCHED_FIFO, &raw const rt), 0);
        assert_eq!(sched_setscheduler(0, SCHED_RR, &raw const rt), 0);
    }

    // -- SchedParam layout --

    #[test]
    fn test_sched_param_alignment() {
        assert_eq!(core::mem::align_of::<SchedParam>(), 4);
    }

    #[test]
    fn test_sched_param_field_access() {
        let p = SchedParam { sched_priority: 42 };
        assert_eq!(p.sched_priority, 42);
    }

    // -- sched_getparam returns zero priority --

    #[test]
    fn test_sched_getparam_pid_zero() {
        let mut param = SchedParam { sched_priority: 99 };
        let ret = sched_getparam(0, &raw mut param);
        assert_eq!(ret, 0);
        assert_eq!(param.sched_priority, 0);
    }

    // -- sched_get_priority range --

    #[test]
    fn test_sched_priority_min_leq_max() {
        let min = sched_get_priority_min(SCHED_OTHER);
        let max = sched_get_priority_max(SCHED_OTHER);
        assert!(min <= max, "min ({min}) should be <= max ({max})");
    }

    // -- cpu_isset with clr'd bit --

    #[test]
    fn test_cpu_isset_after_clr_out_of_range() {
        let mut set = CpuSetT { bits: [u64::MAX; 16] };
        // Clear out of range should be no-op
        cpu_clr(-1, &raw mut set);
        cpu_clr(1024, &raw mut set);
        // All bits should still be set
        assert_eq!(cpu_count(&raw const set), 1024);
    }

    // =====================================================================
    // Phase 74 — sched_* policy / priority / pid validation
    //
    // Linux validation contract (kernel/sched/syscalls.c):
    //   * pid < 0                                  → EINVAL
    //   * policy ∉ recognised SCHED_* constants    → EINVAL
    //   * sched_priority ∉ [min(policy), max(policy)] → EINVAL
    //
    // Order: pid first, then policy, then priority (matches Linux's
    // prologue ordering — bad pid wins over bad policy wins over bad
    // priority).  These tests cover each error class, the ordering, and
    // a handful of buggy-caller patterns.
    // =====================================================================

    // ---- Per-error class: sched_get_priority_min/max unknown policy ----

    #[test]
    fn test_sched_get_priority_min_unknown_policy_einval() {
        errno::set_errno(0);
        assert_eq!(sched_get_priority_min(99), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_get_priority_min_negative_policy_einval() {
        errno::set_errno(0);
        assert_eq!(sched_get_priority_min(-1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_get_priority_min_max_int_einval() {
        errno::set_errno(0);
        assert_eq!(sched_get_priority_min(i32::MAX), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_get_priority_max_unknown_policy_einval() {
        errno::set_errno(0);
        assert_eq!(sched_get_priority_max(99), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_get_priority_max_negative_policy_einval() {
        errno::set_errno(0);
        assert_eq!(sched_get_priority_max(i32::MIN), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_get_priority_min_max_all_recognised_policies() {
        // No errno write for any recognised policy.
        for &p in &[SCHED_OTHER, SCHED_FIFO, SCHED_RR, SCHED_BATCH, SCHED_IDLE, SCHED_DEADLINE] {
            errno::set_errno(0);
            let _ = sched_get_priority_min(p);
            // errno may or may not be set, but the return must be ≥ 0.
            assert!(sched_get_priority_min(p) >= 0);
            assert!(sched_get_priority_max(p) >= 0);
        }
    }

    // ---- Per-error class: sched_setscheduler unknown policy ----

    #[test]
    fn test_sched_setscheduler_unknown_policy_einval() {
        let param = SchedParam { sched_priority: 0 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, 99, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_policy_4_einval() {
        // Linux skipped policy 4 (was SCHED_ISO, never released).  Our
        // recognised set follows mainline: 0,1,2,3,5,6 — so 4 must
        // reject.
        let param = SchedParam { sched_priority: 0 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, 4, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_policy_negative_einval() {
        let param = SchedParam { sched_priority: 0 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, -1, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Per-error class: sched_setscheduler priority out of range ----

    #[test]
    fn test_sched_setscheduler_rr_priority_zero_einval() {
        // SCHED_RR range is [1, 99] — 0 is below min.
        let param = SchedParam { sched_priority: 0 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, SCHED_RR, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_rr_priority_100_einval() {
        // SCHED_RR max is 99 — 100 is above max.
        let param = SchedParam { sched_priority: 100 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, SCHED_RR, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_fifo_priority_negative_einval() {
        let param = SchedParam { sched_priority: -5 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, SCHED_FIFO, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_other_priority_nonzero_einval() {
        // SCHED_OTHER range is [0, 0] — only 0 is valid.
        let param = SchedParam { sched_priority: 1 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, SCHED_OTHER, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_batch_priority_nonzero_einval() {
        let param = SchedParam { sched_priority: 5 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, SCHED_BATCH, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_rr_priority_boundaries_ok() {
        // 1 and 99 are inclusive bounds for SCHED_RR/SCHED_FIFO.
        let lo = SchedParam { sched_priority: 1 };
        let hi = SchedParam { sched_priority: 99 };
        assert_eq!(sched_setscheduler(0, SCHED_RR, &raw const lo), 0);
        assert_eq!(sched_setscheduler(0, SCHED_RR, &raw const hi), 0);
    }

    // ---- Per-error class: sched_setparam priority out of range ----

    #[test]
    fn test_sched_setparam_nonzero_priority_einval() {
        // Reported policy is SCHED_OTHER → only priority 0 is valid.
        let param = SchedParam { sched_priority: 50 };
        errno::set_errno(0);
        assert_eq!(sched_setparam(0, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setparam_negative_priority_einval() {
        let param = SchedParam { sched_priority: -1 };
        errno::set_errno(0);
        assert_eq!(sched_setparam(0, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Per-error class: pid<0 ----

    #[test]
    fn test_sched_getscheduler_negative_pid_einval() {
        errno::set_errno(0);
        assert_eq!(sched_getscheduler(-1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_getscheduler_min_pid_einval() {
        errno::set_errno(0);
        assert_eq!(sched_getscheduler(i32::MIN), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_negative_pid_einval() {
        let param = SchedParam { sched_priority: 0 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(-1, SCHED_OTHER, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setparam_negative_pid_einval() {
        let param = SchedParam { sched_priority: 0 };
        errno::set_errno(0);
        assert_eq!(sched_setparam(-1, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_getparam_negative_pid_einval() {
        let mut param = SchedParam { sched_priority: 99 };
        errno::set_errno(0);
        assert_eq!(sched_getparam(-1, &raw mut param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Buffer untouched on validation failure.
        assert_eq!(param.sched_priority, 99);
    }

    #[test]
    fn test_sched_rr_get_interval_negative_pid_einval() {
        let mut tp = crate::stat::Timespec { tv_sec: 7, tv_nsec: 7 };
        errno::set_errno(0);
        assert_eq!(sched_rr_get_interval(-1, &raw mut tp), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Buffer untouched on validation failure.
        assert_eq!(tp.tv_sec, 7);
        assert_eq!(tp.tv_nsec, 7);
    }

    #[test]
    fn test_sched_getaffinity_negative_pid_einval() {
        let mut cpuset = CpuSetT { bits: [0xAA; 16] };
        errno::set_errno(0);
        assert_eq!(
            sched_getaffinity(-1, core::mem::size_of::<CpuSetT>(), &raw mut cpuset),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Buffer untouched on validation failure.
        assert_eq!(cpuset.bits[0], 0xAA);
    }

    #[test]
    fn test_sched_setaffinity_negative_pid_einval() {
        let mut cpuset = CpuSetT { bits: [0; 16] };
        cpu_set(0, &raw mut cpuset);
        errno::set_errno(0);
        assert_eq!(
            sched_setaffinity(-1, core::mem::size_of::<CpuSetT>(), &raw const cpuset),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Validation ordering ----

    #[test]
    fn test_sched_setscheduler_bad_pid_beats_bad_policy() {
        // pid<0 fires first → EINVAL via the pid arm (not the policy arm).
        // Both arms yield EINVAL, so we can only check the precedence
        // indirectly: with policy=4 (skipped) and pid<0, we still get
        // EINVAL.  Use a NULL param too to make sure the bad-pid gate
        // is what saved us from the param read.
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(-1, 4, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_bad_policy_beats_null_param() {
        // policy=99 (unknown) fires before the NULL-param check.  This
        // matters because a real Linux caller would learn "your policy
        // is wrong" before "your buffer is bad".
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, 99, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_bad_policy_beats_bad_priority() {
        // policy=99 with priority=99 (which would be valid for SCHED_RR).
        // Policy gate fires first.
        let param = SchedParam { sched_priority: 99 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, 99, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Buggy-caller patterns ----

    #[test]
    fn test_sched_setscheduler_buggy_random_int_as_policy() {
        // A program reads an int from a config file and passes it
        // straight through as the policy.  If it doesn't match a
        // recognised SCHED_*, we now reject — silently accepting it
        // (old behaviour) hid the misconfiguration.
        let param = SchedParam { sched_priority: 0 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, 12345, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_buggy_swapped_policy_and_priority() {
        // Caller swaps `policy` and `sched_priority`: calls with
        // policy=50 (intending priority) and priority=SCHED_RR=2.
        // policy=50 is unknown → EINVAL.
        let param = SchedParam { sched_priority: SCHED_RR };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, 50, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setparam_buggy_uses_realtime_priority_under_other() {
        // Caller copies sched_priority=50 from a SCHED_RR example into a
        // sched_setparam call without changing policy.  Since current
        // policy is reported as SCHED_OTHER, the priority must be 0.
        let param = SchedParam { sched_priority: 50 };
        errno::set_errno(0);
        assert_eq!(sched_setparam(0, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_buggy_priority_at_min_minus_one() {
        // Off-by-one: priority = sched_get_priority_min(SCHED_RR) - 1.
        let lo = sched_get_priority_min(SCHED_RR);
        let param = SchedParam { sched_priority: lo - 1 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, SCHED_RR, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sched_setscheduler_buggy_priority_at_max_plus_one() {
        let hi = sched_get_priority_max(SCHED_RR);
        let param = SchedParam { sched_priority: hi + 1 };
        errno::set_errno(0);
        assert_eq!(sched_setscheduler(0, SCHED_RR, &raw const param), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Workflow: success paths after validation ----

    #[test]
    fn test_sched_setscheduler_workflow_each_policy_valid_priority() {
        // For each recognised policy, the lowest and highest in-range
        // priorities must succeed.
        for &p in &[SCHED_OTHER, SCHED_BATCH, SCHED_IDLE, SCHED_DEADLINE] {
            // Range [0, 0] → only 0.
            let param = SchedParam { sched_priority: 0 };
            assert_eq!(sched_setscheduler(0, p, &raw const param), 0);
        }
        for &p in &[SCHED_FIFO, SCHED_RR] {
            // Range [1, 99] — try both bounds and a midpoint.
            for &pri in &[1, 50, 99] {
                let param = SchedParam { sched_priority: pri };
                assert_eq!(sched_setscheduler(0, p, &raw const param), 0);
            }
        }
    }

    #[test]
    fn test_sched_setparam_workflow_priority_zero_succeeds() {
        let param = SchedParam { sched_priority: 0 };
        assert_eq!(sched_setparam(0, &raw const param), 0);
    }

    #[test]
    fn test_sched_getparam_workflow_after_validation_fills_buffer() {
        let mut param = SchedParam { sched_priority: 99 };
        assert_eq!(sched_getparam(0, &raw mut param), 0);
        assert_eq!(param.sched_priority, 0);
    }
}
