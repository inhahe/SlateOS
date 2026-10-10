//! Blocking on several objects at once.
//!
//! Every readiness-waiting interface the kernel offers — `poll`, `select`,
//! `ppoll`, `epoll_wait`, and the `SYS_WAIT_MULTIPLE` lane B asked for — is the
//! same question: *park until one of these N objects becomes interesting, or
//! until a deadline.* Until this module existed the kernel answered it by
//! **spinning**: `poll_core` re-scanned its whole fd set every 10 ms, so an idle
//! `poll` cost 100 wakeups a second per caller and paid up to 10 ms of latency
//! on a fd that became ready one microsecond after a scan. Userspace copied the
//! shape — sshd grew its own 0.5 ms → 20 ms backoff loop
//! (`design-decisions.md` §770) — because there was nothing better to call.
//!
//! The five blocking IPC families each already keep a [`WaiterSet`] of parked
//! tasks and already wake it on every state change; what was missing was a way
//! to be in *several* of those sets at once. That is all this module is.
//!
//! [`WaiterSet`]: super::waiters::WaiterSet
//!
//! # What it can and cannot block on
//!
//! Only objects with a kernel waiter set can be blocked on. Everything else —
//! the daemon-backed `net::socket`, DRM, evdev, signalfd, pidfd, an epoll fd's
//! *readiness*, plain files, the console — has no way to push readiness into the
//! kernel, and is represented by [`WaitTarget::PollOnly`].
//!
//! | set contents | behaviour |
//! |---|---|
//! | every item blockable (pipe, eventfd, socketpair, timerfd, pty, channel, service listener, Unix-domain socket, ALSA PCM substream) | true block: woken by the object, zero wakeups while idle |
//! | any item poll-only | park capped at an adaptive backoff, re-scanning on each wake |
//!
//! [`WaitTarget::EpollCtl`] sits outside that table because it is not a
//! readiness target at all: it is blockable, but what it announces is a change
//! to `epoll_wait`'s *own set of targets*. See its own documentation.
//!
//! The slice is therefore a property of the *set*, not a constant, and the day
//! a poll-only family learns to push readiness every caller gets true blocking
//! with no change at the call site.
//!
//! **The capped path backs off adaptively rather than at a fixed interval, and
//! that is load-bearing, not a refinement.** sshd waits on a socket *and* a pty
//! — a mixed set, so it lands in the capped row — and it is about to delete its
//! own 0.5 ms → 20 ms loop in favour of this one. A fixed 10 ms cap would make
//! its best case 20× worse than the loop it replaces. So sshd's algorithm moves
//! *into* the kernel, where one tuned copy serves every caller: start short,
//! widen while nothing is ready, start short again on the next wait.
//!
//! # Why registering before testing is not interchangeable with the reverse
//!
//! [`wait_multiple`] registers on all N objects, *then* tests them. Testing
//! first and registering after loses any state change that lands in the gap:
//! the waker takes the set while we are not in it, and we then park with the
//! wake already delivered to nobody — sleeping the full timeout on an object
//! that is ready. Registering first cannot lose it, because a wake that arrives
//! before we park sets the task's `pending_wake` flag and
//! [`sched::block_current`] consumes it and returns without parking. The cost
//! of the safe order is one spurious re-scan, which is the same work the loop
//! was going to do anyway.
//!
//! # Why deregistration is a guard and not a call
//!
//! Per-object park loops (`pipe::read`, `timerfd::read_expirations_blocking`,
//! …) deregister by calling [`WaiterSet::remove`] at the top of each iteration.
//! That idiom is complete for N = 1 and *incomplete* for N > 1: if object 1 is
//! not ready we stay registered on it, and if object 5 is ready we return —
//! leaving our entry on object 1 behind. The leak is created by the return, not
//! by the loop, so no amount of care inside the loop removes it.
//!
//! [`WaiterSet::remove`]: super::waiters::WaiterSet::remove
//!
//! A leaked entry does harm two ways, and the second needs no rare
//! precondition:
//!
//! 1. once task ids recycle, it wakes an unrelated task;
//! 2. `pending_wake` is sticky, per-task and **unowned** — it does not record
//!    which object set it. A leaked entry lets a later wake set the flag while
//!    the task is running something else, and that task's next, unrelated
//!    `block_current()` then returns early from a wait nothing satisfied. This
//!    is the mechanism recorded against `BUG-DASH-CMDSUB-INTERMITTENT-HANG`.
//!
//! And multiwait multiplies the exposure by the two largest factors available:
//! up to N entries per call, in the syscall an event loop makes on *every*
//! iteration. So the sweep is structural — [`Registration`]'s `Drop` — rather
//! than disciplined, and the mid-scan early return that would leak becomes
//! unrepresentable.

use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;

use super::waiters::{current_user_pid, deliverable_signal_pending, park_interruptible};

/// First capped-path sleep, and the one that decides the latency of a mixed
/// set's *first* wait. 0.5 ms is sshd's own starting interval (§770): the point
/// of moving the backoff into the kernel is that callers see no regression.
const BACKOFF_MIN_NS: u64 = 500_000;

/// Ceiling for the capped path. Also sshd's, for the same reason. A wait that
/// has already gone 20 ms without anything becoming ready is idle, and idle
/// callers should cost as little as possible.
const BACKOFF_MAX_NS: u64 = 20_000_000;

/// One object a [`wait_multiple`] can park on.
///
/// Resolved by the caller from whatever handle namespace it uses — the Linux fd
/// table for `poll`/`select`/`epoll_wait`, native handles for
/// `SYS_WAIT_MULTIPLE` — so that the kind dispatch below is the only place in
/// the kernel that has to know which families are blockable.
///
/// The payload is the raw `u64` of that family's handle, exactly as an
/// `FdEntry` stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitTarget {
    /// [`super::pipe::PipeHandle`] raw value.
    Pipe(u64),
    /// [`super::eventfd::EventFdHandle`] raw value.
    EventFd(u64),
    /// [`super::stream_socket::StreamSocketHandle`] raw value.
    StreamSocket(u64),
    /// [`super::timerfd::TimerFdHandle`] raw value. Contributes a *deadline*
    /// as well as a registration — see [`super::timerfd::next_deadline_ns`].
    TimerFd(u64),
    /// [`crate::tty::pty::PtyHandle`] raw value.
    Pty(u64),
    /// [`super::channel::ChannelHandle`] raw value: woken when a message
    /// arrives for this end or its peer closes
    /// ([`super::channel::register_waiter`]).
    Channel(u64),
    /// [`super::service::ServiceListenerHandle`] raw value: woken when a
    /// connection is queued or the listener goes
    /// ([`super::service::register_waiter`]).
    Listener(u64),
    /// [`super::unix_socket::UnixHandle`] raw value: woken by a datagram, a
    /// connection to accept, room to send, or -- for a connected stream --
    /// anything its pair does ([`super::unix_socket::register_waiter`]).
    UnixSocket(u64),
    /// [`super::alsa_pcm::AlsaPcmHandle`] raw value: woken each time the audio
    /// output pump takes from the mixer's rings -- the only thing that changes a
    /// playback substream's room, or ends a drain
    /// ([`crate::audio_mixer::register_room_waiter`]).
    AlsaPcm(u64),
    /// [`super::epoll::EpollHandle`] raw value — the instance's
    /// **interest-set-change** notification, *not* its readiness.
    ///
    /// This is the one target whose wake does not mean "something you are
    /// waiting for is ready". It means "the set of things you are waiting for
    /// has changed", which only `epoll_wait` can act on: it resolves its targets
    /// from the interest list before parking, so a concurrent `epoll_ctl` would
    /// otherwise leave it blocked on a stale list. See
    /// [`super::epoll::register_waiter`].
    ///
    /// Because of that, this must never be what an epoll *fd* resolves to in
    /// `poll`/`select`. An epoll fd is readable when a member of its interest
    /// set is ready, and nothing about a member reaches this set — a `poll()`
    /// that parked here would sleep through the very event it asked about. Such
    /// an fd resolves to [`Self::PollOnly`], which re-scans and is correct.
    EpollCtl(u64),
    /// An object with no kernel waiter set: nothing to register on, so the wait
    /// must re-scan it on a timer. One of these in the set is enough to put the
    /// whole wait on the capped path.
    PollOnly,
}

impl WaitTarget {
    /// Add `task` to this object's waiter set(s). No-op for [`Self::PollOnly`]
    /// and for a handle whose object is already gone.
    fn register(self, task: TaskId) {
        match self {
            Self::Pipe(raw) => {
                super::pipe::register_waiter(super::pipe::PipeHandle::from_raw(raw), task);
            }
            Self::EventFd(raw) => {
                super::eventfd::register_waiter(super::eventfd::EventFdHandle::from_raw(raw), task);
            }
            Self::StreamSocket(raw) => {
                super::stream_socket::register_waiter(
                    super::stream_socket::StreamSocketHandle::from_raw(raw),
                    task,
                );
            }
            Self::TimerFd(raw) => {
                super::timerfd::register_waiter(super::timerfd::TimerFdHandle::from_raw(raw), task);
            }
            Self::Pty(raw) => {
                crate::tty::pty::register_waiter(crate::tty::pty::PtyHandle::from_raw(raw), task);
            }
            Self::Channel(raw) => {
                super::channel::register_waiter(super::channel::ChannelHandle::from_raw(raw), task);
            }
            Self::Listener(raw) => {
                super::service::register_waiter(
                    super::service::ServiceListenerHandle::from_raw(raw),
                    task,
                );
            }
            Self::UnixSocket(raw) => {
                super::unix_socket::register_waiter(
                    super::unix_socket::UnixHandle::from_raw(raw),
                    task,
                );
            }
            // One set for every substream: the pump's take changes them all.
            Self::AlsaPcm(_) => crate::audio_mixer::register_room_waiter(task),
            Self::EpollCtl(raw) => {
                super::epoll::register_waiter(super::epoll::EpollHandle::from_raw(raw), task);
            }
            Self::PollOnly => {}
        }
    }

    /// Remove `task` from this object's waiter set(s). Idempotent and
    /// stale-handle-safe, which is what lets [`Registration`]'s `Drop` call it
    /// unconditionally.
    fn deregister(self, task: TaskId) {
        match self {
            Self::Pipe(raw) => {
                super::pipe::deregister_waiter(super::pipe::PipeHandle::from_raw(raw), task);
            }
            Self::EventFd(raw) => {
                super::eventfd::deregister_waiter(
                    super::eventfd::EventFdHandle::from_raw(raw),
                    task,
                );
            }
            Self::StreamSocket(raw) => {
                super::stream_socket::deregister_waiter(
                    super::stream_socket::StreamSocketHandle::from_raw(raw),
                    task,
                );
            }
            Self::TimerFd(raw) => {
                super::timerfd::deregister_waiter(
                    super::timerfd::TimerFdHandle::from_raw(raw),
                    task,
                );
            }
            Self::Pty(raw) => {
                crate::tty::pty::deregister_waiter(crate::tty::pty::PtyHandle::from_raw(raw), task);
            }
            Self::Channel(raw) => {
                super::channel::deregister_waiter(
                    super::channel::ChannelHandle::from_raw(raw),
                    task,
                );
            }
            Self::Listener(raw) => {
                super::service::deregister_waiter(
                    super::service::ServiceListenerHandle::from_raw(raw),
                    task,
                );
            }
            Self::UnixSocket(raw) => {
                super::unix_socket::deregister_waiter(
                    super::unix_socket::UnixHandle::from_raw(raw),
                    task,
                );
            }
            Self::AlsaPcm(_) => crate::audio_mixer::deregister_room_waiter(task),
            Self::EpollCtl(raw) => {
                super::epoll::deregister_waiter(super::epoll::EpollHandle::from_raw(raw), task);
            }
            Self::PollOnly => {}
        }
    }

    /// How long a waiter may sleep before this object could become ready *by
    /// itself*, with no wake to announce it.
    ///
    /// `None` for every family whose state changes are announced by a wake —
    /// which is all of them except an armed timerfd, whose ordinary expiry
    /// wakes nobody (see [`super::timerfd::next_deadline_ns`] for why).
    fn next_deadline_ns(self) -> Option<u64> {
        match self {
            Self::TimerFd(raw) => {
                super::timerfd::next_deadline_ns(super::timerfd::TimerFdHandle::from_raw(raw))
            }
            _ => None,
        }
    }

    /// Whether this target has no waiter set, and so forces the capped path.
    const fn is_poll_only(self) -> bool {
        matches!(self, Self::PollOnly)
    }
}

/// Registration on every target of one wait, undone on **every** exit path.
///
/// Holding the registration in a value whose `Drop` sweeps it is the whole
/// point: see the module docs for what a leaked waiter-set entry does, and why
/// N-object waiting makes the leak easy to write and expensive to have.
struct Registration<'a> {
    targets: &'a [WaitTarget],
    task: TaskId,
}

impl<'a> Registration<'a> {
    /// Register `task` on every target.
    ///
    /// Takes each family's table lock in turn and never two at once, so there
    /// is no lock-ordering hazard however the set is composed. There is
    /// deliberately no moment at which the whole set is consistent — there does
    /// not need to be, because each object's readiness and each object's waiter
    /// entry are guarded by that same object's lock.
    fn new(targets: &'a [WaitTarget], task: TaskId) -> Self {
        for target in targets {
            target.register(task);
        }
        Self { targets, task }
    }
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        for target in self.targets {
            target.deregister(self.task);
        }
    }
}

/// `hrtimer` callback that ends a capped or deadline-bounded park.
///
/// Mirrors the eventfd/timerfd idiom: prefer a direct wake, fall back to a
/// deferred one if the target has not parked yet, which closes the
/// wake-before-block race.
fn multiwait_wake(tid: u64) {
    if !sched::try_wake(tid) {
        sched::defer_wake(tid);
    }
}

/// Park until one of `targets` is ready, `timeout_ns` elapses, or a signal
/// arrives.
///
/// `ready` is the caller's readiness scan, returning how many of its items are
/// ready *now*. It is supplied rather than computed here because readiness is
/// expressed differently in each namespace — `poll` wants Linux `revents` bits
/// masked by what each fd asked for, `SYS_WAIT_MULTIPLE` wants its own
/// `revents` — while the parking is identical. This module therefore owns the
/// blocking and the kind dispatch; the caller owns the meaning of "ready".
///
/// `timeout_ns` of `None` waits indefinitely (`poll(…, -1)`).
///
/// # Returns
///
/// * `Ok(n)`, `n > 0` — the count `ready` reported.
/// * `Ok(0)` — the timeout elapsed with nothing ready.
///
/// # Errors
///
/// [`KernelError::Interrupted`] if a deliverable signal is pending. The scan is
/// always run at least once before this is checked, so a wait whose objects are
/// already ready does not fail on a pending signal — matching `poll(2)`, which
/// reports ready fds rather than `EINTR` when both apply.
pub fn wait_multiple<F>(
    targets: &[WaitTarget],
    timeout_ns: Option<u64>,
    mut ready: F,
) -> KernelResult<usize>
where
    F: FnMut() -> usize,
{
    let pid = current_user_pid();
    let task = sched::current_task_id();
    let start = crate::hrtimer::now_ns();
    let capped = targets.iter().any(|t| t.is_poll_only());
    let mut backoff = BACKOFF_MIN_NS;

    loop {
        // Register on everything before testing anything — see the module docs
        // for why the reverse order loses wakes. The guard's `Drop` sweeps the
        // registration on every path out of this scope, including the returns
        // below and an unwinding one.
        let registration = Registration::new(targets, task);

        let n = ready();
        if n > 0 {
            return Ok(n);
        }

        if deliverable_signal_pending(pid) {
            return Err(KernelError::Interrupted);
        }

        // How much of the caller's timeout is left. Checked after the scan, so
        // a zero timeout is a well-defined non-blocking poll rather than a
        // wait that never looks.
        let remaining = match timeout_ns {
            Some(total) => {
                let elapsed = crate::hrtimer::now_ns().saturating_sub(start);
                let left = total.saturating_sub(elapsed);
                if left == 0 {
                    return Ok(0);
                }
                Some(left)
            }
            None => None,
        };

        let mut slice = remaining;
        for target in targets {
            // A deadline of 0 means "already in the state it was going to
            // reach", and the scan has just said that state is not interesting
            // to this caller — `poll(timerfd, POLLOUT)` on an expired timer is
            // the case. Capping at 0 there would spin at timer resolution until
            // the timeout instead of sleeping, so an elapsed deadline
            // contributes nothing.
            if let Some(deadline) = target.next_deadline_ns().filter(|d| *d > 0) {
                slice = Some(slice.map_or(deadline, |s| s.min(deadline)));
            }
        }
        if capped {
            slice = Some(slice.map_or(backoff, |s| s.min(backoff)));
            backoff = backoff.saturating_mul(2).min(BACKOFF_MAX_NS);
        }

        // No slice at all means every target announces its own changes and the
        // caller set no timeout: park with no timer and cost nothing until a
        // real wake arrives. That is the case this module exists to create.
        let timer = slice.map(|s| crate::hrtimer::schedule_ns(s.max(1), multiwait_wake, task));
        park_interruptible(
            pid,
            task,
            crate::wchan::Wait::on(crate::wchan::WaitChannel::Poll),
        );
        if let Some(handle) = timer {
            // Harmless if it already fired.
            crate::hrtimer::cancel(handle);
        }

        // Deregister before the next iteration re-registers. Dropping and
        // re-taking the registration cannot lose a wake: readiness here is
        // level-triggered, so a change in the gap is seen by the next scan.
        drop(registration);
    }
}

// ---------------------------------------------------------------------------
// Boot self-test
// ---------------------------------------------------------------------------

/// Counted up by each self-test helper task once its action has succeeded, so
/// a failing run can tell "the helper never acted" from "it acted and the wake
/// was lost". Read only after [`join_helper`]: a helper counts *after* acting,
/// so the wait its action released can return before the count moves.
static MW_WROTE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Helper task: sleep, then write one byte to the pipe write handle in `raw`.
///
/// The sleep is what makes the test test something: without it the write could
/// land before the waiter parks, and the wait would be satisfied by its first
/// scan without the waiter-set registration ever being exercised.
extern "C" fn mw_pipe_writer_task(raw: u64) {
    sched::sleep_ns_interruptible(20_000_000); // 20ms
    let wh = super::pipe::PipeHandle::from_raw(raw);
    if super::pipe::try_write(wh, b"x").is_ok() {
        MW_WROTE.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Helper task: sleep, then post 1 to the eventfd counter.
///
/// `write` wakes `reader_waiters`, which is the set
/// [`super::eventfd::register_waiter`] put us in.
extern "C" fn mw_eventfd_writer_task(raw: u64) {
    sched::sleep_ns_interruptible(20_000_000); // 20ms
    let h = super::eventfd::EventFdHandle::from_raw(raw);
    if super::eventfd::write(h, 1).is_ok() {
        MW_WROTE.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Helper task: sleep, then send one byte on a socketpair endpoint.
///
/// `raw` is the *sending* endpoint; the wait is on its peer. Which end owns the
/// woken set is the fact [`super::stream_socket::register_waiter`]'s doc
/// records: a send on one endpoint wakes the **peer's** `reader_waiters`, so
/// the receiver's own entry is the one that fires.
extern "C" fn mw_socket_sender_task(raw: u64) {
    sched::sleep_ns_interruptible(20_000_000); // 20ms
    let h = super::stream_socket::StreamSocketHandle::from_raw(raw);
    if super::stream_socket::try_send(h, b"x").is_ok() {
        MW_WROTE.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Helper task: sleep, then write one byte from the pty **slave**.
///
/// Slave output lands in the output ring and wakes `output_waiters` — the set a
/// master-side reader waits in, despite "output" naming the ring rather than
/// the end. That asymmetry is exactly why
/// [`crate::tty::pty::register_waiter`] joins both sets regardless of end.
extern "C" fn mw_pty_writer_task(raw: u64) {
    sched::sleep_ns_interruptible(20_000_000); // 20ms
    let h = crate::tty::pty::PtyHandle::from_raw(raw);
    if crate::tty::pty::slave_write(h, b"x").is_ok() {
        MW_WROTE.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Helper task: sleep, then add an fd to an epoll instance's interest set.
///
/// The fd is never opened and never has to be: what is under test is the
/// *interest-set-change* notification, which `ctl_add` raises regardless of
/// whether the fd it names is one this instance could ever resolve. That is
/// also why this task cannot be the pipe writer with a different name — a
/// member becoming ready and the member *list* changing are two different
/// wakes, and only the second one reaches [`WaitTarget::EpollCtl`].
extern "C" fn mw_epoll_ctl_task(raw: u64) {
    sched::sleep_ns_interruptible(20_000_000); // 20ms
    let ep = super::epoll::EpollHandle::from_raw(raw);
    if super::epoll::ctl_add(ep, 7, 0x1, 0).is_ok() {
        MW_WROTE.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Helper task: sleep, then arm a **disarmed** timerfd.
///
/// `settime` is a real wake (it drains `reader_waiters`), so this exercises the
/// half of the timerfd story that registration alone does cover; the expiry
/// that follows exercises the half it does not.
extern "C" fn mw_timerfd_armer_task(raw: u64) {
    sched::sleep_ns_interruptible(20_000_000); // 20ms
    let t = super::timerfd::TimerFdHandle::from_raw(raw);
    // One-shot 5 ms out. Deliberately *not* periodic: a periodic timer would
    // paper over a lost expiry by firing again.
    if super::timerfd::settime(t, false, 5_000_000, 0, false).is_ok() {
        MW_WROTE.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Timed phases: what host load can and cannot explain.
//
// Under TCG the guest clock is the host's wall clock (`boot-test.sh` passes
// neither `-icount` nor `-rtc clock=vm`), so a host too busy to schedule QEMU
// for two seconds moves the guest clock two seconds at once. A wait that was
// released on time then *reads* as late, and a helper task whose 20 ms sleep
// expired in the same jump may not get the CPU before the wait's own timeout
// does. Lane C's boot of 993280872 died that way, 2026-10-05: "timerfd expiry
// returned n=1 after 2003606600ns", the host compiling the toolkit.
//
// A stall like that does not repeat on demand; a lost wake, or a park whose
// cap was dropped, is late on every attempt. So a phase that came back late
// is run again, and only lateness on every one of [`PHASE_ATTEMPTS`] fails --
// the treatment `sched::sleep_ns`'s self-test got for the same cause
// (known-issues-resolved/A-a-timing-self-test-panicked-the-kernel-over-a-
// 988ms-sleep-...). A result host load cannot produce -- a wait that came back
// early, or empty before its own timeout, or ready on the wrong object, or
// ready without its helper having acted -- fails on the attempt that saw it.
// Every attempt reports its number, because passing first time and passing on
// a retry are different facts.
// ---------------------------------------------------------------------------

/// Attempts a timed phase gets before lateness counts as a failure.
const PHASE_ATTEMPTS: u32 = 3;

/// The own timeout of a wait a helper task is meant to release.
const WAKE_TIMEOUT_NS: u64 = 2_000_000_000;

/// How soon such a wait must be released to count as released by the wake
/// rather than by its own timeout. The helper acts after 20 ms.
const WAKE_CEILING_NS: u64 = 1_000_000_000;

/// Scheduling rounds (a yield and a 1 ms sleep each) a phase allows its helper
/// task to finish. Counted in rounds, not guest time, so that a host stall
/// cannot use the allowance up.
const HELPER_JOIN_ROUNDS: u32 = 5_000;

/// How one attempt at a timed phase came out. A failure host load cannot
/// explain is not a variant: it is the `Err` the phase returns, already
/// reported on serial.
enum Attempt {
    /// Behaved as specified; the phase printed its OK line.
    Passed,
    /// Missed only a bound host load can miss; the phase printed what it saw.
    Late,
}

/// Run `phase` until it passes, at most [`PHASE_ATTEMPTS`] times.
fn timed_phase(name: &str, phase: fn(u32) -> KernelResult<Attempt>) -> KernelResult<()> {
    for attempt in 1..=PHASE_ATTEMPTS {
        if let Attempt::Passed = phase(attempt)? {
            return Ok(());
        }
    }
    serial_println!(
        "[multiwait]   FAIL: {name}: late on all {PHASE_ATTEMPTS} attempts (each printed above) -- \
         a host stall does not repeat like that"
    );
    Err(KernelError::InternalError)
}

/// Wait for the helper task `tid` to exit.
///
/// Read a helper's `MW_WROTE` count only after this: the helper counts *after*
/// its action, so a wait its action released can return before the count
/// moves. It also keeps a slow helper from one attempt acting during the next.
fn join_helper(name: &str, tid: TaskId) -> KernelResult<()> {
    for _ in 0..HELPER_JOIN_ROUNDS {
        match sched::task_state(tid) {
            None | Some(sched::task::TaskState::Dead) => return Ok(()),
            Some(_) => {}
        }
        sched::yield_now();
        sched::sleep_ms(1);
    }
    serial_println!(
        "[multiwait]   FAIL: {name}: the helper task never finished (state {:?})",
        sched::task_state(tid)
    );
    Err(KernelError::InternalError)
}

/// Judge one attempt at a wait that a helper's action should release: `n`
/// ready objects after `elapsed` ns, with the helper's action counted `acted`
/// times. `ok` describes the phase for its OK line.
fn judge_wake(
    name: &str,
    ok: &str,
    attempt: u32,
    n: usize,
    acted: u32,
    elapsed: u64,
) -> KernelResult<Attempt> {
    if n == 1 && acted == 1 && elapsed <= WAKE_CEILING_NS {
        serial_println!(
            "[multiwait]   {ok} ({elapsed}ns, attempt {attempt} of {PHASE_ATTEMPTS}): OK"
        );
        return Ok(Attempt::Passed);
    }
    // Released by the helper but read late, or timed out empty: the helper may
    // not have had the CPU in time. A lost wake looks the same once -- and
    // then again on every attempt, which is what fails it.
    if (n == 1 && acted == 1) || (n == 0 && elapsed >= WAKE_TIMEOUT_NS) {
        serial_println!(
            "[multiwait]   {name}: attempt {attempt} of {PHASE_ATTEMPTS} late -- n={n} after \
             {elapsed}ns (helper acted: {acted})"
        );
        return Ok(Attempt::Late);
    }
    // Ready without the helper's action, empty before its own timeout, or more
    // ready than there are objects.
    serial_println!(
        "[multiwait]   FAIL: {name} returned n={n} after {elapsed}ns (helper acted: {acted})"
    );
    Err(KernelError::InternalError)
}

/// Boot self-test for multi-object waiting.
///
/// # Why this is not part of the early deterministic-init phase
///
/// Every phase below either parks a task or arms an `hrtimer`, and neither
/// works before `hrtimer::init()` and `sti()`: with the APIC timer ISR not yet
/// running, [`crate::hrtimer::process_expired`] is never called, so a park with
/// a deadline would never end. This test therefore runs from the boot path
/// after interrupts are live, exactly like
/// [`super::timerfd::self_test_blocking_multi_waiter`].
///
/// # Errors
///
/// [`KernelError::InternalError`] if any phase does not behave as described.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[multiwait] Running multi-object wait self-test...");

    timed_phase("ready pipe", phase_ready_object)?;
    timed_phase("poll-only timeout", phase_poll_only_timeout)?;
    timed_phase("pipe wake", phase_pipe_wake)?;
    timed_phase("timerfd expiry", phase_timerfd_expiry)?;
    // Phases 5-7: the other three blockable families each release a park.
    // Phases 3 and 4 proved the mechanism on a pipe and on a timerfd; these
    // three prove the *fan-out* -- that each family's `register_waiter` joins a
    // set that family's own wakes actually drain. Each is the same shape as
    // phase 3 (helper acts after 20 ms, so the wait is genuinely parked when
    // the wake arrives) and each is currently the only test its pair has, since
    // nothing outside this module calls them yet.
    timed_phase("eventfd wake", phase_eventfd_wake)?;
    timed_phase("socketpair wake", phase_socketpair_wake)?;
    timed_phase("pty wake", phase_pty_wake)?;
    phase_registration_swept()?;
    timed_phase("mixed set", phase_mixed_set)?;
    timed_phase("PollOnly cap", phase_poll_only_caps)?;
    timed_phase("epoll_ctl wake", phase_epoll_ctl_wake)?;

    serial_println!("[multiwait] multiwait::self_test PASSED");
    Ok(())
}

/// Phase 1: a ready object returns immediately, without parking.
fn phase_ready_object(attempt: u32) -> KernelResult<Attempt> {
    let (rh, wh) = super::pipe::create();
    if super::pipe::try_write(wh, b"hello").is_err() {
        serial_println!("[multiwait]   FAIL: could not prime the pipe");
        super::pipe::close(rh);
        super::pipe::close(wh);
        return Err(KernelError::InternalError);
    }
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::Pipe(rh.raw())];
    let r = wait_multiple(&targets, Some(1_000_000_000), || {
        usize::from(super::pipe::poll_status(rh) & 0x01 != 0)
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    super::pipe::close(rh);
    super::pipe::close(wh);
    let n =
        r.inspect_err(|e| serial_println!("[multiwait]   FAIL: ready-pipe wait errored: {e}"))?;
    // The pipe was readable before the wait began, so its first scan must see
    // it: no host stall explains a miss.
    if n != 1 {
        serial_println!("[multiwait]   FAIL: ready pipe returned n={n} after {elapsed}ns");
        return Err(KernelError::InternalError);
    }
    if elapsed > 100_000_000 {
        serial_println!(
            "[multiwait]   ready pipe: attempt {attempt} of {PHASE_ATTEMPTS} late -- returned \
             after {elapsed}ns"
        );
        return Ok(Attempt::Late);
    }
    serial_println!(
        "[multiwait]   Ready object returns without parking (attempt {attempt} of \
         {PHASE_ATTEMPTS}): OK"
    );
    Ok(Attempt::Passed)
}

/// Phase 2: nothing ready, poll-only set, timeout is honoured.
///
/// A poll-only target has no waiter set, so nothing can wake this wait: it
/// must come back from the adaptive backoff on its own, and not before the
/// timeout. Both halves matter -- returning early would be a spurious-wake
/// bug, returning late a backoff that overshoots its own cap.
fn phase_poll_only_timeout(attempt: u32) -> KernelResult<Attempt> {
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::PollOnly];
    let n = wait_multiple(&targets, Some(50_000_000), || 0)?; // holds no handle
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    // Audited against 855's second clause. The LOWER bound is the assertion:
    // a 50 ms timeout that returns in less than 50 ms fired early, which is
    // the defect, and it is exact rather than a margin. The UPPER bound is a
    // catastrophe ceiling at 10x nominal -- it catches a timeout that never
    // fires at all, not one that overshoots, because an overshoot under load
    // says nothing about this code. Do not tighten it toward the nominal: a
    // ceiling close enough to 50 ms to detect a slow wakeup is close enough
    // to be hit by whatever else the host is running, and would then redden
    // the tree while naming the wrong subsystem. Even at 10x a host stall can
    // reach it, which is why it is retried rather than failed outright.
    if n != 0 || elapsed < 50_000_000 {
        serial_println!("[multiwait]   FAIL: poll-only timeout returned n={n} after {elapsed}ns");
        return Err(KernelError::InternalError);
    }
    if elapsed > 500_000_000 {
        serial_println!(
            "[multiwait]   poll-only timeout: attempt {attempt} of {PHASE_ATTEMPTS} late -- \
             returned after {elapsed}ns"
        );
        return Ok(Attempt::Late);
    }
    serial_println!(
        "[multiwait]   Capped path honours the timeout ({elapsed}ns, attempt {attempt} of \
         {PHASE_ATTEMPTS}): OK"
    );
    Ok(Attempt::Passed)
}

/// Phase 3: a blocked wait is released by the object's own wake.
///
/// The discriminating phase. Nothing here is on a timer that could rescue a
/// lost wake: the pipe is the only target, so the park has no cap, and the
/// 2 s timeout is far outside the window the assertion allows.
fn phase_pipe_wake(attempt: u32) -> KernelResult<Attempt> {
    use core::sync::atomic::Ordering::SeqCst;
    MW_WROTE.store(0, SeqCst);
    let (rh, wh) = super::pipe::create();
    let helper = match sched::spawn(b"mw-writer", 16, mw_pipe_writer_task, wh.raw(), 0) {
        Ok(tid) => tid,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: could not spawn the writer task: {e}");
            super::pipe::close(rh);
            super::pipe::close(wh);
            return Err(e);
        }
    };
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::Pipe(rh.raw())];
    let r = wait_multiple(&targets, Some(WAKE_TIMEOUT_NS), || {
        usize::from(super::pipe::poll_status(rh) & 0x01 != 0)
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let joined = join_helper("pipe wake", helper);
    let wrote = MW_WROTE.load(SeqCst);
    super::pipe::close(rh);
    super::pipe::close(wh);
    joined?;
    let n =
        r.inspect_err(|e| serial_println!("[multiwait]   FAIL: pipe-wake wait errored: {e}"))?;
    judge_wake(
        "pipe wake",
        "Parked wait released by a pipe write",
        attempt,
        n,
        wrote,
        elapsed,
    )
}

/// Phase 4: a timerfd expiry ends the wait even though it wakes nobody.
///
/// The §4b hazard in one assertion. The wait starts on a *disarmed* timer
/// (no deadline, so an uncapped indefinite park), is woken by `settime`, and
/// must then bound its next park by `next_deadline_ns` -- because the one-shot
/// expiry 5 ms later broadcasts to no waiter set at all. Drop
/// `next_deadline_ns` and every attempt of this phase runs to its timeout.
fn phase_timerfd_expiry(attempt: u32) -> KernelResult<Attempt> {
    use core::sync::atomic::Ordering::SeqCst;
    MW_WROTE.store(0, SeqCst);
    let t = super::timerfd::create(super::timerfd::CLOCK_MONOTONIC);
    let helper = match sched::spawn(b"mw-armer", 16, mw_timerfd_armer_task, t.raw(), 0) {
        Ok(tid) => tid,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: could not spawn the armer task: {e}");
            super::timerfd::close(t);
            return Err(e);
        }
    };
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::TimerFd(t.raw())];
    let r = wait_multiple(&targets, Some(WAKE_TIMEOUT_NS), || {
        usize::from(super::timerfd::is_readable(t))
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let joined = join_helper("timerfd expiry", helper);
    let armed = MW_WROTE.load(SeqCst);
    super::timerfd::close(t);
    joined?;
    let n = r.inspect_err(|e| {
        serial_println!("[multiwait]   FAIL: timerfd-expiry wait errored: {e}");
    })?;
    judge_wake(
        "timerfd expiry",
        "Silent timerfd expiry ends the wait",
        attempt,
        n,
        armed,
        elapsed,
    )
}

/// Phase 5: an eventfd post releases a park.
fn phase_eventfd_wake(attempt: u32) -> KernelResult<Attempt> {
    use core::sync::atomic::Ordering::SeqCst;
    MW_WROTE.store(0, SeqCst);
    let efd = super::eventfd::create(0);
    let helper = match sched::spawn(b"mw-efd", 16, mw_eventfd_writer_task, efd.raw(), 0) {
        Ok(tid) => tid,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: could not spawn the eventfd writer: {e}");
            super::eventfd::close(efd);
            return Err(e);
        }
    };
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::EventFd(efd.raw())];
    let r = wait_multiple(&targets, Some(WAKE_TIMEOUT_NS), || {
        usize::from(super::eventfd::has_value(efd))
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let joined = join_helper("eventfd wake", helper);
    let wrote = MW_WROTE.load(SeqCst);
    super::eventfd::close(efd);
    joined?;
    let n = r?;
    judge_wake(
        "eventfd wake",
        "Parked wait released by an eventfd post",
        attempt,
        n,
        wrote,
        elapsed,
    )
}

/// Phase 6: a socketpair send releases a park.
///
/// The wait is on `b` and the send is from `a`: a send wakes the *peer's*
/// reader set, so this also checks that `register_waiter` joined the right
/// endpoint's sets rather than the sender's.
fn phase_socketpair_wake(attempt: u32) -> KernelResult<Attempt> {
    use core::sync::atomic::Ordering::SeqCst;
    MW_WROTE.store(0, SeqCst);
    let (sa, sb) = super::stream_socket::create();
    let helper = match sched::spawn(b"mw-sock", 16, mw_socket_sender_task, sa.raw(), 0) {
        Ok(tid) => tid,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: could not spawn the socket sender: {e}");
            super::stream_socket::close(sa);
            super::stream_socket::close(sb);
            return Err(e);
        }
    };
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::StreamSocket(sb.raw())];
    let r = wait_multiple(&targets, Some(WAKE_TIMEOUT_NS), || {
        usize::from(super::stream_socket::readable_bytes(sb) > 0)
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let joined = join_helper("socketpair wake", helper);
    let wrote = MW_WROTE.load(SeqCst);
    super::stream_socket::close(sa);
    super::stream_socket::close(sb);
    joined?;
    let n = r?;
    judge_wake(
        "socketpair wake",
        "Parked wait released by a socketpair send",
        attempt,
        n,
        wrote,
        elapsed,
    )
}

/// Phase 7: a pty slave write releases a park on the master.
///
/// The wait is on the master and the write is from the slave, so the set
/// that fires is `output_waiters` -- named for the ring, not the end. A
/// registration that had guessed "master => input_waiters" would hang here.
fn phase_pty_wake(attempt: u32) -> KernelResult<Attempt> {
    use core::sync::atomic::Ordering::SeqCst;
    MW_WROTE.store(0, SeqCst);
    let (pm, ps) = crate::tty::pty::create()?;
    let helper = match sched::spawn(b"mw-pty", 16, mw_pty_writer_task, ps.raw(), 0) {
        Ok(tid) => tid,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: could not spawn the pty writer: {e}");
            // Hangup tells a caller whether the peer went away; nothing to do
            // with it on a teardown path that is closing both ends anyway.
            let _ = crate::tty::pty::close(pm);
            let _ = crate::tty::pty::close(ps);
            return Err(e);
        }
    };
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::Pty(pm.raw())];
    let r = wait_multiple(&targets, Some(WAKE_TIMEOUT_NS), || {
        usize::from(crate::tty::pty::readable(pm))
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let joined = join_helper("pty wake", helper);
    let wrote = MW_WROTE.load(SeqCst);
    // As above: the hangup indication is meaningless on a closing teardown.
    let _ = crate::tty::pty::close(pm);
    let _ = crate::tty::pty::close(ps);
    joined?;
    let n = r?;
    judge_wake(
        "pty wake",
        "Parked wait released by a pty slave write",
        attempt,
        n,
        wrote,
        elapsed,
    )
}

/// Phase 8: registration leaves nothing behind.
///
/// A leaked entry is invisible from outside the waiter set, so this asserts
/// its *consequence* instead: after a wait over a set of objects has returned,
/// a state change on one of those objects must not disturb the task that
/// waited. If the registration had leaked, the write below would set this
/// task's `pending_wake`, and the very next park -- the 30 ms sleep -- would
/// return at once instead of sleeping.
///
/// Not retried: its one timing assertion is a lower bound, and host load
/// cannot make a sleep return early.
fn phase_registration_swept() -> KernelResult<()> {
    let (rh, wh) = super::pipe::create();
    let targets = [WaitTarget::Pipe(rh.raw()), WaitTarget::PollOnly];
    let n = match wait_multiple(&targets, Some(10_000_000), || 0) {
        Ok(n) => n,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: leak-check wait errored: {e}");
            super::pipe::close(rh);
            super::pipe::close(wh);
            return Err(e);
        }
    };
    if n != 0 {
        serial_println!("[multiwait]   FAIL: empty scan returned n={n}");
        super::pipe::close(rh);
        super::pipe::close(wh);
        return Err(KernelError::InternalError);
    }
    // The wait is over; this write must wake nobody.
    if super::pipe::try_write(wh, b"x").is_err() {
        serial_println!("[multiwait]   FAIL: post-wait write failed");
        super::pipe::close(rh);
        super::pipe::close(wh);
        return Err(KernelError::InternalError);
    }
    let started = crate::hrtimer::now_ns();
    sched::sleep_ns_interruptible(30_000_000);
    let slept = crate::hrtimer::now_ns().saturating_sub(started);
    super::pipe::close(rh);
    super::pipe::close(wh);
    if slept < 25_000_000 {
        serial_println!(
            "[multiwait]   FAIL: post-wait sleep cut short after {slept}ns — a waiter-set entry \
             leaked and its wake set pending_wake"
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[multiwait]   Registration is swept on every exit: OK");
    Ok(())
}

/// Phase 9: a mixed set -- two families in one wait, and the ready one is
/// named.
///
/// Every phase above waits on exactly one *blockable* target (phase 8's
/// second member is a `PollOnly` that never becomes ready), so nothing yet
/// tests the property the whole feature exists for: N objects of different
/// families in one wait. A fan-out that only took effect for the last member
/// of `targets` would pass phases 1-8 and hang here.
///
/// The second member is a **disarmed** timerfd, chosen so it contributes no
/// deadline: if the pipe's own wake is lost, there is no timer quietly
/// standing by to rescue the park, and the 2 s timeout is far outside the
/// window the assertion allows.
///
/// Readiness is computed through `revents_for_handle` -- the same fifteen-arm
/// table `poll` and `SYS_WAIT_MULTIPLE` share -- masked by `POLLIN` exactly as
/// the syscall masks by the caller's `events`. A bespoke scan here would test
/// the test; this way the phase also pins that the shared table answers for a
/// *native* handle, which is how `SYS_WAIT_MULTIPLE` calls it.
fn phase_mixed_set(attempt: u32) -> KernelResult<Attempt> {
    use core::sync::atomic::Ordering::SeqCst;
    MW_WROTE.store(0, SeqCst);
    let (rh, wh) = super::pipe::create();
    let t = super::timerfd::create(super::timerfd::CLOCK_MONOTONIC);
    let helper = match sched::spawn(b"mw-mixed", 16, mw_pipe_writer_task, wh.raw(), 0) {
        Ok(tid) => tid,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: could not spawn the mixed-set writer: {e}");
            super::pipe::close(rh);
            super::pipe::close(wh);
            super::timerfd::close(t);
            return Err(e);
        }
    };
    let mut pipe_revents = 0u16;
    let mut timer_revents = 0u16;
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::Pipe(rh.raw()), WaitTarget::TimerFd(t.raw())];
    let r = wait_multiple(&targets, Some(WAKE_TIMEOUT_NS), || {
        use crate::proc::linux_fd::HandleKind;
        use crate::syscall::linux::{poll_bits, revents_for_handle};
        // POLLERR/POLLHUP are unrequestable and always reported, per poll(2);
        // POLLOUT is masked out because a pipe read end is writable-adjacent
        // enough to be permanently "ready" and would defeat the park.
        let mask = poll_bits::POLLIN | poll_bits::POLLERR | poll_bits::POLLHUP;
        pipe_revents = revents_for_handle(HandleKind::Pipe, rh.raw(), 0, None) & mask;
        timer_revents = revents_for_handle(HandleKind::Timerfd, t.raw(), 0, None) & mask;
        usize::from(pipe_revents != 0).saturating_add(usize::from(timer_revents != 0))
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let joined = join_helper("mixed set", helper);
    let wrote = MW_WROTE.load(SeqCst);
    super::pipe::close(rh);
    super::pipe::close(wh);
    super::timerfd::close(t);
    joined?;
    let n = r?;
    // Which object is ready is not a timing question: a wait released with
    // the wrong one named is wrong however busy the host was.
    if n == 1
        && (pipe_revents & crate::syscall::linux::poll_bits::POLLIN == 0 || timer_revents != 0)
    {
        serial_println!(
            "[multiwait]   FAIL: mixed set returned n={n} after {elapsed}ns (writer ran: {wrote}, \
             pipe revents {pipe_revents:#x}, timer revents {timer_revents:#x})"
        );
        return Err(KernelError::InternalError);
    }
    judge_wake(
        "mixed set",
        "Mixed pipe+timerfd set wakes on the pipe alone",
        attempt,
        n,
        wrote,
        elapsed,
    )
}

/// Phase 10: a `PollOnly` member caps the whole wait.
///
/// This is the row a TCP socket lands in -- testable but not blockable -- and
/// the reason readiness and blockability are two dispatches rather than one.
/// The assertion has to be a *contrast*, because "it eventually returned" is
/// true either way and would not prove the backoff ran at all: the same
/// readiness change, one that announces itself to no waiter set, is seen
/// promptly when a `PollOnly` is in the set and only at the timeout when it
/// is not.
///
/// The blockable member is a pipe nobody ever writes to. It is there so the
/// two waits differ in exactly one thing -- the presence of the `PollOnly` --
/// rather than in whether they had anything to register on at all.
fn phase_poll_only_caps(attempt: u32) -> KernelResult<Attempt> {
    const CAPPED_TIMEOUT_NS: u64 = 300_000_000;
    let (rh, wh) = super::pipe::create();

    let silent_at = crate::hrtimer::now_ns().saturating_add(40_000_000);
    let started = crate::hrtimer::now_ns();
    let capped = wait_multiple(
        &[WaitTarget::Pipe(rh.raw()), WaitTarget::PollOnly],
        Some(CAPPED_TIMEOUT_NS),
        || usize::from(crate::hrtimer::now_ns() >= silent_at),
    );
    let capped_elapsed = crate::hrtimer::now_ns().saturating_sub(started);

    let silent_at = crate::hrtimer::now_ns().saturating_add(40_000_000);
    let started = crate::hrtimer::now_ns();
    let uncapped = wait_multiple(
        &[WaitTarget::Pipe(rh.raw())],
        Some(CAPPED_TIMEOUT_NS),
        || usize::from(crate::hrtimer::now_ns() >= silent_at),
    );
    let uncapped_elapsed = crate::hrtimer::now_ns().saturating_sub(started);

    super::pipe::close(rh);
    super::pipe::close(wh);
    let capped_n = capped?;
    uncapped?;
    // A wait with nothing to wake it that comes back before its timeout is
    // the defect this contrast exists to rule out; load cannot cause it.
    if uncapped_elapsed < 250_000_000 {
        serial_println!(
            "[multiwait]   FAIL: blockable-only wait returned after {uncapped_elapsed}ns — it \
             should have slept to its timeout, so the contrast in this phase proves nothing"
        );
        return Err(KernelError::InternalError);
    }
    // 40 ms until ready plus at most one 20 ms backoff step, against a 300 ms
    // timeout: the two outcomes are an order of magnitude apart, so the bounds
    // do not need to be tight to be decisive.
    if capped_n == 1 && capped_elapsed < 200_000_000 {
        serial_println!(
            "[multiwait]   PollOnly member caps the wait ({capped_elapsed}ns vs \
             {uncapped_elapsed}ns without one, attempt {attempt} of {PHASE_ATTEMPTS}): OK"
        );
        return Ok(Attempt::Passed);
    }
    if capped_n == 1 || (capped_n == 0 && capped_elapsed >= CAPPED_TIMEOUT_NS) {
        serial_println!(
            "[multiwait]   PollOnly cap: attempt {attempt} of {PHASE_ATTEMPTS} late -- capped wait \
             returned n={capped_n} after {capped_elapsed}ns"
        );
        return Ok(Attempt::Late);
    }
    serial_println!(
        "[multiwait]   FAIL: capped wait returned n={capped_n} after {capped_elapsed}ns — a \
         PollOnly member did not put the wait on the backoff path"
    );
    Err(KernelError::InternalError)
}

/// Phase 11: an `epoll_ctl` releases a park on `WaitTarget::EpollCtl`.
///
/// The one target whose wake does not mean "something you are waiting for is
/// ready" but "the set of things you are waiting for has changed". It is what
/// stops `epoll_wait` blocking on a target list another thread has already
/// invalidated, and nothing else in the tree exercises it: `epoll`'s own
/// self-test can assert that the generation counter *moves*, but not that a
/// task parked on the instance is actually woken when it does.
///
/// Same discriminating shape as phase 3 -- `EpollCtl` is the only target, so
/// the park is uncapped and no timer can rescue a lost wake, and the 2 s
/// timeout is far outside the window the assertion allows. The closure is
/// deliberately the same generation comparison `epoll_wait_core` uses, so a
/// wake that fired without the counter moving would still fail here.
fn phase_epoll_ctl_wake(attempt: u32) -> KernelResult<Attempt> {
    use core::sync::atomic::Ordering::SeqCst;
    MW_WROTE.store(0, SeqCst);
    let ep = super::epoll::create();
    let Some(g0) = super::epoll::generation(ep) else {
        serial_println!(
            "[multiwait]   FAIL: a freshly created epoll instance reports no generation"
        );
        super::epoll::close(ep);
        return Err(KernelError::InternalError);
    };
    let helper = match sched::spawn(b"mw-epctl", 16, mw_epoll_ctl_task, ep.raw(), 0) {
        Ok(tid) => tid,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: could not spawn the epoll_ctl task: {e}");
            super::epoll::close(ep);
            return Err(e);
        }
    };
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::EpollCtl(ep.raw())];
    let r = wait_multiple(&targets, Some(WAKE_TIMEOUT_NS), || {
        usize::from(super::epoll::generation(ep) != Some(g0))
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let joined = join_helper("epoll_ctl wake", helper);
    let ctled = MW_WROTE.load(SeqCst);
    super::epoll::close(ep);
    joined?;
    let n =
        r.inspect_err(|e| serial_println!("[multiwait]   FAIL: epoll-ctl wait errored: {e}"))?;
    judge_wake(
        "epoll_ctl wake",
        "Parked wait released by an epoll_ctl",
        attempt,
        n,
        ctled,
        elapsed,
    )
}
