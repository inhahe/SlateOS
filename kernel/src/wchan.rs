//! Wait channels -- what a blocked task is waiting on, and who holds it.
//!
//! When a task blocks, the code that blocks it says what for
//! ([`crate::sched::block_current_on`], [`crate::ipc::waiters::park_interruptible`]):
//! the kind of thing waited on and its argument -- the futex address, the
//! channel handle, the child's pid. The scheduler keeps that [`Wait`] on the
//! task, beside the `Blocked` state it describes ([`crate::sched::wait_of`]).
//! This module names the kinds, prints a wait, and works out who *holds* what
//! is waited on where the kernel knows it ([`holder`]).
//!
//! It is what `/proc/<pid>/wchan` and `/proc/<pid>/task/<tid>/wchan` print,
//! what `/proc/<pid>/stat` field 35 reports as 1, and what the kernel shell's
//! `wchan` command lists: Unix's WCHAN column, with the argument Linux's
//! function name leaves out, and the holder its deadlock hunters have to
//! guess.
//!
//! ## The `/proc` line
//!
//! One line, no trailing newline (as Linux's `wchan`), tokens separated by
//! single spaces:
//!
//! ```text
//! 0                                     runnable, or not waiting
//! <kind>                                e.g. `poll`, `terminal`, `stopped`
//! <kind> <arg>                          e.g. `channel 12`, `futex 0x7f001000`
//! <kind> <arg> holder <pid>             e.g. `channel 12 holder 34`
//! <kind> <arg> holder <pid> thread <tid>  the holding thread is known too
//! ```
//!
//! The first token is always the kind, so a reader that wants Linux's single
//! word takes the first token. An argument of 0 means "none" and is left out.
//! An address argument (futex, mutex) is in hex with `0x`; every other
//! argument is decimal.
//!
//! ## Wait channel kinds
//!
//! | Kind | Waiting for | Argument |
//! |---|---|---|
//! | `timer` | a sleep to end | the deadline, `CLOCK_MONOTONIC` nanoseconds |
//! | `channel` | an IPC channel to send or receive on | the channel handle |
//! | `pipe` | a pipe to fill or drain | the pipe handle |
//! | `futex` | a futex word | its address in the waiter's space |
//! | `mutex` | a kernel lock, condition or wait queue | -- (never a kernel address) |
//! | `event` | an eventfd, a timerfd or an inotify instance | its handle |
//! | `join` | another thread to exit | that thread's id |
//! | `completion` | a completion port | -- |
//! | `io` | a device interrupt (a userspace driver) | the IRQ |
//! | `socket` | a socket | its handle |
//! | `terminal` | a terminal, a pseudo-terminal or the keyboard | -- |
//! | `filelock` | an advisory or record file lock | -- |
//! | `poll` | `poll`/`select`/`epoll`, a multi-object wait | -- |
//! | `child` | a child to change state (`wait*`) | its pid; none for any child |
//! | `signal` | a signal (`sigsuspend`, `pause`, `sigtimedwait`, a `signalfd` read) | -- |
//! | `semaphore` | a counting semaphore | its handle |
//! | `service` | a connection to accept | the listener's handle |
//! | `stopped` | to be continued (job control, a debugger) | -- |
//! | `wait` | something no blocking code has described | -- |
//!
//! ## Holders
//!
//! [`holder`] answers only where the kernel knows, and is silent otherwise --
//! never a guess, since a deadlock analyzer that draws a wrong edge finds a
//! cycle that is not there:
//!
//! - **channel**: the process bound to the other end, from the credential
//!   recorded when a service connection was made (`SYS_CHANNEL_PEER_CRED`'s
//!   answer). A channel made by `channel_create` and handed on has no
//!   recorded peer, so it has no holder.
//! - **child**: the child waited for, when the wait names one.
//! - **join**: the thread waited for, and its process.
//! - **futex**: for a priority-inheritance futex (`FUTEX_LOCK_PI`), the
//!   thread that owns it. A plain futex has no owner the kernel can see.
//!
//! ## References
//!
//! - Linux `/proc/[pid]/wchan`, `proc(5)` -- the symbolic wait channel
//! - FreeBSD `ki_wchan` / `ki_wmesg` -- the wait channel and its message
//! - `ps(1)` WCHAN column

use crate::proc::pcb::ProcessId;
use crate::sched::task::TaskId;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Wait channel kinds
// ---------------------------------------------------------------------------

/// The kind of thing a task is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WaitChannel {
    /// Not waiting: running, ready, or exited.
    None = 0,
    /// Sleeping until a deadline.
    Timer = 1,
    /// Waiting on an IPC channel (send or receive).
    Channel = 2,
    /// Blocked on a pipe (read from empty or write to full).
    Pipe = 3,
    /// Waiting on a futex word.
    Futex = 4,
    /// Waiting for a kernel lock or on a wait queue.
    Mutex = 5,
    /// Waiting for an event (eventfd, timerfd, one-shot event).
    Event = 6,
    /// Waiting for another thread to exit (join).
    Join = 7,
    /// Waiting on a completion port.
    Completion = 8,
    /// Waiting for I/O to complete.
    Io = 9,
    /// Blocked on something no blocking code has described.
    Other = 10,
    /// A stream socket.
    Socket = 11,
    /// A terminal, a pseudo-terminal or the keyboard.
    Terminal = 12,
    /// An advisory or record file lock.
    FileLock = 13,
    /// A multi-object wait (`poll`, `select`, `epoll`, multiwait).
    Poll = 14,
    /// A child's state change (`wait`, `waitpid`, `waitid`).
    Child = 15,
    /// A signal (`sigsuspend`, `pause`, `sigtimedwait`).
    Signal = 16,
    /// A counting semaphore.
    Semaphore = 17,
    /// A connection to a service.
    Service = 18,
    /// Stopped, until continued (job control, a debugger).
    Stopped = 19,
}

/// How many kinds there are, [`WaitChannel::None`] included -- the length of
/// [`WchanStats::by_channel`] and of [`WaitChannel::ALL`].
pub const KINDS: usize = 20;

impl WaitChannel {
    /// Every kind, in discriminant order: `ALL[k as usize] == k`.
    pub const ALL: [Self; KINDS] = [
        Self::None,
        Self::Timer,
        Self::Channel,
        Self::Pipe,
        Self::Futex,
        Self::Mutex,
        Self::Event,
        Self::Join,
        Self::Completion,
        Self::Io,
        Self::Other,
        Self::Socket,
        Self::Terminal,
        Self::FileLock,
        Self::Poll,
        Self::Child,
        Self::Signal,
        Self::Semaphore,
        Self::Service,
        Self::Stopped,
    ];

    /// The word `/proc/<pid>/wchan` prints for this kind (the module doc's
    /// table).
    #[must_use]
    pub const fn proc_name(self) -> &'static str {
        match self {
            Self::None => "0",
            Self::Timer => "timer",
            Self::Channel => "channel",
            Self::Pipe => "pipe",
            Self::Futex => "futex",
            Self::Mutex => "mutex",
            Self::Event => "event",
            Self::Join => "join",
            Self::Completion => "completion",
            Self::Io => "io",
            Self::Other => "wait",
            Self::Socket => "socket",
            Self::Terminal => "terminal",
            Self::FileLock => "filelock",
            Self::Poll => "poll",
            Self::Child => "child",
            Self::Signal => "signal",
            Self::Semaphore => "semaphore",
            Self::Service => "service",
            Self::Stopped => "stopped",
        }
    }

    /// Whether this kind's argument is an address, printed in hex.
    const fn arg_is_address(self) -> bool {
        matches!(self, Self::Futex | Self::Mutex)
    }
}

// ---------------------------------------------------------------------------
// A wait
// ---------------------------------------------------------------------------

/// What a task blocks on: the kind and its argument. Built where a task
/// blocks and handed to [`crate::sched::block_current_on`] or
/// [`crate::ipc::waiters::park_interruptible`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wait {
    /// The kind of thing waited on.
    pub channel: WaitChannel,
    /// Its argument: an address, a handle, a pid, a deadline -- see the
    /// module doc's table.
    pub arg: u64,
}

impl Wait {
    /// Not waiting.
    pub const NONE: Self = Self::new(WaitChannel::None, 0);
    /// Blocked, on something the blocking code did not describe.
    pub const UNDESCRIBED: Self = Self::new(WaitChannel::Other, 0);
    /// Stopped until continued.
    pub const STOPPED: Self = Self::new(WaitChannel::Stopped, 0);

    /// A wait on `channel` with argument `arg`.
    #[must_use]
    pub const fn new(channel: WaitChannel, arg: u64) -> Self {
        Self { channel, arg }
    }

    /// A wait on `channel` whose argument says nothing.
    #[must_use]
    pub const fn on(channel: WaitChannel) -> Self {
        Self::new(channel, 0)
    }

    /// Whether this describes a wait at all (anything but [`Wait::NONE`]).
    #[must_use]
    pub const fn is_waiting(self) -> bool {
        !matches!(self.channel, WaitChannel::None)
    }
}

/// The wait as `/proc/<pid>/wchan` prints it, without the holder: the kind,
/// then the argument unless it is 0 (`futex 0x7f001000`, `channel 12`,
/// `poll`, `0`). Allocates nothing, so a hang dump may print it.
impl core::fmt::Display for Wait {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = self.channel.proc_name();
        if !self.is_waiting() || self.arg == 0 {
            f.write_str(name)
        } else if self.channel.arg_is_address() {
            write!(f, "{name} {:#x}", self.arg)
        } else {
            write!(f, "{name} {}", self.arg)
        }
    }
}

// ---------------------------------------------------------------------------
// Holders
// ---------------------------------------------------------------------------

/// Who holds what a task waits on (see the module doc's "Holders").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holder {
    /// The holding process.
    pub pid: ProcessId,
    /// The holding thread, where the kernel knows which one.
    pub thread: Option<TaskId>,
}

/// Who holds what `task` waits on in `wait`, or `None` where the kernel does
/// not know (most kinds, and a channel with no recorded peer).
///
/// Takes the channel table, the PI futex table, the scheduler and the thread
/// table one at a time, never nested, and never with the caller's locks: call
/// it with none held.
#[must_use]
pub fn holder(task: TaskId, wait: Wait) -> Option<Holder> {
    match wait.channel {
        WaitChannel::Channel => {
            let handle = crate::ipc::channel::ChannelHandle::from_raw(wait.arg);
            let peer = crate::ipc::channel::peer_cred(handle)?;
            Some(Holder {
                pid: peer.pid,
                thread: None,
            })
        }
        WaitChannel::Child if wait.arg != 0 => Some(Holder {
            pid: wait.arg,
            thread: None,
        }),
        WaitChannel::Join => {
            let pid = crate::proc::thread::owner_process(wait.arg)?;
            Some(Holder {
                pid,
                thread: Some(wait.arg),
            })
        }
        WaitChannel::Futex => {
            let owner = crate::ipc::futex::pi_owner_of_waiter(task)?;
            let pid = crate::proc::thread::owner_process(owner)?;
            Some(Holder {
                pid,
                thread: Some(owner),
            })
        }
        _ => None,
    }
}

/// A task's wait and its holder: one `/proc/<pid>/wchan` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// What the task waits on ([`Wait::NONE`] if it is not waiting).
    pub wait: Wait,
    /// Who holds it, where known.
    pub holder: Option<Holder>,
}

/// The `/proc` line (the module doc's format), without a trailing newline.
impl core::fmt::Display for Report {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.wait)?;
        if let Some(h) = self.holder {
            write!(f, " holder {}", h.pid)?;
            if let Some(t) = h.thread {
                write!(f, " thread {t}")?;
            }
        }
        Ok(())
    }
}

/// What `task` waits on and who holds it, or `None` if there is no such
/// task. Takes the scheduler lock, then [`holder`]'s: call with none held.
#[must_use]
pub fn report(task: TaskId) -> Option<Report> {
    let wait = crate::sched::wait_of(task)?;
    let holder = if wait.is_waiting() {
        holder(task, wait)
    } else {
        None
    };
    Some(Report { wait, holder })
}

// ---------------------------------------------------------------------------
// Census, for the kernel shell
// ---------------------------------------------------------------------------

/// How many tasks wait, on what.
#[derive(Debug, Clone)]
pub struct WchanStats {
    /// Tasks waiting on anything (every kind but [`WaitChannel::None`]).
    pub currently_blocked: usize,
    /// Tasks by kind, indexed by `WaitChannel as usize`; index 0 counts the
    /// tasks not waiting.
    pub by_channel: [usize; KINDS],
}

/// Count every task's wait. One pass over the scheduler's tasks under its
/// lock.
#[must_use]
pub fn stats() -> WchanStats {
    let by_channel = crate::sched::wait_census();
    let mut currently_blocked = 0usize;
    for (kind, n) in WaitChannel::ALL.iter().zip(by_channel.iter()) {
        if *kind != WaitChannel::None {
            currently_blocked = currently_blocked.saturating_add(*n);
        }
    }
    WchanStats {
        currently_blocked,
        by_channel,
    }
}

/// Every waiting task's wait, as `(task, wait)`, into `buf`; returns how many
/// were written (at most `buf.len()`).
pub fn blocked_list(buf: &mut [(TaskId, Wait)]) -> usize {
    crate::sched::waiting_tasks(buf)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Set by the self-test's waiter once it has stopped waiting.
static TEST_WAITER_DONE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
/// Cleared by the self-test to let its waiter go.
static TEST_WAITER_HOLD: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// The argument the self-test's waiter blocks with: an event id no real
/// wait uses.
const TEST_EVENT_ID: u64 = 0x5157_0001;

/// The self-test's waiter: blocks on [`TEST_EVENT_ID`] until let go. Loops,
/// because a stray wake (any `sched::wake` aimed at a reused id) must not end
/// the wait early -- the same rule every real wait follows.
extern "C" fn test_waiter(_arg: u64) {
    use core::sync::atomic::Ordering;
    while TEST_WAITER_HOLD.load(Ordering::Acquire) {
        crate::sched::block_current_on(Wait::new(WaitChannel::Event, TEST_EVENT_ID));
    }
    TEST_WAITER_DONE.store(true, Ordering::Release);
}

/// Self-test: the `/proc` wording of every shape of wait, and one real wait
/// recorded end to end -- a task that blocks through
/// [`crate::sched::block_current_on`] reads back as waiting on exactly that,
/// and as not waiting once woken.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    use core::sync::atomic::Ordering;

    /// How long the live half waits for the waiter to park or to finish.
    const PATIENCE_MS: u64 = 2000;

    serial_println!("[wchan] Running self-test...");
    let check = |ok: bool, what: &str| {
        if ok {
            Ok(())
        } else {
            serial_println!("[wchan]   FAIL: {}", what);
            Err(KernelError::InternalError)
        }
    };

    // -- How /proc prints a wait --------------------------------------------
    let line = |r: Report| alloc::format!("{r}");
    let bare = |w: Wait| Report {
        wait: w,
        holder: None,
    };
    check(line(bare(Wait::NONE)) == "0", "a task not waiting")?;
    check(
        line(bare(Wait::new(WaitChannel::Futex, 0x7f00_1000))) == "futex 0x7f001000",
        "a futex address",
    )?;
    check(
        line(bare(Wait::new(WaitChannel::Channel, 12))) == "channel 12",
        "a channel",
    )?;
    check(
        line(bare(Wait::on(WaitChannel::Poll))) == "poll",
        "an empty argument",
    )?;
    check(
        line(bare(Wait::new(WaitChannel::Child, 0))) == "child",
        "any child",
    )?;
    check(
        line(bare(Wait::new(WaitChannel::Child, 77))) == "child 77",
        "one child",
    )?;
    check(
        line(bare(Wait::UNDESCRIBED)) == "wait",
        "an undescribed wait",
    )?;
    check(line(bare(Wait::STOPPED)) == "stopped", "a stopped task")?;
    check(
        line(Report {
            wait: Wait::new(WaitChannel::Channel, 12),
            holder: Some(Holder {
                pid: 34,
                thread: None,
            }),
        }) == "channel 12 holder 34",
        "a holder process",
    )?;
    check(
        line(Report {
            wait: Wait::new(WaitChannel::Join, 57),
            holder: Some(Holder {
                pid: 34,
                thread: Some(57),
            }),
        }) == "join 57 holder 34 thread 57",
        "a holder thread",
    )?;
    check(
        WaitChannel::ALL
            .iter()
            .enumerate()
            .all(|(i, k)| *k as usize == i),
        "ALL is not in discriminant order",
    )?;
    check(
        holder(0, Wait::new(WaitChannel::Child, 4242))
            == Some(Holder {
                pid: 4242,
                thread: None,
            }),
        "a named child is not its own holder",
    )?;
    check(
        holder(0, Wait::new(WaitChannel::Child, 0)).is_none(),
        "any child has a holder",
    )?;
    check(
        holder(0, Wait::on(WaitChannel::Poll)).is_none(),
        "a poll has a holder",
    )?;

    // -- One real wait, end to end -------------------------------------------
    TEST_WAITER_DONE.store(false, Ordering::Release);
    TEST_WAITER_HOLD.store(true, Ordering::Release);
    let waiter = crate::sched::spawn(b"wchan-test", 16, test_waiter, 0, 0)?;
    let parked = Wait::new(WaitChannel::Event, TEST_EVENT_ID);
    let start = crate::hrtimer::now_ns();
    let patience_ns = PATIENCE_MS.saturating_mul(1_000_000);
    let mut seen = Wait::NONE;
    while crate::hrtimer::now_ns().saturating_sub(start) < patience_ns {
        seen = crate::sched::wait_of(waiter).unwrap_or(Wait::NONE);
        if seen == parked {
            break;
        }
        crate::sched::sleep_ms(1);
    }
    let parked_ok = seen == parked
        && report(waiter).is_some_and(|r| alloc::format!("{r}") == "event 1364656129");
    // Let it go whatever happened, so a failure leaves no task parked.
    TEST_WAITER_HOLD.store(false, Ordering::Release);
    crate::sched::wake(waiter);
    let start = crate::hrtimer::now_ns();
    while !TEST_WAITER_DONE.load(Ordering::Acquire)
        && crate::hrtimer::now_ns().saturating_sub(start) < patience_ns
    {
        crate::sched::sleep_ms(1);
    }
    if !parked_ok {
        serial_println!("[wchan]   FAIL: the waiter read as {} while parked", seen);
        return Err(KernelError::InternalError);
    }
    check(
        TEST_WAITER_DONE.load(Ordering::Acquire),
        "the waiter never finished",
    )?;
    // Finished: running its exit path, or gone. Whatever its exit path may
    // briefly wait on, it is no longer waiting on the event.
    check(
        crate::sched::wait_of(waiter) != Some(parked),
        "a woken task still reads as waiting on what woke it",
    )?;

    let s = stats();
    serial_println!(
        "[wchan] Self-test PASSED ({} task(s) waiting now)",
        s.currently_blocked
    );
    Ok(())
}
