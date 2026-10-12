//! A process's scheduling priority -- its nice value -- read and set through
//! the linked C library: `getpriority` and `setpriority`.
//!
//! `getpriority` is the call whose failure cannot be told from its answer:
//! -1 is a priority as well as the failure. The caller has to clear `errno`
//! before the call and read it after, and that is done here, once, as
//! everything else in this crate is done once.
//!
//! On a host that is not unix every call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::{clear_errno, last_errno};

/// `which`: one process (or thread), by its id; 0 is the caller.
pub const PRIO_PROCESS: i32 = 0;
/// `which`: a process group; 0 is the caller's.
pub const PRIO_PGRP: i32 = 1;
/// `which`: every process of a user; 0 is the caller's real user.
pub const PRIO_USER: i32 = 2;

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn getpriority(which: i32, who: u32) -> i32;
        pub fn setpriority(which: i32, who: u32, prio: i32) -> i32;
    }
}

/// `getpriority (which, who)`: the nice value, -20 to 19 -- the lowest of
/// them, for a group or a user.
///
/// # Errors
///
/// `ESRCH` when nothing matches, `EINVAL` for a `which` that is none of the
/// three; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn getpriority(which: i32, who: u32) -> Result<i32, i32> {
    getpriority_one(which, who)
}

#[cfg(unix)]
fn getpriority_one(which: i32, who: u32) -> Result<i32, i32> {
    clear_errno();
    // SAFETY: numbers only; the call touches no memory of ours.
    let prio = unsafe { sys::getpriority(which, who) };
    if prio == -1 {
        let errno = last_errno();
        if errno != 0 {
            return Err(errno);
        }
    }
    Ok(prio)
}

#[cfg(not(unix))]
fn getpriority_one(_which: i32, _who: u32) -> Result<i32, i32> {
    Err(ENOSYS)
}

/// `setpriority (which, who, prio)`: the nice value set, the kernel keeping
/// it within -20 to 19.
///
/// # Errors
///
/// `ESRCH` when nothing matches; `EACCES` for a lower value than the caller
/// may set; `EPERM` for a process another user owns; `EINVAL` for a `which`
/// that is none of the three; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn setpriority(which: i32, who: u32, prio: i32) -> Result<(), i32> {
    setpriority_one(which, who, prio)
}

#[cfg(unix)]
fn setpriority_one(which: i32, who: u32, prio: i32) -> Result<(), i32> {
    // SAFETY: numbers only; the call touches no memory of ours.
    let rc = unsafe { sys::setpriority(which, who, prio) };
    if rc < 0 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn setpriority_one(_which: i32, _who: u32, _prio: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// The restated numbers are the library's.
    #[test]
    fn the_numbers_are_the_librarys() {
        assert_eq!(PRIO_PROCESS, posix::resource::PRIO_PROCESS);
        assert_eq!(PRIO_PGRP, posix::resource::PRIO_PGRP);
        assert_eq!(PRIO_USER, posix::resource::PRIO_USER);
    }

    /// This process's own value read, set to itself, and read back; a
    /// process that is not there is `ESRCH` both ways.
    #[cfg(unix)]
    #[test]
    fn this_process_round_trips_and_a_missing_one_is_esrch() {
        let now = getpriority(PRIO_PROCESS, 0).unwrap();
        setpriority(PRIO_PROCESS, 0, now).unwrap();
        assert_eq!(getpriority(PRIO_PROCESS, 0), Ok(now));
        // Above any pid_max Linux allows.
        assert_eq!(getpriority(PRIO_PROCESS, 2_147_483_646), Err(3));
        assert_eq!(setpriority(PRIO_PROCESS, 2_147_483_646, now), Err(3));
        assert_eq!(getpriority(42, 0), Err(22), "no such which");
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        assert_eq!(getpriority(PRIO_PROCESS, 0), Err(ENOSYS));
        assert_eq!(setpriority(PRIO_PROCESS, 0, 0), Err(ENOSYS));
    }
}
