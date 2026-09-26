//! `<linux/futex.h>` — fast userspace locking primitives.
//!
//! Provides futex operation constants and the `futex()` system call
//! wrapper.  Futexes are the building block for userspace
//! synchronization primitives (mutexes, condition variables, etc.).

use crate::errno;
use crate::stat::Timespec;
use crate::syscall::{
    SYS_FUTEX_LOCK_PI, SYS_FUTEX_UNLOCK_PI, SYS_FUTEX_WAIT, SYS_FUTEX_WAIT_TIMEOUT, SYS_FUTEX_WAKE,
    syscall1, syscall2, syscall3,
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

/// `FUTEX_LOCK_PI2` (Linux 5.14): `FUTEX_LOCK_PI` on `CLOCK_MONOTONIC`.
pub const FUTEX_LOCK_PI2: i32 = 13;

/// The command bits of `futex_op`: everything but the two flags.
const FUTEX_CMD_MASK: i32 = !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

/// The commands whose fourth argument is a timeout, which
/// `SYSCALL_DEFINE6(futex)` reads and judges before anything else
/// (`futex_cmd_has_timeout`).
const fn cmd_has_timeout(cmd: i32) -> bool {
    matches!(
        cmd,
        FUTEX_WAIT | FUTEX_LOCK_PI | FUTEX_LOCK_PI2 | FUTEX_WAIT_BITSET | FUTEX_WAIT_REQUEUE_PI
    )
}

/// How long a wait may last, from its timeout argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitFor {
    /// No timeout.
    Ever,
    /// This many nanoseconds.
    Ns(u64),
    /// An absolute deadline that has already passed.
    Expired,
}

/// The wait a timeout argument asks for: `FUTEX_WAIT`'s is relative;
/// `FUTEX_WAIT_BITSET`'s is absolute, on `CLOCK_MONOTONIC`, or on
/// `CLOCK_REALTIME` with `FUTEX_CLOCK_REALTIME` (`futex_init_timeout`).
fn wait_for(
    cmd: i32,
    realtime: bool,
    ts: Option<&Timespec>,
    now: impl Fn(i32) -> Timespec,
) -> WaitFor {
    let Some(ts) = ts else {
        return WaitFor::Ever;
    };
    if cmd == FUTEX_WAIT {
        return timespec_to_ns(ts).map_or(WaitFor::Ever, WaitFor::Ns);
    }
    let clock = if realtime {
        crate::time::CLOCK_REALTIME
    } else {
        crate::time::CLOCK_MONOTONIC
    };
    match crate::lowlevellock::ns_until(&now(clock), ts) {
        Some(ns) => WaitFor::Ns(ns),
        None => WaitFor::Expired,
    }
}

/// `get_futex_key`'s checks on `uaddr`: a misaligned address is `EINVAL`,
/// before anything is read; a NULL one faults when it is (`EFAULT`).
fn check_uaddr(uaddr: *mut u32) -> Result<(), i32> {
    // A futex word is a `u32`, four-byte aligned.
    if (uaddr as usize) & 3 != 0 {
        return Err(errno::EINVAL);
    }
    if uaddr.is_null() {
        return Err(errno::EFAULT);
    }
    Ok(())
}

/// Futex system call, as Linux 6.6's `SYSCALL_DEFINE6(futex)` and `do_futex`
/// order it:
///
/// 1. For a command that takes a timeout, the timeout is read and judged
///    first: a malformed one is `EINVAL`, whatever else is wrong.
/// 2. `FUTEX_CLOCK_REALTIME` with a command that has no absolute timeout is
///    `ENOSYS` (it was ignored here until 2026-09-26).
/// 3. A zero bitset is `EINVAL`; then `uaddr`: misaligned `EINVAL`, NULL
///    `EFAULT` -- except that a private `FUTEX_WAKE` never reads its word,
///    so a NULL one wakes nobody and answers 0.
///
/// - `FUTEX_WAIT` / `FUTEX_WAIT_BITSET`: sleep while `*uaddr == val`.
///   `FUTEX_WAIT`'s timeout is relative; `FUTEX_WAIT_BITSET`'s absolute, on
///   `CLOCK_MONOTONIC` or (with the flag) `CLOCK_REALTIME`.  0 when woken,
///   `EAGAIN` if the word did not hold `val`, `ETIMEDOUT` when the time ran
///   out.  Rust's std sleeps with `FUTEX_WAIT_BITSET` and
///   `FUTEX_BITSET_MATCH_ANY`; until 2026-09-26 that was `ENOSYS`, which
///   std takes as a wake-up, so every contended `Mutex`, `Condvar` and
///   `park` on this system spun instead of sleeping.
/// - `FUTEX_WAKE` / `FUTEX_WAKE_BITSET`: wake up to `val` waiters; the
///   number woken.
/// - `FUTEX_LOCK_PI` / `FUTEX_UNLOCK_PI`: PI-mutex acquire/release; 0.
/// - Anything else: `ENOSYS`, without looking at `uaddr` -- `do_futex`'s
///   default arm (Phase 130: a feature probe with a NULL placeholder must see
///   "not supported", not "bad pointer").
///
/// The kernel's futex has no bitsets: a bitset wait is woken by any wake on
/// its word, and a bitset wake wakes any waiter.  Both are spurious wake-ups
/// at worst, which every futex caller must already tolerate.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn futex(
    uaddr: *mut u32,
    futex_op: i32,
    val: u32,
    timeout: *const Timespec,
    _uaddr2: *mut u32,
    val3: u32,
) -> i64 {
    match futex_inner(uaddr, futex_op, val, timeout, val3) {
        Ok(n) => n,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

fn futex_inner(
    uaddr: *mut u32,
    futex_op: i32,
    val: u32,
    timeout: *const Timespec,
    val3: u32,
) -> Result<i64, i32> {
    let cmd = futex_op & FUTEX_CMD_MASK;
    let private = futex_op & FUTEX_PRIVATE_FLAG != 0;
    let realtime = futex_op & FUTEX_CLOCK_REALTIME != 0;

    let ts = if !timeout.is_null() && cmd_has_timeout(cmd) {
        // SAFETY: non-null; the caller's contract.
        let ts = unsafe { core::ptr::read_unaligned(timeout) };
        if timespec_to_ns(&ts).is_none() {
            return Err(errno::EINVAL); // `futex_init_timeout`
        }
        Some(ts)
    } else {
        None
    };
    if realtime
        && !matches!(
            cmd,
            FUTEX_WAIT_BITSET | FUTEX_WAIT_REQUEUE_PI | FUTEX_LOCK_PI2
        )
    {
        return Err(errno::ENOSYS);
    }

    match cmd {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            if cmd == FUTEX_WAIT_BITSET && val3 == 0 {
                return Err(errno::EINVAL);
            }
            check_uaddr(uaddr)?;
            // The kernel compares again, atomically with going to sleep;
            // looking first answers EAGAIN without a syscall, and decides an
            // expired deadline the way `futex_wait` does (the value first).
            // SAFETY: non-null and aligned; the caller's contract makes it a
            // futex word.
            let current = unsafe { core::sync::atomic::AtomicU32::from_ptr(uaddr) }
                .load(core::sync::atomic::Ordering::SeqCst);
            if current != val {
                return Err(errno::EAGAIN);
            }
            let ret = match wait_for(cmd, realtime, ts.as_ref(), crate::lowlevellock::now_on) {
                WaitFor::Expired => return Err(errno::ETIMEDOUT),
                WaitFor::Ever => syscall2(SYS_FUTEX_WAIT, uaddr as u64, u64::from(val)),
                WaitFor::Ns(ns) => {
                    syscall3(SYS_FUTEX_WAIT_TIMEOUT, uaddr as u64, u64::from(val), ns)
                }
            };
            match ret {
                1 => Ok(0),
                0 => Err(errno::EAGAIN),
                neg => Err(errno::errno_for(neg)),
            }
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => {
            if cmd == FUTEX_WAKE_BITSET && val3 == 0 {
                return Err(errno::EINVAL);
            }
            if (uaddr as usize) & 3 != 0 {
                return Err(errno::EINVAL);
            }
            if uaddr.is_null() {
                // A private wake hashes the address and finds no waiter
                // without reading it; a shared one looks the page up, and
                // faults.
                return if private { Ok(0) } else { Err(errno::EFAULT) };
            }
            let ret = syscall2(SYS_FUTEX_WAKE, uaddr as u64, u64::from(val));
            if ret < 0 {
                Err(errno::errno_for(ret))
            } else {
                Ok(ret)
            }
        }
        FUTEX_LOCK_PI => {
            check_uaddr(uaddr)?;
            let ret = syscall1(SYS_FUTEX_LOCK_PI, uaddr as u64);
            if ret < 0 {
                Err(errno::errno_for(ret))
            } else {
                Ok(0)
            }
        }
        FUTEX_UNLOCK_PI => {
            check_uaddr(uaddr)?;
            let ret = syscall1(SYS_FUTEX_UNLOCK_PI, uaddr as u64);
            if ret < 0 {
                Err(errno::errno_for(ret))
            } else {
                Ok(0)
            }
        }
        // Linux has REQUEUE, CMP_REQUEUE, WAKE_OP, TRYLOCK_PI, LOCK_PI2 and
        // the requeue-PI pair; the kernel here has none of them yet.  Like
        // `do_futex`'s default arm, this answers without reading `uaddr`.
        _ => Err(errno::ENOSYS),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn call(uaddr: *mut u32, op: i32, val: u32, ts: Option<&Timespec>, val3: u32) -> (i64, i32) {
        errno::set_errno(0);
        let r = futex(
            uaddr,
            op,
            val,
            ts.map_or(core::ptr::null(), core::ptr::from_ref),
            core::ptr::null_mut(),
            val3,
        );
        (r, errno::get_errno())
    }

    fn ts(sec: i64, nsec: i64) -> Timespec {
        Timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(
            (FUTEX_WAIT, FUTEX_WAKE, FUTEX_WAIT_BITSET, FUTEX_WAKE_BITSET),
            (0, 1, 9, 10)
        );
        assert_eq!((FUTEX_LOCK_PI, FUTEX_UNLOCK_PI, FUTEX_LOCK_PI2), (6, 7, 13));
        assert_eq!((FUTEX_PRIVATE_FLAG, FUTEX_CLOCK_REALTIME), (128, 256));
        assert_eq!(FUTEX_WAIT_PRIVATE, 128);
        assert_eq!(FUTEX_WAKE_PRIVATE, 129);
        assert_eq!(FUTEX_BITSET_MATCH_ANY, u32::MAX);
    }

    /// A malformed timeout is EINVAL before anything else is looked at --
    /// the NULL word included.
    #[test]
    fn test_timeout_first() {
        for bad in [ts(0, -1), ts(0, 1_000_000_000), ts(-1, 0)] {
            for op in [FUTEX_WAIT, FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG] {
                assert_eq!(
                    call(core::ptr::null_mut(), op, 0, Some(&bad), u32::MAX),
                    (-1, errno::EINVAL)
                );
            }
        }
        // A command that takes no timeout does not read it.
        assert_eq!(
            call(
                core::ptr::null_mut(),
                FUTEX_WAKE_PRIVATE,
                1,
                Some(&ts(0, -1)),
                0
            ),
            (0, 0)
        );
    }

    /// FUTEX_CLOCK_REALTIME belongs to the absolute-timeout commands only.
    #[test]
    fn test_clock_realtime_only_with_absolute_waits() {
        let mut word = 0u32;
        for op in [FUTEX_WAIT, FUTEX_WAKE, FUTEX_LOCK_PI] {
            assert_eq!(
                call(&raw mut word, op | FUTEX_CLOCK_REALTIME, 0, None, 0),
                (-1, errno::ENOSYS)
            );
        }
        // With WAIT_BITSET it is fine: the value differs, so EAGAIN.
        assert_eq!(
            call(
                &raw mut word,
                FUTEX_WAIT_BITSET | FUTEX_CLOCK_REALTIME,
                1,
                None,
                u32::MAX
            ),
            (-1, errno::EAGAIN)
        );
    }

    #[test]
    fn test_zero_bitset_einval() {
        let mut word = 0u32;
        assert_eq!(
            call(&raw mut word, FUTEX_WAIT_BITSET, 0, None, 0),
            (-1, errno::EINVAL)
        );
        assert_eq!(
            call(&raw mut word, FUTEX_WAKE_BITSET, 1, None, 0),
            (-1, errno::EINVAL)
        );
    }

    #[test]
    fn test_uaddr_checks() {
        let words = [0u32; 2];
        let misaligned = words
            .as_ptr()
            .cast::<u8>()
            .wrapping_add(1)
            .cast::<u32>()
            .cast_mut();
        for op in [
            FUTEX_WAIT,
            FUTEX_WAIT_BITSET,
            FUTEX_WAKE,
            FUTEX_LOCK_PI,
            FUTEX_UNLOCK_PI,
        ] {
            assert_eq!(
                call(misaligned, op, 0, None, u32::MAX),
                (-1, errno::EINVAL),
                "op {op}"
            );
        }
        for op in [
            FUTEX_WAIT,
            FUTEX_WAIT_BITSET,
            FUTEX_LOCK_PI,
            FUTEX_UNLOCK_PI,
            FUTEX_WAKE,
        ] {
            assert_eq!(
                call(core::ptr::null_mut(), op, 0, None, u32::MAX),
                (-1, errno::EFAULT),
                "op {op}"
            );
        }
        // A private wake never reads the word.
        assert_eq!(
            call(core::ptr::null_mut(), FUTEX_WAKE_PRIVATE, 1, None, 0),
            (0, 0)
        );
    }

    /// The word's value is compared first: EAGAIN when it differs, even
    /// with a deadline that has passed.
    #[test]
    fn test_value_mismatch_is_eagain() {
        let mut word = 5u32;
        for op in [
            FUTEX_WAIT,
            FUTEX_WAIT_PRIVATE,
            FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG,
        ] {
            assert_eq!(
                call(&raw mut word, op, 99, None, u32::MAX),
                (-1, errno::EAGAIN)
            );
        }
        let past = ts(0, 1);
        assert_eq!(
            call(&raw mut word, FUTEX_WAIT_BITSET, 99, Some(&past), u32::MAX),
            (-1, errno::EAGAIN)
        );
    }

    /// FUTEX_WAIT_BITSET's timeout is a deadline: one already passed is
    /// ETIMEDOUT when the value matches.  (This is how Rust's std waits.)
    #[test]
    fn test_wait_bitset_expired_deadline() {
        let mut word = 7u32;
        let past = ts(0, 1);
        assert_eq!(
            call(
                &raw mut word,
                FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG,
                7,
                Some(&past),
                u32::MAX
            ),
            (-1, errno::ETIMEDOUT)
        );
    }

    #[test]
    fn test_wait_for() {
        let now = |_: i32| ts(100, 500);
        assert_eq!(wait_for(FUTEX_WAIT, false, None, now), WaitFor::Ever);
        assert_eq!(
            wait_for(FUTEX_WAIT, false, Some(&ts(2, 5)), now),
            WaitFor::Ns(2_000_000_005)
        );
        assert_eq!(
            wait_for(FUTEX_WAIT_BITSET, false, Some(&ts(101, 500)), now),
            WaitFor::Ns(1_000_000_000)
        );
        assert_eq!(
            wait_for(FUTEX_WAIT_BITSET, false, Some(&ts(100, 500)), now),
            WaitFor::Expired
        );
        assert_eq!(
            wait_for(FUTEX_WAIT_BITSET, true, Some(&ts(50, 0)), now),
            WaitFor::Expired
        );
        // The realtime flag picks the clock.
        let clocks = |c: i32| {
            if c == crate::time::CLOCK_REALTIME {
                ts(10, 0)
            } else {
                ts(1000, 0)
            }
        };
        assert_eq!(
            wait_for(FUTEX_WAIT_BITSET, true, Some(&ts(11, 0)), clocks),
            WaitFor::Ns(1_000_000_000)
        );
        assert_eq!(
            wait_for(FUTEX_WAIT_BITSET, false, Some(&ts(11, 0)), clocks),
            WaitFor::Expired
        );
    }

    /// Unknown and unwired commands are ENOSYS without looking at the word
    /// (Phase 130: feature probes pass NULL placeholders).
    #[test]
    fn test_unwired_commands_enosys() {
        for op in [
            FUTEX_REQUEUE,
            FUTEX_CMP_REQUEUE,
            FUTEX_WAKE_OP,
            FUTEX_TRYLOCK_PI,
            FUTEX_CMP_REQUEUE_PI,
            FUTEX_LOCK_PI2,
            14,
            99,
            -1,
        ] {
            assert_eq!(
                call(core::ptr::null_mut(), op, 0, None, 0),
                (-1, errno::ENOSYS),
                "op {op}"
            );
            assert_eq!(
                call(core::ptr::null_mut(), op | FUTEX_PRIVATE_FLAG, 0, None, 0),
                (-1, errno::ENOSYS)
            );
        }
    }

    #[test]
    fn test_timespec_to_ns() {
        assert_eq!(timespec_to_ns(&ts(0, 0)), Some(0));
        assert_eq!(timespec_to_ns(&ts(1, 1)), Some(1_000_000_001));
        assert_eq!(timespec_to_ns(&ts(-1, 0)), None);
        assert_eq!(timespec_to_ns(&ts(0, 1_000_000_000)), None);
        assert_eq!(timespec_to_ns(&ts(i64::MAX, 0)), None);
    }
}
