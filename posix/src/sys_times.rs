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
const NS_PER_TICK: u64 = 10_000_000;

/// Convert a nanosecond count to clock ticks (saturating to `i64::MAX`).
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn times(buffer: *mut Tms) -> i64 {
    if buffer.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }

    #[cfg(target_os = "none")]
    {
        // Query the kernel's aggregate CPU time fields.
        let system_ns = read_cpu_time_field(0);
        let irq_ns = read_cpu_time_field(1);
        let softirq_ns = read_cpu_time_field(2);

        let utime_ticks = ns_to_ticks(system_ns);
        let stime_ticks = ns_to_ticks(irq_ns.saturating_add(softirq_ns));

        // SAFETY: caller guarantees buffer is valid for one Tms.
        unsafe {
            (*buffer).tms_utime = utime_ticks;
            (*buffer).tms_stime = stime_ticks;
            // No terminated-children tracking yet.
            (*buffer).tms_cutime = 0;
            (*buffer).tms_cstime = 0;
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
        // SAFETY: caller guarantees buffer is valid for one Tms.
        unsafe {
            (*buffer).tms_utime = 0;
            (*buffer).tms_stime = 0;
            (*buffer).tms_cutime = 0;
            (*buffer).tms_cstime = 0;
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

    #[test]
    fn test_times_null_buffer() {
        crate::errno::set_errno(0);
        let ret = times(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
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
