//! What a test of what something costs measures (tests only).
//!
//! A cost test bounds work that, done wrong, is quadratic: seconds of a
//! test build's CPU done right, minutes done wrong. What it measures is the
//! CPU time its own thread used, not the time on the wall: on a machine
//! busy with other work -- a boot test, a mutation sweep, this binary's
//! other tests -- the wall's time was several times the thread's, and
//! failed a bound the work itself was well inside.

#![allow(
    clippy::expect_used,
    reason = "a test's measure: a failure should be loud"
)]

use std::time::Duration;

/// What a cost test allows the work it bounds: done right, it is a few
/// seconds of a test build's CPU at most; done wrong, minutes.
pub(crate) const BOUND: Duration = Duration::from_secs(10);

/// `f` run, and the CPU time -- user and kernel -- this thread spent on it.
pub(crate) fn of<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let before = thread_cpu_time();
    let out = f();
    (out, thread_cpu_time().saturating_sub(before))
}

/// The CPU time this thread has used.
#[cfg(windows)]
fn thread_cpu_time() -> Duration {
    use core::ffi::c_void;

    /// A FILETIME: a count of 100-nanosecond intervals, in two halves.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// A pseudo-handle naming the calling thread: never invalid, never
        /// closed.
        safe fn GetCurrentThread() -> *mut c_void;
        fn GetThreadTimes(
            thread: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    let [mut creation, mut exit, mut kernel, mut user] = [FileTime::default(); 4];
    // SAFETY: the handle is this thread's pseudo-handle, valid for the call;
    // the four pointers are to distinct live FILETIMEs on this stack, which
    // GetThreadTimes only writes.
    let ok = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    };
    assert_ne!(ok, 0, "GetThreadTimes failed");
    let hundreds = |t: FileTime| (u64::from(t.high) << 32) | u64::from(t.low);
    Duration::from_nanos(
        hundreds(kernel)
            .saturating_add(hundreds(user))
            .saturating_mul(100),
    )
}

/// The CPU time this thread has used.
#[cfg(unix)]
fn thread_cpu_time() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a live timespec on this stack, which clock_gettime
    // only writes.
    let r = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &raw mut now) };
    assert_eq!(r, 0, "clock_gettime failed");
    Duration::new(
        u64::try_from(now.tv_sec).expect("a CPU time before the thread began"),
        u32::try_from(now.tv_nsec).expect("nanoseconds past a second"),
    )
}

mod tests {
    use super::*;
    use std::time::Instant;

    /// Waiting is not counted -- the whole point of measuring so.
    #[test]
    fn a_sleep_costs_nothing() {
        let ((), slept) = of(|| std::thread::sleep(Duration::from_millis(300)));
        assert!(slept < Duration::from_millis(50), "{slept:?}");
    }

    /// Work is counted, and never as more than the wall's time: one thread
    /// is on one CPU at a time. However busy the machine, the work goes on
    /// until the thread has had its 200 ms, however long that takes.
    #[test]
    fn work_is_counted_and_never_faster_than_the_wall() {
        let started = Instant::now();
        let ((), worked) = of(|| {
            let from = thread_cpu_time();
            while thread_cpu_time().saturating_sub(from) < Duration::from_millis(200) {
                assert!(
                    started.elapsed() < Duration::from_mins(1),
                    "a minute's work, and the thread's CPU time never reached 200 ms"
                );
                std::hint::black_box(());
            }
        });
        let wall = started.elapsed();
        assert!(worked >= Duration::from_millis(200), "{worked:?}");
        // A clock tick's grain aside (Windows counts a thread's time in its
        // 15.6 ms ticks).
        assert!(
            worked <= wall.saturating_add(Duration::from_millis(32)),
            "{worked:?} of the CPU in {wall:?} of the wall"
        );
    }
}
