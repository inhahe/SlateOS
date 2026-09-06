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
//! the daemon-backed `net::socket`, DRM, evdev, signalfd, pidfd, epoll, plain
//! files, the console — has no way to push readiness into the kernel, and is
//! represented by [`WaitTarget::PollOnly`].
//!
//! | set contents | behaviour |
//! |---|---|
//! | every item blockable (pipe, eventfd, socketpair, timerfd, pty) | true block: woken by the object, zero wakeups while idle |
//! | any item poll-only | park capped at an adaptive backoff, re-scanning on each wake |
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
        park_interruptible(pid, task);
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

/// Set by [`mw_pipe_writer_task`] once it has written, so a failing run can
/// tell "the writer never ran" from "the writer ran and the wake was lost".
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
    let _ = super::timerfd::settime(t, false, 5_000_000, 0, false);
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
#[allow(clippy::too_many_lines)]
pub fn self_test() -> KernelResult<()> {
    use core::sync::atomic::Ordering::SeqCst;

    serial_println!("[multiwait] Running multi-object wait self-test...");

    // --- Phase 1: a ready object returns immediately, without parking. ---
    let (rh, wh) = super::pipe::create();
    if super::pipe::try_write(wh, b"hello").is_err() {
        serial_println!("[multiwait]   FAIL: could not prime the pipe");
        super::pipe::close(rh);
        super::pipe::close(wh);
        return Err(KernelError::InternalError);
    }
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::Pipe(rh.raw())];
    let n = match wait_multiple(&targets, Some(1_000_000_000), || {
        usize::from(super::pipe::poll_status(rh) & 0x01 != 0)
    }) {
        Ok(n) => n,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: ready-pipe wait errored: {e}");
            super::pipe::close(rh);
            super::pipe::close(wh);
            return Err(e);
        }
    };
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    if n != 1 || elapsed > 100_000_000 {
        serial_println!("[multiwait]   FAIL: ready pipe returned n={n} after {elapsed}ns");
        super::pipe::close(rh);
        super::pipe::close(wh);
        return Err(KernelError::InternalError);
    }
    super::pipe::close(rh);
    super::pipe::close(wh);
    serial_println!("[multiwait]   Ready object returns without parking: OK");

    // --- Phase 2: nothing ready, poll-only set, timeout is honoured. ---
    // A poll-only target has no waiter set, so nothing can wake this wait: it
    // must come back from the adaptive backoff on its own, and not before the
    // timeout. Both halves matter — returning early would be a spurious-wake
    // bug, returning late a backoff that overshoots its own cap.
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::PollOnly];
    let n = wait_multiple(&targets, Some(50_000_000), || 0)?; // holds no handle
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    if n != 0 || elapsed < 50_000_000 || elapsed > 500_000_000 {
        serial_println!("[multiwait]   FAIL: poll-only timeout returned n={n} after {elapsed}ns");
        return Err(KernelError::InternalError);
    }
    serial_println!("[multiwait]   Capped path honours the timeout ({elapsed}ns): OK");

    // --- Phase 3: a blocked wait is released by the object's own wake. ---
    // The discriminating phase. Nothing here is on a timer that could rescue a
    // lost wake: the pipe is the only target, so the park has no cap, and the
    // 2 s timeout is far outside the window the assertion allows.
    MW_WROTE.store(0, SeqCst);
    let (rh, wh) = super::pipe::create();
    if let Err(e) = sched::spawn(b"mw-writer", 16, mw_pipe_writer_task, wh.raw(), 0) {
        serial_println!("[multiwait]   FAIL: could not spawn the writer task: {e}");
        super::pipe::close(rh);
        super::pipe::close(wh);
        return Err(e);
    }
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::Pipe(rh.raw())];
    let n = match wait_multiple(&targets, Some(2_000_000_000), || {
        usize::from(super::pipe::poll_status(rh) & 0x01 != 0)
    }) {
        Ok(n) => n,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: pipe-wake wait errored: {e}");
            super::pipe::close(rh);
            super::pipe::close(wh);
            return Err(e);
        }
    };
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let wrote = MW_WROTE.load(SeqCst);
    if n != 1 || wrote != 1 || elapsed > 1_000_000_000 {
        serial_println!(
            "[multiwait]   FAIL: pipe wake returned n={n} after {elapsed}ns (writer ran: {wrote})"
        );
        super::pipe::close(rh);
        super::pipe::close(wh);
        return Err(KernelError::InternalError);
    }
    super::pipe::close(rh);
    super::pipe::close(wh);
    serial_println!("[multiwait]   Parked wait released by a pipe write ({elapsed}ns): OK");

    // --- Phase 4: a timerfd expiry ends the wait even though it wakes nobody.
    // The §4b hazard in one assertion. The wait starts on a *disarmed* timer
    // (no deadline, so an uncapped indefinite park), is woken by `settime`,
    // and must then bound its next park by `next_deadline_ns` — because the
    // one-shot expiry 5 ms later broadcasts to no waiter set at all. Drop
    // `next_deadline_ns` and this phase hangs until its timeout.
    let t = super::timerfd::create(super::timerfd::CLOCK_MONOTONIC);
    if let Err(e) = sched::spawn(b"mw-armer", 16, mw_timerfd_armer_task, t.raw(), 0) {
        serial_println!("[multiwait]   FAIL: could not spawn the armer task: {e}");
        super::timerfd::close(t);
        return Err(e);
    }
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::TimerFd(t.raw())];
    let n = match wait_multiple(&targets, Some(2_000_000_000), || {
        usize::from(super::timerfd::is_readable(t))
    }) {
        Ok(n) => n,
        Err(e) => {
            serial_println!("[multiwait]   FAIL: timerfd-expiry wait errored: {e}");
            super::timerfd::close(t);
            return Err(e);
        }
    };
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    if n != 1 || elapsed > 1_000_000_000 {
        serial_println!("[multiwait]   FAIL: timerfd expiry returned n={n} after {elapsed}ns");
        super::timerfd::close(t);
        return Err(KernelError::InternalError);
    }
    super::timerfd::close(t);
    serial_println!("[multiwait]   Silent timerfd expiry ends the wait ({elapsed}ns): OK");

    // --- Phases 5-7: the other three blockable families each release a park.
    // Phases 3 and 4 proved the mechanism on a pipe and on a timerfd; these
    // three prove the *fan-out* — that each family's `register_waiter` joins a
    // set that family's own wakes actually drain. Each is the same shape as
    // phase 3 (helper acts after 20 ms, so the wait is genuinely parked when
    // the wake arrives) and each is currently the only test its pair has, since
    // nothing outside this module calls them yet.

    // --- Phase 5: eventfd. ---
    MW_WROTE.store(0, SeqCst);
    let efd = super::eventfd::create(0);
    if let Err(e) = sched::spawn(b"mw-efd", 16, mw_eventfd_writer_task, efd.raw(), 0) {
        serial_println!("[multiwait]   FAIL: could not spawn the eventfd writer: {e}");
        super::eventfd::close(efd);
        return Err(e);
    }
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::EventFd(efd.raw())];
    let r = wait_multiple(&targets, Some(2_000_000_000), || {
        usize::from(super::eventfd::has_value(efd))
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let wrote = MW_WROTE.load(SeqCst);
    super::eventfd::close(efd);
    let n = r?;
    if n != 1 || wrote != 1 || elapsed > 1_000_000_000 {
        serial_println!(
            "[multiwait]   FAIL: eventfd wake returned n={n} after {elapsed}ns (writer ran: {wrote})"
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[multiwait]   Parked wait released by an eventfd post ({elapsed}ns): OK");

    // --- Phase 6: socketpair. ---
    // The wait is on `b` and the send is from `a`: a send wakes the *peer's*
    // reader set, so this also checks that `register_waiter` joined the right
    // endpoint's sets rather than the sender's.
    MW_WROTE.store(0, SeqCst);
    let (sa, sb) = super::stream_socket::create();
    if let Err(e) = sched::spawn(b"mw-sock", 16, mw_socket_sender_task, sa.raw(), 0) {
        serial_println!("[multiwait]   FAIL: could not spawn the socket sender: {e}");
        super::stream_socket::close(sa);
        super::stream_socket::close(sb);
        return Err(e);
    }
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::StreamSocket(sb.raw())];
    let r = wait_multiple(&targets, Some(2_000_000_000), || {
        usize::from(super::stream_socket::readable_bytes(sb) > 0)
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let wrote = MW_WROTE.load(SeqCst);
    super::stream_socket::close(sa);
    super::stream_socket::close(sb);
    let n = r?;
    if n != 1 || wrote != 1 || elapsed > 1_000_000_000 {
        serial_println!(
            "[multiwait]   FAIL: socketpair wake returned n={n} after {elapsed}ns (sender ran: \
             {wrote})"
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[multiwait]   Parked wait released by a socketpair send ({elapsed}ns): OK");

    // --- Phase 7: pty. ---
    // The wait is on the master and the write is from the slave, so the set
    // that fires is `output_waiters` — named for the ring, not the end. A
    // registration that had guessed "master ⇒ input_waiters" would hang here.
    MW_WROTE.store(0, SeqCst);
    let (pm, ps) = crate::tty::pty::create()?;
    if let Err(e) = sched::spawn(b"mw-pty", 16, mw_pty_writer_task, ps.raw(), 0) {
        serial_println!("[multiwait]   FAIL: could not spawn the pty writer: {e}");
        // Hangup tells a caller whether the peer went away; nothing to do with
        // it on a teardown path that is closing both ends anyway.
        let _ = crate::tty::pty::close(pm);
        let _ = crate::tty::pty::close(ps);
        return Err(e);
    }
    let started = crate::hrtimer::now_ns();
    let targets = [WaitTarget::Pty(pm.raw())];
    let r = wait_multiple(&targets, Some(2_000_000_000), || {
        usize::from(crate::tty::pty::readable(pm))
    });
    let elapsed = crate::hrtimer::now_ns().saturating_sub(started);
    let wrote = MW_WROTE.load(SeqCst);
    let _ = crate::tty::pty::close(pm);
    let _ = crate::tty::pty::close(ps);
    let n = r?;
    if n != 1 || wrote != 1 || elapsed > 1_000_000_000 {
        serial_println!(
            "[multiwait]   FAIL: pty wake returned n={n} after {elapsed}ns (writer ran: {wrote})"
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[multiwait]   Parked wait released by a pty slave write ({elapsed}ns): OK");

    // --- Phase 8: registration leaves nothing behind. ---
    // A leaked entry is invisible from outside the waiter set, so this asserts
    // its *consequence* instead: after a wait over a set of objects has
    // returned, a state change on one of those objects must not disturb the
    // task that waited. If the registration had leaked, the write below would
    // set this task's `pending_wake`, and the very next park — the 30 ms sleep —
    // would return at once instead of sleeping.
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

    serial_println!("[multiwait] multiwait::self_test PASSED");
    Ok(())
}
