//! POSIX `<sys/times.h>` — process times.
//!
//! Implements `times()` for querying process and children CPU times.
//!
//! ## Behavior
//!
//! On `target_os = "none"` (kernel target) the CPU-time fields are
//! filled from the kernel's aggregate per-CPU accounting (`SYS_CPU_TIMES`):
//!
//! - `tms_utime` = system_ns / 10_000_000 (kernel+user code time).  Since
//!   we don't separately track user vs kernel for a task yet, this lumps
//!   them together under user time.
//! - `tms_stime` = (irq_ns + softirq_ns) / 10_000_000 (interrupt time).
//! - `tms_cutime` / `tms_cstime` = 0 (no terminated-children tracking).
//!
//! Return value: monotonic wall-clock ticks (CLOCK_MONOTONIC nanoseconds
//! divided by 10_000_000 for the 100 Hz `CLK_TCK`).
//!
//! On host targets, the fields are zeroed and the return value is a
//! monotonic call counter (for unit-test determinism).

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct tms` — process times.
///
/// Times are in clock ticks.  Use `sysconf(_SC_CLK_TCK)` (100 on our
/// OS) to convert to seconds.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Tms {
    /// User CPU time of the process.
    pub tms_utime: i64,
    /// System CPU time of the process.
    pub tms_stime: i64,
    /// User CPU time of terminated children.
    pub tms_cutime: i64,
    /// System CPU time of terminated children.
    pub tms_cstime: i64,
}

/// Clock ticks per second (matches `_SC_CLK_TCK`).
///
/// On `target_os = "none"` we drive timing through `NS_PER_TICK` instead,
/// but keep the constant for ABI documentation and unit tests.
#[allow(dead_code)]
const CLK_TCK: i64 = 100;

/// Nanoseconds per `clock_t` tick (1e9 / CLK_TCK = 10_000_000 for 100 Hz).
///
/// Used by the kernel-target `times()` implementation and by host-side
/// unit tests; not referenced from host non-test code.  Gated to silence
/// the dead-code warning on host non-test builds.
#[cfg(any(target_os = "none", test))]
const NS_PER_TICK: u64 = 10_000_000;

/// Convert a nanosecond count to clock ticks (saturating to `i64::MAX`).
///
/// Same gating as `NS_PER_TICK`: used by the kernel-target `times()`
/// and by unit tests; absent from host non-test builds to avoid a
/// dead-code warning.
#[cfg(any(target_os = "none", test))]
#[allow(clippy::cast_possible_wrap)]
fn ns_to_ticks(ns: u64) -> i64 {
    let ticks = ns / NS_PER_TICK;
    if ticks > i64::MAX as u64 {
        i64::MAX
    } else {
        ticks as i64
    }
}

// ---------------------------------------------------------------------------
// Monotonic tick counter
// ---------------------------------------------------------------------------

/// Simple monotonic tick counter (host-test fallback only).
///
/// On host builds (`not(target_os = "none")`), each call to `times()`
/// increments this counter so unit tests can verify monotonic behavior
/// deterministically.  On the kernel target the return value comes from
/// `SYS_CLOCK_MONOTONIC` and this counter is unused.
#[cfg(not(target_os = "none"))]
static mut TICK_COUNTER: i64 = 0;

// ---------------------------------------------------------------------------
// times
// ---------------------------------------------------------------------------

/// `times` — get process times.
///
/// Fills the `tms` structure pointed to by `buffer` with CPU time
/// information.  Returns the elapsed real time in clock ticks
/// since an arbitrary epoch, or `(clock_t)(-1)` on error.
///
/// On kernel builds, CPU time fields are populated from the kernel's
/// aggregate per-CPU time accounting via `SYS_CPU_TIMES`.  On host
/// builds, fields are zeroed and the return value is a monotonic
/// call counter (for deterministic unit testing).
///
/// # Linux validation order
///
/// `kernel/sys.c::SYSCALL_DEFINE1(times, struct tms __user *, tbuf)`:
///
/// ```c
/// if (tbuf) {
///     struct tms tmp;
///     do_sys_times(&tmp);
///     if (copy_to_user(tbuf, &tmp, sizeof(struct tms)))
///         return -EFAULT;
/// }
/// force_successful_syscall_return();
/// return (long) jiffies_64_to_clock_t(get_jiffies_64());
/// ```
///
/// The `tbuf` pointer is **optional**.  A NULL `tbuf` is not an
/// error — Linux just skips the user-copy and returns the elapsed
/// tick count.  EFAULT only arises if a *non-NULL* `tbuf` is
/// unmapped (which we can't distinguish in userspace tests).
///
/// Precedence:
///
///   1. `tbuf != NULL` → populate (EFAULT on user-copy fail).
///      Reaching this point with a NULL `tbuf` is **not** an error.
///   2. Return the tick count (always).
///
/// **Phase 154**: pre-Phase-154 we returned `-1`/`EFAULT` on a NULL
/// `buffer`.  Linux returns the tick count.  This made callers that
/// pass NULL to query just the elapsed time (a common idiom — and
/// the very reason POSIX documents `times` as returning a useful
/// value rather than just an error code) see a spurious failure.
/// Phase 154 reorders to match Linux: NULL `buffer` silently skips
/// population and returns the tick count without setting errno.
/// Mirrors the Phase 152 fix to `gettimeofday`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn times(buffer: *mut Tms) -> i64 {
    #[cfg(target_os = "none")]
    {
        // Linux: NULL buffer skips population — fall through to the
        // tick-count return.  Non-NULL buffer is populated; on a
        // user-copy failure Linux returns -EFAULT (we can't observe
        // that from our SAFETY-asserted callers, so the only way
        // this returns -1 is the host-side stub on the unused
        // never-fail path; we don't take it here).
        if !buffer.is_null() {
            // Query the kernel's aggregate CPU time fields.
            let system_ns = read_cpu_time_field(0);
            let irq_ns = read_cpu_time_field(1);
            let softirq_ns = read_cpu_time_field(2);

            let utime_ticks = ns_to_ticks(system_ns);
            let stime_ticks = ns_to_ticks(irq_ns.saturating_add(softirq_ns));

            // SAFETY: caller guarantees buffer is valid for one Tms
            // (just confirmed non-null above).
            unsafe {
                (*buffer).tms_utime = utime_ticks;
                (*buffer).tms_stime = stime_ticks;
                // No terminated-children tracking yet.
                (*buffer).tms_cutime = 0;
                (*buffer).tms_cstime = 0;
            }
        }

        // Return value: wall-clock monotonic time in ticks.
        // CLOCK_MONOTONIC returns nanoseconds since boot.
        #[allow(clippy::cast_sign_loss)]
        let mono_ns = {
            let raw = crate::syscall::syscall0(crate::syscall::SYS_CLOCK_MONOTONIC);
            if raw < 0 { 0u64 } else { raw as u64 }
        };
        let ticks = ns_to_ticks(mono_ns);
        // POSIX says (clock_t)(-1) on error, but a real zero return at boot
        // is valid.  Bump by 1 if we'd otherwise return 0 to keep the value
        // strictly positive and consistent with the host-side counter.
        if ticks <= 0 { 1 } else { ticks }
    }

    #[cfg(not(target_os = "none"))]
    {
        // Linux: NULL buffer skips population — fall through to the
        // tick-counter bump and return.
        if !buffer.is_null() {
            // SAFETY: caller guarantees buffer is valid for one Tms
            // (just confirmed non-null above).
            unsafe {
                (*buffer).tms_utime = 0;
                (*buffer).tms_stime = 0;
                (*buffer).tms_cutime = 0;
                (*buffer).tms_cstime = 0;
            }
        }

        // Return a monotonically increasing tick value (host test stub).
        // SAFETY: single-threaded access.
        unsafe {
            TICK_COUNTER = TICK_COUNTER.wrapping_add(1);
            TICK_COUNTER
        }
    }
}

/// Read one aggregate-CPU-time field from the kernel.
///
/// Returns 0 on any error (e.g., field out of range or kernel returning
/// a negative status).  Saturating zero is acceptable here because the
/// caller treats CPU time fields as monotonic counters — clamping below
/// at zero never causes a regression.
#[cfg(target_os = "none")]
#[allow(clippy::cast_sign_loss)]
fn read_cpu_time_field(which: u64) -> u64 {
    let raw = crate::syscall::syscall1(crate::syscall::SYS_CPU_TIMES, which);
    if raw < 0 { 0 } else { raw as u64 }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Struct layout
    // -----------------------------------------------------------------------

    #[test]
    fn test_tms_size() {
        // 4 × i64 = 32 bytes.
        assert_eq!(core::mem::size_of::<Tms>(), 4 * 8);
    }

    #[test]
    fn test_tms_fields_default() {
        let tms = Tms {
            tms_utime: 0,
            tms_stime: 0,
            tms_cutime: 0,
            tms_cstime: 0,
        };
        assert_eq!(tms.tms_utime, 0);
        assert_eq!(tms.tms_stime, 0);
        assert_eq!(tms.tms_cutime, 0);
        assert_eq!(tms.tms_cstime, 0);
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_clk_tck() {
        assert_eq!(CLK_TCK, 100);
    }

    // -----------------------------------------------------------------------
    // times — success
    // -----------------------------------------------------------------------

    #[test]
    fn test_times_returns_positive() {
        let mut tms = Tms {
            tms_utime: 99,
            tms_stime: 99,
            tms_cutime: 99,
            tms_cstime: 99,
        };
        let ret = times(&mut tms);
        assert!(ret > 0, "times should return a positive tick count");
    }

    #[test]
    fn test_times_zeroes_fields() {
        let mut tms = Tms {
            tms_utime: 42,
            tms_stime: 42,
            tms_cutime: 42,
            tms_cstime: 42,
        };
        times(&mut tms);
        assert_eq!(tms.tms_utime, 0);
        assert_eq!(tms.tms_stime, 0);
        assert_eq!(tms.tms_cutime, 0);
        assert_eq!(tms.tms_cstime, 0);
    }

    #[test]
    fn test_times_monotonic() {
        let mut tms = Tms {
            tms_utime: 0, tms_stime: 0,
            tms_cutime: 0, tms_cstime: 0,
        };
        let t1 = times(&mut tms);
        let t2 = times(&mut tms);
        let t3 = times(&mut tms);
        assert!(t2 > t1, "tick count should increase");
        assert!(t3 > t2, "tick count should increase");
    }

    // -----------------------------------------------------------------------
    // times — null buffer
    // -----------------------------------------------------------------------

    /// Phase 154: NULL `buffer` now matches Linux — it is silently
    /// accepted, the tick count is still returned, and errno is not
    /// touched.  Renamed from `test_times_null_buffer` (which
    /// pinned the pre-Phase-154 EFAULT behaviour).
    #[test]
    fn test_times_null_buffer_returns_ticks_phase154() {
        crate::errno::set_errno(0);
        let ret = times(core::ptr::null_mut());
        // Linux: NULL buffer just returns the tick count, no error.
        assert_ne!(
            ret, -1,
            "NULL buffer must NOT return -1 — Linux semantics"
        );
        assert!(
            ret > 0,
            "NULL buffer must still return a positive tick count, got {}",
            ret
        );
        assert_eq!(
            crate::errno::get_errno(),
            0,
            "NULL buffer must not set errno"
        );
    }

    // -- Phase 154: times() NULL buffer tolerance --
    //
    // Linux's `kernel/sys.c::SYSCALL_DEFINE1(times, ...)` wraps the
    // user-copy in `if (tbuf) { ... }`.  A NULL `tbuf` is not an
    // error — the kernel skips the copy and returns the tick count.
    // Pre-Phase-154 we returned `-1`/`EFAULT` instead.  All tests
    // below cover the new NULL-tolerance contract.
    //
    // Mirrors the Phase 152 fix to `gettimeofday`.

    // --- Per-error-class: NULL buffer doesn't set errno.

    #[test]
    fn test_times_null_buffer_does_not_set_errno_phase154() {
        // Seed errno with a sentinel.  A NULL-buffer call must not
        // touch it — Linux never writes errno on this path.
        crate::errno::set_errno(crate::errno::EINVAL);
        let _ = times(core::ptr::null_mut());
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "errno must remain at the seeded sentinel"
        );
    }

    // --- Ordering matrix: NULL/non-NULL paths return consistent ticks.

    #[test]
    fn test_times_null_and_non_null_both_return_ticks_phase154() {
        // Whether buffer is NULL or non-NULL, the return value is a
        // tick count.  Verify the NULL-shortcut path returns the
        // same SHAPE of value (positive monotonic tick) as the
        // non-NULL path.
        let mut tms = Tms {
            tms_utime: 0, tms_stime: 0,
            tms_cutime: 0, tms_cstime: 0,
        };
        let t1 = times(&mut tms);
        let t2 = times(core::ptr::null_mut());
        let t3 = times(&mut tms);
        assert!(t1 > 0);
        assert!(t2 > t1, "NULL call must still advance the counter");
        assert!(t3 > t2, "subsequent non-NULL call advances further");
    }

    #[test]
    fn test_times_null_buffer_monotonic_phase154() {
        // Repeated NULL-buffer calls must still return strictly
        // increasing tick counts — Linux's tick-count return path
        // is independent of the buffer.
        let a = times(core::ptr::null_mut());
        let b = times(core::ptr::null_mut());
        let c = times(core::ptr::null_mut());
        assert!(a > 0);
        assert!(b > a, "second NULL call must advance");
        assert!(c > b, "third NULL call must advance");
    }

    // --- Workflow: probe-then-use pattern.

    #[test]
    fn test_times_probe_then_real_call_workflow_phase154() {
        // A program probes with NULL to check the syscall is wired
        // up, then makes a real call.
        crate::errno::set_errno(0);
        let probe = times(core::ptr::null_mut());
        assert!(probe > 0, "probe must succeed with positive tick count");
        assert_eq!(crate::errno::get_errno(), 0);

        let mut tms = Tms {
            tms_utime: 42, tms_stime: 42,
            tms_cutime: 42, tms_cstime: 42,
        };
        let real = times(&mut tms);
        assert!(real > probe, "real call must advance from probe");
        // On host: fields are zeroed.  On kernel: fields are CPU
        // time.  Either way, the sentinel 42s must be gone from
        // tms_cutime/tms_cstime (we always zero those).
        assert_eq!(tms.tms_cutime, 0);
        assert_eq!(tms.tms_cstime, 0);
    }

    // --- Buggy caller: accidental NULL doesn't fake a failure.

    #[test]
    fn test_times_buggy_caller_phase154() {
        // A buggy caller passes NULL by accident.  Linux returns a
        // tick count, not -1; the caller's existing errno is
        // preserved.
        crate::errno::set_errno(crate::errno::EBADF);
        let ret = times(core::ptr::null_mut());
        assert_ne!(ret, -1, "NULL buffer must return a real tick count");
        assert!(ret > 0);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EBADF,
            "buggy NULL call must not stamp over caller's errno"
        );
    }

    // --- Recovery: a NULL call followed by a real call works.

    #[test]
    fn test_times_recovery_phase154() {
        // NULL call (no-op for fields), then a real call must work.
        let null_ret = times(core::ptr::null_mut());
        assert!(null_ret > 0);

        let mut tms = Tms {
            tms_utime: 0xDEAD, tms_stime: 0xBEEF,
            tms_cutime: 0xCAFE, tms_cstime: 0xFACE,
        };
        let real_ret = times(&mut tms);
        assert!(real_ret > null_ret);
        // Fields must have been overwritten — the sentinels are gone.
        // (On host build, all four become 0; on kernel build only
        // cutime/cstime become 0 and utime/stime become real values.)
        assert_ne!(tms.tms_cutime, 0xCAFE);
        assert_ne!(tms.tms_cstime, 0xFACE);
    }

    // --- No-side-effect loop: 1000 NULL calls all succeed.

    #[test]
    fn test_times_null_buffer_loop_phase154() {
        // 1000 NULL-buffer calls.  Each must succeed with a tick
        // count strictly greater than the previous.
        crate::errno::set_errno(0);
        let mut prev = times(core::ptr::null_mut());
        assert!(prev > 0);
        for i in 0..1000 {
            let cur = times(core::ptr::null_mut());
            assert!(
                cur > prev,
                "iteration {}: tick count must advance (prev={}, cur={})",
                i, prev, cur
            );
            prev = cur;
        }
        assert_eq!(
            crate::errno::get_errno(),
            0,
            "loop must not set errno"
        );
    }

    // --- Sentinel: previously divergent path is gone.

    #[test]
    fn test_times_null_buffer_no_longer_efault_phase154() {
        // Regression guard: explicitly assert the previous divergent
        // EFAULT path is gone.  If a future refactor reintroduces
        // -1/EFAULT, this test breaks loudly.
        crate::errno::set_errno(0);
        let ret = times(core::ptr::null_mut());
        assert_ne!(ret, -1, "Phase 154: NULL buffer must NOT return -1");
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EFAULT,
            "Phase 154: NULL buffer must NOT set EFAULT"
        );
    }

    // --- Errno spread: NULL-buffer preserves any preset errno.

    #[test]
    fn test_times_null_buffer_preserves_prior_errno_phase154() {
        for &e in &[
            crate::errno::EBADF,
            crate::errno::EINVAL,
            crate::errno::ENOENT,
            crate::errno::EAGAIN,
            crate::errno::EFAULT,
        ] {
            crate::errno::set_errno(e);
            let ret = times(core::ptr::null_mut());
            assert_ne!(ret, -1, "preset errno={} must not produce -1", e);
            assert!(ret > 0);
            assert_eq!(
                crate::errno::get_errno(),
                e,
                "errno must remain {} after NULL-buffer call",
                e
            );
        }
    }

    // --- Cross-check: NULL doesn't affect surrounding non-NULL calls.

    #[test]
    fn test_times_alternating_null_and_valid_phase154() {
        // Alternate NULL and non-NULL.  Both must succeed and the
        // tick counter must monotonically advance across the whole
        // sequence.
        let mut tms = Tms {
            tms_utime: 0, tms_stime: 0,
            tms_cutime: 0, tms_cstime: 0,
        };
        let mut prev: i64 = 0;
        for i in 0..40 {
            crate::errno::set_errno(0);
            let cur = if i % 2 == 0 {
                times(core::ptr::null_mut())
            } else {
                times(&mut tms)
            };
            assert!(cur > prev, "iter {}: must advance", i);
            assert_eq!(crate::errno::get_errno(), 0);
            prev = cur;
        }
    }

    // --- Independence: NULL buffer doesn't perturb other state.

    #[test]
    fn test_times_null_doesnt_clobber_caller_tms_phase154() {
        // A caller's existing Tms (not passed in) must not be
        // touched by a NULL-buffer call.  Sanity check that the
        // NULL branch truly skips all field writes.
        let mut bystander = Tms {
            tms_utime: 1111, tms_stime: 2222,
            tms_cutime: 3333, tms_cstime: 4444,
        };
        let _ = times(core::ptr::null_mut());
        // The bystander must be untouched (we never gave times
        // a pointer to it).  Trivially true; guards against any
        // hidden global-state side effect.
        assert_eq!(bystander.tms_utime, 1111);
        assert_eq!(bystander.tms_stime, 2222);
        assert_eq!(bystander.tms_cutime, 3333);
        assert_eq!(bystander.tms_cstime, 4444);

        // Now make a real call with the bystander — verify it works
        // after a NULL call without any cross-contamination.
        let ret = times(&mut bystander);
        assert!(ret > 0);
        assert_eq!(bystander.tms_cutime, 0);
        assert_eq!(bystander.tms_cstime, 0);
    }

    // -----------------------------------------------------------------------
    // times — multiple calls accumulate
    // -----------------------------------------------------------------------

    #[test]
    fn test_times_increments_each_call() {
        let mut tms = Tms {
            tms_utime: 0, tms_stime: 0,
            tms_cutime: 0, tms_cstime: 0,
        };
        let base = times(&mut tms);
        for i in 1..=5 {
            let t = times(&mut tms);
            assert_eq!(t, base + i, "each call should increment by 1");
        }
    }

    // -----------------------------------------------------------------------
    // times — all CPU fields remain zero (stub)
    // -----------------------------------------------------------------------

    #[test]
    fn test_times_all_fields_zero_on_repeated_calls() {
        for _ in 0..10 {
            let mut tms = Tms {
                tms_utime: 0xFF, tms_stime: 0xFF,
                tms_cutime: 0xFF, tms_cstime: 0xFF,
            };
            times(&mut tms);
            assert_eq!(tms.tms_utime, 0);
            assert_eq!(tms.tms_stime, 0);
            assert_eq!(tms.tms_cutime, 0);
            assert_eq!(tms.tms_cstime, 0);
        }
    }

    // -----------------------------------------------------------------------
    // ns_to_ticks — boundary cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_ns_to_ticks_zero() {
        assert_eq!(ns_to_ticks(0), 0);
    }

    #[test]
    fn test_ns_to_ticks_one_tick() {
        assert_eq!(ns_to_ticks(NS_PER_TICK), 1);
    }

    #[test]
    fn test_ns_to_ticks_truncation() {
        // 19_999_999 ns < 2 ticks => 1 tick.
        assert_eq!(ns_to_ticks(2 * NS_PER_TICK - 1), 1);
        // 20_000_000 ns exactly => 2 ticks.
        assert_eq!(ns_to_ticks(2 * NS_PER_TICK), 2);
    }

    #[test]
    fn test_ns_to_ticks_one_second_is_clk_tck() {
        // 1 second of nanoseconds => CLK_TCK ticks (100 for 100 Hz).
        assert_eq!(ns_to_ticks(1_000_000_000), CLK_TCK);
    }

    #[test]
    fn test_ns_to_ticks_saturates_at_i64_max() {
        // u64::MAX nanoseconds / 10_000_000 ns/tick = 1.84e12 ticks,
        // which is < i64::MAX (~9.2e18), so this case does NOT saturate.
        let huge = u64::MAX;
        let ticks = ns_to_ticks(huge);
        assert!(ticks > 0);
        assert_eq!(ticks as u64, huge / NS_PER_TICK);
    }
}
