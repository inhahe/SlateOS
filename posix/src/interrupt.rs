//! What a signal does to a call that is waiting inside this library.
//!
//! Some calls wait inside the C library rather than in the kernel: `sem_wait`
//! and its timed forms, the message-queue calls, System V's `msgsnd`,
//! `msgrcv`, `semop` and `semtimedop`, `io_getevents`, `aio_suspend`,
//! `gai_suspend`, and Linux's `futex(FUTEX_WAIT)`.  Each sleeps in a futex
//! wait ([`crate::lowlevellock`]), and when a signal comes for the process
//! the kernel ends that wait early: `SYS_FUTEX_WAIT` answers
//! `KernelError::Interrupted`.  The call must then decide whether it has
//! been interrupted -- whether it returns `EINTR` (`gai_suspend`:
//! `EAI_INTR`) or goes back to sleep.  Until 2026-09-30 none of them asked
//! and every one went back to sleep; Linux's `futex()` answered `EINTR`
//! instead, for every signal alike (known-issues
//! `D-POSIX-FUTEX-WAITS-DISCARD-EINTR`).
//!
//! ## Why the kernel's answer is not enough
//!
//! A native process's signal dispositions are this library's table
//! ([`crate::signal`]).  The kernel knows only that a trampoline is
//! registered, and hands it every catchable signal, so it ends the wait for
//! signals that must not end the call:
//!
//! - one this library ignores, by `SIG_IGN` or by default (`SIGCHLD`,
//!   `SIGWINCH`, `SIGURG`) -- "Signals that are ignored shall not affect the
//!   behavior of any function" (POSIX, XSH 2.4.4);
//! - one whose default is to stop the process: the call carries on when the
//!   process is continued;
//! - for a timed wait, one another thread takes: the kernel ends such a wait
//!   while any deliverable signal is pending for the process, and a handler
//!   on another thread interrupts that thread's call, not this one's.
//!
//! Nor can the kernel see `SA_RESTART`, which for some of these calls
//! decides whether a handler interrupts them at all.
//!
//! ## How a wait learns what the signal did
//!
//! The trampoline's dispatch runs on the thread whose wait the kernel ended
//! -- the kernel delivers as that thread returns to user mode -- and just
//! before it calls a handler it counts it, in the thread's
//! [`PerThread`](crate::perthread::PerThread) block: every handler, and
//! separately the ones installed without `SA_RESTART` ([`note_handler`]).  A
//! wait takes a [`Mark`] of both counts as it goes to sleep; when the kernel
//! says it was interrupted, the counts say by what.  If neither moved, no
//! handler ran on this thread, and the wait goes on.
//!
//! Counts rather than a flag that is set and cleared, because a handler may
//! itself wait -- on a contended lock, say -- and its wait must not disturb
//! the mark of the wait it interrupted.  They are compared for inequality,
//! so wrapping would matter only after 2^32 handlers in one sleep.
//!
//! ## Which handlers end which calls
//!
//! glibc's answers on Linux -- `interrupt_oracle.txt`, from
//! `posix/tools/oracle/interrupt_harness.py`, which the tests replay -- and
//! they are Linux's kernel's rules, which glibc passes on.  A handler
//! installed without `SA_RESTART` ends every one of these calls; one
//! installed with it divides them in two ([`Restart`]):
//!
//! | rule | calls | a handler installed with `SA_RESTART` |
//! |---|---|---|
//! | [`Restart::IfAsked`] | `sem_wait`; `mq_send`, `mq_receive` and their timed forms; `aio_suspend`, `gai_suspend` and `futex(FUTEX_WAIT)` without a timeout | lets the call wait on |
//! | [`Restart::Never`] | `sem_timedwait`, `sem_clockwait`; `msgsnd`, `msgrcv`, `semop`, `semtimedop`; `io_getevents`; `aio_suspend`, `gai_suspend` and `futex(FUTEX_WAIT)` with a timeout | ends it too |
//!
//! The first row is what Linux's kernel restarts (`ERESTARTSYS`): an
//! untimed futex wait, and the message queues.  The second it does not: a
//! timed futex wait ends in `ERESTART_RESTARTBLOCK`, which any handler turns
//! into `EINTR`, and signal(7) lists the System V calls and `io_getevents`
//! among those "never restarted after being interrupted by a signal
//! handler".  POSIX's `SA_RESTART` would restart the second row too; this
//! follows Linux, whose programs count on those `EINTR`s (design-decisions
//! §1156).
//!
//! ## The calls the kernel sleeps in, and the library's loops
//!
//! A native call the kernel itself blocks in -- `read` on a pipe or the
//! terminal, `write` to a full pipe, `waitpid` -- comes back `Interrupted`
//! for every signal the trampoline takes, ignored ones included, and
//! whatever the handler's `SA_RESTART`.  [`restarting`] issues it again
//! unless a handler that ends it ran here: what Linux's kernel does with its
//! restart sentinels.  The loops the library builds from the kernel's
//! non-blocking calls -- `poll`, `select`, `epoll_wait`, the socket waits,
//! `flock`, the timerfd and inotify reads -- sleep their slices in
//! [`crate::lowlevellock::nap`], which a signal ends at once, and ask their
//! call's mark after each look; the sleeps sleep in
//! [`crate::lowlevellock::sleep_until`] (design-decisions §1157).  The rules
//! are signal(7)'s, and glibc's answers bear them out: `read`, `write`,
//! `accept`, the waits for a child and `flock` restart under `SA_RESTART`;
//! `poll` and its family, the sleeps, `pause`, `sigsuspend`, and a socket
//! call with a timeout end for any handler.
//!
//! The calls POSIX forbids to answer `EINTR` -- `pthread_mutex_lock`,
//! `pthread_cond_wait` and the other thread functions -- wait in
//! [`crate::lowlevellock::futex_wait`], which does not ask; so do
//! `getaddrinfo_a(GAI_WAIT)`, which no signal ends in glibc either, and every
//! lock taken inside the calls above.

use core::sync::atomic::{AtomicU32, Ordering};

/// Whether a signal handler installed with `SA_RESTART` lets a call wait on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Restart {
    /// It does: only a handler installed without `SA_RESTART` ends the call
    /// -- Linux's `ERESTARTSYS`.
    IfAsked,
    /// It does not: any handler that runs on the thread ends the call --
    /// Linux's `ERESTART_RESTARTBLOCK`, `ERESTARTNOHAND` and `-EINTR`.
    Never,
}

/// How many signal handlers this thread had run when a wait began: every
/// one, and those installed without `SA_RESTART`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    all: u32,
    without_restart: u32,
}

impl Mark {
    /// This thread's counts now.
    #[must_use]
    pub(crate) fn now() -> Self {
        let block = crate::perthread::current();
        // SAFETY: `block` is this thread's own block, valid until the thread
        // exits; the two fields are `u32`s that are only ever accessed
        // atomically ([`counter`]).
        unsafe {
            Self {
                all: counter(&raw mut (*block).handlers_run).load(Ordering::Relaxed),
                without_restart: counter(&raw mut (*block).handlers_run_without_restart)
                    .load(Ordering::Relaxed),
            }
        }
    }

    /// Whether a handler that has run on this thread since the mark ends a
    /// call that follows `restart`.
    #[must_use]
    pub(crate) fn interrupted(self, restart: Restart) -> bool {
        let now = Self::now();
        match restart {
            Restart::IfAsked => now.without_restart != self.without_restart,
            Restart::Never => now.all != self.all,
        }
    }
}

/// Count a signal handler that is about to run on this thread, and whether
/// it was installed with `SA_RESTART`.
///
/// Called by the signal dispatch just before it calls the handler, not
/// after, so that a handler that leaves by `longjmp` still counts.
pub(crate) fn note_handler(sa_restart: bool) {
    let block = crate::perthread::current();
    // SAFETY: as in `Mark::now`.  `fetch_add` because the dispatch may itself
    // be interrupted by the delivery of another signal to this thread, and a
    // plain read-modify-write would lose that one's count.
    unsafe {
        counter(&raw mut (*block).handlers_run).fetch_add(1, Ordering::Relaxed);
        if !sa_restart {
            counter(&raw mut (*block).handlers_run_without_restart).fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// One of the block's handler counts, as the atomic every access to it is.
///
/// # Safety
///
/// `p` points at one of the calling thread's own counts, which is valid for
/// as long as the returned reference is used and is never accessed other
/// than atomically.
unsafe fn counter<'a>(p: *mut u32) -> &'a AtomicU32 {
    // SAFETY: a `u32` field of the `repr(C)` block is 4-aligned, as
    // `AtomicU32` requires; the rest is the caller's contract.  Only this
    // thread, and the handlers that run on it, ever touch the counts.
    unsafe { AtomicU32::from_ptr(p) }
}

/// Issue `call` -- a system call the kernel ends for a signal -- again for
/// as long as it comes back `KernelError::Interrupted` without a handler
/// having run on this thread that ends a call following `restart`: what
/// Linux's kernel does with a call it ended in `ERESTARTSYS`, and with any
/// call it ended when no handler ran.  Its answer otherwise, which is
/// `Interrupted` when a handler ended it.
///
/// Each issue counts handlers afresh, as each of Linux's restarts is the
/// system call begun again.
pub(crate) fn restarting(restart: Restart, mut call: impl FnMut() -> i64) -> i64 {
    loop {
        let mark = Mark::now();
        let answer = call();
        if answer != crate::errno::native::INTERRUPTED || mark.interrupted(restart) {
            return answer;
        }
    }
}

/// A stand-in for the kernel, for the host tests: what this thread's futex
/// waits meet.
///
/// The host has no kernel futex and no signals to send; a futex wait there
/// yields and returns ([`crate::lowlevellock`]).  A test scripts its thread's
/// next waits instead -- the kernel ending one for a signal the trampoline
/// then dispatches, or for one another thread takes, or a wake that brings
/// what the call waits for -- so that a call can be followed through an
/// interruption on one thread, with no timing in it.
#[cfg(all(test, not(target_os = "none")))]
pub(crate) mod script {
    use std::boxed::Box;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// What one futex wait meets.
    pub(crate) enum Step {
        /// The kernel ends the wait for signal `sig`, and the trampoline
        /// dispatches it on this thread.
        Signal(i32),
        /// The kernel ends the wait for a signal another thread takes:
        /// nothing runs here, and the closure stands in for what runs there.
        SignalElsewhere(Box<dyn FnOnce()>),
        /// The wait ends as a wake ends it, after the closure -- standing in
        /// for another thread -- brings what the call waits for.
        Wake(Box<dyn FnOnce()>),
    }

    std::thread_local! {
        static STEPS: RefCell<VecDeque<Step>> = const { RefCell::new(VecDeque::new()) };
    }

    /// Script this thread's next futex waits.  A wait with no step left is
    /// the host's plain one: it yields, and ends as a wake ends it.
    pub(crate) fn set(steps: impl IntoIterator<Item = Step>) {
        STEPS.with(|s| *s.borrow_mut() = steps.into_iter().collect());
    }

    /// Whether no step is scripted: a wait for a signal on the host could
    /// then never end (`signal::wait_for_handler`).
    pub(crate) fn is_empty() -> bool {
        STEPS.with(|s| s.borrow().is_empty())
    }

    /// How many scripted steps no wait met; they are dropped.
    pub(crate) fn clear() -> usize {
        STEPS.with(|s| {
            let mut steps = s.borrow_mut();
            let left = steps.len();
            steps.clear();
            left
        })
    }

    /// This thread's next scripted wait: whether the kernel says it was
    /// interrupted, or `None` when no step is left.
    pub(crate) fn next() -> Option<bool> {
        // Taken out before it runs: what it runs may wait itself.
        let step = STEPS.with(|s| s.borrow_mut().pop_front())?;
        Some(match step {
            Step::Signal(sig) => {
                crate::signal::dispatch_delivered(sig);
                true
            }
            Step::SignalElsewhere(elsewhere) => {
                elsewhere();
                true
            }
            Step::Wake(bring) => {
                bring();
                false
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::script::{self, Step};
    use super::*;
    use crate::errno;
    use crate::lowlevellock::{Waited, futex_wait_interruptible, futex_wait_interruptible_since};
    use crate::signal::{
        SA_RESTART, SIG_DFL, SIG_IGN, SIGURG, SIGUSR1, Sigaction, SighandlerT, SigsetT, raise,
        sigaction,
    };
    use crate::stat::Timespec;
    use crate::time::{CLOCK_MONOTONIC, CLOCK_REALTIME};
    use core::cell::Cell;
    use core::ptr::{null, null_mut};
    use core::sync::atomic::{AtomicBool, AtomicI32};
    use std::boxed::Box;
    use std::format;
    use std::string::{String, ToString};

    std::thread_local! {
        /// How often [`counted`] has run on this thread.
        static RAN: Cell<u32> = const { Cell::new(0) };
    }

    extern "C" fn counted(_sig: i32) {
        RAN.with(|r| r.set(r.get() + 1));
    }

    fn act(handler: SighandlerT, flags: u32) -> Sigaction {
        Sigaction {
            sa_handler: handler,
            sa_mask: SigsetT::EMPTY,
            sa_flags: flags,
            sa_restorer: 0,
        }
    }

    /// `sig`'s disposition on this thread (the host's tables are the
    /// thread's own).
    fn install(sig: i32, a: &Sigaction) {
        // SAFETY: `a` is a valid action; the old one is not wanted.
        assert_eq!(unsafe { sigaction(sig, a, null_mut()) }, 0);
    }

    fn defaults() {
        install(SIGUSR1, &act(SIG_DFL, 0));
        install(SIGURG, &act(SIG_DFL, 0));
    }

    fn handler() -> SighandlerT {
        counted as *const () as SighandlerT
    }

    // -- the counts --

    #[test]
    fn a_handler_is_counted_and_sa_restart_is_told_apart() {
        let before = Mark::now();
        install(SIGUSR1, &act(handler(), SA_RESTART));
        assert_eq!(raise(SIGUSR1), 0);
        assert!(before.interrupted(Restart::Never), "any handler");
        assert!(
            !before.interrupted(Restart::IfAsked),
            "not one with SA_RESTART"
        );
        install(SIGUSR1, &act(handler(), 0));
        assert_eq!(raise(SIGUSR1), 0);
        assert!(before.interrupted(Restart::IfAsked), "one without it");
        defaults();
    }

    #[test]
    fn a_signal_that_runs_no_handler_is_not_counted() {
        let before = Mark::now();
        install(SIGUSR1, &act(SIG_IGN, 0));
        assert_eq!(raise(SIGUSR1), 0);
        // SIGURG's default is to be ignored.
        assert_eq!(raise(SIGURG), 0);
        assert!(!before.interrupted(Restart::Never));
        defaults();
    }

    // -- the wait --

    #[test]
    fn the_kernel_ending_a_wait_for_nothing_here_does_not_end_it() {
        let word = AtomicI32::new(0);
        script::set([Step::SignalElsewhere(Box::new(|| {}))]);
        let waited = futex_wait_interruptible(&word, 0, None, Restart::Never);
        assert_eq!(waited, Waited::Woken);
        assert_eq!(script::clear(), 0);
    }

    #[test]
    fn a_handler_run_since_the_mark_ends_the_wait_without_a_sleep() {
        let word = AtomicI32::new(0);
        let mark = Mark::now();
        install(SIGUSR1, &act(handler(), 0));
        assert_eq!(raise(SIGUSR1), 0);
        // A wait that slept would meet this step.
        script::set([Step::Wake(Box::new(|| panic!("it slept")))]);
        let waited = futex_wait_interruptible_since(&word, 0, None, Restart::IfAsked, mark);
        assert_eq!(waited, Waited::Interrupted);
        assert_eq!(script::clear(), 1);
        defaults();
    }

    // -- glibc's answers --

    /// glibc 2.39's answers on Linux (`posix/tools/oracle/interrupt_harness.py`).
    const ORACLE: &str = include_str!("interrupt_oracle.txt");

    /// The five ways the harness signals a call.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Way {
        Handler,
        Restarting,
        Ignored,
        IgnoredByDefault,
        Elsewhere,
    }

    impl Way {
        fn named(name: &str) -> Self {
            match name {
                "handler" => Self::Handler,
                "SA_RESTART handler" => Self::Restarting,
                "SIG_IGN" => Self::Ignored,
                "ignored by default" => Self::IgnoredByDefault,
                "handler on another thread" => Self::Elsewhere,
                other => panic!("no way {other:?}"),
            }
        }

        /// This thread's dispositions for the way, and what the kernel's
        /// ending the call's wait comes to.
        fn arrange(self) -> Step {
            match self {
                Self::Handler => {
                    install(SIGUSR1, &act(handler(), 0));
                    Step::Signal(SIGUSR1)
                }
                Self::Restarting => {
                    install(SIGUSR1, &act(handler(), SA_RESTART));
                    Step::Signal(SIGUSR1)
                }
                Self::Ignored => {
                    install(SIGUSR1, &act(SIG_IGN, 0));
                    Step::Signal(SIGUSR1)
                }
                Self::IgnoredByDefault => {
                    install(SIGURG, &act(SIG_DFL, 0));
                    Step::Signal(SIGURG)
                }
                Self::Elsewhere => {
                    install(SIGUSR1, &act(handler(), 0));
                    // The handler runs on the thread the signal went to.
                    Step::SignalElsewhere(Box::new(|| counted(SIGUSR1)))
                }
            }
        }
    }

    /// One case, as the harness writes its line: the way arranged; `call`
    /// made on this thread, its first wait ended for the signal and its next
    /// by a wake after `release` brings what it waits for; its answer; and
    /// whether it needed the release.
    fn replay_one(
        way: Way,
        call: impl FnOnce() -> String,
        release: impl FnOnce() + 'static,
    ) -> String {
        replay_with(way, call, Step::Wake(Box::new(release)))
    }

    /// [`replay_one`] with the release a step of its own: for the calls a
    /// wake cannot end, which the harness releases with a signal.
    fn replay_with(way: Way, call: impl FnOnce() -> String, release: Step) -> String {
        RAN.with(|r| r.set(0));
        let signal = way.arrange();
        script::set([signal, release]);
        let answer = call();
        let unmet = script::clear();
        defaults();
        let how = match unmet {
            0 => "waits on, then:",
            1 => "ends at once:",
            _ => panic!("the call did not wait"),
        };
        format!("{how} {answer} (handler ran {})", RAN.with(Cell::get))
    }

    fn errno_name(e: i32) -> String {
        match e {
            errno::EINTR => "EINTR".into(),
            errno::ETIMEDOUT => "ETIMEDOUT".into(),
            errno::EAGAIN => "EAGAIN".into(),
            errno::EINVAL => "EINVAL".into(),
            errno::EIO => "EIO".into(),
            other => other.to_string(),
        }
    }

    /// The harness's `said`: the value, and errno's name when it is -1.
    fn said(rc: isize) -> String {
        if rc == -1 {
            format!("-1 {}", errno_name(errno::get_errno()))
        } else {
            rc.to_string()
        }
    }

    /// Ten seconds from now on `clock`: a deadline the replay never reaches.
    fn in_ten(clock: i32) -> Timespec {
        let mut t = crate::lowlevellock::now_on(clock);
        t.tv_sec += 10;
        t
    }

    const TEN: Timespec = Timespec {
        tv_sec: 10,
        tv_nsec: 0,
    };

    fn semaphore(call: &str, way: Way) -> String {
        use crate::semaphore::{SemT, sem_clockwait, sem_post, sem_timedwait, sem_wait};
        let sem = Box::into_raw(Box::new(SemT::new(0)));
        let line = replay_one(
            way,
            || {
                let rc = match call {
                    "sem_wait" => sem_wait(sem),
                    "sem_timedwait" => sem_timedwait(sem, &in_ten(CLOCK_REALTIME)),
                    _ => sem_clockwait(sem, CLOCK_MONOTONIC, &in_ten(CLOCK_MONOTONIC)),
                };
                said(rc as isize)
            },
            move || assert_eq!(sem_post(sem), 0),
        );
        // SAFETY: made above, and nothing refers to it now.
        drop(unsafe { Box::from_raw(sem) });
        line
    }

    fn message_queue(call: &str, way: Way) -> String {
        use crate::fcntl::{O_CREAT, O_RDWR};
        use crate::mqueue::{
            MqAttr, mq_close, mq_open, mq_receive, mq_send, mq_timedreceive, mq_timedsend,
            mq_unlink,
        };
        const NAME: &[u8] = b"/interrupt-probe\0";
        mq_unlink(NAME.as_ptr());
        // SAFETY: all-zero is a valid `struct mq_attr`.
        let mut attr: MqAttr = unsafe { core::mem::zeroed() };
        attr.mq_maxmsg = 1;
        attr.mq_msgsize = 8;
        let q = mq_open(NAME.as_ptr(), O_CREAT | O_RDWR, 0o600, &attr);
        assert!(q >= 0, "mq_open");
        let sends = call.ends_with("send");
        if sends {
            // Full: its one message.
            assert_eq!(mq_send(q, b"f".as_ptr(), 1, 0), 0);
        }
        let line = replay_one(
            way,
            || {
                let mut buf = [0u8; 8];
                let at = in_ten(CLOCK_REALTIME);
                said(match call {
                    "mq_receive" => mq_receive(q, buf.as_mut_ptr(), 8, null_mut()),
                    "mq_timedreceive" => mq_timedreceive(q, buf.as_mut_ptr(), 8, null_mut(), &at),
                    "mq_send" => mq_send(q, b"s".as_ptr(), 1, 0) as isize,
                    _ => mq_timedsend(q, b"s".as_ptr(), 1, 0, &at) as isize,
                })
            },
            move || {
                if sends {
                    let mut buf = [0u8; 8];
                    assert_eq!(mq_receive(q, buf.as_mut_ptr(), 8, null_mut()), 1);
                } else {
                    assert_eq!(mq_send(q, b"r".as_ptr(), 1, 0), 0);
                }
            },
        );
        assert_eq!(mq_close(q), 0);
        assert_eq!(mq_unlink(NAME.as_ptr()), 0);
        line
    }

    /// A `struct msgbuf` of type 1 holding `text`.
    fn message(text: &[u8]) -> [u8; 16] {
        let mut m = [0u8; 16];
        m[..8].copy_from_slice(&1i64.to_ne_bytes());
        m[8..8 + text.len()].copy_from_slice(text);
        m
    }

    fn system_v_message(call: &str, way: Way) -> String {
        use crate::sysv_msg::{
            IPC_CREAT, IPC_NOWAIT, IPC_PRIVATE, IPC_RMID, IPC_SET, IPC_STAT, MsqidDs, msgctl,
            msgget, msgrcv, msgsnd,
        };
        let id = msgget(IPC_PRIVATE, IPC_CREAT | 0o600);
        assert!(id >= 0, "msgget");
        let sends = call == "msgsnd";
        if sends {
            // Full: eight bytes of room, and eight taken.
            // SAFETY: all-zero is a valid `struct msqid_ds`.
            let mut ds: MsqidDs = unsafe { core::mem::zeroed() };
            assert_eq!(msgctl(id, IPC_STAT, &mut ds), 0);
            ds.msg_qbytes = 8;
            assert_eq!(msgctl(id, IPC_SET, &mut ds), 0);
            assert_eq!(msgsnd(id, message(b"full").as_ptr(), 8, 0), 0);
        }
        let line = replay_one(
            way,
            || {
                if sends {
                    said(msgsnd(id, message(b"more").as_ptr(), 8, 0) as isize)
                } else {
                    let mut buf = [0u8; 16];
                    said(msgrcv(id, buf.as_mut_ptr(), 8, 0, 0))
                }
            },
            move || {
                if sends {
                    let mut buf = [0u8; 16];
                    assert_eq!(msgrcv(id, buf.as_mut_ptr(), 8, 0, IPC_NOWAIT), 8);
                } else {
                    assert_eq!(msgsnd(id, message(b"r").as_ptr(), 1, IPC_NOWAIT), 0);
                }
            },
        );
        assert_eq!(msgctl(id, IPC_RMID, null_mut()), 0);
        line
    }

    fn system_v_semaphore(call: &str, way: Way) -> String {
        use crate::sysv_sem::{
            IPC_CREAT, IPC_PRIVATE, IPC_RMID, Sembuf, Semun, semctl, semget, semop, semtimedop,
        };
        let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
        assert!(id >= 0, "semget");
        let line = replay_one(
            way,
            || {
                let down = [Sembuf {
                    sem_num: 0,
                    sem_op: -1,
                    sem_flg: 0,
                }];
                let rc = if call == "semop" {
                    semop(id, down.as_ptr(), 1)
                } else {
                    semtimedop(id, down.as_ptr(), 1, &TEN)
                };
                said(rc as isize)
            },
            move || {
                let up = [Sembuf {
                    sem_num: 0,
                    sem_op: 1,
                    sem_flg: 0,
                }];
                assert_eq!(semop(id, up.as_ptr(), 1), 0);
            },
        );
        // SAFETY: `IPC_RMID` reads nothing from its argument.
        assert_eq!(unsafe { semctl(id, 0, IPC_RMID, Semun { val: 0 }) }, 0);
        line
    }

    fn kernel_aio(call: &str, way: Way) -> String {
        use crate::linux_aio_abi::{IoEvent, sys_io_getevents, test_complete_one, test_context};
        let ctx = test_context();
        let line = replay_one(
            way,
            || {
                let mut ev = [IoEvent::zeroed(); 1];
                let timeout: *const Timespec = if call == "io_getevents" { null() } else { &TEN };
                // SAFETY: a local buffer of one event, and a local timeout.
                match unsafe { sys_io_getevents(ctx, 1, 1, ev.as_mut_ptr(), timeout) } {
                    Ok(n) => n.to_string(),
                    Err(e) => format!("-1 {}", errno_name(e)),
                }
            },
            move || test_complete_one(ctx),
        );
        assert_eq!(crate::linux_aio_abi::sys_io_destroy(ctx), Ok(()));
        line
    }

    fn posix_aio(call: &str, way: Way) -> String {
        use crate::aio::{aio_suspend, test_finish, test_in_progress};
        let cb = Box::into_raw(Box::new(test_in_progress()));
        let line = replay_one(
            way,
            || {
                let list = [cb.cast_const()];
                let timeout: *const Timespec = if call == "aio_suspend" { null() } else { &TEN };
                said(aio_suspend(list.as_ptr(), 1, timeout) as isize)
            },
            // SAFETY: `cb` is freed only after the replay.
            move || test_finish(unsafe { &*cb }),
        );
        // SAFETY: made above, and nothing refers to it now.
        drop(unsafe { Box::from_raw(cb) });
        line
    }

    fn lookup(call: &str, way: Way) -> String {
        use crate::gai_a::tests::{PAUSED, eai, serial};
        use crate::gai_a::{
            EAI_INPROGRESS, GAI_NOWAIT, GAI_WAIT, Gaicb, gai_error, gai_suspend, getaddrinfo_a,
        };
        use crate::nss_files::{Which, set_test_text};
        let _serial = serial();
        set_test_text(Which::Hosts, Some(b"127.0.0.1 localhost\n"));
        // Held in the queue, so that the lookup is still to be made.
        PAUSED.store(true, Ordering::Release);
        let req = Box::into_raw(Box::new(Gaicb {
            ar_name: c"localhost".as_ptr().cast(),
            ar_service: null(),
            ar_request: null(),
            ar_result: null_mut(),
            ret: 0,
            reserved: [0; 5],
        }));
        let list = [req];
        // `GAI_WAIT` asks for the lookup in the call it waits in.
        let in_the_call = call == "getaddrinfo_a GAI_WAIT";
        if !in_the_call {
            // SAFETY: one control block, which lives until the lookup is done.
            assert_eq!(
                unsafe { getaddrinfo_a(GAI_NOWAIT, list.as_ptr(), 1, null()) },
                0
            );
        }
        let line = replay_one(
            way,
            || {
                if in_the_call {
                    // SAFETY: as above.
                    let rc = unsafe { getaddrinfo_a(GAI_WAIT, list.as_ptr(), 1, null()) };
                    // SAFETY: as above.
                    let made = unsafe { gai_error(req) } != EAI_INPROGRESS;
                    let lookup = if made { "answered" } else { "still being made" };
                    return format!("{}, the lookup {lookup}", eai(rc));
                }
                let wait = [req.cast_const()];
                let timeout: *const Timespec = if call == "gai_suspend" { null() } else { &TEN };
                // SAFETY: a list of one control block, and a local timeout.
                eai(unsafe { gai_suspend(wait.as_ptr(), 1, timeout) })
            },
            || PAUSED.store(false, Ordering::Release),
        );
        PAUSED.store(false, Ordering::Release);
        // SAFETY: the control block made above.
        while unsafe { gai_error(req) } == EAI_INPROGRESS {
            std::thread::yield_now();
        }
        // SAFETY: the answer is the caller's once the lookup is done, and
        // nothing else refers to the control block.
        unsafe {
            crate::gai::freeaddrinfo((*req).ar_result);
            drop(Box::from_raw(req));
        }
        set_test_text(Which::Hosts, None);
        line
    }

    fn linux_futex(call: &str, way: Way) -> String {
        use crate::linux_futex::{FUTEX_WAIT_PRIVATE, futex};
        let word = Box::into_raw(Box::new(0u32));
        let line = replay_one(
            way,
            || {
                let timeout: *const Timespec = if call == "futex FUTEX_WAIT" {
                    null()
                } else {
                    &TEN
                };
                said(futex(word, FUTEX_WAIT_PRIVATE, 0, timeout, null_mut(), 0) as isize)
            },
            // SAFETY: the word outlives the replay.  The wake itself is the
            // scripted step this runs in.
            move || unsafe { *word = 1 },
        );
        // SAFETY: made above, and nothing refers to it now.
        drop(unsafe { Box::from_raw(word) });
        line
    }

    fn condition(way: Way) -> String {
        use crate::pthread::{
            PthreadCondT, PthreadMutexT, pthread_cond_init, pthread_cond_signal, pthread_cond_wait,
            pthread_mutex_init, pthread_mutex_lock, pthread_mutex_unlock,
        };
        // SAFETY: both are initialised before use, by the calls below.
        let m: *mut PthreadMutexT = Box::into_raw(Box::new(unsafe { core::mem::zeroed() }));
        // SAFETY: as above.
        let c: *mut PthreadCondT = Box::into_raw(Box::new(unsafe { core::mem::zeroed() }));
        let signalled: *const AtomicBool = Box::into_raw(Box::new(AtomicBool::new(false)));
        // SAFETY: `m` and `c` are valid, owned by this test until freed below.
        unsafe {
            assert_eq!(pthread_mutex_init(m, null()), 0);
            assert_eq!(pthread_cond_init(c, null()), 0);
            assert_eq!(pthread_mutex_lock(m), 0);
        }
        let line = replay_one(
            way,
            || {
                // SAFETY: as above.
                unsafe {
                    let rc = pthread_cond_wait(c, m);
                    let was = (*signalled).load(Ordering::Relaxed);
                    assert_eq!(pthread_mutex_unlock(m), 0);
                    format!("{rc}{}", if was { "" } else { " unsignalled" })
                }
            },
            // SAFETY: as above; the waiter has let the mutex go.
            move || unsafe {
                assert_eq!(pthread_mutex_lock(m), 0);
                (*signalled).store(true, Ordering::Relaxed);
                assert_eq!(pthread_cond_signal(c), 0);
                assert_eq!(pthread_mutex_unlock(m), 0);
            },
        );
        // SAFETY: made above, and nothing refers to them now.
        unsafe {
            drop(Box::from_raw(m));
            drop(Box::from_raw(c));
            drop(Box::from_raw(signalled.cast_mut()));
        }
        line
    }

    fn mutex(way: Way) -> String {
        use crate::pthread::{
            PthreadMutexT, pthread_mutex_init, pthread_mutex_lock, pthread_mutex_unlock,
        };
        // SAFETY: initialised before use, below.
        let m: *mut PthreadMutexT = Box::into_raw(Box::new(unsafe { core::mem::zeroed() }));
        // SAFETY: `m` is valid, owned by this test until freed below.  A
        // default mutex taken again by its holder waits, as glibc's does, for
        // the release to let it go.
        unsafe {
            assert_eq!(pthread_mutex_init(m, null()), 0);
            assert_eq!(pthread_mutex_lock(m), 0);
        }
        let line = replay_one(
            way,
            || {
                // SAFETY: as above.
                let rc = unsafe { pthread_mutex_lock(m) };
                if rc == 0 {
                    // SAFETY: as above.
                    assert_eq!(unsafe { pthread_mutex_unlock(m) }, 0);
                }
                rc.to_string()
            },
            // SAFETY: as above.
            move || assert_eq!(unsafe { pthread_mutex_unlock(m) }, 0),
        );
        // SAFETY: made above, and nothing refers to it now.
        drop(unsafe { Box::from_raw(m) });
        line
    }

    /// A sleep, as the harness sleeps: two seconds -- or, where glibc's runs
    /// its course, a short one, since the host would spin out each of those
    /// two seconds: all that is asserted of them is that the signal did not
    /// end them, which a short sleep shows as well.  (`sleep` counts whole
    /// seconds, so its short one is one.)
    fn sleeping(call: &str, way: Way, runs_its_course: bool) -> String {
        use crate::time::{CLOCK_MONOTONIC, clock_nanosleep, nanosleep, sleep, usleep};
        let t = if runs_its_course {
            Timespec {
                tv_sec: 0,
                tv_nsec: 20_000_000,
            }
        } else {
            Timespec {
                tv_sec: 2,
                tv_nsec: 0,
            }
        };
        let call = call.to_string();
        replay_one(
            way,
            move || {
                let mut rem = Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                match call.as_str() {
                    "nanosleep" => {
                        if nanosleep(&t, &mut rem) == 0 {
                            "0".into()
                        } else {
                            format!(
                                "-1 {}, {} s left",
                                errno_name(errno::get_errno()),
                                rem.tv_sec
                            )
                        }
                    }
                    "clock_nanosleep" => match clock_nanosleep(CLOCK_MONOTONIC, 0, &t, &mut rem) {
                        0 => "0".into(),
                        e => format!("{}, {} s left", errno_name(e), rem.tv_sec),
                    },
                    "usleep" => {
                        let us = t.tv_sec * 1_000_000 + t.tv_nsec / 1_000;
                        said(usleep(u32::try_from(us).unwrap()) as isize)
                    }
                    _ => format!("{} left", sleep(if runs_its_course { 1 } else { 2 })),
                }
            },
            || {},
        )
    }

    /// `pause` and `sigsuspend`, released as the harness releases them: by
    /// SIGUSR2, whose handler has no `SA_RESTART` and is not counted.
    fn waiting_for_a_signal(call: &str, way: Way) -> String {
        use crate::signal::SIGUSR2;
        extern "C" fn uncounted(_: i32) {}
        install(SIGUSR2, &act(uncounted as *const () as SighandlerT, 0));
        let line = replay_with(
            way,
            || {
                let rc = if call == "pause" {
                    crate::unistd::pause()
                } else {
                    crate::signal::sigsuspend(&SigsetT::EMPTY)
                };
                said(rc as isize)
            },
            Step::Signal(SIGUSR2),
        );
        install(SIGUSR2, &act(SIG_DFL, 0));
        line
    }

    /// The calls the kernel itself sleeps in, which the host cannot make, and
    /// the rule each follows here -- `None` for one this library does not
    /// make wait at all.  [`every_interruption_is_glibcs`] holds each rule to
    /// glibc's answers for its call; `services/ctest-eintr` runs some of them
    /// on the target.
    const KERNEL_CALLS: &[(&str, Option<Restart>)] = &[
        // `file.rs`'s `read` and `write`: the kernel's calls restarted by
        // `restarting`, the timerfd loop by its mark.
        ("read, a pipe", Some(Restart::IfAsked)),
        ("write, a full pipe", Some(Restart::IfAsked)),
        ("read, an eventfd", Some(Restart::IfAsked)),
        ("read, a timerfd", Some(Restart::IfAsked)),
        // `socket.rs`: a timeout on the socket makes it `Never`.
        ("recv, a socket", Some(Restart::IfAsked)),
        ("recv, a socket with SO_RCVTIMEO", Some(Restart::Never)),
        ("accept", Some(Restart::IfAsked)),
        // `process.rs`'s `wait_common`, and `file.rs`'s `do_flock`.
        ("waitpid", Some(Restart::IfAsked)),
        ("flock", Some(Restart::IfAsked)),
        // `poll.rs` and `epoll.rs`: their loops' marks.
        ("poll", Some(Restart::Never)),
        ("poll timed", Some(Restart::Never)),
        ("ppoll", Some(Restart::Never)),
        ("select", Some(Restart::Never)),
        ("pselect", Some(Restart::Never)),
        ("epoll_wait", Some(Restart::Never)),
        // A FIFO cannot be made here (`mknod` of one answers ENOSYS), nor a
        // signalfd; a record lock is granted at once, so nothing waits in
        // `F_SETLKW`; and `sigtimedwait` is a stub.
        ("open, a FIFO", None),
        ("read, a signalfd", None),
        ("fcntl F_SETLKW", None),
        ("sigtimedwait", None),
    ];

    /// Whether `rule` gives glibc's line for `way`: a handler without
    /// `SA_RESTART` ends the call with EINTR, one with it only under `Never`,
    /// and any other way leaves it waiting.
    fn rule_gives(rule: Restart, way: Way, glibc: &str) -> bool {
        let ends = way == Way::Handler || way == Way::Restarting && rule == Restart::Never;
        if ends {
            glibc.starts_with("ends at once: -1 EINTR ")
        } else {
            glibc.starts_with("waits on, then: ")
        }
    }

    #[test]
    fn every_interruption_is_glibcs() {
        let (mut replayed, mut ruled) = (0, 0);
        for line in ORACLE
            .lines()
            .filter(|l| !l.starts_with('#') && *l != "done")
        {
            let (case, glibc) = line.split_once(": ").expect("a case");
            let (call, way) = case.rsplit_once(", ").expect("a call and a way");
            let way = Way::named(way);
            let ours = match call {
                "sem_wait" | "sem_timedwait" | "sem_clockwait" => semaphore(call, way),
                "mq_receive" | "mq_timedreceive" | "mq_send" | "mq_timedsend" => {
                    message_queue(call, way)
                }
                "msgrcv" | "msgsnd" => system_v_message(call, way),
                "semop" | "semtimedop" => system_v_semaphore(call, way),
                "io_getevents" | "io_getevents timed" => kernel_aio(call, way),
                "aio_suspend" | "aio_suspend timed" => posix_aio(call, way),
                "gai_suspend" | "gai_suspend timed" | "getaddrinfo_a GAI_WAIT" => lookup(call, way),
                "futex FUTEX_WAIT" | "futex FUTEX_WAIT timed" => linux_futex(call, way),
                "pthread_cond_wait" => condition(way),
                "pthread_mutex_lock" => mutex(way),
                "nanosleep" | "clock_nanosleep" | "usleep" | "sleep" => {
                    sleeping(call, way, glibc.starts_with("waits on"))
                }
                "pause" | "sigsuspend" => waiting_for_a_signal(call, way),
                // glibc's `lio_listio(LIO_WAIT)` sleeps until its requests
                // are done; this library performs them on the calling thread
                // as the call is made (`crate::aio`), so it never sleeps in a
                // futex, and a signal interrupts the transfer itself.
                "lio_listio" => continue,
                other => {
                    let Some(&(_, rule)) = KERNEL_CALLS.iter().find(|(name, _)| *name == other)
                    else {
                        panic!("the oracle has a call no replay knows: {other}");
                    };
                    if let Some(rule) = rule {
                        assert!(
                            rule_gives(rule, way, glibc),
                            "{other}, {way:?}: {rule:?} is not glibc's {glibc}"
                        );
                        ruled += 1;
                    }
                    continue;
                }
            };
            assert_eq!(ours, glibc, "{call}, {way:?}");
            replayed += 1;
        }
        assert_eq!(
            replayed, 140,
            "every line the host can make but lio_listio's five"
        );
        assert_eq!(
            ruled, 75,
            "every kernel call's line that this library makes wait"
        );
    }
}
