//! Waiting on several sockets at once.
//!
//! # Why this exists
//!
//! The standard library can block on *one* socket — `read`, or `peek` with a
//! timeout, which is how [`Socket::wait`](crate::Socket) parks — and cannot
//! block on several. A server that must watch a listener and every client, and
//! a display that must also watch its keyboard and mouse, is left with two
//! shapes: a thread per socket, or a loop that wakes on a timer to ask each one
//! in turn. The compositor took the second (`known-issues.md` →
//! `TD-COMPOSITOR-POLLS-INSTEAD-OF-WAITING`), and paid for it twice: a request
//! that arrived just after a tick waited up to a whole frame to be read, and a
//! desktop nobody was touching still woke sixty times a second to find that
//! out.
//!
//! [`WaitSet`] is the missing primitive: block until any of a set of handles
//! has something to read — or has hung up, which is also something the caller
//! must wake to find — or a timeout passes.
//!
//! # How, per platform
//!
//! - **SlateOS**, and Linux generally, has `poll(2)`: SlateOS's kernel
//!   implements it (`kernel/src/syscall/linux.rs`, `sys_poll`) over its
//!   multi-object wait (`kernel/src/ipc/multiwait.rs`). The call is made
//!   directly, the way the compositor's DRM and evdev modules make theirs: the
//!   workspace carries no `libc` crate, and one system call does not justify
//!   one. Anything `poll` accepts can go in the set, so the compositor's
//!   input devices wait beside its sockets.
//!
//!   How well that blocks is the kernel's business, and today it is uneven:
//!   SlateOS's network sockets and evdev devices cannot yet push readiness
//!   into the kernel, so a set containing them is re-scanned on an adaptive
//!   0.5 → 20 ms backoff rather than truly parked (see the table at the top
//!   of `multiwait.rs`). The module is written so that the day they can, every
//!   caller blocks properly with no change here — which is the reason to
//!   *declare* a wait rather than poll: the kernel can improve a wait, and
//!   cannot improve a loop that never tells it what it is waiting for.
//!
//!   One gap is a bug rather than a limitation: a *listening* socket is never
//!   reported ready on SlateOS, however many connections are waiting. See
//!   [`LISTENER_READINESS`] for what a server must do about it until it is
//!   fixed.
//!
//! - **Windows**, the development host, is harder, because a GUI thread there
//!   must also wake for its *window messages* ([`WaitSet::wait_or_message`]),
//!   and `WSAPoll` — Windows' `poll` — cannot wait on a message queue. So a
//!   wait registers every socket against one event object with
//!   `WSAEventSelect`, waits on that event (and on a high-resolution timer, so
//!   a frame deadline is met to the millisecond rather than to the 15.6 ms
//!   system tick), and unregisters again before returning. Unregistering
//!   matters: while a socket is registered, Windows refuses to switch it back
//!   to blocking mode, and [`Socket`](crate::Socket) does exactly that to
//!   finish a large write. Which sockets are ready is then asked of `WSAPoll`
//!   with a zero timeout, which is exact.
//!
//! - **Anywhere else** there is no multiplexed wait here, and a wait sleeps
//!   for at most [`FALLBACK_SLICE`] and then reports every handle as possibly
//!   ready. That is the polling this replaces — correct, because every caller
//!   re-checks by reading, and no worse than before.
//!
//! # The contract
//!
//! A handle reported ready has something to read, has been hung up on, or has
//! failed; reading it will say which. A wait may also return with nothing
//! ready — a timeout, a signal, a window message — and a caller must treat
//! that as "look again", never as an error. What a wait never does is return
//! *late*: a timeout is rounded up to the platform's resolution, not down,
//! so a caller waiting for a deadline wakes at or after it and does not spin
//! on a zero-length remainder.

use std::io;
use std::time::Duration;

/// Something [`WaitSet`] can watch: a file descriptor on Unix-likes, a
/// `SOCKET` on Windows.
#[cfg(unix)]
pub type WaitHandle = std::os::fd::RawFd;
/// Something [`WaitSet`] can watch: a file descriptor on Unix-likes, a
/// `SOCKET` on Windows.
#[cfg(windows)]
pub type WaitHandle = std::os::windows::io::RawSocket;
/// Something [`WaitSet`] can watch. Nothing can be on this platform — see the
/// module documentation — so every handle is the same placeholder.
#[cfg(not(any(unix, windows)))]
pub type WaitHandle = u64;

/// How long a wait lasts on a platform with no multiplexed wait, at most.
///
/// One frame at 60 Hz: a caller that asked to wait for ever is then no worse
/// off than the timer loop the wait replaced, and one that asked for less gets
/// less.
pub const FALLBACK_SLICE: Duration = Duration::from_millis(16);

/// Whether a listening socket in a [`WaitSet`] is reported ready when a
/// connection is waiting to be accepted.
///
/// `false` on SlateOS today, and there it is a bug rather than a design: the
/// network daemon answers a readiness probe only for connections, and the
/// kernel reads its "no such connection" about a listener as "nothing waiting"
/// (`requests/f-a-poll-never-reports-a-connection-waiting-on-a-listening-socket.md`).
/// A server that waited on its listener there would never learn that anyone had
/// connected. So where this is `false` a server must ask its listener on every
/// tick and bound its wait, which is what the compositor does; the day lane A's
/// fix lands, this becomes `true` everywhere and can go.
pub const LISTENER_READINESS: bool = !cfg!(target_vendor = "slateos");

/// A socket, or anything else [`WaitSet`] can wait on.
pub trait AsWaitHandle {
    /// The handle the platform's wait takes for this.
    fn wait_handle(&self) -> WaitHandle;
}

#[cfg(unix)]
mod handles {
    use std::os::fd::AsRawFd;

    use super::{AsWaitHandle, WaitHandle};

    impl AsWaitHandle for std::net::TcpStream {
        fn wait_handle(&self) -> WaitHandle {
            self.as_raw_fd()
        }
    }

    impl AsWaitHandle for std::net::TcpListener {
        fn wait_handle(&self) -> WaitHandle {
            self.as_raw_fd()
        }
    }

    impl AsWaitHandle for std::net::UdpSocket {
        fn wait_handle(&self) -> WaitHandle {
            self.as_raw_fd()
        }
    }
}

#[cfg(windows)]
mod handles {
    use std::os::windows::io::AsRawSocket;

    use super::{AsWaitHandle, WaitHandle};

    impl AsWaitHandle for std::net::TcpStream {
        fn wait_handle(&self) -> WaitHandle {
            self.as_raw_socket()
        }
    }

    impl AsWaitHandle for std::net::TcpListener {
        fn wait_handle(&self) -> WaitHandle {
            self.as_raw_socket()
        }
    }

    impl AsWaitHandle for std::net::UdpSocket {
        fn wait_handle(&self) -> WaitHandle {
            self.as_raw_socket()
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod handles {
    use super::{AsWaitHandle, WaitHandle};

    impl AsWaitHandle for std::net::TcpStream {
        fn wait_handle(&self) -> WaitHandle {
            0
        }
    }

    impl AsWaitHandle for std::net::TcpListener {
        fn wait_handle(&self) -> WaitHandle {
            0
        }
    }

    impl AsWaitHandle for std::net::UdpSocket {
        fn wait_handle(&self) -> WaitHandle {
            0
        }
    }
}

/// A set of handles to wait on together.
///
/// Built afresh before each wait — [`clear`](Self::clear), then
/// [`add`](Self::add) each handle — and kept between waits so that a loop
/// waiting sixty times a second allocates nothing after its first.
#[derive(Debug, Default)]
pub struct WaitSet {
    /// One per handle, in the order they were added; the index is what
    /// [`add`](Self::add) returns.
    entries: Vec<platform::Entry>,
    /// The operating-system objects a wait needs beyond the handles, made on
    /// first use and kept.
    os: platform::Resources,
}

impl WaitSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget every handle, keeping the storage.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Watch `handle` in the next wait, and return the index to ask
    /// [`is_ready`](Self::is_ready) about afterwards.
    pub fn add(&mut self, handle: WaitHandle) -> usize {
        let index = self.entries.len();
        self.entries.push(platform::Entry::new(handle));
        index
    }

    /// Watch `source` in the next wait. [`add`](Self::add) for anything that
    /// knows its own handle.
    pub fn add_source(&mut self, source: &impl AsWaitHandle) -> usize {
        self.add(source.wait_handle())
    }

    /// How many handles the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set holds no handles at all — in which case a wait is a
    /// sleep for its timeout.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether the last wait found the handle at `index` ready.
    ///
    /// `false` for an index the set does not hold, and for every handle
    /// before the first wait.
    #[must_use]
    pub fn is_ready(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(platform::Entry::is_ready)
    }

    /// Block until a handle in the set is ready, or `timeout` passes (`None`:
    /// no timeout).
    ///
    /// Returns how many handles are ready — zero on a timeout, and possibly
    /// zero on a spurious wake, which the caller treats as "look again". See
    /// the module documentation for the contract.
    ///
    /// # Errors
    ///
    /// Whatever the platform's wait reports, other than being interrupted by
    /// a signal, which is a spurious wake.
    pub fn wait(&mut self, timeout: Option<Duration>) -> io::Result<usize> {
        platform::wait(
            &mut self.entries,
            &mut self.os,
            timeout,
            platform::Also::Nothing,
        )
    }

    /// [`Self::wait`], and also return when the calling thread has window
    /// messages to handle.
    ///
    /// A Windows GUI thread must pump its message queue or the window stops
    /// responding and repainting, and the queue is not a socket: a thread
    /// blocked in [`Self::wait`] would not wake for the user closing, resizing
    /// or typing into its window. Messages alone do not make a handle ready,
    /// so a wake for one returns zero, like a timeout.
    ///
    /// # Errors
    ///
    /// As [`Self::wait`].
    #[cfg(windows)]
    pub fn wait_or_message(&mut self, timeout: Option<Duration>) -> io::Result<usize> {
        platform::wait(
            &mut self.entries,
            &mut self.os,
            timeout,
            platform::Also::Messages,
        )
    }
}

/// A timeout in whole milliseconds, as `poll` takes it: `-1` for none, `0`
/// for none at all, and anything else rounded *up*.
///
/// Up, because a wait for a deadline that is 0.4 ms away must not come back
/// immediately and be asked again, and again, until the deadline passes: a
/// zero timeout means "return at once", the opposite of a short wait.
fn timeout_ms(timeout: Option<Duration>) -> i32 {
    match timeout {
        None => -1,
        Some(d) => {
            let ms = d.as_nanos().div_ceil(1_000_000);
            i32::try_from(ms).unwrap_or(i32::MAX)
        }
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod platform {
    use std::io;
    use std::time::Duration;

    use super::{WaitHandle, timeout_ms};

    /// `struct pollfd`: `{ int fd; short events; short revents; }`.
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub(super) struct Entry {
        fd: i32,
        events: i16,
        revents: i16,
    }

    /// `POLLIN`. `POLLHUP`, `POLLERR` and `POLLNVAL` are reported whether or
    /// not they were asked for, and each is something the caller must wake to
    /// discover — a peer gone, a device unplugged — so any non-zero `revents`
    /// counts as ready.
    const POLLIN: i16 = 0x0001;

    impl Entry {
        pub(super) const fn new(fd: WaitHandle) -> Self {
            Self {
                fd,
                events: POLLIN,
                revents: 0,
            }
        }

        pub(super) const fn is_ready(&self) -> bool {
            self.revents != 0
        }
    }

    /// Nothing beyond the handles themselves: `poll` needs no other object.
    #[derive(Debug, Default)]
    pub(super) struct Resources;

    /// What else a wait may return for. Only Windows has anything.
    pub(super) enum Also {
        Nothing,
    }

    /// `poll` on x86-64 Linux, which the SlateOS kernel implements.
    const SYS_POLL: u64 = 7;
    /// A signal cut the wait short.
    const EINTR: i64 = 4;

    pub(super) fn wait(
        entries: &mut [Entry],
        _os: &mut Resources,
        timeout: Option<Duration>,
        also: Also,
    ) -> io::Result<usize> {
        let Also::Nothing = also;
        for entry in entries.iter_mut() {
            entry.revents = 0;
        }
        let count = u64::try_from(entries.len()).unwrap_or(u64::MAX);
        // The kernel reads the timeout as an `int`; a negative one goes in
        // sign-extended, which is how every caller of `poll` passes `-1`.
        #[allow(
            clippy::cast_sign_loss,
            reason = "a register holds the int's two's-complement bits; the kernel reads the low 32 as signed"
        )]
        let ms = i64::from(timeout_ms(timeout)) as u64;
        let ret: i64;
        // SAFETY: `poll(fds, nfds, timeout)` reads and writes `nfds` eight-byte
        // `struct pollfd`s at `fds`. `entries` is a live, exclusively borrowed
        // slice of exactly `nfds` `#[repr(C)]` values of that layout (a null
        // pointer with `nfds == 0` is also valid: nothing is touched). The
        // `syscall` instruction clobbers `rcx` and `r11`, declared below, and
        // returns in `rax`; the kernel preserves every other register.
        unsafe {
            core::arch::asm!(
                "syscall",
                inlateout("rax") SYS_POLL => ret,
                in("rdi") entries.as_mut_ptr(),
                in("rsi") count,
                in("rdx") ms,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        if ret < 0 {
            let errno = ret.checked_neg().unwrap_or(0);
            if errno == EINTR {
                // A signal is a spurious wake, not a failure: nothing was
                // written back, so report nothing ready.
                for entry in entries.iter_mut() {
                    entry.revents = 0;
                }
                return Ok(0);
            }
            return Err(io::Error::from_raw_os_error(
                i32::try_from(errno).unwrap_or(i32::MAX),
            ));
        }
        Ok(usize::try_from(ret).unwrap_or(usize::MAX))
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::io;
    use std::time::Duration;

    use super::{WaitHandle, timeout_ms};

    type Handle = *mut c_void;

    /// `WSAPOLLFD`: `{ SOCKET fd; SHORT events; SHORT revents; }`.
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub(super) struct Entry {
        fd: usize,
        events: i16,
        revents: i16,
    }

    /// `POLLRDNORM`. Windows spells `POLLIN` as this plus `POLLRDBAND`, and
    /// `WSAPoll` refuses a request for `POLLRDBAND` on a TCP socket, so this is
    /// what is asked for. Hang-up and error are reported unasked, as on Unix.
    const POLLRDNORM: i16 = 0x0100;

    impl Entry {
        pub(super) fn new(socket: WaitHandle) -> Self {
            Self {
                // A `SOCKET` is a `UINT_PTR`; std widens it to `u64` on every
                // target, and on a 64-bit Windows the round trip is exact.
                fd: usize::try_from(socket).unwrap_or(usize::MAX),
                events: POLLRDNORM,
                revents: 0,
            }
        }

        pub(super) const fn is_ready(&self) -> bool {
            self.revents != 0
        }
    }

    /// `FD_READ | FD_ACCEPT | FD_CLOSE`: a byte to read, a connection to
    /// accept, or a peer gone.
    const WAKE_EVENTS: i32 = 0x01 | 0x08 | 0x20;
    const INFINITE: u32 = u32::MAX;
    const WAIT_FAILED: u32 = u32::MAX;
    const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x0000_0002;
    const TIMER_ALL_ACCESS: u32 = 0x001F_0003;
    /// Every kind of message, so a thread wakes for anything its window needs
    /// to handle — input, painting, posted and sent messages, timers.
    const QS_ALLINPUT: u32 = 0x04FF;
    /// Wake for messages already in the queue, not only new ones: a message
    /// that arrived while the loop was busy must not wait for another.
    const MWMO_INPUTAVAILABLE: u32 = 0x0004;

    // The calls a wait needs, declared here rather than through a crate for
    // the reason `compositor::present::host` gives for its own: a handful of
    // stable, documented functions does not justify a dependency tree.
    #[link(name = "ws2_32")]
    unsafe extern "system" {
        fn WSAPoll(fds: *mut Entry, count: u32, timeout: i32) -> i32;
        fn WSAGetLastError() -> i32;
        fn WSACreateEvent() -> Handle;
        fn WSACloseEvent(event: Handle) -> i32;
        fn WSAResetEvent(event: Handle) -> i32;
        fn WSAEventSelect(socket: usize, event: Handle, events: i32) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn WaitForMultipleObjects(count: u32, handles: *const Handle, all: i32, ms: u32) -> u32;
        fn CreateWaitableTimerExW(
            attributes: *const c_void,
            name: *const u16,
            flags: u32,
            access: u32,
        ) -> Handle;
        fn SetWaitableTimer(
            timer: Handle,
            due: *const i64,
            period: i32,
            routine: *const c_void,
            argument: *const c_void,
            resume: i32,
        ) -> i32;
        fn CancelWaitableTimer(timer: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MsgWaitForMultipleObjectsEx(
            count: u32,
            handles: *const Handle,
            ms: u32,
            wake_mask: u32,
            flags: u32,
        ) -> u32;
    }

    /// The event every socket is registered against during a wait, and the
    /// timer that bounds it. Made on first use and kept, since making them is
    /// two system calls a wait would otherwise pay every time.
    ///
    /// Stored as addresses rather than pointers so that a [`super::WaitSet`]
    /// stays `Send`: both are process-wide kernel objects, usable from any
    /// thread, which a raw pointer field would stop the compiler knowing.
    #[derive(Debug, Default)]
    pub(super) struct Resources {
        event: usize,
        timer: usize,
    }

    impl Resources {
        fn event(&mut self) -> io::Result<Handle> {
            if self.event == 0 {
                // SAFETY: no arguments; returns a new manual-reset event or
                // null. Winsock is initialised, because this is reached only
                // with a socket in the set and std initialises Winsock before
                // it makes one.
                let event = unsafe { WSACreateEvent() };
                if event.is_null() {
                    return Err(last_winsock_error());
                }
                self.event = event as usize;
            }
            Ok(self.event as Handle)
        }

        fn timer(&mut self) -> io::Result<Handle> {
            if self.timer == 0 {
                // SAFETY: null attributes and name are documented as "default
                // security, unnamed"; the call only returns a handle or null.
                let mut timer = unsafe {
                    CreateWaitableTimerExW(
                        std::ptr::null(),
                        std::ptr::null(),
                        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                        TIMER_ALL_ACCESS,
                    )
                };
                if timer.is_null() {
                    // Before Windows 10 1803 the high-resolution flag is
                    // refused. An ordinary timer is late by up to a system
                    // tick, which is worse and still correct.
                    // SAFETY: as above.
                    timer = unsafe {
                        CreateWaitableTimerExW(
                            std::ptr::null(),
                            std::ptr::null(),
                            0,
                            TIMER_ALL_ACCESS,
                        )
                    };
                }
                if timer.is_null() {
                    return Err(io::Error::last_os_error());
                }
                self.timer = timer as usize;
            }
            Ok(self.timer as Handle)
        }
    }

    impl Drop for Resources {
        fn drop(&mut self) {
            // Failures are ignored: there is nothing to do about a handle that
            // will not close, and the process is the only thing holding it.
            if self.event != 0 {
                // SAFETY: made by `WSACreateEvent`, closed exactly once here.
                let _ = unsafe { WSACloseEvent(self.event as Handle) };
            }
            if self.timer != 0 {
                // SAFETY: made by `CreateWaitableTimerExW`, closed once here.
                let _ = unsafe { CloseHandle(self.timer as Handle) };
            }
        }
    }

    /// What else a wait may return for.
    pub(super) enum Also {
        Nothing,
        Messages,
    }

    fn last_winsock_error() -> io::Error {
        // SAFETY: no preconditions; reads this thread's last Winsock error.
        io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
    }

    /// Ask `WSAPoll` which entries are ready, without waiting.
    fn poll_now(entries: &mut [Entry]) -> io::Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let count = u32::try_from(entries.len()).unwrap_or(u32::MAX);
        // SAFETY: `WSAPoll` reads and writes `count` `WSAPOLLFD`s at the
        // pointer, which is a live, exclusively borrowed slice of exactly that
        // many `#[repr(C)]` values of that layout.
        let ret = unsafe { WSAPoll(entries.as_mut_ptr(), count, 0) };
        if ret < 0 {
            return Err(last_winsock_error());
        }
        Ok(usize::try_from(ret).unwrap_or(0))
    }

    /// Undo `WSAEventSelect` on every entry up to `upto`, returning the first
    /// failure.
    fn unregister(entries: &[Entry], upto: usize) -> io::Result<()> {
        let mut first = Ok(());
        for entry in entries.iter().take(upto) {
            // SAFETY: a zero event mask cancels the association; the event
            // argument is then ignored and may be null.
            if unsafe { WSAEventSelect(entry.fd, std::ptr::null_mut(), 0) } != 0 && first.is_ok() {
                first = Err(last_winsock_error());
            }
        }
        first
    }

    pub(super) fn wait(
        entries: &mut [Entry],
        os: &mut Resources,
        timeout: Option<Duration>,
        also: Also,
    ) -> io::Result<usize> {
        for entry in entries.iter_mut() {
            entry.revents = 0;
        }
        // Something is ready already: one call, and no registration at all.
        let ready = poll_now(entries)?;
        if ready > 0 || timeout.is_some_and(|d| d.is_zero()) {
            return Ok(ready);
        }

        let mut handles: [Handle; 2] = [std::ptr::null_mut(); 2];
        let mut count = 0usize;
        if !entries.is_empty() {
            let event = os.event()?;
            for (registered, entry) in entries.iter().enumerate() {
                // SAFETY: `entry.fd` is a socket the caller holds open for the
                // duration of the call, and `event` is ours.
                if unsafe { WSAEventSelect(entry.fd, event, WAKE_EVENTS) } != 0 {
                    let error = last_winsock_error();
                    // The ones already registered must not stay so: a
                    // registered socket cannot be made blocking again.
                    let _ = unregister(entries, registered);
                    return Err(error);
                }
            }
            if let Some(slot) = handles.get_mut(count) {
                *slot = event;
                count = count.saturating_add(1);
            }
        }
        // The timeout, as a timer rather than as the wait's own millisecond
        // argument, which Windows honours only to the system tick.
        let mut fallback_ms = INFINITE;
        if let Some(duration) = timeout {
            match os.timer() {
                Ok(timer) => {
                    // Relative due times are negative, in 100 ns units, and
                    // rounded up so the timer cannot fire early.
                    let ticks =
                        i64::try_from(duration.as_nanos().div_ceil(100)).unwrap_or(i64::MAX);
                    let due = ticks.saturating_neg();
                    // SAFETY: `timer` is ours; `due` is a live local the call
                    // only reads; no completion routine.
                    let set = unsafe {
                        SetWaitableTimer(
                            timer,
                            &raw const due,
                            0,
                            std::ptr::null(),
                            std::ptr::null(),
                            0,
                        )
                    };
                    if set == 0 {
                        fallback_ms = u32::try_from(timeout_ms(timeout)).unwrap_or(INFINITE);
                    } else if let Some(slot) = handles.get_mut(count) {
                        *slot = timer;
                        count = count.saturating_add(1);
                    }
                }
                Err(_) => {
                    fallback_ms = u32::try_from(timeout_ms(timeout)).unwrap_or(INFINITE);
                }
            }
        }

        let n = u32::try_from(count).unwrap_or(0);
        let woke = match also {
            // SAFETY: `handles` holds `n` valid handles owned by this set;
            // the call only reads them.
            Also::Messages => unsafe {
                MsgWaitForMultipleObjectsEx(
                    n,
                    handles.as_ptr(),
                    fallback_ms,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                )
            },
            Also::Nothing if n == 0 => {
                // Nothing to wait on and no timer: only a timeout can end
                // this, so it is a sleep.
                if fallback_ms != INFINITE {
                    std::thread::sleep(Duration::from_millis(u64::from(fallback_ms)));
                }
                0
            }
            // SAFETY: as above; `all == 0` waits for any one of them.
            Also::Nothing => unsafe { WaitForMultipleObjects(n, handles.as_ptr(), 0, fallback_ms) },
        };
        let wait_error = (woke == WAIT_FAILED).then(io::Error::last_os_error);

        // Undone on every path, the failing one included — see the module
        // docs for why a socket must not stay registered.
        let unregistered = unregister(entries, entries.len());
        if !entries.is_empty() {
            if let Ok(event) = os.event() {
                // SAFETY: `event` is ours.
                let _ = unsafe { WSAResetEvent(event) };
            }
        }
        if timeout.is_some() && os.timer != 0 {
            // SAFETY: the timer is ours; cancelling an unarmed or fired timer
            // is a no-op.
            let _ = unsafe { CancelWaitableTimer(os.timer as Handle) };
        }
        if let Some(error) = wait_error {
            return Err(error);
        }
        unregistered?;
        poll_now(entries)
    }
}

#[cfg(not(any(windows, all(target_os = "linux", target_arch = "x86_64"))))]
mod platform {
    use std::io;
    use std::time::Duration;

    use super::{FALLBACK_SLICE, WaitHandle};

    /// Whether this handle is reported ready: always, after a wait.
    #[derive(Clone, Copy, Debug)]
    pub(super) struct Entry {
        ready: bool,
    }

    impl Entry {
        pub(super) const fn new(_handle: WaitHandle) -> Self {
            Self { ready: false }
        }

        pub(super) const fn is_ready(&self) -> bool {
            self.ready
        }
    }

    #[derive(Debug, Default)]
    pub(super) struct Resources;

    pub(super) enum Also {
        Nothing,
    }

    pub(super) fn wait(
        entries: &mut [Entry],
        _os: &mut Resources,
        timeout: Option<Duration>,
        also: Also,
    ) -> io::Result<usize> {
        let Also::Nothing = also;
        // No multiplexed wait here: sleep a slice and let the caller look at
        // everything, which is what it did before this module existed.
        std::thread::sleep(timeout.map_or(FALLBACK_SLICE, |d| d.min(FALLBACK_SLICE)));
        for entry in entries.iter_mut() {
            entry.ready = true;
        }
        Ok(entries.len())
    }
}

// ---------------------------------------------------------------------------
// Waking a wait from another thread
// ---------------------------------------------------------------------------

/// A way to end a wait from another thread: the self-pipe trick.
///
/// Returns the two halves. The [`WakeReceiver`] goes into a [`WaitSet`] like
/// any socket; the [`WakeSender`] is `Send + Sync`, is shared by every thread
/// that may need to wake the waiter, and makes the receiver readable with
/// [`WakeSender::wake`]. The waiter [`drain`](WakeReceiver::drain)s the
/// receiver once woken, so that its next wait blocks again.
///
/// A wake sent while nobody is waiting is not lost: it leaves the receiver
/// readable, and the next wait returns at once.
///
/// It is a **pipe** on Linux and SlateOS — kernel-native, needing nothing of
/// the network daemon, and on SlateOS one of the few objects a `poll` truly
/// parks on — and a pair of **loopback UDP sockets** elsewhere, because a
/// Windows wait takes only sockets.
///
/// # Errors
///
/// Whatever making the pipe or the sockets fails with — out of descriptors,
/// most likely.
pub fn wake_channel() -> io::Result<(WakeSender, WakeReceiver)> {
    let (sender, receiver) = wake::channel()?;
    Ok((
        WakeSender { inner: sender },
        WakeReceiver { inner: receiver },
    ))
}

/// The half of a [`wake_channel`] that wakes. Share it — behind an `Arc`, or as
/// a [`std::task::Waker`], which it converts into — with whatever thread will
/// need to end the wait.
#[derive(Debug)]
pub struct WakeSender {
    inner: wake::Sender,
}

impl WakeSender {
    /// Make the receiver readable, ending the wait it is in, or the next one
    /// to begin.
    ///
    /// Never blocks, and reports nothing, because nothing that can go wrong is
    /// the waker's to act on: a full pipe already holds a wake, and a receiver
    /// that has gone away has nobody left to wake.
    pub fn wake(&self) {
        self.inner.wake();
    }
}

/// A [`WakeSender`] is a [`std::task::Waker`] — the standard handle for "tell
/// whoever is waiting to look again" — so it can be handed to anything that
/// already speaks that, an async executor included.
impl std::task::Wake for WakeSender {
    fn wake(self: std::sync::Arc<Self>) {
        self.inner.wake();
    }

    fn wake_by_ref(self: &std::sync::Arc<Self>) {
        self.inner.wake();
    }
}

/// The half of a [`wake_channel`] that is waited on.
#[derive(Debug)]
pub struct WakeReceiver {
    inner: wake::Receiver,
}

impl WakeReceiver {
    /// Consume every wake sent so far, so that the next wait blocks again.
    ///
    /// Bounded: a thread waking in a tight loop cannot hold the caller here.
    /// Whatever is left over simply wakes the next wait at once.
    pub fn drain(&self) {
        self.inner.drain();
    }
}

impl AsWaitHandle for WakeReceiver {
    fn wait_handle(&self) -> WaitHandle {
        self.inner.handle()
    }
}

/// How many reads [`WakeReceiver::drain`] makes at most. Each takes up to
/// [`DRAIN_CHUNK`] wakes.
const DRAIN_READS: usize = 16;

/// Bytes read per [`WakeReceiver::drain`] read.
const DRAIN_CHUNK: usize = 64;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod wake {
    use std::io;

    use super::{DRAIN_CHUNK, DRAIN_READS, WaitHandle};

    const SYS_READ: u64 = 0;
    const SYS_WRITE: u64 = 1;
    const SYS_CLOSE: u64 = 3;
    const SYS_PIPE2: u64 = 293;
    /// Neither end may ever block: a waker that blocked on a full pipe would
    /// stall the thread that was only trying to say it had finished.
    const O_NONBLOCK: u64 = 0o4000;
    /// A child the process starts must not inherit a way to wake it.
    const O_CLOEXEC: u64 = 0o2_000_000;
    const EINTR: i64 = 4;

    /// A three-argument system call, returning the kernel's raw result:
    /// `-errno` in `-4095..0`, anything else a success.
    ///
    /// # Safety
    ///
    /// The arguments must be valid for the call named by `n` — in particular
    /// any pointer must cover the memory the kernel will read or write.
    unsafe fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> i64 {
        let ret: i64;
        // SAFETY: the `syscall` instruction clobbers `rcx` and `r11`, both
        // declared below, and returns in `rax`; the argument registers are the
        // x86-64 Linux ABI's. Validity of the arguments is the caller's
        // documented obligation.
        unsafe {
            core::arch::asm!(
                "syscall",
                inlateout("rax") n => ret,
                in("rdi") a1,
                in("rsi") a2,
                in("rdx") a3,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }

    /// A pipe end, closed exactly once, when this is dropped.
    #[derive(Debug)]
    struct Fd(i32);

    impl Drop for Fd {
        fn drop(&mut self) {
            // SAFETY: `close` takes an integer and touches no memory; the
            // descriptor came from `pipe2` and is closed only here, since `Fd`
            // is neither `Clone` nor `Copy`. A failed close leaves nothing to
            // do.
            let _ = unsafe { syscall3(SYS_CLOSE, u64::from(self.0.cast_unsigned()), 0, 0) };
        }
    }

    #[derive(Debug)]
    pub(super) struct Sender(Fd);

    #[derive(Debug)]
    pub(super) struct Receiver(Fd);

    pub(super) fn channel() -> io::Result<(Sender, Receiver)> {
        let mut fds = [-1i32; 2];
        // SAFETY: `pipe2` writes two `int`s at the pointer, which is a live,
        // exclusively borrowed array of exactly two.
        let ret = unsafe {
            syscall3(
                SYS_PIPE2,
                fds.as_mut_ptr() as u64,
                O_NONBLOCK | O_CLOEXEC,
                0,
            )
        };
        if ret < 0 {
            return Err(io::Error::from_raw_os_error(
                i32::try_from(ret.saturating_neg()).unwrap_or(i32::MAX),
            ));
        }
        let [read_end, write_end] = fds;
        Ok((Sender(Fd(write_end)), Receiver(Fd(read_end))))
    }

    impl Sender {
        pub(super) fn wake(&self) {
            let byte = [1u8];
            loop {
                // SAFETY: `write` reads one byte at the pointer, which is a
                // live one-byte array.
                let ret = unsafe {
                    syscall3(
                        SYS_WRITE,
                        u64::from((self.0).0.cast_unsigned()),
                        byte.as_ptr() as u64,
                        1,
                    )
                };
                // Retried only if a signal cut it short. `EAGAIN` is a full
                // pipe, which already holds a wake; anything else means the
                // reading end is gone, and there is nobody left to wake.
                if ret != -EINTR {
                    return;
                }
            }
        }
    }

    impl Receiver {
        pub(super) fn drain(&self) {
            let mut buf = [0u8; DRAIN_CHUNK];
            let mut reads = 0;
            while reads < DRAIN_READS {
                // SAFETY: `read` writes at most `DRAIN_CHUNK` bytes at the
                // pointer, which is a live, exclusively borrowed array of that
                // many.
                let ret = unsafe {
                    syscall3(
                        SYS_READ,
                        u64::from((self.0).0.cast_unsigned()),
                        buf.as_mut_ptr() as u64,
                        DRAIN_CHUNK as u64,
                    )
                };
                if ret == -EINTR {
                    continue;
                }
                reads = reads.saturating_add(1);
                // Only a full read can have left more behind. Empty
                // (`EAGAIN`), closed, failed, or a short read that took
                // everything: done either way.
                match usize::try_from(ret) {
                    Ok(n) if n == DRAIN_CHUNK => {}
                    _ => return,
                }
            }
        }

        pub(super) fn handle(&self) -> WaitHandle {
            (self.0).0
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
mod wake {
    use std::io::{self, ErrorKind};
    use std::net::UdpSocket;

    use super::{AsWaitHandle, DRAIN_CHUNK, DRAIN_READS, WaitHandle};

    #[derive(Debug)]
    pub(super) struct Sender(UdpSocket);

    #[derive(Debug)]
    pub(super) struct Receiver(UdpSocket);

    pub(super) fn channel() -> io::Result<(Sender, Receiver)> {
        let receiver = UdpSocket::bind(("127.0.0.1", 0))?;
        let sender = UdpSocket::bind(("127.0.0.1", 0))?;
        sender.connect(receiver.local_addr()?)?;
        // Connected both ways, so that only the sender can wake the receiver:
        // a stray datagram from elsewhere on the machine cannot, and cannot
        // fill its buffer either.
        receiver.connect(sender.local_addr()?)?;
        // Neither end may ever block — see the pipe's `O_NONBLOCK`.
        sender.set_nonblocking(true)?;
        receiver.set_nonblocking(true)?;
        Ok((Sender(sender), Receiver(receiver)))
    }

    impl Sender {
        pub(super) fn wake(&self) {
            loop {
                match self.0.send(&[1]) {
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    // A full buffer already holds a wake; any other failure
                    // means the receiver is gone.
                    _ => return,
                }
            }
        }
    }

    impl Receiver {
        pub(super) fn drain(&self) {
            let mut buf = [0u8; DRAIN_CHUNK];
            let mut reads = 0;
            while reads < DRAIN_READS {
                match self.0.recv(&mut buf) {
                    Ok(_) => reads = reads.saturating_add(1),
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    // Empty, or failed: done either way.
                    Err(_) => return,
                }
            }
        }

        pub(super) fn handle(&self) -> WaitHandle {
            self.0.wait_handle()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    /// A connected pair on loopback, both ends non-blocking the way the
    /// display protocol's sockets are.
    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let near = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (far, _) = listener.accept().unwrap();
        near.set_nonblocking(true).unwrap();
        far.set_nonblocking(true).unwrap();
        (near, far)
    }

    /// A timeout no test here waits out.
    const LONG: Duration = Duration::from_mins(1);

    /// "Returned long before its timeout": half of [`LONG`]. The claims below
    /// are that a wait ended early *because something happened*, never that it
    /// ended within some short time — a loaded machine decides the second, and
    /// a timing assertion is only safe bounding a measurement from below (see
    /// the commit that made this the rule, 6cd54fc62).
    const PROMPT: Duration = Duration::from_secs(30);

    #[test]
    fn a_socket_with_bytes_waiting_is_ready_and_its_idle_neighbour_is_not() {
        let (mut a, b) = pair();
        let (_c, d) = pair();
        a.write_all(b"x").unwrap();

        let mut set = WaitSet::new();
        let has_data = set.add_source(&b);
        let idle = set.add_source(&d);
        let began = Instant::now();
        let ready = set.wait(Some(LONG)).unwrap();

        assert!(
            began.elapsed() < PROMPT,
            "waited for a byte that was already there"
        );
        assert_eq!(ready, 1);
        assert!(set.is_ready(has_data));
        assert!(!set.is_ready(idle), "an idle socket was reported ready");
    }

    #[test]
    fn a_wait_with_nothing_ready_lasts_its_timeout_and_reports_nothing() {
        let (_a, b) = pair();
        let mut set = WaitSet::new();
        let only = set.add_source(&b);
        let timeout = Duration::from_millis(60);
        let began = Instant::now();
        let ready = set.wait(Some(timeout)).unwrap();
        let took = began.elapsed();

        assert_eq!(ready, 0);
        assert!(!set.is_ready(only));
        assert!(
            took >= timeout,
            "came back early, after {took:?}: a caller waiting for a deadline would spin"
        );
    }

    #[test]
    fn a_short_timeout_is_not_rounded_down_to_no_wait_at_all() {
        // 0.3 ms rounds to *one* millisecond, not zero: zero is "return at
        // once", and a loop asking again until its deadline passed would spin.
        assert_eq!(timeout_ms(Some(Duration::from_micros(300))), 1);
        assert_eq!(timeout_ms(Some(Duration::from_micros(16_667))), 17);
        assert_eq!(timeout_ms(Some(Duration::ZERO)), 0);
        assert_eq!(timeout_ms(None), -1);
        assert_eq!(timeout_ms(Some(Duration::from_secs(u64::MAX))), i32::MAX);
    }

    #[test]
    fn a_zero_timeout_returns_at_once() {
        let (_a, b) = pair();
        let mut set = WaitSet::new();
        set.add_source(&b);
        let began = Instant::now();
        assert_eq!(set.wait(Some(Duration::ZERO)).unwrap(), 0);
        assert!(began.elapsed() < PROMPT);
    }

    #[test]
    fn a_blocked_wait_wakes_when_another_thread_writes() {
        let (mut a, b) = pair();
        let writer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            a.write_all(b"wake").unwrap();
            a
        });
        let mut set = WaitSet::new();
        let only = set.add_source(&b);
        let began = Instant::now();
        // Only the byte can end this before the minute is up.
        let ready = set.wait(Some(LONG)).unwrap();
        let took = began.elapsed();
        let _a = writer.join().unwrap();

        assert_eq!(ready, 1);
        assert!(set.is_ready(only));
        assert!(took < PROMPT, "the byte did not wake the wait: {took:?}");
    }

    #[test]
    fn a_peer_hanging_up_is_something_to_wake_for() {
        let (a, b) = pair();
        drop(a);
        let mut set = WaitSet::new();
        let only = set.add_source(&b);
        let began = Instant::now();
        let ready = set.wait(Some(LONG)).unwrap();
        assert!(
            began.elapsed() < PROMPT,
            "a hung-up socket did not wake the wait"
        );
        assert_eq!(ready, 1);
        assert!(set.is_ready(only));
    }

    #[test]
    fn a_listener_with_a_connection_waiting_is_ready() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut set = WaitSet::new();
        let at = set.add_source(&listener);
        assert_eq!(
            set.wait(Some(Duration::ZERO)).unwrap(),
            0,
            "nobody has connected yet"
        );

        let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let began = Instant::now();
        let ready = set.wait(Some(LONG)).unwrap();
        assert!(began.elapsed() < PROMPT);
        assert_eq!(ready, 1);
        assert!(set.is_ready(at));
    }

    #[test]
    fn an_empty_set_is_a_sleep() {
        let mut set = WaitSet::new();
        let began = Instant::now();
        assert_eq!(set.wait(Some(Duration::from_millis(30))).unwrap(), 0);
        assert!(began.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    fn the_set_is_rebuilt_between_waits_and_remembers_nothing() {
        let (mut a, b) = pair();
        let (_c, d) = pair();
        a.write_all(b"x").unwrap();
        let mut set = WaitSet::new();
        set.add_source(&b);
        assert_eq!(set.wait(Some(LONG)).unwrap(), 1);

        set.clear();
        assert!(set.is_empty());
        let other = set.add_source(&d);
        assert_eq!(other, 0, "indices start again from zero");
        assert_eq!(set.wait(Some(Duration::from_millis(20))).unwrap(), 0);
        assert!(
            !set.is_ready(other),
            "the old socket's readiness leaked into the new set"
        );
        assert!(
            !set.is_ready(7),
            "an index the set does not hold is never ready"
        );
    }

    #[test]
    fn reading_what_woke_the_wait_leaves_the_socket_idle_again() {
        // Level-triggered, the way `poll` is: ready while there is something
        // to read, and not once it has been read. A caller that drains what it
        // was woken for must be able to wait again without spinning.
        let (mut a, mut b) = pair();
        a.write_all(b"once").unwrap();
        let mut set = WaitSet::new();
        set.add_source(&b);
        assert_eq!(set.wait(Some(LONG)).unwrap(), 1);
        let mut buf = [0u8; 16];
        // The four bytes may take a moment to all arrive; read until they have.
        let mut got = 0;
        let until = Instant::now() + LONG;
        while got < 4 && Instant::now() < until {
            match b.read(&mut buf[got..]) {
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(&buf[..4], b"once");
        assert_eq!(set.wait(Some(Duration::from_millis(20))).unwrap(), 0);
    }

    #[test]
    fn a_socket_can_be_made_blocking_again_after_a_wait() {
        // Windows refuses to make a socket blocking while it is registered
        // with `WSAEventSelect`, and `Socket` makes one blocking to finish a
        // large write. A wait that left its registrations behind would turn
        // the next slow reader into a failed write.
        let (_a, b) = pair();
        let mut set = WaitSet::new();
        set.add_source(&b);
        set.wait(Some(Duration::from_millis(5))).unwrap();
        b.set_nonblocking(false)
            .expect("the wait left the socket registered");
        b.set_nonblocking(true).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_window_message_wakes_a_gui_threads_wait() {
        // The reason `wait_or_message` exists: a compositor's host window has
        // its input on a message queue, and a thread parked on sockets alone
        // would not wake for a keystroke — or for the user closing the window.
        #[link(name = "user32")]
        unsafe extern "system" {
            fn PostThreadMessageW(thread: u32, msg: u32, w: usize, l: isize) -> i32;
            fn PeekMessageW(
                msg: *mut [u8; 48],
                hwnd: *mut u8,
                min: u32,
                max: u32,
                remove: u32,
            ) -> i32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentThreadId() -> u32;
        }
        const WM_USER: u32 = 0x0400;
        const PM_NOREMOVE: u32 = 0;
        const PM_REMOVE: u32 = 1;

        let mut scratch = [0u8; 48];
        // SAFETY: `scratch` is larger than a `MSG`; a thread gets its message
        // queue the first time it peeks, which `PostThreadMessageW` needs.
        unsafe { PeekMessageW(&raw mut scratch, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };
        // SAFETY: no arguments.
        let me = unsafe { GetCurrentThreadId() };
        let poster = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            // SAFETY: plain integers; posting to a thread that exists.
            unsafe { PostThreadMessageW(me, WM_USER, 0, 0) }
        });

        let (_a, b) = pair();
        let mut set = WaitSet::new();
        let only = set.add_source(&b);
        let began = Instant::now();
        let ready = set.wait_or_message(Some(LONG)).unwrap();
        let took = began.elapsed();
        assert_ne!(poster.join().unwrap(), 0, "the message was not posted");
        // SAFETY: as above; removes the test's message so it cannot leak into
        // another test on this thread.
        unsafe { PeekMessageW(&raw mut scratch, std::ptr::null_mut(), 0, 0, PM_REMOVE) };

        assert!(took < PROMPT, "the message did not wake the wait: {took:?}");
        assert_eq!(ready, 0, "a message is not a socket being ready");
        assert!(!set.is_ready(only));
    }

    // ---- waking a wait from another thread ------------------------------

    #[test]
    fn a_wake_from_another_thread_ends_a_wait() {
        let (sender, receiver) = wake_channel().unwrap();
        let waker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            sender.wake();
            sender
        });
        let mut set = WaitSet::new();
        let at = set.add_source(&receiver);
        let began = Instant::now();
        // Only the wake can end this before the minute is up.
        assert_eq!(set.wait(Some(LONG)).unwrap(), 1);
        assert!(set.is_ready(at));
        assert!(began.elapsed() < PROMPT);
        let _sender = waker.join().unwrap();
    }

    #[test]
    fn a_wake_sent_before_anyone_waits_is_not_lost() {
        let (sender, receiver) = wake_channel().unwrap();
        sender.wake();
        let mut set = WaitSet::new();
        set.add_source(&receiver);
        let began = Instant::now();
        assert_eq!(set.wait(Some(LONG)).unwrap(), 1);
        assert!(
            began.elapsed() < PROMPT,
            "the wake was lost and the wait ran its course"
        );
    }

    #[test]
    fn a_drained_receiver_blocks_again() {
        let (sender, receiver) = wake_channel().unwrap();
        sender.wake();
        let mut set = WaitSet::new();
        set.add_source(&receiver);
        assert_eq!(set.wait(Some(LONG)).unwrap(), 1);
        receiver.drain();
        let timeout = Duration::from_millis(30);
        let began = Instant::now();
        assert_eq!(set.wait(Some(timeout)).unwrap(), 0, "one wake woke twice");
        assert!(began.elapsed() >= timeout);
    }

    #[test]
    fn waking_never_blocks_the_waker_however_often_it_wakes() {
        // A pipe holds 64 KiB; past that a write would block, and a waker that
        // blocked would stall the worker that was only saying it had finished.
        // Done on a thread, so that a blocked waker fails this rather than
        // hanging it; no bound is put on how long the wakes take.
        let (sender, receiver) = wake_channel().unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            for _ in 0..200_000 {
                sender.wake();
            }
            done_tx.send(sender).unwrap();
        });
        let _sender = done_rx
            .recv_timeout(LONG)
            .expect("a waker blocked: nobody drains the pipe, so it would block for ever");
        // And the receiver can be emptied, a bounded drain at a time.
        let mut set = WaitSet::new();
        set.add_source(&receiver);
        let mut drains = 0;
        while set.wait(Some(Duration::ZERO)).unwrap() > 0 {
            receiver.drain();
            drains += 1;
            assert!(drains < 10_000, "the receiver never emptied");
        }
    }

    #[test]
    fn a_standard_waker_made_from_the_sender_wakes_the_wait() {
        let (sender, receiver) = wake_channel().unwrap();
        let waker = std::task::Waker::from(std::sync::Arc::new(sender));
        let from_elsewhere = waker.clone();
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            from_elsewhere.wake_by_ref();
        });
        let mut set = WaitSet::new();
        set.add_source(&receiver);
        assert_eq!(set.wait(Some(LONG)).unwrap(), 1);
        worker.join().unwrap();
        drop(waker);
    }

    #[test]
    fn the_sending_half_can_be_shared_between_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<WakeSender>();
    }
}
