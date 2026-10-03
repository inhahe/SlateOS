//! `epoll`: waiting for any of several descriptors to become ready.
//!
//! The three calls in their C shapes -- an instance, descriptors added to it,
//! and a wait that reports how many became ready -- for a program whose
//! upstream waits that way. `pidwait` is the first: it adds one `pidfd` per
//! process it found and waits until every one has reported its process gone.

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// The descriptor has something to read -- for a `pidfd`, its process has
/// exited.
pub const EPOLLIN: u32 = 0x001;
/// Edge-triggered: report a descriptor once per change rather than for as
/// long as it stays ready.
pub const EPOLLET: u32 = 1 << 31;
/// `epoll_ctl`'s operation for adding a descriptor.
pub const EPOLL_CTL_ADD: i32 = 1;

/// `struct epoll_event`: what to watch for on a descriptor going in, what
/// happened coming out, and a cookie the caller chose.
///
/// Packed on x86-64, as the Linux headers declare it there (12 bytes, the
/// 64-bit cookie unaligned), and only there -- every other architecture pads
/// it to 16. Getting this wrong would put each event after the first at the
/// wrong offset, so it follows the headers exactly.
#[cfg_attr(target_arch = "x86_64", repr(C, packed))]
#[cfg_attr(not(target_arch = "x86_64"), repr(C))]
#[derive(Clone, Copy, Debug, Default)]
pub struct EpollEvent {
    /// The events: watched for when added, happened when returned.
    pub events: u32,
    /// The caller's cookie, returned as given.
    pub data: u64,
}

#[cfg(unix)]
mod sys {
    use super::EpollEvent;

    unsafe extern "C" {
        pub fn epoll_create(size: i32) -> i32;
        pub fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: *mut EpollEvent) -> i32;
        pub fn epoll_wait(epfd: i32, events: *mut EpollEvent, maxevents: i32, timeout: i32) -> i32;
    }
}

/// A new epoll instance: `epoll_create (size)`. `size` must be positive and
/// is otherwise ignored, as it has been since Linux 2.6.8.
///
/// The descriptor is the caller's to close.
///
/// # Errors
///
/// The `errno` from `epoll_create`: `EINVAL` for a `size` below 1, `EMFILE`
/// or `ENFILE` when no descriptor is left, `ENOMEM`; [`ENOSYS`](crate::ENOSYS)
/// off Unix.
pub fn create(size: i32) -> Result<i32, i32> {
    create_one(size)
}

#[cfg(unix)]
fn create_one(size: i32) -> Result<i32, i32> {
    // SAFETY: one integer in, one descriptor or -1 out; no memory of ours.
    let fd = unsafe { sys::epoll_create(size) };
    if fd >= 0 { Ok(fd) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn create_one(_size: i32) -> Result<i32, i32> {
    Err(ENOSYS)
}

/// Watch `fd` for `events` on instance `epfd`, reporting `data` when they
/// happen: `epoll_ctl (epfd, EPOLL_CTL_ADD, fd, &event)`.
///
/// # Errors
///
/// The `errno` from `epoll_ctl`: `EBADF` for either descriptor, `EEXIST` for
/// one already watched, `EPERM` for a file that cannot be polled;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn add(epfd: i32, fd: i32, events: u32, data: u64) -> Result<(), i32> {
    add_one(epfd, fd, EpollEvent { events, data })
}

#[cfg(unix)]
fn add_one(epfd: i32, fd: i32, mut event: EpollEvent) -> Result<(), i32> {
    // SAFETY: `event` is a live, correctly laid out `struct epoll_event` that
    // the call reads (and, for `ADD`, does not keep) for its duration.
    let rc = unsafe { sys::epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &raw mut event) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn add_one(_epfd: i32, _fd: i32, _event: EpollEvent) -> Result<(), i32> {
    Err(ENOSYS)
}

/// Wait up to `timeout` milliseconds (-1: for ever) for watched descriptors
/// to become ready: `epoll_wait (epfd, events, events.len (), timeout)`.
/// Returns how many entries at the front of `events` were filled.
///
/// # Errors
///
/// The `errno` from `epoll_wait`: `EINTR` when a signal arrived first,
/// `EBADF` or `EINVAL` for a bad instance or an empty `events`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn wait(epfd: i32, events: &mut [EpollEvent], timeout: i32) -> Result<usize, i32> {
    wait_one(epfd, events, timeout)
}

#[cfg(unix)]
fn wait_one(epfd: i32, events: &mut [EpollEvent], timeout: i32) -> Result<usize, i32> {
    // More than `i32::MAX` entries cannot be asked for; asking for fewer than
    // there is room for is always safe.
    let max = i32::try_from(events.len()).unwrap_or(i32::MAX);
    // SAFETY: `events` is valid for `max` entries, which is all the call
    // writes, and the borrow outlives the call.
    let rc = unsafe { sys::epoll_wait(epfd, events.as_mut_ptr(), max, timeout) };
    match usize::try_from(rc) {
        Ok(n) => Ok(n),
        Err(_) => Err(last_errno()),
    }
}

#[cfg(not(unix))]
fn wait_one(_epfd: i32, _events: &mut [EpollEvent], _timeout: i32) -> Result<usize, i32> {
    Err(ENOSYS)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::EpollEvent;

    #[test]
    fn the_event_is_laid_out_as_the_headers_have_it() {
        #[cfg(target_arch = "x86_64")]
        assert_eq!(core::mem::size_of::<EpollEvent>(), 12);
        #[cfg(not(target_arch = "x86_64"))]
        assert_eq!(core::mem::size_of::<EpollEvent>(), 16);
    }

    #[test]
    fn constants_agree_with_posix() {
        assert_eq!(super::EPOLLIN, posix::epoll::EPOLLIN);
        assert_eq!(super::EPOLLET, posix::epoll::EPOLLET);
        assert_eq!(super::EPOLL_CTL_ADD, posix::epoll::EPOLL_CTL_ADD);
        assert_eq!(
            core::mem::size_of::<EpollEvent>(),
            core::mem::size_of::<posix::epoll::EpollEvent>()
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        assert_eq!(super::create(1), Err(crate::ENOSYS));
        assert_eq!(super::add(3, 4, super::EPOLLIN, 0), Err(crate::ENOSYS));
        let mut ev = [EpollEvent::default(); 2];
        assert_eq!(super::wait(3, &mut ev, 0), Err(crate::ENOSYS));
    }

    /// A real instance on a real kernel: a pipe with something in it is
    /// ready, and is reported with the cookie it was added with.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_ready_pipe_is_reported_with_its_cookie() {
        unsafe extern "C" {
            fn pipe(fds: *mut i32) -> i32;
            fn write(fd: i32, buf: *const u8, n: usize) -> isize;
            fn close(fd: i32) -> i32;
        }
        let mut fds = [0i32; 2];
        // SAFETY: `fds` has room for the two descriptors `pipe` writes.
        assert_eq!(unsafe { pipe(fds.as_mut_ptr()) }, 0);
        let [r, w] = fds;
        let ep = super::create(1).expect("epoll_create");
        super::add(ep, r, super::EPOLLIN, 77).expect("epoll_ctl");
        let mut ev = [EpollEvent::default(); 4];
        assert_eq!(super::wait(ep, &mut ev, 0), Ok(0));
        // SAFETY: one byte from a live buffer to a pipe we own.
        assert_eq!(unsafe { write(w, b"x".as_ptr(), 1) }, 1);
        assert_eq!(super::wait(ep, &mut ev, 1000), Ok(1));
        let first = ev[0];
        let data = first.data;
        assert_eq!(data, 77);
        for fd in [r, w, ep] {
            // SAFETY: each is a descriptor this test opened and still owns.
            unsafe { close(fd) };
        }
    }
}
