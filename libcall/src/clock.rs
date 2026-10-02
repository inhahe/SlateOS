//! The one clock `std` does not offer: the time since boot, sleep included.
//!
//! `std::time::Instant` is `CLOCK_MONOTONIC`, which on Linux stops while the
//! machine is suspended. gnulib's last way of finding when the machine booted
//! -- `who -b` on a system that writes no boot entry, SlateOS among them -- is
//! the clock less `CLOCK_BOOTTIME`, which does not stop; this is that call.

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `CLOCK_BOOTTIME`: 7 in glibc, musl and SlateOS's library alike.
#[cfg(any(unix, test))]
const CLOCK_BOOTTIME: i32 = 7;

#[cfg(unix)]
mod sys {
    /// C's `struct timespec` on x86-64: two 64-bit fields.
    #[repr(C)]
    pub struct Timespec {
        pub tv_sec: i64,
        pub tv_nsec: i64,
    }

    unsafe extern "C" {
        pub fn clock_gettime(clock: i32, tp: *mut Timespec) -> i32;
    }
}

/// How long the machine has been up, suspended time included:
/// `clock_gettime (CLOCK_BOOTTIME)`, as seconds and nanoseconds.
///
/// # Errors
///
/// `EINVAL` from a library without the clock, and
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn since_boot() -> Result<(i64, i64), i32> {
    since_boot_one()
}

#[cfg(unix)]
fn since_boot_one() -> Result<(i64, i64), i32> {
    let mut ts = sys::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a live `struct timespec` that the call writes and
    // nothing else; the clock id is a number.
    let rc = unsafe { sys::clock_gettime(CLOCK_BOOTTIME, &raw mut ts) };
    if rc == 0 {
        Ok((ts.tv_sec, ts.tv_nsec))
    } else {
        Err(last_errno())
    }
}

#[cfg(not(unix))]
fn since_boot_one() -> Result<(i64, i64), i32> {
    Err(ENOSYS)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_clock_id_is_the_librarys() {
        assert_eq!(CLOCK_BOOTTIME, posix::time::CLOCK_BOOTTIME);
    }

    /// The real library: the machine has been up for some time, and a
    /// nanosecond count is less than a second.
    #[cfg(unix)]
    #[test]
    fn the_machine_has_been_up_a_while() {
        let (sec, nsec) = since_boot().unwrap();
        assert!(sec >= 0);
        assert!((0..1_000_000_000).contains(&nsec), "{nsec}");
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_it_declines() {
        assert_eq!(since_boot(), Err(ENOSYS));
    }
}
