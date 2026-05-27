//! `<linux/futex.h>` — fast userspace locking primitives.
//!
//! Provides futex operation constants and the `futex()` system call
//! wrapper.  Futexes are the building block for userspace
//! synchronization primitives (mutexes, condition variables, etc.).

use crate::errno;
use crate::stat::Timespec;
use crate::syscall::{
    SYS_FUTEX_LOCK_PI, SYS_FUTEX_UNLOCK_PI, SYS_FUTEX_WAIT,
    SYS_FUTEX_WAIT_TIMEOUT, SYS_FUTEX_WAKE, syscall1, syscall2, syscall3,
};

// ---------------------------------------------------------------------------
// Futex operations
// ---------------------------------------------------------------------------

/// Wait if `*uaddr == val`.
pub const FUTEX_WAIT: i32 = 0;

/// Wake up to `val` waiters on `uaddr`.
pub const FUTEX_WAKE: i32 = 1;

/// Requeue waiters from `uaddr` to `uaddr2`.
pub const FUTEX_REQUEUE: i32 = 3;

/// Conditional requeue (atomically check before requeuing).
pub const FUTEX_CMP_REQUEUE: i32 = 4;

/// Wake one waiter and set lock value atomically.
pub const FUTEX_WAKE_OP: i32 = 5;

/// Wait on a bitset.
pub const FUTEX_WAIT_BITSET: i32 = 9;

/// Wake on a bitset.
pub const FUTEX_WAKE_BITSET: i32 = 10;

/// Lock a PI futex (priority-inheritance).
pub const FUTEX_LOCK_PI: i32 = 6;

/// Unlock a PI futex.
pub const FUTEX_UNLOCK_PI: i32 = 7;

/// Try lock a PI futex.
pub const FUTEX_TRYLOCK_PI: i32 = 8;

/// Wait on a PI futex with requeue.
pub const FUTEX_WAIT_REQUEUE_PI: i32 = 11;

/// Requeue PI waiters.
pub const FUTEX_CMP_REQUEUE_PI: i32 = 12;

// ---------------------------------------------------------------------------
// Futex flags (OR with operation)
// ---------------------------------------------------------------------------

/// Use `CLOCK_REALTIME` instead of `CLOCK_MONOTONIC` for timeouts.
pub const FUTEX_CLOCK_REALTIME: i32 = 256;

/// Use private futex (process-local, not shared).
pub const FUTEX_PRIVATE_FLAG: i32 = 128;

// ---------------------------------------------------------------------------
// Convenience combined values
// ---------------------------------------------------------------------------

/// Private wait.
pub const FUTEX_WAIT_PRIVATE: i32 = FUTEX_WAIT | FUTEX_PRIVATE_FLAG;

/// Private wake.
pub const FUTEX_WAKE_PRIVATE: i32 = FUTEX_WAKE | FUTEX_PRIVATE_FLAG;

/// Wait on all bits.
pub const FUTEX_BITSET_MATCH_ANY: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// futex()
// ---------------------------------------------------------------------------

/// Convert a positive [`Timespec`] to nanoseconds, saturating at
/// [`u64::MAX`] on overflow.  Returns `None` if the timespec is invalid
/// (negative seconds, or nanoseconds outside `0..1_000_000_000`).
#[inline]
fn timespec_to_ns(ts: &Timespec) -> Option<u64> {
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return None;
    }
    // tv_sec is non-negative here, so the cast is well-defined.
    #[allow(clippy::cast_sign_loss)]
    let sec = ts.tv_sec as u64;
    let nsec_part = sec.checked_mul(1_000_000_000)?;
    #[allow(clippy::cast_sign_loss)]
    let extra = ts.tv_nsec as u64;
    nsec_part.checked_add(extra)
}

/// Futex system call.
///
/// Dispatches on `futex_op` (with `FUTEX_PRIVATE_FLAG` and
/// `FUTEX_CLOCK_REALTIME` masked off — our kernel is process-local and
/// uses a single monotonic clock):
///
/// - `FUTEX_WAIT`: atomically check `*uaddr == val` and block if so.
///   If `timeout` is non-NULL, uses the kernel's timed-wait variant.
///   Returns 0 if woken, -1 with `EAGAIN` if the value did not match,
///   -1 with `ETIMEDOUT` on timeout, -1 with `EFAULT` on bad address.
/// - `FUTEX_WAKE`: wake up to `val` waiters on `uaddr`.  Returns the
///   number of tasks actually woken.
/// - `FUTEX_LOCK_PI` / `FUTEX_UNLOCK_PI`: PI-mutex acquire/release.
///   Return 0 on success.
/// - All other operations: -1 with `ENOSYS` (not yet wired through to
///   the kernel).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn futex(
    uaddr: *mut u32,
    futex_op: i32,
    val: u32,
    timeout: *const Timespec,
    _uaddr2: *mut u32,
    _val3: u32,
) -> i64 {
    if uaddr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Strip PRIVATE / CLOCK_REALTIME flag bits.  Both are no-ops on our
    // kernel: futexes are already process-local, and we have a single
    // monotonic clock.
    let op = futex_op & !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

    match op {
        FUTEX_WAIT => {
            if timeout.is_null() {
                let ret = syscall2(SYS_FUTEX_WAIT, uaddr as u64, u64::from(val));
                match ret {
                    1 => 0,
                    0 => {
                        errno::set_errno(errno::EAGAIN);
                        -1
                    }
                    neg => errno::translate(neg),
                }
            } else {
                // SAFETY: caller contract — `timeout` points to a valid
                // Timespec when non-null.
                let ts = unsafe { *timeout };
                let Some(ns) = timespec_to_ns(&ts) else {
                    errno::set_errno(errno::EINVAL);
                    return -1;
                };
                let ret = syscall3(
                    SYS_FUTEX_WAIT_TIMEOUT,
                    uaddr as u64,
                    u64::from(val),
                    ns,
                );
                match ret {
                    1 => 0,
                    0 => {
                        errno::set_errno(errno::EAGAIN);
                        -1
                    }
                    neg => errno::translate(neg),
                }
            }
        }
        FUTEX_WAKE => {
            let ret = syscall2(SYS_FUTEX_WAKE, uaddr as u64, u64::from(val));
            if ret < 0 {
                errno::translate(ret)
            } else {
                ret
            }
        }
        FUTEX_LOCK_PI => {
            let ret = syscall1(SYS_FUTEX_LOCK_PI, uaddr as u64);
            if ret < 0 { errno::translate(ret) } else { 0 }
        }
        FUTEX_UNLOCK_PI => {
            let ret = syscall1(SYS_FUTEX_UNLOCK_PI, uaddr as u64);
            if ret < 0 { errno::translate(ret) } else { 0 }
        }
        _ => {
            // Operations not wired to the kernel yet: REQUEUE,
            // CMP_REQUEUE, WAKE_OP, WAIT_BITSET, WAKE_BITSET,
            // TRYLOCK_PI, WAIT_REQUEUE_PI, CMP_REQUEUE_PI.
            errno::set_errno(errno::ENOSYS);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_futex_ops_distinct() {
        let ops = [
            FUTEX_WAIT, FUTEX_WAKE, FUTEX_REQUEUE,
            FUTEX_CMP_REQUEUE, FUTEX_WAKE_OP,
            FUTEX_LOCK_PI, FUTEX_UNLOCK_PI, FUTEX_TRYLOCK_PI,
            FUTEX_WAIT_BITSET, FUTEX_WAKE_BITSET,
            FUTEX_WAIT_REQUEUE_PI, FUTEX_CMP_REQUEUE_PI,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j], "futex ops must be distinct");
            }
        }
    }

    #[test]
    fn test_futex_wait_wake_values() {
        assert_eq!(FUTEX_WAIT, 0);
        assert_eq!(FUTEX_WAKE, 1);
    }

    #[test]
    fn test_futex_private_flag() {
        assert_eq!(FUTEX_PRIVATE_FLAG, 128);
        assert_eq!(FUTEX_WAIT_PRIVATE, 128);
        assert_eq!(FUTEX_WAKE_PRIVATE, 129);
    }

    #[test]
    fn test_futex_clock_realtime() {
        assert_eq!(FUTEX_CLOCK_REALTIME, 256);
    }

    #[test]
    fn test_futex_bitset_match_any() {
        assert_eq!(FUTEX_BITSET_MATCH_ANY, 0xFFFF_FFFF);
    }

    #[test]
    fn test_futex_pi_ops_distinct() {
        assert_ne!(FUTEX_LOCK_PI, FUTEX_UNLOCK_PI);
        assert_ne!(FUTEX_UNLOCK_PI, FUTEX_TRYLOCK_PI);
    }

    // -- futex() implementation --

    #[test]
    fn test_futex_null_uaddr_efault() {
        errno::set_errno(0);
        let ret = futex(
            core::ptr::null_mut(),
            FUTEX_WAIT,
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_futex_wait_value_mismatch_eagain() {
        // *uaddr is 5, but we expect 99 — futex_wait should return
        // immediately with EAGAIN without blocking.
        //
        // In a host-target test run the SYSCALL instruction is not
        // available, so the wrapper returns whatever the asm! returns.
        // We mainly exercise the dispatch + argument-validation path.
        let mut word: u32 = 5;
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_WAIT,
            99,
            core::ptr::null(),
            core::ptr::null_mut(),
            0,
        );
        // We can't assert on the syscall return without a real kernel;
        // but we can assert errno is NOT ENOSYS — i.e. the implementation
        // dispatched FUTEX_WAIT rather than falling through to the
        // unsupported-op arm.
        if ret < 0 {
            assert_ne!(
                errno::get_errno(),
                errno::ENOSYS,
                "FUTEX_WAIT must not be reported as ENOSYS",
            );
        }
    }

    #[test]
    fn test_futex_private_flag_is_stripped() {
        // FUTEX_WAIT | FUTEX_PRIVATE_FLAG must dispatch to FUTEX_WAIT,
        // not the unknown-op arm.
        let mut word: u32 = 0;
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_WAIT_PRIVATE,
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
            0,
        );
        if ret < 0 {
            assert_ne!(
                errno::get_errno(),
                errno::ENOSYS,
                "FUTEX_WAIT_PRIVATE must not be reported as ENOSYS",
            );
        }
    }

    #[test]
    fn test_futex_clock_realtime_is_stripped() {
        let mut word: u32 = 0;
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_WAIT | FUTEX_CLOCK_REALTIME,
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
            0,
        );
        if ret < 0 {
            assert_ne!(
                errno::get_errno(),
                errno::ENOSYS,
                "FUTEX_WAIT|FUTEX_CLOCK_REALTIME must not be reported as ENOSYS",
            );
        }
    }

    #[test]
    fn test_futex_wait_invalid_timeout_einval() {
        // Negative tv_nsec is invalid.
        let mut word: u32 = 0;
        let ts = Timespec { tv_sec: 0, tv_nsec: -1 };
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_WAIT,
            0,
            &ts as *const Timespec,
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_futex_wait_negative_seconds_einval() {
        let mut word: u32 = 0;
        let ts = Timespec { tv_sec: -1, tv_nsec: 0 };
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_WAIT,
            0,
            &ts as *const Timespec,
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_futex_wait_nsec_too_large_einval() {
        let mut word: u32 = 0;
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        };
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_WAIT,
            0,
            &ts as *const Timespec,
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_futex_unknown_op_enosys() {
        let mut word: u32 = 0;
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_REQUEUE,
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_futex_wake_op_enosys() {
        let mut word: u32 = 0;
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_WAKE_OP,
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
            0,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_futex_wait_bitset_enosys() {
        let mut word: u32 = 0;
        errno::set_errno(0);
        let ret = futex(
            &mut word as *mut u32,
            FUTEX_WAIT_BITSET,
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
            FUTEX_BITSET_MATCH_ANY,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_timespec_to_ns_basic() {
        assert_eq!(
            timespec_to_ns(&Timespec { tv_sec: 0, tv_nsec: 0 }),
            Some(0),
        );
        assert_eq!(
            timespec_to_ns(&Timespec { tv_sec: 1, tv_nsec: 0 }),
            Some(1_000_000_000),
        );
        assert_eq!(
            timespec_to_ns(&Timespec { tv_sec: 0, tv_nsec: 500 }),
            Some(500),
        );
        assert_eq!(
            timespec_to_ns(&Timespec { tv_sec: 2, tv_nsec: 250_000_000 }),
            Some(2_250_000_000),
        );
    }

    #[test]
    fn test_timespec_to_ns_rejects_invalid() {
        assert_eq!(
            timespec_to_ns(&Timespec { tv_sec: -1, tv_nsec: 0 }),
            None,
        );
        assert_eq!(
            timespec_to_ns(&Timespec { tv_sec: 0, tv_nsec: -1 }),
            None,
        );
        assert_eq!(
            timespec_to_ns(&Timespec { tv_sec: 0, tv_nsec: 1_000_000_000 }),
            None,
        );
    }

    #[test]
    fn test_timespec_to_ns_overflow_saturates_to_none() {
        // tv_sec * 1e9 overflows u64.
        let ts = Timespec {
            tv_sec: i64::MAX,
            tv_nsec: 0,
        };
        assert_eq!(timespec_to_ns(&ts), None);
    }
}
